// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.5"
let releaseChecksum = "d51f1e265635f5b1a36f715a7f55d559131eb213a378690cb600851ed507a20a"

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
