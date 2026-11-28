import AppKit

enum AppBrand {
    static let symbol = "✍️"

    @MainActor static let menuBarImage: NSImage = {
        let image = NSImage(size: NSSize(width: 19, height: 19), flipped: false) { rect in
            let text = NSAttributedString(string: symbol, attributes: [
                .font: NSFont(name: "Apple Color Emoji", size: 17) ?? NSFont.systemFont(ofSize: 17)
            ])
            let size = text.size()
            text.draw(at: NSPoint(x: rect.midX - size.width / 2, y: rect.midY - size.height / 2))
            return true
        }
        // A native template uses only the glyph's alpha: macOS supplies the
        // appropriate monochrome color for the current menu-bar appearance.
        image.isTemplate = true
        return image
    }()
}
