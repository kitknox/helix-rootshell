#!/bin/bash
set -euo pipefail

FRAMEWORK_PATH="${1:-}"
[[ -n "$FRAMEWORK_PATH" ]] || { echo "usage: $0 /path/to/HelixKit.xcframework" >&2; exit 2; }
[[ -f "$FRAMEWORK_PATH/Info.plist" ]] || { echo "error: invalid XCFramework" >&2; exit 1; }

EXPECTED_SLICES=(
    "ios-arm64:arm64:18.0"
    "ios-arm64-simulator:arm64:18.0"
    "ios-arm64_x86_64-maccatalyst:arm64 x86_64:18.0"
    "xros-arm64:arm64:26.0"
    "xros-arm64-simulator:arm64:26.0"
)
EXPECTED_SYMBOLS=(
    gix_main
    helix_create
    helix_create_with_args
    helix_destroy
    helix_is_running
    helix_last_error_code
    helix_resize
    helix_shutdown
    helix_version
)

library_count="$(plutil -extract AvailableLibraries raw -o - "$FRAMEWORK_PATH/Info.plist")"
[[ "$library_count" == "5" ]] || { echo "error: expected 5 library records, found $library_count" >&2; exit 1; }

for expected_slice in "${EXPECTED_SLICES[@]}"; do
    slice="${expected_slice%%:*}"
    remainder="${expected_slice#*:}"
    expected_archs="${remainder%%:*}"
    expected_minimum_os="${remainder##*:}"
    slice_path="$FRAMEWORK_PATH/$slice"
    library="$slice_path/libHelixKit.a"

    [[ -d "$slice_path" ]] || { echo "error: missing slice $slice" >&2; exit 1; }
    [[ -f "$library" ]] || { echo "error: missing static library in $slice" >&2; exit 1; }
    [[ -f "$slice_path/Headers/HelixKit/helix_ios.h" ]] || { echo "error: missing header in $slice" >&2; exit 1; }
    [[ -f "$slice_path/Headers/module.modulemap" ]] || { echo "error: missing module map in $slice" >&2; exit 1; }
    [[ ! -d "$slice_path/HelixKit.framework" ]] || { echo "error: static library is wrapped in a framework in $slice" >&2; exit 1; }

    record_index=""
    for ((index = 0; index < library_count; index++)); do
        identifier="$(plutil -extract "AvailableLibraries.$index.LibraryIdentifier" raw -o - "$FRAMEWORK_PATH/Info.plist")"
        if [[ "$identifier" == "$slice" ]]; then
            record_index="$index"
            break
        fi
    done
    [[ -n "$record_index" ]] || { echo "error: no XCFramework record for $slice" >&2; exit 1; }
    library_path="$(plutil -extract "AvailableLibraries.$record_index.LibraryPath" raw -o - "$FRAMEWORK_PATH/Info.plist")"
    headers_path="$(plutil -extract "AvailableLibraries.$record_index.HeadersPath" raw -o - "$FRAMEWORK_PATH/Info.plist")"
    [[ "$library_path" == "libHelixKit.a" ]] || { echo "error: $slice LibraryPath is $library_path" >&2; exit 1; }
    [[ "$headers_path" == "Headers" ]] || { echo "error: $slice HeadersPath is $headers_path" >&2; exit 1; }

    actual_archs="$(lipo -archs "$library")"
    for arch in $expected_archs; do
        [[ " $actual_archs " == *" $arch "* ]] || { echo "error: $slice missing $arch" >&2; exit 1; }
    done

    minimum_versions="$(otool -l "$library" 2>/dev/null | awk '$1 == "minos" { print $2 }' | sort -u)"
    [[ "$minimum_versions" == "$expected_minimum_os" ]] || {
        echo "error: $slice object minimum versions are '$minimum_versions', expected $expected_minimum_os" >&2
        exit 1
    }

    symbols="$(nm -gU "$library" 2>/dev/null || true)"
    for symbol in "${EXPECTED_SYMBOLS[@]}"; do
        count="$(grep -c " _${symbol}$" <<<"$symbols" || true)"
        [[ "$count" == "1" ]] || { echo "error: $slice exports $count copies of $symbol" >&2; exit 1; }
    done
done

smoke_root="$(mktemp -d "${TMPDIR:-/tmp}/helixkit-module-audit.XXXXXX")"
trap 'rm -rf "$smoke_root"' EXIT
printf '%s\n' '#include <HelixKit/helix_ios.h>' 'int main(void) { return 0; }' > "$smoke_root/module-smoke.c"
printf '%s\n' 'import HelixKit' > "$smoke_root/importer-smoke.swift"
xcrun --sdk iphonesimulator clang \
    -target "arm64-apple-ios18.0-simulator" \
    -fmodules \
    -fmodules-cache-path="$smoke_root/module-cache" \
    -I "$FRAMEWORK_PATH/ios-arm64-simulator/Headers" \
    -fsyntax-only "$smoke_root/module-smoke.c"
xcrun --sdk iphonesimulator swiftc \
    -target "arm64-apple-ios18.0-simulator" \
    -module-cache-path "$smoke_root/swift-module-cache" \
    -I "$FRAMEWORK_PATH/ios-arm64-simulator/Headers" \
    -typecheck "$smoke_root/importer-smoke.swift"

echo "HelixKit XCFramework audit passed"
