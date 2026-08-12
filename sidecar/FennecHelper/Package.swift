// swift-tools-version: 6.0
import PackageDescription
import Foundation

let packageRoot = URL(fileURLWithPath: #filePath).deletingLastPathComponent()
let infoPlistPath = packageRoot.appendingPathComponent("Sources/FennecHelper/Info.plist").path

let package = Package(
    name: "FennecHelper",
    platforms: [.macOS("15.0")],
    products: [
        .executable(name: "fennec-helper", targets: ["FennecHelper"])
    ],
    targets: [
        .executableTarget(
            name: "FennecHelper",
            exclude: ["Info.plist"],
            swiftSettings: [
                .swiftLanguageMode(.v5)
            ],
            linkerSettings: [
                .unsafeFlags([
                    "-Xlinker", "-sectcreate",
                    "-Xlinker", "__TEXT",
                    "-Xlinker", "__info_plist",
                    "-Xlinker", infoPlistPath,
                ])
            ]
        )
    ]
)
