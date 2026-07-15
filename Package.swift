// swift-tools-version: 5.9

import PackageDescription

let package = Package(
    name: "HITAutoLogin",
    platforms: [
        .macOS(.v13)
    ],
    products: [
        .executable(name: "HITAutoLogin", targets: ["HITAutoLogin"])
    ],
    targets: [
        .executableTarget(
            name: "HITAutoLogin",
            path: "Sources/HITAutoLogin"
        ),
        .testTarget(
            name: "HITAutoLoginTests",
            dependencies: ["HITAutoLogin"],
            path: "Tests/HITAutoLoginTests"
        )
    ]
)
