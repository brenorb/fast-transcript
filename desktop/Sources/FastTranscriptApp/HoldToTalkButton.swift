import AppKit
import SwiftUI

/// Native down/up events leave the main run loop free for the CLI stream.
struct HoldToTalkButton: NSViewRepresentable {
    let title: String
    let enabled: Bool
    let shortcutLabel: String?
    let pressed: (Bool) -> Void
    let accessiblePress: () -> Void

    func makeNSView(context: Context) -> HoldButton {
        let button = HoldButton()
        button.bezelStyle = .rounded
        button.image = NSImage(systemSymbolName: "mic.fill", accessibilityDescription: nil)
        button.imagePosition = .imageLeading
        button.setAccessibilityIdentifier("holdToDictate")
        return button
    }
    func updateNSView(_ button: HoldButton, context: Context) {
        button.title = title
        button.isEnabled = enabled
        button.pressed = pressed
        button.accessiblePress = accessiblePress
        let shortcut = shortcutLabel.map { " Or hold \($0) anywhere after dictation is prepared." } ?? ""
        button.toolTip = "Hold the button to dictate; release to transcribe.\(shortcut) Return or accessibility activation toggles start/stop."
    }
}

final class HoldButton: NSButton {
    var pressed: ((Bool) -> Void)?
    var accessiblePress: (() -> Void)?
    private var holding = false
    override var acceptsFirstResponder: Bool { true }
    override func acceptsFirstMouse(for event: NSEvent?) -> Bool { true }
    override func mouseDown(with event: NSEvent) {
        guard isEnabled else { return }
        holding = true; isHighlighted = true
        window?.makeFirstResponder(self)
        pressed?(true)
    }
    override func mouseUp(with event: NSEvent) { release() }
    override func keyDown(with event: NSEvent) {
        if [36, 76].contains(event.keyCode) && !event.isARepeat && isEnabled { accessiblePress?(); return }
        if event.keyCode == 49 && !event.isARepeat && isEnabled { holding = true; isHighlighted = true; pressed?(true) }
        else { super.keyDown(with: event) }
    }
    override func keyUp(with event: NSEvent) {
        if event.keyCode == 49 { release() } else { super.keyUp(with: event) }
    }
    override func resignFirstResponder() -> Bool { release(); return super.resignFirstResponder() }
    override func viewWillMove(toWindow newWindow: NSWindow?) { if newWindow == nil { release() }; super.viewWillMove(toWindow: newWindow) }
    override func accessibilityPerformPress() -> Bool { guard isEnabled else { return false }; accessiblePress?(); return true }
    private func release() { if holding { holding = false; isHighlighted = false; pressed?(false) } }
}
