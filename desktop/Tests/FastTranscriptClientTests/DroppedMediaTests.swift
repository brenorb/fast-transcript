import Foundation
import UniformTypeIdentifiers
import XCTest
@testable import FastTranscriptClient

final class DroppedMediaTests: XCTestCase {
    func testPropertyFileURLsAndByteStringPayloadsPreservePaths() {
        for index in 0..<500 {
            let path = "/tmp/speech \(index) é 🦀 # % ' [x].wav"
            let url = URL(fileURLWithPath: path)
            for item: NSSecureCoding in [url as NSURL, url.absoluteString as NSString, Data(url.absoluteString.utf8) as NSData] {
                let actual = droppedMediaSource(item, typeIdentifier: UTType.fileURL.identifier)
                XCTAssertEqual(actual, path, "Payload type \(type(of: item)): \(item)")
                guard actual == path else { return }
            }
            XCTAssertEqual(droppedMediaSource(path as NSString, typeIdentifier: UTType.plainText.identifier), path)
            let remote = "https://example.test/clip?id=\(index)&part=first"
            XCTAssertEqual(droppedMediaSource(remote as NSString, typeIdentifier: UTType.url.identifier), remote)
        }
        XCTAssertNil(droppedMediaSource(Data([0xff]) as NSData, typeIdentifier: UTType.plainText.identifier))
        XCTAssertNil(droppedMediaSource(Data("file:///tmp/raw space.wav".utf8) as NSData, typeIdentifier: UTType.fileURL.identifier))
    }
}
