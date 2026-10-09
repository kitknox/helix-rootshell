// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.8"
let releaseChecksum = "d3ef08fc1aebbd7ef2279607ec20b70bcf2fe5d84ffbb8803978d592d64e5a0f"

let package = Package(
    name: "helix-rootshell",
    platforms: [
        .iOS("18.0"),
        .macCatalyst("18.0"),
        .visionOS("26.0"),
    ],
    products: [
        .library(name: "HelixKit", targets: ["HelixKit"]),
    ],
    targets: [
        .binaryTarget(
            name: "HelixKit",
            url: "https://github.com/kitknox/helix-rootshell/releases/download/v\(releaseVersion)/HelixKit.xcframework.zip",
            checksum: releaseChecksum
        ),
    ]
)
