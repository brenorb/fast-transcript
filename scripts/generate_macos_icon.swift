import AppKit
import Foundation

/// Render the same native emoji used by the menu bar. Regeneration is opt-in;
/// ordinary app builds copy the checked-in icon, independent of installed fonts.
@main
struct GenerateMacOSIcon {
    static func main() throws {
        guard CommandLine.arguments.count == 2 else {
            throw NSError(domain: "FastTranscriptIcon", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "Usage: icon-generator OUTPUT.icns"])
        }
        let output = URL(fileURLWithPath: CommandLine.arguments[1]).standardizedFileURL
        let scratch = FileManager.default.temporaryDirectory.appendingPathComponent("fscript-icon-" + UUID().uuidString)
        let iconset = scratch.appendingPathComponent("FastTranscript.iconset")
        try FileManager.default.createDirectory(at: iconset, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: scratch) }

        for points in [16, 32, 128, 256, 512] {
            for scale in [1, 2] {
                let pixels = points * scale
                let bitmap = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
                                              bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                              isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
                bitmap.size = NSSize(width: pixels, height: pixels)
                NSGraphicsContext.saveGraphicsState()
                NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bitmap)
                let text = NSAttributedString(string: AppBrand.symbol,
                                              attributes: [.font: NSFont(name: "Apple Color Emoji", size: CGFloat(pixels) * 0.82)!])
                let size = text.size()
                text.draw(at: NSPoint(x: (CGFloat(pixels) - size.width) / 2,
                                      y: (CGFloat(pixels) - size.height) / 2))
                NSGraphicsContext.restoreGraphicsState()
                let suffix = scale == 2 ? "@2x" : ""
                try bitmap.representation(using: .png, properties: [:])!
                    .write(to: iconset.appendingPathComponent("icon_\(points)x\(points)\(suffix).png"))
            }
        }
        let converter = Process()
        converter.executableURL = URL(fileURLWithPath: "/usr/bin/iconutil")
        converter.arguments = ["--convert", "icns", "--output", output.path, iconset.path]
        try converter.run()
        converter.waitUntilExit()
        guard converter.terminationStatus == 0 else {
            throw NSError(domain: "FastTranscriptIcon", code: Int(converter.terminationStatus),
                          userInfo: [NSLocalizedDescriptionKey: "iconutil failed to assemble the app icon"])
        }
        print(output.path)
    }
}
