import Foundation
import XCTest
import FastTranscriptClient
@testable import FastTranscriptApp

@MainActor
final class CalendarSettingsTests: XCTestCase {
    private func setup(blocked: Bool) throws -> (Workspace, URL) {
        var environment = CLIProcess.inheritedEnvironment()
        guard let binary = environment["FSCRIPT_DESKTOP_TEST_CLI"] else { throw XCTSkip("Run scripts/test_desktop_client.py") }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("calendar-settings-" + UUID().uuidString)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let fixture: [String: Any] = ["authorization": blocked ? "not_determined" : "full_access",
                                      "request_authorization": "full_access", "request_delay_ms": blocked ? 60_000 : 0]
        try JSONSerialization.data(withJSONObject: fixture).write(to: root.appendingPathComponent("fixture.json"))
        environment["FSCRIPT_CALENDAR_TEST_JSON"] = root.appendingPathComponent("fixture.json").path
        environment["FSCRIPT_CALENDAR_CONFIG_FILE"] = root.appendingPathComponent("preferences.json").path
        environment["FSCRIPT_CALENDAR_TEST_REQUEST_LOG"] = root.appendingPathComponent("requested").path
        let workspace = Workspace(installation: CLIInstallation(executable: URL(fileURLWithPath: binary)), controlEnvironment: environment)
        return (workspace, root)
    }

    private func wait(_ condition: () -> Bool) async throws {
        let deadline = Date().addingTimeInterval(8)
        while !condition() {
            guard Date() < deadline else { throw ClientError("Calendar settings did not reach the expected state") }
            try await Task.sleep(for: .milliseconds(10))
        }
    }

    func testSettingsUseSharedCLIAndDoNotRequestAlreadyGrantedAccess() async throws {
        let (workspace, root) = try setup(blocked: false)
        defer { try? FileManager.default.removeItem(at: root) }
        await workspace.refreshCalendar()
        XCTAssertEqual(workspace.calendar?.name_meetings, false)
        workspace.setCalendarNames(true)
        try await wait { !workspace.savingPreference }
        XCTAssertNil(workspace.error)
        XCTAssertEqual(workspace.calendar?.name_meetings, true)
        let saved = try JSONSerialization.jsonObject(with: Data(contentsOf: root.appendingPathComponent("preferences.json"))) as! [String: Any]
        XCTAssertEqual(Set(saved.keys), Set(["version", "name_meetings"]))
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("requested").path))
        workspace.setCalendarNames(false)
        try await wait { !workspace.savingPreference }
        XCTAssertEqual(workspace.calendar?.name_meetings, false)
        await workspace.shutdown()
    }

    func testQuitCancelsAConfirmedBlockedPermissionHelper() async throws {
        let (workspace, root) = try setup(blocked: true)
        defer { try? FileManager.default.removeItem(at: root) }
        workspace.setCalendarNames(true)
        try await wait { FileManager.default.fileExists(atPath: root.appendingPathComponent("requested").path) }
        let began = Date()
        await workspace.shutdown()
        XCTAssertLessThan(Date().timeIntervalSince(began), 3)
        try await wait { !workspace.savingPreference }
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("preferences.json").path))
    }
}
