import AppKit
import XCTest
@testable import FastTranscriptApp

final class WindowCloseObserverTests: XCTestCase {
    @MainActor
    func testPropertyOnlyTheHostingWindowCanFinishAndDetachmentRemovesObserver() {
        // Unshown windows in this test process; no desktop interaction or device access.
        let application = NSApplication.shared
        application.setActivationPolicy(.prohibited)
        let windows = (0..<2).map { _ in
            let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 20, height: 20), styleMask: .titled, backing: .buffered, defer: false)
            window.isReleasedWhenClosed = false
            return window
        }
        defer { windows.forEach { $0.close() } }
        let view = WindowCloseView(frame: .zero)
        var finishes = 0
        view.onClose = { finishes += 1 }
        var seed: UInt64 = 0xF5C71
        for _ in 0..<500 {
            seed = seed &* 6364136223846793005 &+ 1
            let index = Int((seed >> 32) & 1)
            windows[index].contentView!.addSubview(view)
            let before = finishes
            NotificationCenter.default.post(name: NSWindow.willCloseNotification, object: windows[1 - index])
            XCTAssertEqual(finishes, before, "Another window or menu must not finish dictation")
            NotificationCenter.default.post(name: NSWindow.willCloseNotification, object: windows[index])
            XCTAssertEqual(finishes, before + 1, "The hosting window must finish exactly once")
            view.removeFromSuperview()
            windows.forEach { NotificationCenter.default.post(name: NSWindow.willCloseNotification, object: $0) }
            XCTAssertEqual(finishes, before + 1, "A detached observer must not receive close events")
        }
    }
}
