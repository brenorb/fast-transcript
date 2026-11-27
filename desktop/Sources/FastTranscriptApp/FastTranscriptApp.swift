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
        if !sender.windows.contains(where: { $0.isVisible && !($0 is NSPanel) }) {
            if let window = sender.windows.first(where: { $0.isMiniaturized && !($0 is NSPanel) }) {
                window.deminiaturize(nil)
            } else {
                openMainWindow?()
            }
        }
        return true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        logger.info("Quit callback; workspace=\(self.workspace != nil)")
        guard let workspace else { return .terminateNow }
        guard !terminating else { return .terminateLater }
        terminating = true
        // Stop capture synchronously before closing-window notifications can
        // turn Quit into a normal finish-and-export request.
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
                Divider()
                Button(workspace.dictation.state == .recording || workspace.dictation.state == .starting ? "Finish dictation" : "Start dictation", action: workspace.dictation.accessiblePress)
                    .keyboardShortcut("d", modifiers: [.command, .shift])
                    .disabled(workspace.operation != nil || workspace.loading || workspace.savingPreference || !workspace.dictation.canHold || workspace.dictation.state == .preparing)
            }
        }
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
            .onOpenURL { workspace.open($0) }
    }
}
