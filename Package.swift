// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.7"
let releaseChecksum = "219ec3d5f9238d3d5ef5fda8c6d2e37e892f457eeb6016a081c54c1def2fd1ef"

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
