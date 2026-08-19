// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.3"
let releaseChecksum = "6fb9e29f6a4e1ef2583daa25463b54b7325b3ab70e43ee5d39661c03b358fb57"

let package = Package(
    name: "helix-rootshell",
    platforms: [
        .iOS("18.0"),
        .macCatalyst("18.0"),
        .visionOS("2.0"),
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
