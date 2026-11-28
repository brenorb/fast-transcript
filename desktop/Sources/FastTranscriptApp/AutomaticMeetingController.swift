import FastTranscriptClient
import Foundation
import Observation

/// Owns one supervised watcher. Rust owns call timing, episodes and capture.
@MainActor @Observable
final class AutomaticMeetingController {
    enum State { case off, starting, watching, recording, stopping, failed }
    var state = State.off
    var status = "Automatic meetings off"
    var error: String?
    var episode: UInt64?
    @ObservationIgnored var admit: ((UInt64, [String]) async -> Bool)?
    @ObservationIgnored var onMeeting: ((MeetingEvent) -> Void)?
    @ObservationIgnored var onSessionExit: (() -> Void)?
    @ObservationIgnored var onReservationExpired: (() -> Void)?
    @ObservationIgnored var onEnded: ((String?) -> Void)?
    @ObservationIgnored private var process: CLIProcess?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var generation = UUID()
    @ObservationIgnored private var requestedStop = false
    @ObservationIgnored private var accepted = false
    @ObservationIgnored private var pendingSuppressions = 0
    @ObservationIgnored private var transportError: String?

    var isRunning: Bool { ![.off, .failed].contains(state) }

    func start(executable: URL, outputDirectory: URL, environment: [String: String]? = nil) {
        guard !isRunning else { return }
        generation = UUID()
        let token = generation
        state = .starting; status = "Preparing automatic meetings…"; error = nil
        requestedStop = false; accepted = false; pendingSuppressions = 0; episode = nil; transportError = nil
        let process = CLIProcess()
        self.process = process
        task = Task {
            var decoder = MeetingWatchStreamDecoder()
            var diagnostics = Data()
            var exitStatus: Int32?
            var failure: String?
            defer {
                if generation == token {
                    self.process = nil; task = nil; episode = nil; accepted = false
                    error = failure
                    state = failure == nil ? .off : .failed
                    status = failure == nil ? "Automatic meetings off" : "Automatic meetings stopped · turn off and on to retry"
                    onEnded?(failure)
                }
            }
            do {
                for try await event in process.events(executable: executable,
                    arguments: ["meeting", "watch", "--control", "--json", "--quiet", "--output-dir", outputDirectory.path],
                    environment: environment, controlInput: true) {
                    switch event {
                    case .stdout(let bytes):
                        for message in try decoder.append(bytes) {
                            switch message.type {
                            case "watch_ready":
                                if !requestedStop {
                                    state = .watching; status = "Watching for calls · no audio recorded"
                                    for _ in 0..<pendingSuppressions { send("suppress") }
                                }
                            case "offer":
                                guard !requestedStop, pendingSuppressions == 0, episode == nil,
                                      let value = message.data.episode else { continue }
                                episode = value
                                let reserved = await admit?(value, message.data.apps ?? []) ?? false
                                guard generation == token else { return }
                                if reserved && !requestedStop && pendingSuppressions == 0 {
                                    try await process.writeInput(Self.request("accept", episode: value))
                                } else {
                                    episode = nil
                                    if reserved { onReservationExpired?() }
                                }
                            case "accepted":
                                guard episode == message.data.episode else { throw ClientError("An unreserved automatic meeting was accepted.") }
                                accepted = true; state = .recording; status = "Automatic meeting active"
                            case "offer_expired":
                                if episode == message.data.episode && !accepted {
                                    episode = nil; onReservationExpired?()
                                    if !requestedStop { state = .watching }
                                }
                            case "meeting":
                                if let inner = message.data.event { onMeeting?(inner) }
                            case "session_exit":
                                onSessionExit?()
                                episode = nil; accepted = false
                                if requestedStop { process.cancel() }
                                else { state = .watching; status = "Watching for calls · no audio recorded" }
                            case "suppressed": pendingSuppressions = max(0, pendingSuppressions - 1)
                            default: break
                            }
                        }
                    case .stderr(let bytes):
                        diagnostics.append(bytes)
                        if diagnostics.count > 8192 { diagnostics = Data(diagnostics.suffix(8192)) }
                    case .exited(let status): exitStatus = status
                    }
                }
                if !requestedStop { try decoder.finish(exitStatus: exitStatus ?? 1) }
                if let transportError { throw ClientError(transportError) }
            } catch {
                process.cancel()
                if !requestedStop || transportError != nil {
                    let detail = String(decoding: diagnostics, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    failure = transportError ?? (detail.isEmpty ? error.localizedDescription : detail)
                }
            }
        }
    }

    private static func request(_ action: String, episode: UInt64? = nil) -> Data {
        var request: [String: Any] = ["action": action]
        if let episode { request["episode"] = episode }
        return (try! JSONSerialization.data(withJSONObject: request)) + Data([10])
    }

    private func send(_ action: String, episode: UInt64? = nil) {
        guard let process else { return }
        let token = generation
        Task {
            do { try await process.writeInput(Self.request(action, episode: episode)) }
            catch {
                guard generation == token else { return }
                if !requestedStop { transportError = error.localizedDescription }
                process.cancel()
            }
        }
    }

    func suppress() {
        guard isRunning, !requestedStop else { return }
        pendingSuppressions += 1
        if state != .starting { send("suppress") }
    }

    func stopCurrent() {
        guard let episode, !requestedStop else { return }
        if accepted { send("stop", episode: episode) }
        else { cancel() }
    }

    /// Disabling finishes an owned recording; Quit/Cancel use immediate shutdown.
    func stopWatching() {
        guard isRunning else { return }
        requestedStop = true; state = .stopping; status = "Finishing automatic meetings…"
        if let episode, accepted { send("stop", episode: episode) }
        else { process?.cancel() }
    }

    func cancel() {
        guard isRunning else { return }
        requestedStop = true; state = .stopping
        process?.cancel()
    }

    func shutdown() async {
        let pending = task
        cancel()
        await pending?.value
    }
}
