import Foundation

/// Byte framing shared by media and long-lived recording protocols.
struct JSONLines {
    private var pending = Data()

    mutating func append(_ bytes: Data) -> [Data] {
        pending.append(bytes)
        var lines: [Data] = []
        while let newline = pending.firstIndex(of: 10) {
            let line = Data(pending[..<newline])
            pending.removeSubrange(...newline)
            if !line.allSatisfy({ [UInt8(9), 13, 32].contains($0) }) { lines.append(line) }
        }
        return lines
    }

    func finish() throws {
        guard pending.allSatisfy({ [UInt8(9), 10, 13, 32].contains($0) }) else {
            throw ClientError("CLI stream ended with an incomplete event.")
        }
    }
}
