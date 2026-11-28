import AppKit
import SwiftUI
import OSLog

@MainActor
final class ApplicationDelegate: NSObject, NSApplicationDelegate {
    weak var workspace: Workspace?
    var openMainWindow: (() -> Void)?
    private var terminating = false
    private let logger = Logger(subsystem: "com.brenorb.fast-transcript", category: "lifecycle")

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        // A visible status-item window is not a visible document. After handling
        // reopen ourselves, suppress AppKit's separate untitled-window fallback.
        !showMainWindow(sender)
    }

    @discardableResult
    func showMainWindow(_ sender: NSApplication = NSApp, opener: (() -> Void)? = nil) -> Bool {
        let documents = sender.windows.filter { $0.canBecomeMain && !($0 is NSPanel) }
        let summary = sender.windows.map { "\(NSStringFromClass(type(of: $0))):visible=\($0.isVisible),main=\($0.canBecomeMain)" }.joined(separator: ";")
        logger.info("Reopen windows: \(summary, privacy: .public)")
        if let visible = documents.first(where: { $0.isVisible && !$0.isMiniaturized }) {
            logger.info("Reopen: front existing document")
            visible.makeKeyAndOrderFront(nil)
        } else if let minimized = documents.first(where: { $0.isMiniaturized }) {
            logger.info("Reopen: restore minimized document")
            minimized.deminiaturize(nil)
            minimized.makeKeyAndOrderFront(nil)
        } else if let open = opener ?? openMainWindow {
            logger.info("Reopen: open registered document")
            open()
        } else {
            logger.info("Reopen: no registered opener")
            return false
        }
        return true
    }

    func mainWindowAppeared() { logger.info("Main document appeared") }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        logger.info("Quit callback; workspace=\(self.workspace != nil)")
        guard let workspace else { return .terminateNow }
        guard !terminating else { return .terminateLater }
        terminating = true
        // Stop capture synchronously before closing-window notifications can
        // turn Quit into a normal finish-and-export request.
        workspace.meeting.cancel()
        workspace.dictation.cancel()
        Task {
            await workspace.shutdown()
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }

    func applicationWillTerminate(_ notification: Notification) {
        logger.info("App will terminate")
    }
}

@main
@MainActor
struct FastTranscriptApp: App {
    @NSApplicationDelegateAdaptor(ApplicationDelegate.self) private var delegate
    @State private var workspace = Workspace()

    var body: some Scene {
        WindowGroup("Fast Transcript", id: "main") {
            WorkspaceWindow(workspace: workspace, delegate: delegate)
        }
        .defaultSize(width: 920, height: 660)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(replacing: .newItem) {
                Button("Open audio or video…", action: workspace.openFile)
                    .keyboardShortcut("o")
                    .disabled(workspace.busy)
                Button("Paste link and transcribe", action: workspace.pasteLink)
                    .keyboardShortcut("v", modifiers: [.command, .shift])
                    .disabled(workspace.busy)
                Button("Record meeting", action: { workspace.startMeeting() })
                    .keyboardShortcut("r", modifiers: [.command, .shift])
                    .disabled(!workspace.canStartMeeting)
                Button("Open last meeting", action: workspace.openLastMeeting)
                    .disabled(!workspace.canStartMeeting || workspace.lastMeetingDirectory == nil)
                Button("Transcribe saved meeting audio…", action: workspace.openSavedMeeting)
                    .disabled(!workspace.canStartMeeting)
                Divider()
                Button(workspace.dictation.state == .recording || workspace.dictation.state == .starting ? "Finish dictation" : "Start dictation", action: workspace.toggleDictation)
                    .keyboardShortcut("d", modifiers: [.command, .shift])
                    .disabled(workspace.meeting.isActive || workspace.operation != nil || workspace.loading || workspace.savingPreference || !workspace.dictation.canHold || workspace.dictation.state == .preparing)
            }
        }
        MenuBarExtra(isInserted: Binding(
            get: { workspace.meeting.isActive && workspace.meetingIndicatorVisible },
            set: { visible in
                if !visible && workspace.meeting.isActive {
                    workspace.meetingIndicatorVisible = false
                    workspace.meeting.cancel()
                }
            })) {
            MeetingMenu(workspace: workspace, delegate: delegate)
        } label: {
            Text(AppBrand.symbol)
                .accessibilityLabel(workspace.meeting.indicatorTitle)
        }
        .menuBarExtraStyle(.menu)
    }
}

@MainActor
private struct MeetingMenu: View {
    let workspace: Workspace
    let delegate: ApplicationDelegate
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Text(workspace.meeting.status)
        Button("Show transcript") {
            NSApp.activate(ignoringOtherApps: true)
            delegate.showMainWindow(opener: { openWindow(id: "main") })
        }
        if workspace.meeting.canStop { Button("Stop meeting", action: workspace.meeting.stop) }
        Button("Cancel and keep saved audio", action: workspace.meeting.cancel)
            .disabled(workspace.meeting.state == .cancelling)
        Divider()
        Button("Quit Fast Transcript") { NSApp.terminate(nil) }
    }
}

@MainActor
private struct WorkspaceWindow: View {
    let workspace: Workspace
    let delegate: ApplicationDelegate
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        ContentView(workspace: workspace)
            .task {
                let action = openWindow
                delegate.openMainWindow = { action(id: "main") }
                delegate.workspace = workspace
                await workspace.bootstrap()
            }
            .onAppear { delegate.mainWindowAppeared() }
            .onOpenURL { workspace.open($0) }
    }
}
