import AppKit
import SwiftUI

/// Observe only the window hosting this view, excluding menus and other panels.
@MainActor
struct WindowCloseObserver: NSViewRepresentable {
    let onClose: () -> Void

    func makeNSView(context: Context) -> WindowCloseView {
        let view = WindowCloseView(frame: .zero)
        view.onClose = onClose
        view.setAccessibilityElement(false)
        return view
    }

    func updateNSView(_ view: WindowCloseView, context: Context) { view.onClose = onClose }
}

@MainActor
final class WindowCloseView: NSView {
    var onClose: (() -> Void)?
    private var observer: NSObjectProtocol?

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if let observer { NotificationCenter.default.removeObserver(observer) }
        observer = nil
        guard let window else { return }
        observer = NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.onClose?() }
        }
    }

    deinit { if let observer { NotificationCenter.default.removeObserver(observer) } }
}
