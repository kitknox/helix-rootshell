// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.1"
let releaseChecksum = "ac7dc5f3efdcb5d726103c8584a09ed6d9658537dcb906081ea52168744f7c5a"

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
