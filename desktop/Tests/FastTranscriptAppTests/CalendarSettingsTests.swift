import Foundation
import XCTest
import FastTranscriptClient
@testable import FastTranscriptApp

@MainActor
final class CalendarSettingsTests: XCTestCase {
    private struct Generator: RandomNumberGenerator {
        var state: UInt64 = 247
        mutating func next() -> UInt64 {
            state = state &* 6364136223846793005 &+ 1442695040888963407
            return state
        }
    }
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

    func testInventoryPreservesEveryCalendarAcrossSourcesAndDoesNotEnableNaming() async throws {
        let (workspace, root) = try setup(blocked: false)
        defer { try? FileManager.default.removeItem(at: root) }
        var generator = Generator()
        for iteration in 0..<16 {
            let count = iteration == 0 ? 0 : Int(generator.next() % 64) + 1
            let expected = (0..<count).map { index in
                CalendarStatus.Calendar(id: "calendar-\(iteration)-\(index)",
                    title: ["Work", "Personal", "Café ✍️", ""][index % 4], source: "Account \(index % 7)")
            }.shuffled(using: &generator)
            let fixture: [String: Any] = ["authorization": "full_access", "calendars": expected.map {
                ["id": $0.id, "title": $0.title, "source": $0.source]
            }]
            try JSONSerialization.data(withJSONObject: fixture).write(to: root.appendingPathComponent("fixture.json"))
            let result = try await workspace.readCalendarAccounts()
            XCTAssertTrue(result.queried)
            XCTAssertEqual(result.calendars.count, expected.count)
            XCTAssertEqual(Dictionary(uniqueKeysWithValues: result.calendars.map { ($0.id, $0) }),
                           Dictionary(uniqueKeysWithValues: expected.map { ($0.id, $0) }),
                           "Do not collapse repeated titles or omit a source; the CLI owns ordering")
            XCTAssertEqual(workspace.calendar?.calendars, result.calendars)
            XCTAssertFalse(result.name_meetings)
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("requested").path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("preferences.json").path))
        await workspace.shutdown()
    }

    func testInventoryWithoutAccessIsNotAnEmptySuccessfulQueryAndNeverPrompts() async throws {
        let (workspace, root) = try setup(blocked: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let result = try await workspace.readCalendarAccounts()
        XCTAssertEqual(result.authorization, "not_determined")
        XCTAssertFalse(result.queried)
        XCTAssertTrue(result.calendars.isEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("requested").path))
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("preferences.json").path))
        await workspace.shutdown()
    }

    func testInventoryRefreshDoesNotKeepOldCalendarsAfterRevocation() async throws {
        let (workspace, root) = try setup(blocked: false)
        defer { try? FileManager.default.removeItem(at: root) }
        try Data(#"{"authorization":"full_access","calendars":[{"id":"one","title":"Work","source":"Google"}]}"#.utf8)
            .write(to: root.appendingPathComponent("fixture.json"))
        let granted = try await workspace.readCalendarAccounts()
        XCTAssertEqual(granted.calendars.count, 1)
        try Data(#"{"authorization":"denied"}"#.utf8).write(to: root.appendingPathComponent("fixture.json"))
        let denied = try await workspace.readCalendarAccounts()
        XCTAssertEqual(denied.authorization, "denied")
        XCTAssertFalse(denied.queried)
        XCTAssertEqual(workspace.calendar?.calendars, [])
        XCTAssertFalse(FileManager.default.fileExists(atPath: root.appendingPathComponent("requested").path))
        await workspace.shutdown()
    }
}
