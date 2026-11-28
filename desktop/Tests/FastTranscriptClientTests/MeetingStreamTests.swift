import Foundation
import XCTest
@testable import FastTranscriptClient

final class MeetingStreamTests: XCTestCase {
    private let directory = "/private/tmp/meeting é 🦀"
    private var manifest: [String: Any] { ["state": "stopped", "track_frames": [16000, 16000]] }

    private func line(_ type: String, _ sequence: Int, id: String? = "one", _ data: [String: Any] = [:]) throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: ["version": 1, "sequence": sequence, "type": type, "session_id": id as Any? ?? NSNull(), "data": data], options: [.sortedKeys])
        bytes.append(10)
        return bytes
    }

    func testPropertyFragmentsAndIndependentReadinessReplacePreviewWithFinalText() throws {
        var random: UInt64 = 71
        for iteration in 0..<200 {
            let final = "Final \(iteration): é 漢 🦀, revised words"
            let modelFirst = iteration.isMultiple(of: 2)
            var bytes = try line("ready", 0, id: nil, ["requires_start": true]) + line("started", 1, ["directory": directory])
            bytes += try line(modelFirst ? "model_ready" : "recording", 2, modelFirst ? [:] : ["directory": directory])
            bytes += try line(modelFirst ? "recording" : "model_ready", 3, modelFirst ? ["directory": directory] : [:])
            let preview: [String: Any] = ["text": "Provisional prefix", "segments": [["start_s": 0, "end_s": 2, "text": "Speech é 🦀", "speaker": "System"]]]
            bytes += try line("update", 4, preview) + line("saved", 5, ["directory": directory, "manifest": manifest])
            bytes += try line("completed", 6, ["directory": directory, "output_path": directory + "/transcript.json", "result": ["text": final, "meeting": manifest]])
            var decoder = MeetingStreamDecoder(mode: .live)
            var messages: [MeetingEvent] = []
            var position = 0
            while position < bytes.count {
                random = random &* 6364136223846793005 &+ 1
                let end = min(bytes.count, position + Int(random % 73) + 1)
                messages += try decoder.append(bytes.subdata(in: position..<end))
                position = end
            }
            try decoder.finish(exitStatus: 0)
            XCTAssertTrue(decoder.completed)
            XCTAssertEqual(messages.last?.data.result?.text, final)
            XCTAssertEqual(messages.first(where: { $0.type == "update" })?.data.segments?.first?.speaker, "System")
            XCTAssertEqual(messages.last?.data.result?.meeting?.state, "stopped")
        }
    }

    func testSavedAudioMayPrecedeModelLoadingAndCancellationPreservesTheCorrectState() throws {
        let ready = try line("ready", 0, id: nil, ["requires_start": true]) + line("started", 1, ["directory": directory])
        let saved = try ready + line("saved", 2, ["directory": directory, "manifest": manifest])
        for (bytes, alreadySaved) in [(ready, false), (saved, true)] {
            var decoder = MeetingStreamDecoder(mode: .live)
            _ = try decoder.append(bytes + line("cancelled", alreadySaved ? 3 : 2, ["directory": directory, "audio_stopped": alreadySaved, "recoverable": !alreadySaved]))
            try decoder.finish(exitStatus: 0)
            XCTAssertTrue(decoder.cancelled)
            XCTAssertFalse(decoder.completed)
        }
        var decoder = MeetingStreamDecoder(mode: .saved)
        let interrupted: [String: Any] = ["state": "interrupted", "track_frames": [16000, 0], "route_interruptions": [["track": "system", "reason": "device unavailable"]]]
        _ = try decoder.append(line("ready", 0, id: nil, ["requires_start": false]) + line("started", 1, ["directory": directory]) + line("restored", 2, ["directory": directory, "manifest": interrupted]) + line("model_ready", 3) + line("completed", 4, ["directory": directory, "output_path": directory + "/transcript.json", "result": ["text": "Recovered", "meeting": interrupted]]))
        try decoder.finish(exitStatus: 0)
        XCTAssertTrue(decoder.completed)
    }

    func testInvalidCompletionAndRecognitionFailureCannotBecomeSuccess() throws {
        let ready = try line("ready", 0, id: nil, ["requires_start": true]) + line("started", 1, ["directory": directory])
        let completed: [String: Any] = ["directory": directory, "output_path": directory + "/transcript.json", "result": ["text": "final"]]
        let invalid: [Data] = try [
            line("recording", 0, ["directory": directory]),
            ready + line("completed", 2, completed),
            ready + line("model_ready", 2) + line("completed", 3, completed),
            ready + line("saved", 2, ["directory": directory, "manifest": manifest]) + line("completed", 3, completed),
            ready + line("model_ready", 2) + line("model_ready", 3),
            ready + line("recording", 2, id: "other", ["directory": directory]),
            ready + line("update", 2, ["text": "before model"]),
            ready + line("recognition_error", 2, ["message": "failed", "recording_continues": false]),
            ready + line("source_status", 2, ["track": "unknown", "state": "ready"]),
        ]
        for bytes in invalid { var decoder = MeetingStreamDecoder(mode: .live); XCTAssertThrowsError(try decoder.append(bytes)) }
        var failed = MeetingStreamDecoder(mode: .live)
        _ = try failed.append(ready + line("recognition_error", 2, ["message": "inference failed", "recording_continues": true]) + line("saved", 3, ["directory": directory, "manifest": manifest]) + line("error", 4, id: nil, ["message": "audio retained", "fatal": true]))
        XCTAssertThrowsError(try failed.finish(exitStatus: 1))
        var early = MeetingStreamDecoder(mode: .live)
        _ = try early.append(ready)
        XCTAssertThrowsError(try early.finish(exitStatus: 0))
    }
}
