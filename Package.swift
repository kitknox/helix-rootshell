// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.4"
let releaseChecksum = "0ca50a6a983f3a1cc23333ef7e95d582e2ed32f8a3daba64350aebc568f4baa3"

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
