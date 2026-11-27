// Disposable, local-only native target for end-to-end dictation/paste validation.
import AppKit

@MainActor
final class PasteTextView: NSTextView {
    func recordState(_ kind: String) {
        guard let path = ProcessInfo.processInfo.environment["FSCRIPT_PASTE_TEST_RESULT"],
              let data = try? JSONSerialization.data(withJSONObject: ["type": kind, "pid": ProcessInfo.processInfo.processIdentifier, "text": string]) else { return }
        let url = URL(fileURLWithPath: path)
        if !FileManager.default.fileExists(atPath: path) { FileManager.default.createFile(atPath: path, contents: nil) }
        if let file = try? FileHandle(forWritingTo: url) {
            _ = try? file.seekToEnd()
            try? file.write(contentsOf: data + Data([10]))
            try? file.close()
        }
    }

    override func paste(_ sender: Any?) {
        super.paste(sender)
        recordState("paste")
        if ProcessInfo.processInfo.environment["FSCRIPT_PASTE_TEST_EVENTS"] == "1" {
            print("Paste action reached; characters=\(string.count)")
            fflush(stdout)
        }
    }

    override func didChangeText() {
        super.didChangeText()
        recordState("change")
    }
}

@MainActor
final class Delegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?
    var eventMonitor: Any?
    func applicationDidFinishLaunching(_ notification: Notification) {
        if ProcessInfo.processInfo.environment["FSCRIPT_PASTE_TEST_EVENTS"] == "1" {
            eventMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .keyUp, .flagsChanged]) { event in
                // No typed text is logged: only delivery/routing metadata.
                let isV = event.type != .flagsChanged && event.charactersIgnoringModifiers?.lowercased() == "v"
                print("Event type=\(event.type.rawValue) key=\(event.keyCode) command=\(event.modifierFlags.contains(.command)) isV=\(isV) active=\(NSApp.isActive) keyWindow=\(NSApp.keyWindow != nil)")
                fflush(stdout)
                return event
            }
        }
        // Command-V is a menu key equivalent in AppKit. A bare NSTextView
        // without an Edit menu is not a representative native paste target.
        let menu = NSMenu()
        let applicationItem = NSMenuItem()
        menu.addItem(applicationItem)
        let applicationMenu = NSMenu(title: "Paste Test")
        applicationMenu.addItem(withTitle: "Quit Paste Test", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        applicationItem.submenu = applicationMenu
        let editItem = NSMenuItem()
        menu.addItem(editItem)
        let edit = NSMenu(title: "Edit")
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        editItem.submenu = edit
        NSApp.mainMenu = menu
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 700, height: 420), styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false)
        window.title = "Fast Transcript Paste Test"
        let scroll = NSScrollView(frame: window.contentView!.bounds)
        scroll.hasVerticalScroller = true
        scroll.autoresizingMask = [.width, .height]
        let text = PasteTextView(frame: scroll.bounds)
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
        text.recordState("ready")
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
