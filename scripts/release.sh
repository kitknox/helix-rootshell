#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="${1:-}"
REMOTE="${HELIX_RELEASE_REMOTE:-origin}"

[[ -n "$VERSION" ]] || { echo "usage: $0 0.1.0" >&2; exit 2; }
VERSION="${VERSION#v}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "error: version must be semantic" >&2; exit 2; }

cd "$ROOT_DIR"
[[ "$(git branch --show-current)" == "main" ]] || { echo "error: releases must be made from main" >&2; exit 1; }
git diff --quiet && git diff --cached --quiet || { echo "error: working tree must be clean" >&2; exit 1; }
git rev-parse "v$VERSION" >/dev/null 2>&1 && { echo "error: tag v$VERSION already exists" >&2; exit 1; }
gh auth status >/dev/null

source_revision="$(git rev-parse HEAD)"
HELIX_FRAMEWORK_VERSION="$VERSION" "$ROOT_DIR/scripts/build-framework.sh"

zip_path="$ROOT_DIR/.build/HelixKit.xcframework.zip"
checksum="$(swift package compute-checksum "$zip_path")"

sed -i '' -E "s/let releaseVersion = \"[^\"]+\"/let releaseVersion = \"$VERSION\"/" Package.swift
sed -i '' -E "s/let releaseChecksum = \"[0-9a-f]+\"/let releaseChecksum = \"$checksum\"/" Package.swift
swift package dump-package >/dev/null

notes_path="$ROOT_DIR/.build/release.md"
cat >"$notes_path" <<EOF
# HelixKit v$VERSION

Binary Swift package for rootshell's Helix fork.

- Helix source revision: \`$source_revision\`
- Gitoxide revision: \`938506bf12c920a6f815425600075d387b5a603b\`
- Product: \`HelixKit\`
- Platforms: iOS, iOS Simulator, Mac Catalyst, visionOS, visionOS Simulator
EOF

git add Package.swift
git commit -m "Release v$VERSION"
git tag -a "v$VERSION" -m "HelixKit v$VERSION"
git push "$REMOTE" main
git push "$REMOTE" "v$VERSION"
gh release create "v$VERSION" "$zip_path" \
    --repo kitknox/helix-rootshell \
    --title "HelixKit v$VERSION" \
    --notes-file "$notes_path"

echo "Published HelixKit v$VERSION"
