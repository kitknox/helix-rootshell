#!/bin/bash
set -euo pipefail

FRAMEWORK_PATH="${1:-}"
[[ -n "$FRAMEWORK_PATH" ]] || { echo "usage: $0 /path/to/HelixKit.xcframework" >&2; exit 2; }
[[ -f "$FRAMEWORK_PATH/Info.plist" ]] || { echo "error: invalid XCFramework" >&2; exit 1; }

EXPECTED_SLICES=(
    "ios-arm64:arm64"
    "ios-arm64-simulator:arm64"
    "ios-arm64_x86_64-maccatalyst:arm64 x86_64"
    "xros-arm64:arm64"
    "xros-arm64-simulator:arm64"
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

for expected_slice in "${EXPECTED_SLICES[@]}"; do
    slice="${expected_slice%%:*}"
    expected_archs="${expected_slice#*:}"
    slice_path="$FRAMEWORK_PATH/$slice"
    [[ -d "$slice_path" ]] || { echo "error: missing slice $slice" >&2; exit 1; }
    framework="$slice_path/HelixKit.framework"
    library="$framework/HelixKit"
    [[ -f "$framework/Headers/helix_ios.h" ]] || { echo "error: missing header in $slice" >&2; exit 1; }
    [[ -f "$framework/Modules/module.modulemap" ]] || { echo "error: missing module map in $slice" >&2; exit 1; }
    if [[ "$slice" == *-maccatalyst ]]; then
        [[ -L "$framework/Versions/Current" ]] || { echo "error: Catalyst framework is not versioned" >&2; exit 1; }
        [[ -f "$framework/Versions/Current/Resources/Info.plist" ]] || { echo "error: missing versioned framework Info.plist in $slice" >&2; exit 1; }
    else
        [[ -f "$framework/Info.plist" ]] || { echo "error: missing framework Info.plist in $slice" >&2; exit 1; }
    fi
    [[ -f "$library" ]] || { echo "error: missing static framework binary in $slice" >&2; exit 1; }
    [[ ! -e "$slice_path/Headers/module.modulemap" ]] || { echo "error: library-style module map remains in $slice" >&2; exit 1; }

    actual_archs="$(lipo -archs "$library")"
    for arch in $expected_archs; do
        [[ " $actual_archs " == *" $arch "* ]] || { echo "error: $slice missing $arch" >&2; exit 1; }
    done

    symbols="$(nm -gU "$library" 2>/dev/null || true)"
    for symbol in "${EXPECTED_SYMBOLS[@]}"; do
        count="$(grep -c " _${symbol}$" <<<"$symbols" || true)"
        [[ "$count" == "1" ]] || { echo "error: $slice exports $count copies of $symbol" >&2; exit 1; }
    done
done

slice_count="$(find "$FRAMEWORK_PATH" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')"
[[ "$slice_count" == "5" ]] || { echo "error: expected 5 slices, found $slice_count" >&2; exit 1; }

echo "HelixKit audit passed"
