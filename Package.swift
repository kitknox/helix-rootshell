// swift-tools-version: 5.9
import PackageDescription

let releaseVersion = "0.1.6"
let releaseChecksum = "6a57142657aa2c33dc91754f04f53986fe8c2feac5c331d18ed8b29cfa46976d"

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
