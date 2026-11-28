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

struct AutomaticMeetingPreference: Decodable { let version: Int; let enabled: Bool }

struct CalendarStatus: Decodable {
    let version: Int
    let supported: Bool
    let authorization: String
    let name_meetings: Bool
    let queried: Bool
    let calendars: [Calendar]

    struct Calendar: Decodable, Identifiable, Equatable {
        let id: String
        let title: String
        let source: String
    }
}

struct PermissionStatus: Decodable {
    let version: Int
    let paste: Paste
    struct Paste: Decodable { let supported: Bool; let granted: Bool }
}

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
    var calendar: CalendarStatus?
    @ObservationIgnored private var controlJobs: [UUID: (process: CLIProcess, task: Task<Data, Error>)] = [:]
    @ObservationIgnored private let controlEnvironment: [String: String]?
    let dictation = DictationController()
    let meeting = MeetingController()
    let automatic = AutomaticMeetingController()
    var automaticMeetingsEnabled = false
    var meetingIndicatorVisible = true
    @ObservationIgnored private var shuttingDown = false
    @ObservationIgnored private var restoreDictation = false
    var lastMeetingDirectory: URL? = preferences.string(forKey: "lastMeetingDirectory").map { URL(fileURLWithPath: $0) }
    var pasteAfterDictation = preferences.bool(forKey: "pasteAfterDictation") { didSet { Self.preferences.set(pasteAfterDictation, forKey: "pasteAfterDictation") } }
    private static var preferences: UserDefaults {
        ProcessInfo.processInfo.environment["FSCRIPT_APP_PREFERENCES_SUITE"].flatMap(UserDefaults.init(suiteName:)) ?? .standard
    }
    @ObservationIgnored private var pasteTarget: NSRunningApplication?
    @ObservationIgnored private var deliveryProcess: CLIProcess?
    @ObservationIgnored private var deliveryTask: Task<Void, Never>?
    @ObservationIgnored private var deliveryID = UUID()
    @ObservationIgnored private var permissionProcess: CLIProcess?
    @ObservationIgnored private var permissionTask: Task<Void, Never>?
    @ObservationIgnored private var permissionID = UUID()
    var input = ""
    var rangeMode = RangeMode.all
    var rangeStart = ""
    var rangeEnd = ""
    var rangeLast = "5:00"
    var pane = DocumentPane.transcript
    var transcript = ""
    var meetingSegments: [TranscriptSegment] = []
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

    var busy: Bool { operation != nil || loading || savingPreference || dictation.isBusy || meeting.isActive }
    var canDictate: Bool { !meeting.isActive && operation == nil && !loading && !savingPreference && dictation.canHold }
    var canCopy: Bool { !busy && !displayedText.isEmpty && (pane == .notes ? notesURL != nil : completedURL != nil) }
    var canStartMeeting: Bool { !busy && installation != nil }
    var canTranscribe: Bool { !busy && installation != nil && !input.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var displayedText: String { pane == .notes ? notes : transcript }
    var hasMeetingIndicator: Bool { meetingIndicatorVisible && (meeting.isActive || automatic.isRunning || automaticMeetingsEnabled) }

    init(installation: CLIInstallation? = nil, controlEnvironment: [String: String]? = nil) {
        self.installation = installation
        self.controlEnvironment = controlEnvironment
        dictation.onRecording = { [weak self] shortcut in self?.recordingBegan(shortcut: shortcut) }
        dictation.onCompleted = { [weak self] message in self?.dictationCompleted(message) }
        meeting.onEvent = { [weak self] message in self?.meetingEvent(message) }
        meeting.onCompleted = { [weak self] message in self?.meetingCompleted(message) }
        meeting.onFinished = { [weak self] in
            guard let self else { return }
            status = meeting.status
            if restoreDictation && !shuttingDown { dictation.prepareForShortcut() }
            restoreDictation = false
        }
        meeting.onManualEndRequested = { [weak self] in self?.automatic.suppress() }
        automatic.admit = { [weak self] episode, apps in await self?.admitAutomaticMeeting(episode: episode, apps: apps) ?? false }
        automatic.onMeeting = { [weak self] in self?.meeting.receiveAutomatic($0) }
        automatic.onSessionExit = { [weak self] in self?.meeting.finishAutomatic() }
        automatic.onReservationExpired = { [weak self] in self?.meeting.endAutomatic() }
        automatic.onEnded = { [weak self] message in
            guard let self else { return }
            meeting.endAutomatic(error: message)
            if let message, !shuttingDown { error = message }
        }
    }

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
            dictation.configure(executable: installation.executable, directory: jobDirectory())
            try await refreshModels()
            await refreshAgents()
            await refreshCalendar()
            await refreshAutomaticMeetings()
            if automaticMeetingsEnabled { startAutomaticWatcher() }
            status = "Ready"
            error = nil
            if pasteAfterDictation { checkPasteAccess(request: false) }
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
        guard !shuttingDown, let installation else { throw ClientError("The app is not available.") }
        let id = UUID()
        let process = CLIProcess()
        let task = Task { try await process.output(executable: installation.executable, arguments: arguments, environment: controlEnvironment) }
        controlJobs[id] = (process, task)
        defer { controlJobs[id] = nil }
        return try await task.value
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

    func refreshCalendar() async {
        let state = try? await JSONDecoder().decode(CalendarStatus.self, from: control(["calendar", "status", "--json"]))
        calendar = state?.version == 1 ? state : nil
    }

    /// The existing CLI inventories local calendars; this never requests access.
    func readCalendarAccounts() async throws -> CalendarStatus {
        let state = try JSONDecoder().decode(CalendarStatus.self, from: await control(["calendar", "--json"]))
        guard state.version == 1 else { throw ClientError("Unsupported calendar response. Update the app.") }
        guard !shuttingDown, !Task.isCancelled else { throw CancellationError() }
        calendar = state
        return state
    }

    /// Refresh is passive: it never restarts a failed/cancelled watch generation.
    func refreshAutomaticMeetings() async {
        if let data = try? await control(["meeting", "auto", "status", "--json"]),
           let preference = try? JSONDecoder().decode(AutomaticMeetingPreference.self, from: data), preference.version == 1 {
            automaticMeetingsEnabled = preference.enabled
        }
    }

    func setAutomaticMeetings(_ enabled: Bool) {
        guard !loading, !savingPreference, !shuttingDown, installation != nil else { return }
        savingPreference = true
        Task {
            defer { savingPreference = false }
            do {
                let data = try await control(["meeting", "auto", enabled ? "enable" : "disable", "--json"])
                guard !shuttingDown else { return }
                let preference = try JSONDecoder().decode(AutomaticMeetingPreference.self, from: data)
                guard preference.version == 1 else { throw ClientError("Unsupported automatic meeting preference.") }
                automaticMeetingsEnabled = preference.enabled
                error = nil
                if preference.enabled { startAutomaticWatcher() }
                else { automatic.stopWatching() }
            } catch { if !shuttingDown { self.error = error.localizedDescription } }
        }
    }

    private func startAutomaticWatcher() {
        guard !shuttingDown, automaticMeetingsEnabled, let installation else { return }
        meetingIndicatorVisible = true
        automatic.start(executable: installation.executable,
            outputDirectory: dataDirectory().appendingPathComponent("meetings", isDirectory: true), environment: controlEnvironment)
    }

    private func cancelAutomaticMeeting() {
        automatic.cancel()
        if !shuttingDown { setAutomaticMeetings(false) }
    }

    private func admitAutomaticMeeting(episode: UInt64, apps: [String]) async -> Bool {
        guard automaticMeetingsEnabled, !shuttingDown, !busy else { return false }
        guard meeting.beginAutomatic(stop: { [weak self] in self?.automatic.stopCurrent() },
            cancel: { [weak self] in self?.cancelAutomaticMeeting() }) else { return false }
        restoreDictation = dictation.isRunning
        dictation.cancel(); dictation.error = nil
        meetingIndicatorVisible = true
        completedURL = nil; notesURL = nil; transcript = ""; notes = ""; meetingSegments = []
        pane = .transcript; error = nil; sourceTitle = apps.isEmpty ? "Meeting" : apps.joined(separator: ", ")
        await dictation.shutdown()
        return !shuttingDown && automaticMeetingsEnabled && meeting.state == .starting
    }

    func removeMeetingIndicator() {
        guard hasMeetingIndicator else { return }
        meetingIndicatorVisible = false
        cancelAutomaticMeeting()
        meeting.cancel()
    }

    func prepareToQuit() { shuttingDown = true; automatic.cancel() }

    func setCalendarNames(_ enabled: Bool) {
        guard !busy, installation != nil else { return }
        savingPreference = true
        status = enabled ? "Connecting calendar… Respond to macOS if prompted." : "Disabling calendar meeting names…"
        Task {
            defer { savingPreference = false }
            do {
                let data = try await control(["calendar", enabled ? "connect" : "disconnect", "--json"])
                guard !shuttingDown else { return }
                let state = try JSONDecoder().decode(CalendarStatus.self, from: data)
                guard state.version == 1 else { throw ClientError("Unsupported calendar response. Update the app.") }
                calendar = state
                error = nil
                status = state.name_meetings ? "Calendar meeting names enabled" : "Calendar meeting names disabled"
            } catch {
                guard !shuttingDown else { return }
                self.error = error.localizedDescription
                status = "Calendar setup needs attention"
                await refreshCalendar()
            }
        }
    }

    func chooseModel(_ name: String) {
        guard !busy else { return }
        savingPreference = true
        Task {
            defer { savingPreference = false }
            do {
                await dictation.shutdown()
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
                await dictation.shutdown()
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
        meeting.clearPresentation()
        meetingSegments = []
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
                await dictation.shutdown()
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

    func holdDictation(_ pressed: Bool) {
        guard !pressed || canDictate else { return }
        dictation.hold(pressed)
    }

    func toggleDictation() {
        guard canDictate else { return }
        dictation.accessiblePress()
    }

    func startMeeting(savedDirectory: URL? = nil) {
        guard !busy, let installation else { return }
        automatic.suppress()
        restoreDictation = dictation.isRunning
        dictation.cancel()
        dictation.error = nil
        meetingIndicatorVisible = true
        completedURL = nil; notesURL = nil; transcript = ""; notes = ""; meetingSegments = []
        pane = .transcript; error = nil; sourceTitle = savedDirectory == nil ? "Meeting" : "Saved meeting"
        meeting.start(executable: installation.executable, outputDirectory: dataDirectory().appendingPathComponent("meetings", isDirectory: true), savedDirectory: savedDirectory) { [weak self] in
            await self?.dictation.shutdown()
        }
    }

    func openLastMeeting() {
        guard canStartMeeting, let directory = lastMeetingDirectory, let installation else { return }
        meeting.clearPresentation()
        meeting.directory = directory
        let process = CLIProcess()
        activeProcess = process; operation = "Opening meeting"; cancelRequested = false
        completedURL = nil; notesURL = nil; transcript = ""; notes = ""; meetingSegments = []; pane = .transcript
        sourceTitle = "Saved meeting"; error = nil; status = "Opening saved meeting…"
        jobTask = Task {
            defer { finishOperation() }
            do {
                await dictation.shutdown()
                let document = directory.appendingPathComponent("transcript.json")
                let data = try await process.output(executable: installation.executable, arguments: ["export", document.path, "--format", "json", "--stdout", "--quiet"])
                guard !cancelRequested else { status = "Cancelled"; return }
                let result = try JSONDecoder().decode(TranscriptSnapshot.self, from: data)
                transcript = result.text; meetingSegments = result.segments ?? []; completedURL = document
                sourceTitle = result.meeting?.calendar_event?.title.nonempty ?? "Saved meeting"
                meeting.manifest = result.meeting
                meeting.coverageChanged = result.meeting?.hasCoverageChanges == true
                status = meeting.coverageChanged ? "Saved meeting · review audio coverage" : "Saved meeting opened"
            } catch {
                if !cancelRequested { self.error = error.localizedDescription; status = "Saved audio needs attention" }
            }
        }
    }

    func openSavedMeeting() {
        guard !busy else { return }
        let panel = NSOpenPanel()
        panel.title = "Transcribe saved meeting audio"
        panel.canChooseDirectories = true; panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.directoryURL = lastMeetingDirectory?.deletingLastPathComponent() ?? dataDirectory().appendingPathComponent("meetings")
        panel.begin { response in
            if response == .OK, let url = panel.url { Task { @MainActor in self.startMeeting(savedDirectory: url) } }
        }
    }

    func retrySavedMeeting() { if let lastMeetingDirectory { startMeeting(savedDirectory: lastMeetingDirectory) } }

    private func meetingEvent(_ message: MeetingEvent) {
        if let title = message.data.manifest?.calendar_event?.title.nonempty { sourceTitle = title }
        if message.type == "started", let path = message.data.directory {
            let directory = URL(fileURLWithPath: path)
            lastMeetingDirectory = directory
            Self.preferences.set(path, forKey: "lastMeetingDirectory")
        }
        if message.type == "update" {
            transcript = message.data.text ?? transcript
            meetingSegments = message.data.segments ?? []
        }
        if message.type == "completed" {
            transcript = message.data.result?.text ?? transcript
            meetingSegments = message.data.result?.segments ?? []
        }
    }

    private func meetingCompleted(_ message: MeetingEvent) {
        sourceTitle = message.data.result?.meeting?.calendar_event?.title.nonempty ?? sourceTitle
        completedURL = message.data.output_path.map { URL(fileURLWithPath: $0) }
        transcript = message.data.result?.text ?? transcript
        meetingSegments = message.data.result?.segments ?? []
        status = meeting.status
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

    func cancel() {
        if meeting.isActive { meeting.cancel(); return }
        if dictation.isBusy {
            status = "Stopping dictation…"
            Task { await dictation.shutdown(); status = "Dictation cancelled" }
            return
        }
        cancelRequested = true; status = "Stopping…"; activeProcess?.cancel()
    }

    func shutdown() async {
        prepareToQuit()
        let pendingControls = Array(controlJobs.values)
        for job in pendingControls { job.process.cancel() }
        meeting.cancel()
        activeProcess?.cancel()
        cancelRequested = true
        deliveryProcess?.cancel()
        deliveryTask?.cancel()
        permissionProcess?.cancel()
        permissionTask?.cancel()
        await dictation.shutdown()
        await automatic.shutdown()
        await meeting.shutdown()
        await jobTask?.value
        await deliveryTask?.value
        await permissionTask?.value
        for job in pendingControls { _ = try? await job.task.value }
    }

    func setAutomaticPaste(_ enabled: Bool) {
        pasteAfterDictation = enabled
        error = nil
        permissionID = UUID()
        permissionProcess?.cancel()
        permissionTask?.cancel()
        if enabled { checkPasteAccess(request: true) }
    }

    private func checkPasteAccess(request: Bool) {
        guard let installation else { return }
        let token = UUID()
        permissionID = token
        let process = CLIProcess()
        permissionProcess = process
        permissionTask = Task {
            defer { if permissionID == token { permissionProcess = nil; permissionTask = nil } }
            do {
                var args = ["permissions", "--json", "--quiet"]
                if request { args += ["--request", "paste"] }
                let response = try await process.output(executable: installation.executable, arguments: args)
                guard permissionID == token, !Task.isCancelled, pasteAfterDictation else { return }
                let access = try JSONDecoder().decode(PermissionStatus.self, from: response)
                guard access.version == 1 else { throw ClientError("Unsupported permission response. Update the app.") }
                if !access.paste.supported {
                    error = "Automatic paste is unavailable on this platform. Dictation is copied instead."
                } else if !access.paste.granted {
                    error = "Allow Fast Transcript in System Settings > Privacy & Security > Accessibility (Device Control and Data Access). An enabled entry may belong to an earlier build. Dictation is copied until access is granted."
                }
            } catch {
                guard permissionID == token, !Task.isCancelled else { return }
                self.error = error.localizedDescription
            }
        }
    }

    private func recordingBegan(shortcut: Bool) {
        guard operation == nil, !meeting.isActive, !shuttingDown else { return }
        meeting.clearPresentation()
        meetingSegments = []
        deliveryID = UUID()
        deliveryProcess?.cancel()
        deliveryTask?.cancel()
        pasteTarget = shortcut ? NSWorkspace.shared.frontmostApplication : nil
        cancelRequested = false
        error = nil
        status = "Listening · release to finish"
    }

    private func dictationCompleted(_ event: RecordingEvent) {
        guard operation == nil, !meeting.isActive, !shuttingDown else { return }
        meeting.clearPresentation()
        meetingSegments = []
        guard let result = event.data.result else { return }
        completedURL = event.data.output_path.map { URL(fileURLWithPath: $0) }
        transcript = result.text
        notes = ""; notesURL = nil; pane = .transcript; sourceTitle = "Dictation"
        status = result.text.isEmpty ? "No speech recognized" : "Dictation complete"
        guard !result.text.isEmpty, let completedURL, let installation else { return }
        let token = UUID()
        deliveryID = token
        let process = CLIProcess()
        deliveryProcess = process
        let target = pasteTarget
        let paste = pasteAfterDictation && target?.isTerminated == false && target?.processIdentifier != ProcessInfo.processInfo.processIdentifier && target == NSWorkspace.shared.frontmostApplication
        deliveryTask = Task {
            defer { if deliveryID == token { deliveryProcess = nil; deliveryTask = nil } }
            do {
                var args = ["export", completedURL.path, "--format", "compact", "--quiet"]
                if paste, let target { args += ["--paste", "--target-pid", String(target.processIdentifier)] }
                else { args += ["--copy"] }
                _ = try await process.output(executable: installation.executable, arguments: args)
                guard deliveryID == token else { return }
                status = paste ? "Dictation copied and sent to the original app" : "Dictation copied"
            } catch let deliveryError {
                guard deliveryID == token, !Task.isCancelled else { return }
                if paste {
                    let fallback = CLIProcess()
                    deliveryProcess = fallback
                    do {
                        _ = try await fallback.output(executable: installation.executable, arguments: ["export", completedURL.path, "--format", "compact", "--copy", "--quiet"])
                        guard deliveryID == token, !Task.isCancelled else { return }
                        status = "Dictation copied · automatic paste unavailable"
                        self.error = deliveryError.localizedDescription
                        return
                    } catch {
                        guard deliveryID == token, !Task.isCancelled else { return }
                        self.error = error.localizedDescription
                    }
                } else { self.error = deliveryError.localizedDescription }
                status = "Dictation saved · copy needs attention"
            }
        }
    }

    private func finishOperation() { activeProcess = nil; jobTask = nil; operation = nil }

    private func dataDirectory() -> URL {
        let root = (controlEnvironment ?? ProcessInfo.processInfo.environment)["FSCRIPT_APP_DATA_DIR"].map { URL(fileURLWithPath: $0) }
            ?? FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0].appendingPathComponent("fast-transcript/desktop", isDirectory: true)
        return root
    }

    private func jobDirectory() -> URL {
        // Rust creates the destination and writes the completed document atomically.
        return dataDirectory().appendingPathComponent(UUID().uuidString, isDirectory: true)
    }

    #if DEBUG
    static func previewMeeting(recording: Bool) -> Workspace {
        let workspace = Workspace()
        workspace.installation = CLIInstallation(executable: URL(fileURLWithPath: "/preview/fscript"))
        workspace.status = "Ready"
        if recording {
            workspace.meeting.state = .recording
            workspace.meeting.status = "Recording meeting · microphone and system audio"
            workspace.sourceTitle = "Meeting"
            workspace.transcript = "Microphone: Let's agree on next steps.\nSystem: I'll prepare the draft."
        }
        return workspace
    }
    #endif

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
                await dictation.shutdown()
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
        guard canCopy, let installation else { return }
        let source = pane == .notes ? notesURL : completedURL
        guard let source else { return }
        let format = pane == .notes ? "markdown" : "compact"
        let process = CLIProcess()
        activeProcess = process; operation = "Copying"; cancelRequested = false
        jobTask = Task {
            defer { finishOperation() }
            do {
                _ = try await process.output(executable: installation.executable, arguments: ["export", source.path, "--format", format, "--copy", "--quiet"])
                status = "Copied"
            } catch { if !cancelRequested { self.error = error.localizedDescription } }
        }
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
