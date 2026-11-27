// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "FastTranscriptDesktop",
    platforms: [.macOS("14.2")],
    products: [.executable(name: "FastTranscriptApp", targets: ["FastTranscriptApp"])],
    targets: [
        .target(name: "FastTranscriptClient"),
        .executableTarget(name: "FastTranscriptApp", dependencies: ["FastTranscriptClient"]),
        .testTarget(name: "FastTranscriptClientTests", dependencies: ["FastTranscriptClient"]),
        .testTarget(name: "FastTranscriptAppTests", dependencies: ["FastTranscriptApp"]),
    ]
)
