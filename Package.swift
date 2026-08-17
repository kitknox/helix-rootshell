// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.2"
let releaseChecksum = "ea99f2c1aaf43e6b68926558670b2e9273f545a68fcd65746da188fd8ec54105"

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
