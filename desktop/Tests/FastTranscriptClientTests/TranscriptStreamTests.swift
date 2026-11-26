import Foundation
import XCTest
@testable import FastTranscriptClient

final class TranscriptStreamTests: XCTestCase {
    private func line(_ type: String, sequence: Int, data: [String: Any] = [:], job: String = "job", version: Int = 1) throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: ["version": version, "job_id": job, "sequence": sequence, "type": type, "data": data], options: [.sortedKeys])
        bytes.append(10)
        return bytes
    }

    func testPropertyArbitraryByteFragmentsPreserveUnicodeSnapshots() throws {
        var random: UInt64 = 42
        for index in 0..<500 {
            let text = "Speech \(index): é 🦀 漢 \" \\ \n end"
            let bytes = try line("started", sequence: 0) + line("update", sequence: 1, data: ["text": "partial"]) + line("completed", sequence: 2, data: ["result": ["text": text]])
            var decoder = TranscriptStreamDecoder()
            var received: [TranscriptEvent] = []
            var offset = 0
            while offset < bytes.count {
                random = random &* 6364136223846793005 &+ 1
                let end = min(bytes.count, offset + Int(random % 83) + 1)
                received += try decoder.append(bytes.subdata(in: offset..<end))
                offset = end
            }
            try decoder.finish(exitStatus: 0)
            XCTAssertEqual(received.map(\.type), ["started", "update", "completed"])
            XCTAssertEqual(received.last?.data.result?.text, text)
        }
    }

    func testInvalidLifecyclesCannotBecomeSuccess() throws {
        let start = try line("started", sequence: 0)
        let end = try line("completed", sequence: 1, data: ["result": ["text": "final"]])
        let invalid: [Data] = try [
            line("update", sequence: 0, data: ["text": "partial"]),
            start + line("completed", sequence: 2, data: ["result": ["text": "final"]]),
            start + line("completed", sequence: 1, data: ["result": ["text": "final"]], job: "other"),
            start + line("completed", sequence: 1, data: ["result": ["text": "final"]], version: 2),
            start + line("completed", sequence: 1),
            start + line("started", sequence: 1),
            start + end + end,
            Data("{broken}\n".utf8),
        ]
        for bytes in invalid {
            var decoder = TranscriptStreamDecoder()
            XCTAssertThrowsError(try decoder.append(bytes))
        }
        for bytes in [start, start + end.dropLast(), Data()] {
            var decoder = TranscriptStreamDecoder()
            _ = try decoder.append(bytes)
            XCTAssertThrowsError(try decoder.finish(exitStatus: 0))
        }
        var failed = TranscriptStreamDecoder()
        _ = try failed.append(start + line("error", sequence: 1, data: ["message": "model failed"]))
        XCTAssertThrowsError(try failed.finish(exitStatus: 1))
        XCTAssertEqual(failed.errorMessage, "model failed")
        var nonzero = TranscriptStreamDecoder()
        _ = try nonzero.append(start + end)
        XCTAssertThrowsError(try nonzero.finish(exitStatus: 1))
    }
}
