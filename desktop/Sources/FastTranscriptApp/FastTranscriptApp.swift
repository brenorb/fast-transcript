import AppKit
import SwiftUI

@MainActor
final class ApplicationDelegate: NSObject, NSApplicationDelegate {
    weak var workspace: Workspace?
    private var terminating = false

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let workspace else { return .terminateNow }
        guard !terminating else { return .terminateLater }
        terminating = true
        Task {
            await workspace.shutdown()
            sender.reply(toApplicationShouldTerminate: true)
        }
        return .terminateLater
    }
}

@main
@MainActor
struct FastTranscriptApp: App {
    @NSApplicationDelegateAdaptor(ApplicationDelegate.self) private var delegate
    @State private var workspace = Workspace()

    var body: some Scene {
        Window("Fast Transcript", id: "main") {
            ContentView(workspace: workspace)
                .task { delegate.workspace = workspace; await workspace.bootstrap() }
                .onOpenURL { workspace.open($0) }
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
