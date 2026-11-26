import Foundation
import AppKit
import UniformTypeIdentifiers
import XCTest
@testable import FastTranscriptClient

final class CLIProcessTests: XCTestCase {
    func testControlInputIsFullDuplexAndClosesCleanly() async throws {
        let process = CLIProcess()
        defer { process.cancel() }
        let payload = Data((String(repeating: "é 🦀\n", count: 1000)).utf8)
        var output = Data()
        var sent = false
        var status: Int32?
        for try await event in process.events(executable: URL(fileURLWithPath: "/bin/sh"), arguments: ["-c", "printf ready; exec cat"], controlInput: true) {
            switch event {
            case .stdout(let bytes):
                output.append(bytes)
                if !sent {
                    sent = true
                    try await process.writeInput(payload)
                    process.closeInput()
                }
            case .exited(let code): status = code
            default: break
            }
        }
        XCTAssertEqual(status, 0)
        XCTAssertEqual(output, Data("ready".utf8) + payload)
        do { try await process.writeInput(Data("late".utf8)); XCTFail("Closed input must reject writes") }
        catch {}
    }

    func testBrokenControlPipeThrowsWithoutTerminatingTheApp() async throws {
        let process = CLIProcess()
        defer { process.cancel() }
        var rejected = false
        for try await event in process.events(executable: URL(fileURLWithPath: "/bin/sh"), arguments: ["-c", "exec 0<&-; printf ready; sleep 0.05"], controlInput: true) {
            if case .stdout = event {
                do { try await process.writeInput(Data("request\n".utf8)); XCTFail("Broken pipe must fail") }
                catch { rejected = true }
            }
        }
        XCTAssertTrue(rejected)
    }

    func testWarmRecordingControllerMatchesBatchAcrossTwoNativeClientClips() async throws {
        let (binary, _, _) = try fixture()
        let environment = ProcessInfo.processInfo.environment
        guard let capture = environment["FSCRIPT_DESKTOP_TEST_CAPTURE"], let batch = environment["FSCRIPT_DESKTOP_TEST_RECORDING_BATCH"] else { throw XCTSkip("Run scripts/test_desktop_client.py") }
        let expected = try JSONDecoder().decode(TranscriptSnapshot.self, from: Data(contentsOf: URL(fileURLWithPath: batch)))
        var childEnvironment = CLIProcess.inheritedEnvironment()
        childEnvironment["FSCRIPT_CAPTURE_TEST_WAV"] = capture
        let log = try XCTUnwrap(childEnvironment["STREAM_WORKER_LOG"])
        let before = try String(contentsOfFile: log).split(separator: "\n").count
        let process = CLIProcess()
        defer { process.cancel() }
        let timeout = Task { try? await Task.sleep(for: .seconds(10)); if !Task.isCancelled { process.cancel() } }
        defer { timeout.cancel() }
        var decoder = RecordingStreamDecoder()
        var clips = 0
        var status: Int32?
        for try await event in process.events(executable: binary, arguments: ["record", "--control", "--model", "ultra", "--quiet"], environment: childEnvironment, controlInput: true) {
            switch event {
            case .stdout(let bytes):
                for message in try decoder.append(bytes) {
                    switch message.type {
                    case "ready": try await process.writeInput(Data("{\"action\":\"start\"}\n".utf8))
                    case "recording":
                        try await Task.sleep(for: .milliseconds(1200))
                        try await process.writeInput(Data("{\"action\":\"stop\"}\n".utf8))
                    case "completed":
                        XCTAssertEqual(message.data.result, expected)
                        XCTAssertEqual(message.data.model_reused, clips > 0)
                        clips += 1
                        let action = clips == 2 ? "shutdown" : "start"
                        try await process.writeInput(Data("{\"action\":\"\(action)\"}\n".utf8))
                    case "error": XCTFail(message.data.message ?? "Recording error")
                    default: break
                    }
                }
            case .exited(let code): status = code
            default: break
            }
        }
        try decoder.finish(exitStatus: XCTUnwrap(status))
        XCTAssertEqual(clips, 2)
        XCTAssertEqual(try String(contentsOfFile: log).split(separator: "\n").count - before, 1)
    }

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

    func testNativeFileDropProviderFeedsTheSharedCLI() async throws {
        let (binary, source, batch) = try fixture()
        let provider = NSItemProvider(item: URL(fileURLWithPath: source) as NSURL, typeIdentifier: UTType.fileURL.identifier)
        let decoded: String = try await withCheckedThrowingContinuation { continuation in
            provider.loadItem(forTypeIdentifier: UTType.fileURL.identifier) { item, error in
                if let error { continuation.resume(throwing: error) }
                else if let path = droppedMediaSource(item, typeIdentifier: UTType.fileURL.identifier) { continuation.resume(returning: path) }
                else { continuation.resume(throwing: ClientError("Native drop did not supply a file path.")) }
            }
        }
        XCTAssertEqual(decoded, source)
        let result = try await CLIProcess().output(executable: binary, arguments: [decoded, "--model", "ultra", "--json", "--stdout", "--no-diarization", "--quiet", "--chunk", "2.5", "--overlap", "0.5"])
        XCTAssertEqual(try JSONDecoder().decode(TranscriptSnapshot.self, from: result), try JSONDecoder().decode(TranscriptSnapshot.self, from: Data(contentsOf: batch)))
    }
}
