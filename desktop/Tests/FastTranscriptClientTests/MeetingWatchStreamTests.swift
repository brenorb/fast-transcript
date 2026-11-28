import Foundation
import XCTest
@testable import FastTranscriptClient

final class MeetingWatchStreamTests: XCTestCase {
    private func line(_ type: String, _ sequence: Int, _ data: [String: Any] = [:]) throws -> Data {
        var bytes = try JSONSerialization.data(withJSONObject: ["version": 1, "sequence": sequence, "type": type, "session_id": NSNull(), "data": data], options: [.sortedKeys])
        bytes.append(10)
        return bytes
    }

    private func inner(_ type: String, _ sequence: Int, id: String? = "one", _ data: [String: Any] = [:]) -> [String: Any] {
        ["version": 1, "sequence": sequence, "type": type, "session_id": id as Any? ?? NSNull(), "data": data]
    }

    func testPropertyFragmentationPreservesIndependentEpisodesAndFinalUnicode() throws {
        var bytes = try line("watch_ready", 0, ["requires_accept": true])
        var sequence = 1
        for episode in 1...2 {
            let directory = "/private/tmp/automatic é 🦀/\(episode)"
            let manifest: [String: Any] = ["state": "stopped", "track_frames": [16000, 16000]]
            bytes += try line("accepted", sequence, ["episode": episode]); sequence += 1
            let events = [
                inner("ready", 0, id: nil, ["requires_start": true]),
                inner("started", 1, ["directory": directory]),
                inner("recording", 2, ["directory": directory]),
                inner("model_ready", 3),
                inner("saved", 4, ["directory": directory, "manifest": manifest]),
                inner("completed", 5, ["directory": directory, "output_path": directory + "/transcript.json", "result": ["text": "Final \(episode) é 漢 ✍️", "meeting": manifest]]),
            ]
            for event in events { bytes += try line("meeting", sequence, ["episode": episode, "event": event]); sequence += 1 }
            bytes += try line("session_exit", sequence, ["episode": episode, "status": 0]); sequence += 1
        }
        bytes += try line("watch_completed", sequence)
        var random: UInt64 = 31
        for _ in 0..<128 {
            var decoder = MeetingWatchStreamDecoder()
            var received: [MeetingWatchEvent] = []
            var position = 0
            while position < bytes.count {
                random = random &* 6364136223846793005 &+ 1
                let end = min(bytes.count, position + 1 + Int(random % 97))
                received += try decoder.append(bytes.subdata(in: position..<end))
                position = end
            }
            try decoder.finish(exitStatus: 0)
            XCTAssertEqual(received.count, sequence + 1)
            XCTAssertEqual(received.compactMap { $0.data.event }.filter { $0.type == "completed" }.map { $0.data.result?.text }, ["Final 1 é 漢 ✍️", "Final 2 é 漢 ✍️"])
        }
    }

    func testUnfinishedWrongEpisodeAndOutOfOrderStreamsCannotSucceed() throws {
        let ready = try line("watch_ready", 0, ["requires_accept": true])
        let accepted = try line("accepted", 1, ["episode": 1])
        let invalid: [Data] = try [
            line("watch_completed", 0),
            ready + line("accepted", 8, ["episode": 1]),
            ready + accepted + line("accepted", 2, ["episode": 2]),
            ready + accepted + line("session_exit", 2, ["episode": 1, "status": 0]),
            ready + accepted + line("watch_completed", 2),
            ready + accepted + line("meeting", 2, ["episode": 2, "event": inner("ready", 0, id: nil, ["requires_start": true])]),
            ready + accepted + line("meeting", 2, ["episode": 1, "event": inner("ready", 3, id: nil, ["requires_start": true])]),
        ]
        for bytes in invalid { var decoder = MeetingWatchStreamDecoder(); XCTAssertThrowsError(try decoder.append(bytes)) }
        var incomplete = MeetingWatchStreamDecoder()
        _ = try incomplete.append(ready + accepted)
        XCTAssertThrowsError(try incomplete.finish(exitStatus: 0))
        var cancelled = MeetingWatchStreamDecoder()
        _ = try cancelled.append(ready + accepted + line("watch_cancelled", 2, ["episode": 1]))
        try cancelled.finish(exitStatus: 0)
        XCTAssertTrue(cancelled.cancelled)
    }
}
