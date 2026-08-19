#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT_DIR/.build"
OUT_DIR="$BUILD_DIR/HelixKit.xcframework"
ZIP_PATH="$BUILD_DIR/HelixKit.xcframework.zip"
HEADER_DIR="$ROOT_DIR/helix-ios/include"
FRAMEWORK_INFO_PLIST="$ROOT_DIR/helix-ios/Info.plist"
FRAMEWORK_STAGE_DIR="$BUILD_DIR/frameworks"
MANIFEST="$ROOT_DIR/helix-ios/Cargo.toml"
TARGET_DIR="$ROOT_DIR/target"
RUST_TOOLCHAIN="${HELIX_RUST_TOOLCHAIN:-}"
EXPECTED_RUSTC_COMMIT="7057231bd78d6c7893f905ea1832365d4c5efe17"

MIN_IOS_VERSION="18.0"
MIN_CATALYST_VERSION="18.0"
MIN_XROS_VERSION="2.0"
FRAMEWORK_VERSION="${HELIX_FRAMEWORK_VERSION:-}"

BUILD_RELEASE=true
SKIP_CATALYST=false
SKIP_VISIONOS=false
SKIP_BUILD="${HELIX_SKIP_BUILD:-false}"

die() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*"; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) BUILD_RELEASE=false ;;
        --skip-catalyst) SKIP_CATALYST=true ;;
        --skip-visionos) SKIP_VISIONOS=true ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done

for command_name in xcrun xcodebuild lipo ditto plutil swift; do
    command -v "$command_name" >/dev/null || die "$command_name not found"
done

if [[ "$SKIP_BUILD" != "true" ]]; then
    for command_name in cargo rustup; do
        command -v "$command_name" >/dev/null || die "$command_name not found"
    done

    if [[ -n "$RUST_TOOLCHAIN" ]]; then
        actual_rustc_commit="$(rustup run "$RUST_TOOLCHAIN" rustc -Vv 2>/dev/null | awk '/^commit-hash:/ { print $2 }' || true)"
        [[ -n "$actual_rustc_commit" ]] || die "Rust toolchain $RUST_TOOLCHAIN is not installed"
        [[ "$actual_rustc_commit" == "$EXPECTED_RUSTC_COMMIT" ]] || \
            die "Rust toolchain $RUST_TOOLCHAIN has commit $actual_rustc_commit; expected $EXPECTED_RUSTC_COMMIT"
    else
        for candidate in nightly-2026-02-12 nightly; do
            candidate_commit="$(rustup run "$candidate" rustc -Vv 2>/dev/null | awk '/^commit-hash:/ { print $2 }' || true)"
            if [[ "$candidate_commit" == "$EXPECTED_RUSTC_COMMIT" ]]; then
                RUST_TOOLCHAIN="$candidate"
                break
            fi
        done
        [[ -n "$RUST_TOOLCHAIN" ]] || \
            die "no installed Rust toolchain has expected commit $EXPECTED_RUSTC_COMMIT"
    fi
fi

[[ -f "$MANIFEST" ]] || die "helix-ios crate not found"
[[ -f "$HEADER_DIR/helix_ios.h" ]] || die "helix_ios.h not found"
[[ -f "$HEADER_DIR/module.modulemap" ]] || die "module.modulemap not found"
[[ -f "$FRAMEWORK_INFO_PLIST" ]] || die "HelixKit framework Info.plist not found"
if [[ -z "$FRAMEWORK_VERSION" ]]; then
    FRAMEWORK_VERSION="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$FRAMEWORK_INFO_PLIST")"
fi

if [[ "$SKIP_BUILD" != "true" && ! -d "$ROOT_DIR/runtime/grammars/sources/rust/src" ]]; then
    info "Fetching tree-sitter grammar sources"
    cargo "+${RUST_TOOLCHAIN}" run --manifest-path "$ROOT_DIR/helix-term/Cargo.toml" -- --grammar fetch
fi

SDKROOT_IOS="$(xcrun --sdk iphoneos --show-sdk-path)"
SDKROOT_SIM="$(xcrun --sdk iphonesimulator --show-sdk-path)"
SDKROOT_MACOS="$(xcrun --sdk macosx --show-sdk-path)"
SDKROOT_XROS=""
SDKROOT_XROS_SIM=""

if xcrun --sdk xros --show-sdk-path >/dev/null 2>&1; then
    SDKROOT_XROS="$(xcrun --sdk xros --show-sdk-path)"
fi
if xcrun --sdk xrsimulator --show-sdk-path >/dev/null 2>&1; then
    SDKROOT_XROS_SIM="$(xcrun --sdk xrsimulator --show-sdk-path)"
fi

PROFILE_ARGS=()
PROFILE_DIR="debug"
if $BUILD_RELEASE; then
    PROFILE_ARGS=(--release)
    PROFILE_DIR="release"
fi

build_target() {
    local target="$1"
    local sdk_path="$2"
    local deploy_key
    local deploy_value
    local cflags_key="CFLAGS_${target//-/_}"

    case "$target" in
        *-apple-ios-macabi)
            deploy_key="IPHONEOS_DEPLOYMENT_TARGET"
            deploy_value="$MIN_CATALYST_VERSION"
            ;;
        *-apple-ios|*-apple-ios-sim)
            deploy_key="IPHONEOS_DEPLOYMENT_TARGET"
            deploy_value="$MIN_IOS_VERSION"
            ;;
        *-apple-visionos|*-apple-visionos-sim)
            deploy_key="XROS_DEPLOYMENT_TARGET"
            deploy_value="$MIN_XROS_VERSION"
            ;;
        *) die "unsupported target: $target" ;;
    esac

    info "Building $target"
    env \
        "SDKROOT=$sdk_path" \
        "$deploy_key=$deploy_value" \
        "$cflags_key=-isysroot $sdk_path" \
        cargo "+${RUST_TOOLCHAIN}" build \
            --locked \
            --manifest-path "$MANIFEST" \
            --target "$target" \
            "${PROFILE_ARGS[@]}" \
            -Z build-std
}

if [[ "$SKIP_BUILD" != "true" ]]; then
    build_target aarch64-apple-ios "$SDKROOT_IOS"
    build_target aarch64-apple-ios-sim "$SDKROOT_SIM"
fi

stage_framework() {
    local archive="$1"
    local stage_name="$2"
    local supported_platform="$3"
    local minimum_os_version="$4"
    local framework_path="$FRAMEWORK_STAGE_DIR/$stage_name/HelixKit.framework"
    local info_plist

    rm -rf "$framework_path"
    if [[ "$stage_name" == *-maccatalyst ]]; then
        mkdir -p \
            "$framework_path/Versions/A/Headers" \
            "$framework_path/Versions/A/Modules" \
            "$framework_path/Versions/A/Resources"
        cp "$archive" "$framework_path/Versions/A/HelixKit"
        cp "$HEADER_DIR/helix_ios.h" "$framework_path/Versions/A/Headers/helix_ios.h"
        cp "$HEADER_DIR/module.modulemap" "$framework_path/Versions/A/Modules/module.modulemap"
        info_plist="$framework_path/Versions/A/Resources/Info.plist"
        cp "$FRAMEWORK_INFO_PLIST" "$info_plist"
        ln -s A "$framework_path/Versions/Current"
        ln -s Versions/Current/HelixKit "$framework_path/HelixKit"
        ln -s Versions/Current/Headers "$framework_path/Headers"
        ln -s Versions/Current/Modules "$framework_path/Modules"
        ln -s Versions/Current/Resources "$framework_path/Resources"
    else
        mkdir -p "$framework_path/Headers" "$framework_path/Modules"
        cp "$archive" "$framework_path/HelixKit"
        cp "$HEADER_DIR/helix_ios.h" "$framework_path/Headers/helix_ios.h"
        cp "$HEADER_DIR/module.modulemap" "$framework_path/Modules/module.modulemap"
        info_plist="$framework_path/Info.plist"
        cp "$FRAMEWORK_INFO_PLIST" "$info_plist"
    fi

    /usr/libexec/PlistBuddy -c 'Delete :CFBundleSupportedPlatforms' "$info_plist" >/dev/null 2>&1 || true
    /usr/libexec/PlistBuddy -c 'Delete :MinimumOSVersion' "$info_plist" >/dev/null 2>&1 || true
    /usr/libexec/PlistBuddy -c 'Add :CFBundleSupportedPlatforms array' "$info_plist"
    /usr/libexec/PlistBuddy -c "Add :CFBundleSupportedPlatforms:0 string $supported_platform" "$info_plist"
    /usr/libexec/PlistBuddy -c "Add :MinimumOSVersion string $minimum_os_version" "$info_plist"
    /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $FRAMEWORK_VERSION" "$info_plist"
    printf '%s\n' "$framework_path"
}

mkdir -p "$FRAMEWORK_STAGE_DIR"
ios_framework="$(stage_framework "$TARGET_DIR/aarch64-apple-ios/$PROFILE_DIR/libhelix_ios.a" ios-arm64 iPhoneOS "$MIN_IOS_VERSION")"
simulator_framework="$(stage_framework "$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE_DIR/libhelix_ios.a" ios-arm64-simulator iPhoneSimulator "$MIN_IOS_VERSION")"

XCFRAMEWORK_ARGS=(
    -framework "$ios_framework"
    -framework "$simulator_framework"
)

if ! $SKIP_CATALYST; then
    if [[ "$SKIP_BUILD" != "true" ]]; then
        build_target aarch64-apple-ios-macabi "$SDKROOT_MACOS"
        build_target x86_64-apple-ios-macabi "$SDKROOT_MACOS"
    fi

    catalyst_library="$TARGET_DIR/libhelix_ios_catalyst_universal.a"
    lipo -create \
        "$TARGET_DIR/aarch64-apple-ios-macabi/$PROFILE_DIR/libhelix_ios.a" \
        "$TARGET_DIR/x86_64-apple-ios-macabi/$PROFILE_DIR/libhelix_ios.a" \
        -output "$catalyst_library"
    catalyst_framework="$(stage_framework "$catalyst_library" ios-arm64_x86_64-maccatalyst MacOSX "$MIN_CATALYST_VERSION")"
    XCFRAMEWORK_ARGS+=(-framework "$catalyst_framework")
fi

if ! $SKIP_VISIONOS; then
    [[ -n "$SDKROOT_XROS" && -n "$SDKROOT_XROS_SIM" ]] || \
        die "visionOS device and simulator SDKs are required"
    if [[ "$SKIP_BUILD" != "true" ]]; then
        build_target aarch64-apple-visionos "$SDKROOT_XROS"
        build_target aarch64-apple-visionos-sim "$SDKROOT_XROS_SIM"
    fi
    visionos_framework="$(stage_framework "$TARGET_DIR/aarch64-apple-visionos/$PROFILE_DIR/libhelix_ios.a" xros-arm64 XROS "$MIN_XROS_VERSION")"
    visionos_simulator_framework="$(stage_framework "$TARGET_DIR/aarch64-apple-visionos-sim/$PROFILE_DIR/libhelix_ios.a" xros-arm64-simulator XRSimulator "$MIN_XROS_VERSION")"
    XCFRAMEWORK_ARGS+=(
        -framework "$visionos_framework"
        -framework "$visionos_simulator_framework"
    )
fi

mkdir -p "$BUILD_DIR"
rm -rf "$OUT_DIR"
rm -f "$ZIP_PATH"

info "Creating HelixKit.xcframework"
xcodebuild -create-xcframework "${XCFRAMEWORK_ARGS[@]}" -output "$OUT_DIR"

HELIX_FRAMEWORK_VERSION="$FRAMEWORK_VERSION" "$ROOT_DIR/scripts/audit-framework.sh" "$OUT_DIR"

info "Creating SwiftPM archive"
ditto -c -k --sequesterRsrc --keepParent "$OUT_DIR" "$ZIP_PATH"

checksum="$(swift package compute-checksum "$ZIP_PATH")"
info "Artifact: $ZIP_PATH"
info "Checksum: $checksum"
