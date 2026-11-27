// Opt-in macOS acceptance fixture. Launch from the app/terminal holding the grant.
// It never requests access, activates an app, captures audio, or simulates keys.
// Only the real fscript export --paste action delivers to the exact foreground target.
import AppKit
import Foundation

struct PasteSpec: Decodable {
    let targetPid: Int32
    let observations: String
    let report: String
    let cases: [PasteCase]
}

struct PasteCase: Decodable {
    let name: String
    let binary: String
    let transcript: String
    let expectedInsertion: Bool?
}

struct PasteObservation: Decodable {
    let type: String
    let pid: Int32
    let text: String
}

@MainActor
final class PasteCheck {
    let spec: PasteSpec
    let workspace = NSWorkspace.shared
    var expected: [String] = []
    var results: [[String: Any]] = []
    var index = 0
    var busy = false
    var failure: String?
    var observation: NSKeyValueObservation?

    init(_ spec: PasteSpec) { self.spec = spec }

    func output(_ binary: String, _ arguments: [String]) throws -> Data {
        let process = Process()
        let pipe = Pipe()
        process.executableURL = URL(fileURLWithPath: binary)
        process.arguments = arguments
        process.standardOutput = pipe
        try process.run()
        let bytes = pipe.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        guard process.terminationStatus == 0 else { throw NSError(domain: "fscript command failed", code: Int(process.terminationStatus)) }
        return bytes
    }

    func observations() -> [PasteObservation] {
        guard let bytes = try? Data(contentsOf: URL(fileURLWithPath: spec.observations)) else { return [] }
        return bytes.split(separator: 10).compactMap { try? JSONDecoder().decode(PasteObservation.self, from: Data($0)) }
            .filter { $0.pid == spec.targetPid }
    }

    func save() {
        let finished = index == spec.cases.count && failure == nil
        let passed = finished && results.allSatisfy { ($0["passed"] as? Bool) == true }
        let report: [String: Any] = ["version": 1, "finished": finished, "passed": passed, "error": failure.map { $0 as Any } ?? NSNull(), "cases": results]
        do {
            let bytes = try JSONSerialization.data(withJSONObject: report, options: [.prettyPrinted, .sortedKeys])
            try bytes.write(to: URL(fileURLWithPath: spec.report), options: .atomic)
        } catch { failure = "Cannot save acceptance report: \(error.localizedDescription)" }
    }

    func check() {
        guard !busy, index < spec.cases.count, failure == nil,
              let foreground = workspace.frontmostApplication,
              foreground.processIdentifier == spec.targetPid,
              foreground.bundleIdentifier == "com.brenorb.fast-transcript.paste-test" else { return }
        busy = true
        defer { busy = false }
        let item = spec.cases[index]
        let before = observations()
        let prior = before.last?.text ?? ""
        let beforePastes = before.filter { $0.type == "paste" }.count
        do {
            let process = Process()
            process.executableURL = URL(fileURLWithPath: item.binary)
            process.arguments = ["export", item.transcript, "--format", "compact", "--paste", "--target-pid", String(spec.targetPid), "--quiet"]
            var environment = ProcessInfo.processInfo.environment
            environment.removeValue(forKey: "FSCRIPT_INTERNAL_STREAM_JOB")
            process.environment = environment
            // No stdout backpressure or caller-imposed sender lifetime.
            process.standardOutput = FileHandle.nullDevice
            try process.run()
            process.waitUntilExit()
            let deadline = Date(timeIntervalSinceNow: 1)
            while Date() < deadline && observations().filter({ $0.type == "paste" }).count == beforePastes {
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
            }
            let after = observations()
            let callbacks = after.filter { $0.type == "paste" }.count - beforePastes
            let inserted = callbacks == 1 && after.last?.text == prior + expected[index]
            let foregroundAfter = workspace.frontmostApplication?.processIdentifier ?? -1
            let unchanged = callbacks == 0 && after.last?.text == prior && foregroundAfter == spec.targetPid
            let matched = item.expectedInsertion.map { $0 ? inserted : unchanged } ?? true
            let passed = process.terminationStatus == 0 && matched
            results.append(["name": item.name, "binary": item.binary, "cli_pid": process.processIdentifier, "foreground_before": spec.targetPid,
                            "foreground_after": foregroundAfter, "exit": process.terminationStatus, "paste_callback": callbacks > 0, "paste_callbacks": callbacks,
                            "exact_insertion": inserted, "expected_insertion": item.expectedInsertion.map { $0 as Any } ?? NSNull(), "passed": passed])
            index += 1
            save()
        } catch { failure = error.localizedDescription; save() }
    }

    func run() -> Bool {
        do {
            guard spec.targetPid > 0, !spec.cases.isEmpty else { throw NSError(domain: "Invalid paste specification", code: 1) }
            guard spec.cases.contains(where: { $0.expectedInsertion == true }) else {
                throw NSError(domain: "Native paste acceptance requires at least one asserted positive case", code: 1)
            }
            guard observations().contains(where: { $0.type == "ready" }) else {
                throw NSError(domain: "Native paste receiver is not ready for this target PID", code: 1)
            }
            for item in spec.cases {
                let capabilities = try JSONSerialization.jsonObject(with: output(item.binary, ["permissions", "--json", "--quiet"])) as? [String: Any]
                guard (capabilities?["paste"] as? [String: Any])?["granted"] as? Bool == true else {
                    throw NSError(domain: "Paste access unavailable for this execution path; no access was requested", code: 1)
                }
                var text = String(decoding: try output(item.binary, ["export", item.transcript, "--format", "compact", "--stdout", "--quiet"]), as: UTF8.self)
                if text.hasSuffix("\n") { text.removeLast() }
                guard !text.isEmpty else { throw NSError(domain: "Empty test payload", code: 1) }
                expected.append(text)
            }
            observation = workspace.observe(\.frontmostApplication, options: [.initial, .new]) { [weak self] _, _ in
                DispatchQueue.main.async { self?.check() }
            }
            let deadline = Date(timeIntervalSinceNow: 90)
            while index < spec.cases.count && failure == nil && Date() < deadline {
                RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.01))
                check()
            }
            if index < spec.cases.count && failure == nil { failure = "Foreground acceptance deadline reached; remaining cases were not executed" }
        } catch { failure = error.localizedDescription }
        save()
        if let report = try? String(contentsOfFile: spec.report, encoding: .utf8) { print(report) }
        return failure == nil && index == spec.cases.count && results.allSatisfy { ($0["passed"] as? Bool) == true }
    }
}

MainActor.assumeIsolated {
    guard CommandLine.arguments.count == 2 else {
        fputs("Usage: native-paste-check SPEC.json\n", stderr)
        exit(2)
    }
    do {
        let spec = try JSONDecoder().decode(PasteSpec.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
        NSApplication.shared.setActivationPolicy(.prohibited)
        let check = PasteCheck(spec)
        let passed = withExtendedLifetime(check) { check.run() }
        exit(passed ? 0 : 1)
    } catch { fputs("\(error)\n", stderr); exit(1) }
}
