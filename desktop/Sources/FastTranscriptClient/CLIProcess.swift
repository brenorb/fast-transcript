import Foundation
import Darwin

public enum ProcessEvent: Sendable {
    case stdout(Data)
    case stderr(Data)
    case exited(Int32)
}

/// Owns one CLI supervisor. Its worker owns and cleans up all model/media helpers.
public final class CLIProcess: @unchecked Sendable {
    private let lock = NSLock()
    private var pid: Int32?
    private var cancelled = false
    private var started = false

    public init() {}

    /// Finder/XCTest may launch with a minimal PATH rather than a login shell's.
    public static func inheritedEnvironment() -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        var paths = (environment["PATH"] ?? "").split(separator: ":").map(String.init)
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        for path in ["\(home)/.local/bin", "\(home)/.cargo/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"] where !paths.contains(path) {
            paths.append(path)
        }
        environment["PATH"] = paths.joined(separator: ":")
        return environment
    }

    private func claim() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        guard !started else { return false }
        started = true
        return true
    }

    public func cancel() {
        lock.lock()
        defer { lock.unlock() }
        cancelled = true
        if let pid { kill(pid, SIGTERM) }
    }

    private func launched(_ pid: Int32) {
        lock.lock()
        defer { lock.unlock() }
        self.pid = pid
        if cancelled { kill(pid, SIGTERM) }
    }

    private func exited() {
        lock.lock()
        defer { lock.unlock() }
        pid = nil
    }

    public func events(executable: URL, arguments: [String], environment: [String: String]? = nil) -> AsyncThrowingStream<ProcessEvent, Error> {
        AsyncThrowingStream { continuation in
            guard claim() else {
                continuation.finish(throwing: ClientError("A CLI process can only run once."))
                return
            }
            continuation.onTermination = { [self] termination in
                if case .cancelled = termination { cancel() }
            }
            DispatchQueue.global(qos: .userInitiated).async { [self] in
                let process = Process()
                let stdout = Pipe()
                let stderr = Pipe()
                process.executableURL = executable
                process.arguments = arguments
                process.environment = environment ?? Self.inheritedEnvironment()
                process.standardInput = FileHandle.nullDevice
                process.standardOutput = stdout
                process.standardError = stderr
                do {
                    try process.run()
                    launched(process.processIdentifier)
                } catch {
                    continuation.finish(throwing: error)
                    return
                }
                let readers = DispatchGroup()
                for (pipe, isError) in [(stdout, false), (stderr, true)] {
                    readers.enter()
                    DispatchQueue.global(qos: .userInitiated).async {
                        defer { readers.leave() }
                        do {
                            var buffer = [UInt8](repeating: 0, count: 64 * 1024)
                            while true {
                                // FileHandle.read(upToCount:) can wait to fill a short pipe.
                                // One POSIX read returns as soon as any bytes are available.
                                let count = buffer.withUnsafeMutableBytes {
                                    Darwin.read(pipe.fileHandleForReading.fileDescriptor, $0.baseAddress, $0.count)
                                }
                                if count == 0 { break }
                                if count < 0 {
                                    if errno == EINTR { continue }
                                    throw NSError(domain: NSPOSIXErrorDomain, code: Int(errno))
                                }
                                let bytes = Data(buffer.prefix(count))
                                continuation.yield(isError ? .stderr(bytes) : .stdout(bytes))
                            }
                        } catch {
                            self.cancel()
                            continuation.finish(throwing: error)
                        }
                        try? pipe.fileHandleForReading.close()
                    }
                }
                process.waitUntilExit()
                exited()
                readers.wait()
                continuation.yield(.exited(process.terminationStatus))
                continuation.finish()
            }
        }
    }

    public func output(executable: URL, arguments: [String], environment: [String: String]? = nil) async throws -> Data {
        var stdout = Data()
        var stderr = Data()
        for try await event in events(executable: executable, arguments: arguments, environment: environment) {
            switch event {
            case .stdout(let data): stdout.append(data)
            case .stderr(let data):
                stderr.append(data)
                if stderr.count > 8192 { stderr = Data(stderr.suffix(8192)) }
            case .exited(let status):
                guard status == 0 else {
                    throw ClientError(String(decoding: stderr, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines).nonempty ?? "Command failed with status \(status).")
                }
            }
        }
        try Task.checkCancellation()
        return stdout
    }
}

private extension String {
    var nonempty: String? { isEmpty ? nil : self }
}
