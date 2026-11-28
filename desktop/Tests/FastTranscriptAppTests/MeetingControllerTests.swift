import Darwin
import Foundation
import XCTest
import FastTranscriptClient
@testable import FastTranscriptApp

@MainActor
final class MeetingControllerTests: XCTestCase {
    private func fixture() async throws -> (URL, URL, [String: String]) {
        let inherited = CLIProcess.inheritedEnvironment()
        guard let binary = inherited["FSCRIPT_DESKTOP_TEST_CLI"], let capture = inherited["FSCRIPT_DESKTOP_TEST_CAPTURE"], let source = inherited["FSCRIPT_DESKTOP_TEST_SOURCE"] else { throw XCTSkip("Run scripts/test_desktop_client.py") }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("meeting-controller-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        var environment = inherited
        environment["FSCRIPT_CONFIG_FILE"] = root.appendingPathComponent("model.json").path
        environment["FSCRIPT_CAPTURE_TEST_WAV"] = capture
        environment["FSCRIPT_SYSTEM_CAPTURE_TEST_WAV"] = source
        environment["STREAM_WORKER_LOG"] = root.appendingPathComponent("workers").path
        environment["STREAM_REQUEST_LOG"] = root.appendingPathComponent("requests").path
        let executable = URL(fileURLWithPath: binary)
        _ = try await CLIProcess().output(executable: executable, arguments: ["models", "set", "ultra", "--quiet"], environment: environment)
        return (executable, root, environment)
    }

    private func wait(_ condition: () -> Bool, timeout: TimeInterval = 10) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            guard Date() < deadline else { throw ClientError("Meeting presenter did not reach its expected state.") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    func testPropertyEveryDictationEntryPointRejectsAnActiveMeetingOrFileJob() throws {
        guard ProcessInfo.processInfo.environment["FSCRIPT_CAPTURE_TEST_WAV"] != nil else { throw XCTSkip("Run scripts/test_desktop_client.py") }
        let workspace = Workspace()
        workspace.dictation.configure(executable: URL(fileURLWithPath: "/missing/fscript"), directory: FileManager.default.temporaryDirectory)
        defer { workspace.dictation.cancel() }
        var random: UInt64 = 17
        for _ in 0..<500 {
            random = random &* 6364136223846793005 &+ 1
            workspace.meeting.state = [MeetingController.State.starting, .recording, .finishing, .replaying, .cancelling][Int(random % 5)]
            workspace.dictation.state = [.off, .ready, .failed][Int((random >> 8) % 3)]
            let before = workspace.dictation.state
            XCTAssertFalse(workspace.canDictate)
            workspace.toggleDictation()
            workspace.holdDictation(true)
            XCTAssertEqual(workspace.dictation.state, before)
            XCTAssertFalse(workspace.dictation.isRunning)
        }
        workspace.meeting.state = .off
        workspace.operation = "Transcribing"
        workspace.dictation.state = .ready
        workspace.toggleDictation(); workspace.holdDictation(true)
        XCTAssertEqual(workspace.dictation.state, .ready)
        XCTAssertFalse(workspace.dictation.isRunning)
    }

    func testRealControllerUpdatesBeforeStopThenReplaysTheSameSavedResult() async throws {
        let (binary, root, environment) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let controller = MeetingController()
        var updates = 0
        var text = ""
        var final: TranscriptSnapshot?
        var path: String?
        controller.onEvent = { message in
            if message.type == "update" { updates += 1; text = message.data.text ?? text }
        }
        controller.onCompleted = { message in final = message.data.result; path = message.data.output_path }
        controller.start(executable: binary, outputDirectory: root, environment: environment, beforeStart: {})
        try await wait { updates > 0 || controller.error != nil }
        XCTAssertNil(controller.error)
        XCTAssertTrue(controller.isActive)
        XCTAssertFalse(text.isEmpty)
        controller.stop()
        try await wait { !controller.isActive }
        XCTAssertEqual(controller.state, .completed)
        XCTAssertNotNil(final)
        XCTAssertNotEqual(final?.text, text, "Final recognition replaces the preview")
        let live = try XCTUnwrap(final)
        let document = try JSONDecoder().decode(TranscriptSnapshot.self, from: Data(contentsOf: URL(fileURLWithPath: try XCTUnwrap(path))))
        XCTAssertEqual(live, document)
        let directory = try XCTUnwrap(controller.directory)
        final = nil
        controller.start(executable: binary, outputDirectory: root, savedDirectory: directory, environment: environment, beforeStart: {})
        try await wait { !controller.isActive }
        XCTAssertEqual(controller.state, .completed)
        XCTAssertEqual(final, live)
        await controller.shutdown()
    }

    func testStopWhilePreparingDoesNotStartCaptureOrTheModel() async throws {
        let (binary, root, environment) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        let controller = MeetingController()
        controller.start(executable: binary, outputDirectory: root, environment: environment) {
            try? await Task.sleep(for: .milliseconds(100))
        }
        controller.stop()
        await controller.shutdown()
        XCTAssertEqual(controller.state, .cancelled)
        XCTAssertNil(controller.directory)
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("workers").path))
    }

    func testShutdownDuringBlockedFinalInferenceKeepsAudioAndKillsOwnedModel() async throws {
        let (binary, root, inherited) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        var environment = inherited
        environment["STREAM_HANG_WORKER"] = "1"
        let controller = MeetingController()
        var saved = false
        controller.onEvent = { message in if message.type == "saved" { saved = true } }
        controller.start(executable: binary, outputDirectory: root, environment: environment, beforeStart: {})
        try await wait { controller.state == .recording || controller.error != nil }
        XCTAssertNil(controller.error)
        try await Task.sleep(for: .milliseconds(150))
        controller.stop()
        try await wait { saved || controller.error != nil }
        try await wait { FileManager.default.fileExists(atPath: root.appendingPathComponent("requests").path) }
        let began = Date()
        await controller.shutdown()
        XCTAssertLessThan(Date().timeIntervalSince(began), 3)
        XCTAssertEqual(controller.state, .cancelled)
        let directory = try XCTUnwrap(controller.directory)
        XCTAssertTrue(FileManager.default.fileExists(atPath: directory.appendingPathComponent("microphone.pcm").path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: directory.appendingPathComponent("transcript.json").path))
        let workers = try String(contentsOf: root.appendingPathComponent("workers"))
        for entry in workers.split(separator: "\n") {
            let pid = try XCTUnwrap(Int32(entry))
            XCTAssertEqual(kill(pid, 0), -1)
            XCTAssertEqual(errno, ESRCH)
        }
    }
}
