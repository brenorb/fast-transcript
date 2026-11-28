import AppKit
import FastTranscriptClient
import SwiftUI
import UniformTypeIdentifiers

@MainActor
struct ContentView: View {
    @Bindable var workspace: Workspace
    @State private var advanced = false
    @State private var dropTarget = false
    @State private var calendarAccounts = false

    var body: some View {
        VStack(spacing: 0) {
            inputControls
            Divider()
            document
            Divider()
            footer
        }
        .frame(minWidth: 720, minHeight: 500)
        .background(Color(nsColor: .windowBackgroundColor))
        .overlay { if dropTarget { RoundedRectangle(cornerRadius: 12).stroke(.tint, lineWidth: 3).padding(8).allowsHitTesting(false) } }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didResignActiveNotification)) { _ in workspace.dictation.releasePhysicalHold() }
        .onReceive(NotificationCenter.default.publisher(for: NSApplication.didBecomeActiveNotification)) { _ in
            if !workspace.busy { Task { await workspace.refreshCalendar() } }
        }
        .background(WindowCloseObserver { workspace.dictation.windowClosed(origin: "native-main-window") }.frame(width: 0, height: 0).allowsHitTesting(false))
        .onDisappear { workspace.dictation.windowClosed(origin: "view-disappeared") }
        .onDrop(of: [.fileURL, .url, .plainText], isTargeted: $dropTarget, perform: acceptDrop)
        .sheet(isPresented: $calendarAccounts) { CalendarAccountsView(workspace: workspace) }
    }

    private var inputControls: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                if workspace.meeting.isActive {
                    Label(workspace.meeting.status, systemImage: workspace.meeting.state == .recording ? "record.circle.fill" : "waveform")
                        .lineLimit(1)
                        .accessibilityIdentifier("meetingStatus")
                    Spacer()
                    if workspace.meeting.canStop {
                        Button("Stop meeting", systemImage: "stop.fill", action: workspace.meeting.stop)
                            .buttonStyle(.borderedProminent)
                            .tint(.red)
                            .accessibilityIdentifier("stopMeeting")
                    } else {
                        Button("Cancel", systemImage: "xmark", action: workspace.meeting.cancel)
                            .disabled(workspace.meeting.state == .cancelling)
                            .accessibilityIdentifier("cancelMeeting")
                    }
                } else {
                    TextField("Video URL or file path", text: $workspace.input)
                        .textFieldStyle(.roundedBorder)
                        .accessibilityIdentifier("mediaInput")
                        .onSubmit { workspace.start() }
                        .disabled(workspace.busy)
                    if workspace.operation != nil || workspace.dictation.isBusy {
                        Button("Cancel", systemImage: "stop.fill", action: workspace.cancel)
                            .disabled((workspace.operation != nil && workspace.cancelRequested) || workspace.dictation.state == .stopping)
                            .accessibilityIdentifier("cancelJob")
                    } else {
                        Button("Transcribe", systemImage: "waveform", action: { workspace.start() })
                            .buttonStyle(.borderedProminent)
                            .disabled(!workspace.canTranscribe)
                            .accessibilityIdentifier("transcribe")
                    }
                }
            }
            HStack {
                Group {
                    Button("Open file…", systemImage: "folder", action: workspace.openFile)
                        .accessibilityIdentifier("openFile")
                    Button("Paste link", systemImage: "link", action: workspace.pasteLink)
                        .accessibilityIdentifier("pasteLink")
                    if !workspace.meeting.isActive {
                        Button("Record meeting", systemImage: "record.circle", action: { workspace.startMeeting() })
                            .accessibilityIdentifier("recordMeeting")
                            .disabled(!workspace.canStartMeeting)
                            .help("Record microphone and system audio locally. Click Stop when the meeting ends.")
                    }
                    if workspace.lastMeetingDirectory != nil && !workspace.meeting.isActive {
                        Button("Open last meeting", systemImage: "clock", action: workspace.openLastMeeting)
                            .disabled(!workspace.canStartMeeting)
                            .accessibilityIdentifier("openLastMeeting")
                    }
                    Spacer()
                    modelMenu
                    Menu {
                        if let agents = workspace.agents {
                            ForEach(agents.agents) { agent in
                                Button {
                                    workspace.chooseAgent(agent.name)
                                } label: {
                                    if agent.name == agents.default_agent { Label(agent.name, systemImage: "checkmark") }
                                    else { Text(agent.name) }
                                }
                                .disabled(!agent.available)
                            }
                        }
                        Button("Choose automatically") { workspace.chooseAgent("auto") }
                    } label: {
                        Label(workspace.agents?.default_agent ?? "Notes agent", systemImage: "sparkles")
                    }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .help("The selected external agent uses its own login and model provider.")
                    .accessibilityIdentifier("agentPicker")
                }
                .disabled(workspace.busy)
                Menu {
                    Toggle("Record calls automatically", isOn: Binding(get: { workspace.automaticMeetingsEnabled }, set: { workspace.setAutomaticMeetings($0) }))
                        .disabled(workspace.savingPreference || workspace.loading)
                    Text(workspace.automatic.status)
                    Divider()
                    Toggle("Use calendar meeting names", isOn: Binding(get: { workspace.calendar?.name_meetings ?? false }, set: { workspace.setCalendarNames($0) }))
                        .disabled(workspace.busy || workspace.calendar?.supported == false)
                    Text("Calendar access: \(workspace.calendar?.authorization.replacingOccurrences(of: "_", with: " ") ?? "unavailable")")
                    Button("Calendar accounts…") { calendarAccounts = true }
                        .accessibilityIdentifier("calendarAccounts")
                    Button("Refresh calendar status") { Task { await workspace.refreshCalendar() } }
                } label: {
                    Image(systemName: "gearshape")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .help("Meeting settings")
                .accessibilityLabel("Meeting settings")
                .accessibilityIdentifier("meetingSettings")
            }
            HStack(spacing: 12) {
                HoldToTalkButton(title: workspace.dictation.title,
                    enabled: workspace.canDictate,
                    shortcutLabel: workspace.dictation.shortcutWarning == nil ? workspace.dictation.shortcutLabel : nil,
                    pressed: workspace.holdDictation,
                    accessiblePress: workspace.toggleDictation)
                    .disabled(!workspace.canDictate)
                    .frame(width: 260, height: 30)
                Text(workspace.dictation.shortcutWarning == nil ? "\(workspace.dictation.shortcutLabel) anywhere once prepared" : "Use the hold button").font(.caption).foregroundStyle(.secondary)
                Spacer()
                Toggle("Paste automatically", isOn: Binding(get: { workspace.pasteAfterDictation }, set: { workspace.setAutomaticPaste($0) }))
                    .toggleStyle(.checkbox).font(.caption)
                    .help("After shortcut dictation, paste only when the original app is still active. Requires Accessibility permission. Otherwise the text is copied.")
                    .disabled(workspace.busy)
            }
            if let warning = workspace.dictation.shortcutWarning { Text(warning).font(.caption).foregroundStyle(.secondary) }
            DisclosureGroup(selectionTitle, isExpanded: $advanced) {
                HStack {
                    Picker("Selection", selection: $workspace.rangeMode) {
                        ForEach(RangeMode.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .labelsHidden()
                    .frame(width: 175)
                    if workspace.rangeMode == .last {
                        TextField("Duration", text: $workspace.rangeLast).frame(width: 90)
                            .accessibilityLabel("Last duration")
                            .accessibilityIdentifier("rangeLast")
                    } else if workspace.rangeMode == .interval {
                        TextField("Start", text: $workspace.rangeStart).frame(width: 90)
                            .accessibilityLabel("Start time")
                            .accessibilityIdentifier("rangeStart")
                        Text("to").foregroundStyle(.secondary)
                        TextField("End", text: $workspace.rangeEnd).frame(width: 90)
                            .accessibilityLabel("End time")
                            .accessibilityIdentifier("rangeEnd")
                    }
                    Text("Minutes:seconds, or seconds").font(.caption).foregroundStyle(.secondary)
                    Spacer()
                }
                .textFieldStyle(.roundedBorder)
                .padding(.top, 8)
            }
            .font(.callout)
            .disabled(workspace.busy)
            if workspace.meeting.coverageChanged {
                Label("Audio coverage may be incomplete. Review the saved meeting for interruptions or device changes.", systemImage: "exclamationmark.circle")
                    .font(.caption).foregroundStyle(.secondary)
                    .accessibilityIdentifier("meetingCoverage")
            }
            if let warning = workspace.meeting.recognitionWarning {
                Text(warning).font(.caption).foregroundStyle(.secondary)
            }
            if let error = workspace.error ?? workspace.meeting.error ?? workspace.dictation.error {
                errorBanner(error)
            }
        }
        .padding(20)
    }

    private var modelMenu: some View {
        Menu {
            if let models = workspace.models {
                ForEach(models.models) { model in
                    Button {
                        workspace.chooseModel(model.name)
                    } label: {
                        if model.name == (models.saved_default ?? models.models.first?.name) {
                            Label(model.name, systemImage: "checkmark")
                        } else { Text(model.name) }
                    }
                    .help(model.advice)
                }
                if models.environment_override != nil {
                    Text("An environment override is active")
                }
                Divider()
            }
            Button("Reset model preference", action: workspace.resetModel)
        } label: {
            Label(workspace.models?.effective_model ?? "Model", systemImage: "cpu")
                .lineLimit(1)
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("modelPicker")
    }

    private var selectionTitle: String {
        switch workspace.rangeMode {
        case .all: return "Transcribe part of the recording"
        case .last: return "Selection: last \(workspace.rangeLast)"
        case .interval: return "Selection: \(workspace.rangeStart.isEmpty ? "beginning" : workspace.rangeStart) to \(workspace.rangeEnd.isEmpty ? "end" : workspace.rangeEnd)"
        }
    }

    private var document: some View {
        VStack(spacing: 0) {
            if !workspace.transcript.isEmpty || workspace.completedURL != nil {
                HStack {
                    Text(workspace.sourceTitle).lineLimit(1).foregroundStyle(.secondary)
                    Spacer()
                    if !workspace.notes.isEmpty {
                        Picker("Document", selection: $workspace.pane) {
                            ForEach(DocumentPane.allCases) { Text($0.rawValue).tag($0) }
                        }
                        .pickerStyle(.segmented)
                        .frame(width: 210)
                        .accessibilityIdentifier("documentPicker")
                    }
                }
                .padding(.horizontal, 24).padding(.top, 16)
            }
            if workspace.displayedText.isEmpty {
                ContentUnavailableView {
                    Label(emptyDocumentTitle, systemImage: "waveform")
                } description: {
                    Text(workspace.meeting.isActive ? "Microphone and system audio are saved locally. Live text appears as it is recognized." : workspace.dictation.isBusy ? "Recognition runs locally. Your transcript appears when recording finishes." : workspace.completedURL != nil ? "No speech was recognized." : "Open a file or paste a video link. Transcription runs locally and appears as it arrives.")
                }
            } else {
                ScrollView {
                    if workspace.pane == .transcript && !workspace.meetingSegments.isEmpty {
                        LazyVStack(alignment: .leading, spacing: 20) {
                            ForEach(Array(workspace.meetingSegments.enumerated()), id: \.offset) { index, segment in
                                VStack(alignment: .leading, spacing: 5) {
                                    Text(segment.speaker ?? "Speech").font(.caption.weight(.semibold)).foregroundStyle(.secondary)
                                    Text(segment.text).font(.system(size: 16)).lineSpacing(5).textSelection(.enabled)
                                }
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .accessibilityElement(children: .combine)
                                .accessibilityIdentifier("meetingSegment-\(index)")
                            }
                        }
                        .padding(24)
                        .accessibilityIdentifier("meetingTranscript")
                    } else {
                        Text(workspace.displayedText)
                            .font(.system(size: 16))
                            .lineSpacing(5)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(24)
                            .accessibilityIdentifier("transcriptText")
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var emptyDocumentTitle: String {
        if workspace.meeting.isActive { return workspace.meeting.status }
        if workspace.dictation.isBusy { return workspace.dictation.title }
        if workspace.completedURL != nil { return "No speech recognized" }
        return workspace.operation == nil ? "Drop audio or video" : "Preparing your transcript"
    }

    private var footer: some View {
        HStack(spacing: 12) {
            if workspace.busy { ProgressView().controlSize(.small) }
            Text(workspace.meeting.isActive ? workspace.meeting.status : workspace.dictation.isBusy ? workspace.dictation.title : workspace.status).font(.callout).foregroundStyle(.secondary)
                .lineLimit(1).accessibilityIdentifier("jobStatus")
            Spacer()
            if workspace.completedURL == nil, workspace.meeting.directory != nil, !workspace.meeting.isActive {
                Button("Transcribe saved audio", action: workspace.retrySavedMeeting)
                    .disabled(workspace.busy)
                    .accessibilityIdentifier("retryMeeting")
            }
            Button("Create notes", systemImage: "sparkles", action: workspace.generateNotes)
                .disabled(workspace.busy || workspace.completedURL == nil)
                .accessibilityIdentifier("createNotes")
                .help("Create a summary and action items using your selected external agent.")
            Button("Copy", systemImage: "doc.on.doc", action: workspace.copy)
                .disabled(!workspace.canCopy)
                .accessibilityIdentifier("copyDocument")
            HStack(spacing: 0) {
                Button("Export…", action: { workspace.export() })
                    .accessibilityIdentifier("exportDocument")
                if workspace.pane == .transcript {
                    Menu {
                        ForEach(ExportFormat.allCases) { format in
                            Button(format.title) { workspace.export(format) }
                        }
                    } label: { Image(systemName: "chevron.down") }
                    .menuStyle(.borderlessButton)
                    .fixedSize()
                    .accessibilityLabel("Export format")
                }
            }
            .disabled(workspace.busy || workspace.completedURL == nil)
        }
        .padding(16)
    }

    private func errorBanner(_ message: String) -> some View {
        HStack(alignment: .top) {
            Image(systemName: "exclamationmark.circle.fill").foregroundStyle(.red)
            VStack(alignment: .leading, spacing: 6) {
                Text(message.components(separatedBy: "\n").first ?? message)
                    .font(.callout).textSelection(.enabled)
                if message.contains("\n") {
                    DisclosureGroup("Details") {
                        ScrollView { Text(message).font(.caption.monospaced()).textSelection(.enabled).frame(maxWidth: .infinity, alignment: .leading) }
                            .frame(maxHeight: 160)
                    }
                    .font(.caption)
                }
            }
            Spacer()
            Button { workspace.error = nil; workspace.dictation.error = nil; workspace.meeting.error = nil } label: { Image(systemName: "xmark") }
                .buttonStyle(.plain).accessibilityLabel("Dismiss error")
        }
        .padding(12)
        .background(.red.opacity(0.07), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("errorMessage")
    }

    private func acceptDrop(_ providers: [NSItemProvider], _ location: CGPoint) -> Bool {
        guard !workspace.busy, let provider = providers.first else { return false }
        // Start loading inside the drop callback, as required by NSItemProvider.
        for type in [UTType.fileURL, .url, .plainText] where provider.hasItemConformingToTypeIdentifier(type.identifier) {
            provider.loadItem(forTypeIdentifier: type.identifier) { item, _ in
                if let source = droppedMediaSource(item, typeIdentifier: type.identifier) {
                    Task { @MainActor in workspace.start(source) }
                } else {
                    Task { @MainActor in workspace.error = "The dropped item did not contain a readable file path or media URL." }
                }
            }
            return true
        }
        return false
    }
}

#if DEBUG
#Preview("Ready") {
    ContentView(workspace: Workspace.previewMeeting(recording: false))
}
#Preview("Meeting recording") {
    ContentView(workspace: Workspace.previewMeeting(recording: true))
}
#endif
