// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "Fennec",
    platforms: [.macOS(.v15)],
    dependencies: [
        .package(url: "https://github.com/argmaxinc/WhisperKit.git", from: "1.0.0"),
    ],
    targets: [
        .executableTarget(
            name: "Fennec",
            dependencies: ["WhisperKit", "SpeakerKit"],
            path: "Sources",
            swiftSettings: [.swiftLanguageMode(.v5)]
        )
    ]
)
