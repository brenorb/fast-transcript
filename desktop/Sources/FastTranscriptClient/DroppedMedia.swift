import Foundation
import UniformTypeIdentifiers

/// Decode native drag payloads into the same path/URL strings accepted by fscript.
public func droppedMediaSource(_ item: NSSecureCoding?, typeIdentifier: String) -> String? {
    if let url = item as? URL { return url.isFileURL ? url.path : url.absoluteString }
    if let data = item as? Data, typeIdentifier != UTType.plainText.identifier {
        // URL wire data must be an encoded ASCII URI, not raw filesystem bytes.
        guard data.allSatisfy({ $0 < 128 }), let string = String(data: data, encoding: .utf8),
              let url = URL(string: string, encodingInvalidCharacters: false) else { return nil }
        return url.isFileURL ? url.path : url.absoluteString
    }
    let text: String?
    if let string = item as? String { text = string }
    else if let data = item as? Data { text = String(data: data, encoding: .utf8) }
    else { return nil }
    guard let text else { return nil }
    if typeIdentifier != UTType.plainText.identifier, let url = URL(string: text), url.isFileURL {
        return url.path
    }
    return text
}
