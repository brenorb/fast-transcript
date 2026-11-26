import Foundation
import XCTest
@testable import FastTranscriptClient

final class RecordingStreamTests: XCTestCase {
    private func line(_ type: String, _ sequence: Int, id: String? = nil, data: [String: Any] = [:], version: Int = 1) throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: ["version": version, "sequence": sequence, "type": type, "session_id": id as Any? ?? NSNull(), "data": data], options: [.sortedKeys])
        bytes.append(10)
        return bytes
    }

    func testPropertyFragmentedWarmSessionsPreserveUnicodeAndCancellation() throws {
        var random: UInt64 = 91
        for index in 0..<500 {
            let text = "Dictation \(index): é 🦀 漢 \" \\ \n end"
            let bytes = try line("ready", 0) + line("recording", 1, id: "cancel") + line("cancelled", 2, id: "cancel") + line("recording", 3, id: "one") + line("processing", 4, id: "one") + line("completed", 5, id: "one", data: ["result": ["text": text], "model_reused": false]) + line("recording", 6, id: "two") + line("processing", 7, id: "two") + line("completed", 8, id: "two", data: ["result": ["text": text], "model_reused": true])
            var decoder = RecordingStreamDecoder()
            var messages: [RecordingEvent] = []
            var offset = 0
            while offset < bytes.count {
                random = random &* 6364136223846793005 &+ 1
                let end = min(bytes.count, offset + Int(random % 89) + 1)
                messages += try decoder.append(bytes.subdata(in: offset..<end))
                offset = end
            }
            try decoder.finish(exitStatus: 0)
            XCTAssertEqual(messages.count, 9)
            XCTAssertEqual(messages.last?.data.result?.text, text)
            XCTAssertEqual(messages.last?.data.model_reused, true)
        }
    }

    func testInvalidLifecycleOrExitCannotBecomeSuccess() throws {
        let ready = try line("ready", 0)
        let recording = try line("recording", 1, id: "one")
        let processing = try line("processing", 2, id: "one")
        let result: [String: Any] = ["result": ["text": "final"], "model_reused": false]
        let completed = try line("completed", 3, id: "one", data: result)
        let invalid: [Data] = try [
            line("recording", 0, id: "one"), line("ready", 1), line("ready", 0, version: 2),
            ready + line("ready", 1), ready + line("recording", 1),
            ready + line("processing", 1, id: "one"), ready + line("completed", 1, id: "one", data: result),
            ready + recording + line("processing", 2, id: "other"),
            ready + recording + processing + line("completed", 3, id: "one"),
            ready + recording + processing + line("cancelled", 3, id: "one"),
            ready + recording + processing + completed + line("recording", 4, id: "one"),
            ready + line("error", 1, data: ["message": "failure"]),
            ready + line("error", 1, data: ["message": "fatal", "fatal": true]) + line("recording", 2, id: "one"),
        ]
        for bytes in invalid { var decoder = RecordingStreamDecoder(); XCTAssertThrowsError(try decoder.append(bytes)) }
        for bytes in [Data(), ready + recording, ready + recording + processing, ready + recording + processing + completed.dropLast()] {
            var decoder = RecordingStreamDecoder()
            _ = try decoder.append(bytes)
            XCTAssertThrowsError(try decoder.finish(exitStatus: 0))
        }
        var decoder = RecordingStreamDecoder()
        _ = try decoder.append(ready + recording + processing + completed)
        XCTAssertThrowsError(try decoder.finish(exitStatus: 130))
        var failed = RecordingStreamDecoder()
        _ = try failed.append(line("error", 0, data: ["message": "load failed", "fatal": true]))
        XCTAssertEqual(failed.errorMessage, "load failed")
        XCTAssertThrowsError(try failed.finish(exitStatus: 1))
    }

    func testRecoverableErrorsPreserveOrResetTheCorrectSessionState() throws {
        var decoder = RecordingStreamDecoder()
        let bytes = try line("ready", 0) + line("error", 1, data: ["message": "cannot open microphone", "fatal": false]) + line("recording", 2, id: "one") + line("error", 3, id: "one", data: ["message": "already recording", "fatal": false]) + line("processing", 4, id: "one") + line("error", 5, id: "one", data: ["message": "inference failed", "fatal": false]) + line("recording", 6, id: "two") + line("cancelled", 7, id: "two")
        _ = try decoder.append(bytes)
        try decoder.finish(exitStatus: 0)
    }
}
