// Disposable, local-only native target for end-to-end dictation/paste validation.
import AppKit

@MainActor
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 420), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Fast Transcript Paste Test"
        let scroll = NSScrollView(frame: window.contentView!.bounds)
        scroll.hasVerticalScroller = true
        scroll.autoresizingMask = [.width, .height]
        let text = NSTextView(frame: scroll.bounds)
        text.isRichText = false
        text.font = .systemFont(ofSize: 16)
        text.autoresizingMask = [.width, .height]
        text.setAccessibilityIdentifier("pasteTestField")
        scroll.documentView = text
        window.contentView = scroll
        window.center()
        window.makeKeyAndOrderFront(nil)
        window.makeFirstResponder(text)
        self.window = window
        FileHandle.standardOutput.write(Data("Paste target ready\n".utf8))
        NSApp.activate(ignoringOtherApps: true)
    }
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { true }
}
MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.setActivationPolicy(.regular)
    let delegate = Delegate()
    app.delegate = delegate
    withExtendedLifetime(delegate) { app.run() }
}
