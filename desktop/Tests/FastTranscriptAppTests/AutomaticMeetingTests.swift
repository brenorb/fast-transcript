import Darwin
import Foundation
import XCTest
import FastTranscriptClient
@testable import FastTranscriptApp

@MainActor
final class AutomaticMeetingTests: XCTestCase {
    private func fixture(blocked: Bool = false, active: Bool = true, missingTrace: Bool = false) async throws -> (Workspace, URL) {
        var environment = CLIProcess.inheritedEnvironment()
        guard let binary = environment["FSCRIPT_DESKTOP_TEST_CLI"] else { throw XCTSkip("Run scripts/test_desktop_client.py") }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("automatic-meeting-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        environment["FSCRIPT_CONFIG_FILE"] = root.appendingPathComponent("model.json").path
        environment["FSCRIPT_CALENDAR_CONFIG_FILE"] = root.appendingPathComponent("calendar.json").path
        environment["FSCRIPT_MEETING_CONFIG_FILE"] = root.appendingPathComponent("meetings.json").path
        environment["FSCRIPT_APP_DATA_DIR"] = root.appendingPathComponent("data").path
        environment["STREAM_WORKER_LOG"] = root.appendingPathComponent("workers").path
        environment["STREAM_REQUEST_LOG"] = root.appendingPathComponent("requests").path
        environment["FSCRIPT_CALLS_TEST_TRACE"] = root.appendingPathComponent(missingTrace ? "missing-calls.json" : "calls.json").path
        if blocked { environment["STREAM_HANG_WORKER"] = "1" }
        let clients: [[String: Any]] = active ? [["pid": 100, "bundle_id": "us.zoom.xos", "input": true, "output": true]] : []
        let frames: [[String: Any]] = (0..<200).map { ["at_ms": $0 * 2000, "complete": true, "clients": clients] }
        try JSONSerialization.data(withJSONObject: ["frames": frames, "interval_ms": 30, "hold": true]).write(to: root.appendingPathComponent("calls.json"))
        let executable = URL(fileURLWithPath: binary)
        _ = try await CLIProcess().output(executable: executable, arguments: ["models", "set", "ultra", "--quiet"], environment: environment)
        return (Workspace(installation: CLIInstallation(executable: executable), controlEnvironment: environment), root)
    }

    private func wait(_ condition: () -> Bool, timeout: TimeInterval = 10) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            guard Date() < deadline else { throw ClientError("Automatic meeting did not reach its expected state") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    func testBusyWorkspaceDefersAdmissionThenManualStopSuppressesTheCall() async throws {
        let (workspace, root) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.operation = "Transcribing"
        workspace.setAutomaticMeetings(true)
        try await wait { workspace.automatic.state == .watching && !workspace.savingPreference }
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertFalse(workspace.meeting.isActive)
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("workers").path))
        workspace.operation = nil
        try await wait { workspace.meeting.state == .recording || workspace.automatic.error != nil }
        XCTAssertNil(workspace.automatic.error)
        XCTAssertTrue(workspace.busy)
        XCTAssertFalse(workspace.canDictate)
        try await Task.sleep(for: .milliseconds(150))
        workspace.meeting.stop()
        try await wait { workspace.meeting.state == .completed || workspace.automatic.error != nil }
        XCTAssertNil(workspace.automatic.error)
        let completed = try XCTUnwrap(workspace.completedURL)
        let result = try JSONDecoder().decode(TranscriptSnapshot.self, from: Data(contentsOf: completed))
        XCTAssertEqual(result.text, workspace.transcript)
        XCTAssertEqual(result.meeting?.state, "stopped")
        try await Task.sleep(for: .milliseconds(200))
        XCTAssertEqual(workspace.meeting.state, .completed)
        XCTAssertEqual(workspace.automatic.state, .watching)
        XCTAssertFalse(workspace.busy)
        workspace.setAutomaticMeetings(false)
        try await wait { !workspace.savingPreference && !workspace.automatic.isRunning }
        await workspace.shutdown()
    }

    func testIdleWatcherLeavesEverydayEntryPointsAvailable() async throws {
        let (workspace, root) = try await fixture(active: false)
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.input = "./video.mp4"
        workspace.dictation.configure(executable: URL(fileURLWithPath: "/missing/helper"), directory: root)
        workspace.dictation.state = .ready
        workspace.setAutomaticMeetings(true)
        try await wait { workspace.automatic.state == .watching && !workspace.savingPreference }
        XCTAssertFalse(workspace.busy)
        XCTAssertTrue(workspace.canTranscribe)
        XCTAssertTrue(workspace.canStartMeeting)
        XCTAssertTrue(workspace.canDictate)
        await workspace.shutdown()
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("workers").path))
    }

    func testInitiallyHiddenIndicatorDoesNotWritePreferencesOrStartAnOperation() async throws {
        let (workspace, root) = try await fixture(active: false)
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.removeMeetingIndicator()
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertFalse(workspace.savingPreference)
        XCTAssertFalse(workspace.busy)
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("meetings.json").path))
        await workspace.shutdown()
    }

    func testDisablingDuringCaptureFinishesTheTranscriptBeforeStoppingTheWatcher() async throws {
        let (workspace, root) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.setAutomaticMeetings(true)
        try await wait { workspace.meeting.state == .recording || workspace.automatic.error != nil }
        XCTAssertNil(workspace.automatic.error)
        try await Task.sleep(for: .milliseconds(150))
        workspace.setAutomaticMeetings(false)
        try await wait { !workspace.automatic.isRunning && !workspace.savingPreference }
        XCTAssertNil(workspace.automatic.error)
        XCTAssertFalse(workspace.automaticMeetingsEnabled)
        XCTAssertEqual(workspace.meeting.state, .completed)
        XCTAssertNotNil(workspace.completedURL)
        await workspace.shutdown()
    }

    func testCancelThenActivationRefreshCannotRecaptureTheContinuingCall() async throws {
        let (workspace, root) = try await fixture()
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.setAutomaticMeetings(true)
        try await wait { workspace.meeting.state == .recording || workspace.automatic.error != nil }
        XCTAssertNil(workspace.automatic.error)
        workspace.meeting.cancel()
        try await wait { !workspace.automatic.isRunning && !workspace.savingPreference }
        XCTAssertFalse(workspace.automaticMeetingsEnabled)
        await workspace.refreshAutomaticMeetings()
        await workspace.refreshCalendar()
        try await Task.sleep(for: .milliseconds(300))
        XCTAssertFalse(workspace.automatic.isRunning)
        XCTAssertFalse(workspace.meeting.isActive)
        let directories = (try? FileManager.default.contentsOfDirectory(at: root.appendingPathComponent("data/meetings"), includingPropertiesForKeys: nil)) ?? []
        XCTAssertEqual(directories.count, 1)
        await workspace.shutdown()
    }

    func testFailedWatcherRemainsVisibleAndRefreshDoesNotRestartIt() async throws {
        let (workspace, root) = try await fixture(missingTrace: true)
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.setAutomaticMeetings(true)
        try await wait { workspace.automatic.state == .failed && !workspace.savingPreference }
        XCTAssertNotNil(workspace.automatic.error)
        XCTAssertTrue(workspace.hasMeetingIndicator)
        await workspace.refreshAutomaticMeetings()
        try await Task.sleep(for: .milliseconds(100))
        XCTAssertEqual(workspace.automatic.state, .failed)
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("workers").path))
        workspace.setAutomaticMeetings(false)
        try await wait { !workspace.savingPreference }
        XCTAssertFalse(workspace.hasMeetingIndicator)
        await workspace.shutdown()
    }

    func testQuitDuringConfirmedBlockedInferenceKillsTheWholeWatchJob() async throws {
        let (workspace, root) = try await fixture(blocked: true)
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.setAutomaticMeetings(true)
        try await wait { FileManager.default.fileExists(atPath: root.appendingPathComponent("requests").path) }
        let pids = try String(contentsOf: root.appendingPathComponent("workers")).split(separator: "\n").compactMap { Int32($0) }
        XCTAssertFalse(pids.isEmpty)
        let began = Date()
        await workspace.shutdown()
        XCTAssertLessThan(Date().timeIntervalSince(began), 3)
        XCTAssertFalse(workspace.automatic.isRunning)
        for pid in pids { XCTAssertEqual(kill(pid, 0), -1); XCTAssertEqual(errno, ESRCH) }
        let saved = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("meetings.json"))) as! [String: Any]
        XCTAssertEqual(saved["enabled"] as? Bool, true, "Quit must preserve the explicit startup preference")
    }
}
