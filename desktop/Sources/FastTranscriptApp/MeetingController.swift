import FastTranscriptClient
import Foundation
import Observation

/// Owns a presentation session. All capture, recovery, models and files belong
/// to the CLI supervisor; this object survives main-window dismissal.
@MainActor @Observable
final class MeetingController {
    enum State { case off, starting, recording, finishing, replaying, cancelling, completed, cancelled, failed }
    var state = State.off
    var error: String?
    var status = "Ready"
    var directory: URL?
    var manifest: MeetingManifest?
    var coverageChanged = false
    var recognitionWarning: String?
    @ObservationIgnored var onEvent: ((MeetingEvent) -> Void)?
    @ObservationIgnored var onCompleted: ((MeetingEvent) -> Void)?
    @ObservationIgnored var onFinished: (() -> Void)?
    @ObservationIgnored private var process: CLIProcess?
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var session = UUID()
    @ObservationIgnored private var savedMode = false
    @ObservationIgnored private var controlStarted = false
    @ObservationIgnored private var stopRequested = false
    @ObservationIgnored private var cancelRequested = false
    @ObservationIgnored private var inputError: String?

    var isActive: Bool { [.starting, .recording, .finishing, .replaying, .cancelling].contains(state) }
    var canStop: Bool { !savedMode && [.starting, .recording].contains(state) }
    var indicatorTitle: String {
        switch state {
        case .starting: return "Fast Transcript is preparing a meeting"
        case .recording: return "Fast Transcript is recording a meeting"
        case .replaying: return "Fast Transcript is transcribing saved meeting audio"
        default: return "Fast Transcript is finishing a meeting"
        }
    }

    func start(executable: URL, outputDirectory: URL, savedDirectory: URL? = nil,
               environment: [String: String]? = nil, beforeStart: @escaping @MainActor () async -> Void) {
        guard !isActive else { return }
        let token = UUID()
        session = token
        savedMode = savedDirectory != nil
        state = savedMode ? .replaying : .starting
        status = savedMode ? "Transcribing saved meeting…" : "Preparing meeting…"
        directory = savedDirectory
        manifest = nil
        coverageChanged = false
        recognitionWarning = nil
        error = nil
        inputError = nil
        stopRequested = false
        cancelRequested = false
        controlStarted = false
        let process = CLIProcess()
        self.process = process
        task = Task {
            defer {
                if session == token {
                    self.process = nil
                    task = nil
                    onFinished?()
                }
            }
            await beforeStart()
            guard !Task.isCancelled, !cancelRequested else { state = .cancelled; status = "Stopped before recording"; return }
            let arguments: [String]
            if let savedDirectory { arguments = ["meeting", "transcribe", savedDirectory.path, "--json", "--quiet"] }
            else { arguments = ["meeting", "--control", "--json", "--output-dir", outputDirectory.path, "--quiet"] }
            var decoder = MeetingStreamDecoder(mode: savedMode ? .saved : .live)
            var diagnostics = Data()
            var exitStatus: Int32?
            var completedEvent: MeetingEvent?
            do {
                for try await event in process.events(executable: executable, arguments: arguments, environment: environment, controlInput: !savedMode) {
                    switch event {
                    case .stdout(let bytes):
                        for message in try decoder.append(bytes) {
                            switch message.type {
                            case "ready" where !savedMode:
                                if stopRequested || cancelRequested { process.cancel() }
                                else {
                                    controlStarted = true
                                    try await process.writeInput(Data("{\"action\":\"start\"}\n".utf8))
                                }
                            case "started":
                                directory = message.data.directory.map { URL(fileURLWithPath: $0) }
                                if !savedMode { status = stopRequested ? "Stopping capture…" : "Opening microphone and system audio…" }
                            case "recording":
                                state = stopRequested ? .finishing : .recording
                                status = stopRequested ? "Stopping capture…" : "Recording meeting · microphone and system audio"
                            case "source_status":
                                if message.data.state == "reconnecting" { coverageChanged = true }
                            case "recognition_error":
                                recognitionWarning = message.data.recording_continues == true
                                    ? "Recognition failed. Meeting audio is still being saved."
                                    : "Recognition failed. Saved audio can be transcribed again."
                            case "saved":
                                manifest = message.data.manifest
                                coverageChanged = coverageChanged || manifest?.hasCoverageChanges == true
                                state = .finishing
                                status = "Audio saved · finishing transcript…"
                            case "restored":
                                manifest = message.data.manifest
                                coverageChanged = manifest?.hasCoverageChanges == true
                            case "completed":
                                completedEvent = message
                                manifest = message.data.result?.meeting
                                coverageChanged = coverageChanged || manifest?.hasCoverageChanges == true
                            case "cancelled":
                                state = .cancelling
                            default: break
                            }
                            onEvent?(message)
                        }
                    case .stderr(let bytes):
                        diagnostics.append(bytes)
                        if diagnostics.count > 8192 { diagnostics = Data(diagnostics.suffix(8192)) }
                    case .exited(let status): exitStatus = status
                    }
                }
                try decoder.finish(exitStatus: exitStatus ?? 1)
                if decoder.completed, let completedEvent {
                    state = .completed
                    status = coverageChanged ? "Meeting saved · review audio coverage" : "Meeting complete · saved locally"
                    onCompleted?(completedEvent)
                } else {
                    state = .cancelled
                    status = directory == nil ? "Stopped before recording" : "Cancelled · saved audio retained"
                }
            } catch {
                process.cancel()
                if cancelRequested || stopRequested && !controlStarted {
                    state = .cancelled
                    status = directory == nil ? "Stopped before recording" : "Cancelled · saved audio retained"
                } else {
                    let detail = String(decoding: diagnostics, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    self.error = inputError ?? decoder.errorMessage ?? (detail.isEmpty ? error.localizedDescription : detail)
                    state = .failed
                    status = directory == nil ? "Meeting could not start" : "Meeting stopped · saved audio retained"
                }
            }
        }
    }

    func clearPresentation() {
        guard !isActive else { return }
        state = .off; error = nil; directory = nil; manifest = nil
        coverageChanged = false; recognitionWarning = nil
    }

    func stop() {
        guard canStop else { return }
        stopRequested = true
        state = .finishing
        status = "Stopping capture…"
        if controlStarted { send("stop") }
        else { cancelRequested = true; process?.cancel() }
    }

    func cancel() {
        guard isActive else { return }
        cancelRequested = true
        state = .cancelling
        status = "Cancelling · keeping saved audio…"
        // The supervisor bounds termination even if native setup/inference blocks.
        process?.cancel()
    }

    private func send(_ action: String) {
        guard let process else { return }
        let token = session
        Task {
            do { try await process.writeInput(Data("{\"action\":\"\(action)\"}\n".utf8)) }
            catch {
                guard session == token, isActive, !cancelRequested else { return }
                inputError = error.localizedDescription
                process.cancel()
            }
        }
    }

    func shutdown() async {
        let pending = task
        cancel()
        await pending?.value
    }
}
