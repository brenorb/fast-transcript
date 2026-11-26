import Foundation
import XCTest
@testable import FastTranscriptClient

final class CLIProcessTests: XCTestCase {
    func testArgumentsAreLiteralAndDiagnosticsRemainSeparate() async throws {
        let result = try await CLIProcess().output(executable: URL(fileURLWithPath: "/usr/bin/printf"), arguments: ["%s|", "a b", "$(echo bad)", "", "🦀"])
        XCTAssertEqual(String(decoding: result, as: UTF8.self), "a b|$(echo bad)||🦀|")
        do {
            _ = try await CLIProcess().output(executable: URL(fileURLWithPath: "/bin/sh"), arguments: ["-c", "printf diagnostic >&2; exit 3"])
            XCTFail("Nonzero exit must fail")
        } catch {
            XCTAssertEqual(error.localizedDescription, "diagnostic")
        }
    }

    func testStdoutArrivesWhileProducerIsBlockedBeforeEOF() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: directory) }
        let gate = directory.appendingPathComponent("continue")
        let process = CLIProcess()
        let timeout = Task { try? await Task.sleep(for: .seconds(3)); if !Task.isCancelled { process.cancel() } }
        defer { timeout.cancel() }
        var received = Data()
        var status: Int32?
        for try await event in process.events(executable: URL(fileURLWithPath: "/bin/sh"), arguments: ["-c", "printf early; while [ ! -f \"$1\" ]; do sleep 0.01; done; printf late", "fixture", gate.path]) {
            switch event {
            case .stdout(let bytes):
                if received.isEmpty {
                    XCTAssertEqual(String(decoding: bytes, as: UTF8.self), "early")
                    try Data().write(to: gate)
                }
                received.append(bytes)
            case .exited(let code): status = code
            default: break
            }
        }
        XCTAssertEqual(status, 0)
        XCTAssertEqual(String(decoding: received, as: UTF8.self), "earlylate")
    }

    func testCancellationEndsOwnedProcess() async throws {
        let process = CLIProcess()
        var status: Int32?
        for try await event in process.events(executable: URL(fileURLWithPath: "/bin/sh"), arguments: ["-c", "printf ready; exec sleep 30"]) {
            switch event {
            case .stdout: process.cancel()
            case .exited(let code): status = code
            default: break
            }
        }
        XCTAssertNotEqual(status, 0)
    }

    private func fixture() throws -> (URL, String, URL) {
        let env = ProcessInfo.processInfo.environment
        guard let binary = env["FSCRIPT_DESKTOP_TEST_CLI"], let source = env["FSCRIPT_DESKTOP_TEST_SOURCE"], let batch = env["FSCRIPT_DESKTOP_TEST_BATCH"] else {
            throw XCTSkip("Run python3 scripts/test_desktop_client.py for real-CLI differential checks.")
        }
        return (URL(fileURLWithPath: binary), source, URL(fileURLWithPath: batch))
    }

    func testRealCLIStreamMatchesBatchDocument() async throws {
        let (binary, source, batch) = try fixture()
        let expected = try JSONDecoder().decode(TranscriptSnapshot.self, from: Data(contentsOf: batch))
        let process = CLIProcess()
        defer { process.cancel() }
        var decoder = TranscriptStreamDecoder()
        var result: TranscriptSnapshot?
        var status: Int32?
        var updates = 0
        for try await event in process.events(executable: binary, arguments: [source, "--model", "ultra", "--stream", "--json", "--quiet", "--chunk", "2.5", "--overlap", "0.5"]) {
            switch event {
            case .stdout(let bytes):
                for message in try decoder.append(bytes) {
                    if message.type == "update" { updates += 1 }
                    if message.type == "completed" { result = message.data.result }
                }
            case .exited(let code): status = code
            default: break
            }
        }
        try decoder.finish(exitStatus: XCTUnwrap(status))
        XCTAssertGreaterThan(updates, 0)
        XCTAssertEqual(result, expected)
    }

    func testCancelRealCLIJobBeforeFirstInferenceFinishes() async throws {
        let (binary, source, _) = try fixture()
        let process = CLIProcess()
        defer { process.cancel() }
        var environment = CLIProcess.inheritedEnvironment()
        environment["STREAM_HANG_WORKER"] = "1"
        var decoder = TranscriptStreamDecoder()
        var status: Int32?
        for try await event in process.events(executable: binary, arguments: [source, "--model", "ultra", "--stream", "--json", "--quiet"], environment: environment) {
            switch event {
            case .stdout(let bytes):
                for message in try decoder.append(bytes) {
                    if message.type == "ready" { process.cancel() }
                }
            case .exited(let code): status = code
            default: break
            }
        }
        XCTAssertEqual(status, 130)
        XCTAssertFalse(decoder.completed)
        XCTAssertThrowsError(try decoder.finish(exitStatus: 130))
    }
}
