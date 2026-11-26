import AppKit
import FastTranscriptClient
import Foundation
import Observation
import UniformTypeIdentifiers

struct ModelCatalog: Decodable {
    let version: Int
    let saved_default: String?
    let environment_override: String?
    let effective_model: String
    let models: [Model]
    struct Model: Decodable, Identifiable {
        let name: String
        let advice: String
        var id: String { name }
    }
}

struct AgentCatalog: Decodable {
    let version: Int
    let default_agent: String?
    let agents: [Agent]
    struct Agent: Decodable, Identifiable {
        let name: String
        let available: Bool
        var id: String { name }
    }
}

struct NotesResponse: Decodable { let notes: String }

struct CLIInstallation {
    let executable: URL

    static func locate() throws -> Self {
        let helpers = ProcessInfo.processInfo.environment["FSCRIPT_APP_HELPERS"].map { URL(fileURLWithPath: $0) }
            ?? Bundle.main.bundleURL.appendingPathComponent("Contents/Helpers", isDirectory: true)
        let installation = Self(executable: helpers.appendingPathComponent("fscript"))
        guard [installation.executable].allSatisfy({ FileManager.default.isExecutableFile(atPath: $0.path) }) else {
            throw ClientError("Fast Transcript's command helper is missing. Reinstall the app or rebuild it with scripts/build_macos_app.sh.")
        }
        return installation
    }
}

enum RangeMode: String, CaseIterable, Identifiable {
    case all = "Whole recording", last = "Last part", interval = "Start and end"
    var id: String { rawValue }
}

enum DocumentPane: String, CaseIterable, Identifiable {
    case transcript = "Transcript", notes = "Notes"
    var id: String { rawValue }
}

enum ExportFormat: String, CaseIterable, Identifiable {
    case text, plain, srt, vtt, json
    var id: String { rawValue }
    var title: String {
        switch self {
        case .text: return "Text with timestamps"
        case .plain: return "Plain text"
        case .srt: return "SRT subtitles"
        case .vtt: return "WebVTT subtitles"
        case .json: return "JSON transcript"
        }
    }
    var fileExtension: String { [.text, .plain].contains(self) ? "txt" : rawValue }
}

@MainActor @Observable
final class Workspace {
    var input = ""
    var rangeMode = RangeMode.all
    var rangeStart = ""
    var rangeEnd = ""
    var rangeLast = "5:00"
    var pane = DocumentPane.transcript
    var transcript = ""
    var notes = ""
    var status = "Preparing…"
    var error: String?
    var models: ModelCatalog?
    var agents: AgentCatalog?
    var loading = false
    var savingPreference = false
    var operation: String?
    var cancelRequested = false
    var completedURL: URL?
    var notesURL: URL?
    var sourceTitle = ""
    @ObservationIgnored private var installation: CLIInstallation?
    @ObservationIgnored private var activeProcess: CLIProcess?
    @ObservationIgnored private var jobTask: Task<Void, Never>?
    @ObservationIgnored private var pendingInput: String?

    var busy: Bool { operation != nil || loading || savingPreference }
    var canTranscribe: Bool { !busy && installation != nil && !input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var displayedText: String { pane == .notes ? notes : transcript }

    func bootstrap() async {
        guard !loading, installation == nil else { return }
        loading = true
        defer { loading = false }
        do {
            let installation = try CLIInstallation.locate()
            let output = try await CLIProcess().output(executable: installation.executable, arguments: ["--version"])
            let version = String(decoding: output, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            guard version.hasPrefix("fscript ") else { throw ClientError("The app's command helper is invalid. Reinstall the app.") }
            self.installation = installation
            try await refreshModels()
            await refreshAgents()
            status = "Ready"
            error = nil
        } catch {
            self.installation = nil
            self.error = error.localizedDescription
            status = "Setup needs attention"
        }
        if let pendingInput, installation != nil {
            self.pendingInput = nil
            // Start after bootstrap releases its loading state.
            Task { await Task.yield(); start(pendingInput) }
        }
    }

    private func control(_ arguments: [String]) async throws -> Data {
        guard let installation else { throw ClientError("The app is still preparing.") }
        return try await CLIProcess().output(executable: installation.executable, arguments: arguments)
    }

    private func refreshModels() async throws {
        let catalog = try JSONDecoder().decode(ModelCatalog.self, from: await control(["models", "--json"]))
        guard catalog.version == 1 else { throw ClientError("Unsupported model catalog version. Update the app.") }
        models = catalog
    }

    func refreshAgents() async {
        let catalog = try? await JSONDecoder().decode(AgentCatalog.self, from: control(["agents", "--json"]))
        agents = catalog?.version == 1 ? catalog : nil
    }

    func chooseModel(_ name: String) {
        guard !busy else { return }
        savingPreference = true
        Task {
            defer { savingPreference = false }
            do {
                _ = try await control(["models", "set", name, "--json", "--quiet"])
                try await refreshModels()
                error = nil
            } catch { self.error = error.localizedDescription }
        }
    }

    func chooseAgent(_ name: String) {
        guard !busy else { return }
        savingPreference = true
        Task {
            defer { savingPreference = false }
            do {
                _ = try await control(["agents", "set", name, "--json", "--quiet"])
                await refreshAgents()
                error = nil
            } catch { self.error = error.localizedDescription }
        }
    }

    func resetModel() {
        guard !busy else { return }
        savingPreference = true
        Task {
            defer { savingPreference = false }
            do {
                let installation = try CLIInstallation.locate()
                _ = try await CLIProcess().output(executable: installation.executable, arguments: ["models", "reset", "--quiet"])
                self.installation = installation
                try await refreshModels()
                status = "Ready"
                error = nil
            } catch { self.error = error.localizedDescription }
        }
    }

    func openFile() {
        guard !busy else { return }
        let panel = NSOpenPanel()
        panel.title = "Transcribe audio or video"
        panel.allowedContentTypes = [.audio, .movie, .data]
        panel.allowsMultipleSelection = false
        panel.canChooseDirectories = false
        panel.begin { response in
            if response == .OK, let url = panel.url { Task { @MainActor in self.start(url.path) } }
        }
    }

    func pasteLink() {
        guard !busy else { return }
        guard let text = NSPasteboard.general.string(forType: .string), !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            error = "Copy a video or audio URL first."
            return
        }
        start(text)
    }

    func open(_ url: URL) { start(url.isFileURL ? url.path : url.absoluteString) }

    func start(_ source: String? = nil) {
        guard !busy else {
            if loading, let source { pendingInput = source }
            return
        }
        if let source { input = source.trimmingCharacters(in: .whitespacesAndNewlines) }
        guard let installation else { pendingInput = input; return }
        guard canTranscribe else { return }
        let source = input.trimmingCharacters(in: .whitespacesAndNewlines)
        let process = CLIProcess()
        activeProcess = process
        operation = "Transcribing"
        cancelRequested = false
        completedURL = nil
        notesURL = nil
        transcript = ""
        notes = ""
        pane = .transcript
        sourceTitle = URL(string: source)?.lastPathComponent.nonempty ?? URL(fileURLWithPath: source).lastPathComponent
        status = "Preparing transcription…"
        error = nil
        let range = rangeArguments()
        jobTask = Task {
            defer { finishOperation() }
            do {
                let directory = jobDirectory()
                let output = directory.appendingPathComponent("transcript.json")
                let arguments = ["--stream", "--json", "--output", output.path] + range + ["--", source]
                var decoder = TranscriptStreamDecoder()
                var diagnostics = Data()
                var status: Int32?
                for try await event in process.events(executable: installation.executable, arguments: arguments) {
                    switch event {
                    case .stdout(let bytes):
                        for message in try decoder.append(bytes) {
                            switch message.type {
                            case "ready": self.status = "Transcribing…"
                            case "update": transcript = message.data.text ?? transcript
                            case "completed": transcript = message.data.result?.text ?? transcript
                            default: break
                            }
                        }
                    case .stderr(let bytes):
                        diagnostics.append(bytes)
                        if diagnostics.count > 8192 { diagnostics = Data(diagnostics.suffix(8192)) }
                    case .exited(let code): status = code
                    }
                }
                if cancelRequested { self.status = "Cancelled · partial text retained"; return }
                do { try decoder.finish(exitStatus: status ?? 1) }
                catch {
                    let details = String(decoding: diagnostics, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
                    if let message = decoder.errorMessage {
                        throw ClientError(details.isEmpty || details.contains(message) ? message : "\(message)\n\n\(details)")
                    }
                    if details.hasPrefix("error: ") { throw ClientError(String(details.dropFirst(7))) }
                    throw ClientError(details.isEmpty ? error.localizedDescription : details)
                }
                completedURL = output
                self.status = "Complete · saved locally"
            } catch {
                process.cancel()
                if cancelRequested { status = "Cancelled · partial text retained" }
                else { self.error = error.localizedDescription; status = "Transcription failed" }
            }
        }
    }

    private func rangeArguments() -> [String] {
        switch rangeMode {
        case .all: return []
        case .last: return ["--last", rangeLast]
        case .interval:
            var result: [String] = []
            if !rangeStart.isEmpty { result += ["--start", rangeStart] }
            if !rangeEnd.isEmpty { result += ["--end", rangeEnd] }
            return result
        }
    }

    func cancel() { cancelRequested = true; status = "Stopping…"; activeProcess?.cancel() }

    private func finishOperation() { activeProcess = nil; jobTask = nil; operation = nil }

    private func jobDirectory() -> URL {
        let root = ProcessInfo.processInfo.environment["FSCRIPT_APP_DATA_DIR"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("fast-transcript/desktop", isDirectory: true)
        // Rust creates the destination and writes the completed document atomically.
        return root.appendingPathComponent(UUID().uuidString, isDirectory: true)
    }

    func generateNotes() {
        guard !busy, let completedURL, let installation else { return }
        let process = CLIProcess()
        activeProcess = process
        operation = "Creating notes"
        cancelRequested = false
        status = "Creating notes…"
        error = nil
        jobTask = Task {
            defer { finishOperation() }
            do {
                let output = completedURL.deletingLastPathComponent().appendingPathComponent("notes.md")
                let data = try await process.output(executable: installation.executable, arguments: ["notes", completedURL.path, "--output", output.path, "--json"])
                if cancelRequested { status = "Notes cancelled"; return }
                notes = try JSONDecoder().decode(NotesResponse.self, from: data).notes
                notesURL = output
                pane = .notes
                status = "Notes saved locally"
            } catch {
                status = cancelRequested ? "Notes cancelled" : "Could not create notes"
                if !cancelRequested { self.error = error.localizedDescription }
            }
        }
    }

    func copy() {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(displayedText, forType: .string)
        status = pane == .notes ? "Notes copied" : "Transcript copied"
    }

    func export(_ format: ExportFormat = .text) {
        guard !busy, let completedURL, let installation else { return }
        let panel = NSSavePanel()
        let exportingNotes = pane == .notes
        let ext = exportingNotes ? "md" : format.fileExtension
        panel.title = exportingNotes ? "Export notes" : "Export transcript"
        panel.allowedContentTypes = [UTType(filenameExtension: ext) ?? .plainText]
        // AppKit supplies the selected type's suffix; supplying it twice can
        // produce names such as Transcript.srt.srt for dynamic UTTypes.
        panel.nameFieldStringValue = exportingNotes ? "Notes" : "Transcript"
        panel.canCreateDirectories = true
        panel.begin { response in
            guard response == .OK, let target = panel.url else { return }
            Task { @MainActor in
                guard !self.busy else { return }
                let process = CLIProcess()
                self.activeProcess = process
                self.operation = "Exporting"
                self.cancelRequested = false
                defer { self.finishOperation() }
                do {
                    if exportingNotes {
                        // Notes are already Markdown produced by the shared notes job.
                        // The save action uses the CLI's atomic delivery, like transcripts.
                        guard let notesURL = self.notesURL else { return }
                        _ = try await process.output(executable: installation.executable, arguments: ["export", notesURL.path, "--format", "markdown", "--output", target.path])
                    } else {
                        _ = try await process.output(executable: installation.executable, arguments: ["export", completedURL.path, "--format", format.rawValue, "--output", target.path])
                    }
                    self.status = "Exported \(target.lastPathComponent)"
                    self.error = nil
                } catch { self.error = error.localizedDescription }
            }
        }
    }
}

private extension String {
    var nonempty: String? { isEmpty ? nil : self }
}
