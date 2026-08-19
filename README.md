# Helix — rootshell fork

This repository is the [rootshell](https://www.rootshell.com)-maintained fork
of [helix-editor/helix](https://github.com/helix-editor/helix). It contains the
complete Helix source used by rootshell together with an iOS terminal backend,
the `helix-ios` C interface, and the Gitoxide-powered `gix` command. The fork is
maintained independently and does not automatically track later upstream
changes.

The initial fork is based on upstream commit
[`6be178fe`](https://github.com/helix-editor/helix/commit/6be178fe8e721c7ae54060c58f4913f37928c4be).
Gitoxide dependencies are resolved from one exact source revision so Helix's
diff gutter and the exported `gix_main` command use the same implementation.

## Swift binary package

The public Swift package exposes the static `HelixKit` product for:

- iOS 18 or later (arm64 device and arm64 simulator)
- Mac Catalyst 18 or later (arm64 and x86_64)
- visionOS 2 or later (arm64 device and arm64 simulator)

Add this repository as a Swift package dependency, pin an exact release, and
select the `HelixKit` product:

```text
https://github.com/kitknox/helix-rootshell.git
```

Helix runtime data is intentionally not part of the binary product. rootshell
bundles the matching `runtime/queries`, `runtime/themes`, and `runtime/tutor`
files in its application resources and passes that directory to HelixKit.

## Building and publishing

Release builds require rustc commit `7057231bd78d6c7893f905ea1832365d4c5efe17`
(the `nightly-2026-02-12` toolchain), Xcode command-line tools, and the iOS and
visionOS SDKs. The build accepts either the dated toolchain or an installed
`nightly` alias at that exact commit. Gitoxide and tree-house are pinned Git
dependencies, so a sibling source checkout is not required.

```sh
rustup toolchain install nightly-2026-02-12 --component rust-src
./scripts/build-framework.sh
```

The build produces ignored `.build/HelixKit.xcframework` and
`.build/HelixKit.xcframework.zip` artifacts, audits all platform slices and C
exports, and prints the SwiftPM checksum. Authenticated maintainers can publish
an exact release from a clean `main` branch:

```sh
./scripts/release.sh 0.1.0
```

Report rootshell application problems in the
[rootshell issue tracker](https://github.com/kitknox/rootshell/issues). Report
reproducible upstream Helix problems to the upstream project.

---

<div align="center">

<h1>
<picture>
  <source media="(prefers-color-scheme: dark)" srcset="logo_dark.svg">
  <source media="(prefers-color-scheme: light)" srcset="logo_light.svg">
  <img alt="Helix" height="128" src="logo_light.svg">
</picture>
</h1>

[![Build status](https://github.com/helix-editor/helix/actions/workflows/build.yml/badge.svg)](https://github.com/helix-editor/helix/actions)
[![GitHub Release](https://img.shields.io/github/v/release/helix-editor/helix)](https://github.com/helix-editor/helix/releases/latest)
[![Documentation](https://shields.io/badge/-documentation-452859)](https://docs.helix-editor.com/)
[![GitHub contributors](https://img.shields.io/github/contributors/helix-editor/helix)](https://github.com/helix-editor/helix/graphs/contributors)
[![Matrix Space](https://img.shields.io/matrix/helix-community:matrix.org)](https://matrix.to/#/#helix-community:matrix.org)

</div>

![Screenshot](./screenshot.png)

A [Kakoune](https://github.com/mawww/kakoune) / [Neovim](https://github.com/neovim/neovim) inspired editor, written in Rust.

The editing model is very heavily based on Kakoune; during development I found
myself agreeing with most of Kakoune's design decisions.

For more information, see the [website](https://helix-editor.com) or
[documentation](https://docs.helix-editor.com/).

All shortcuts/keymaps can be found [in the documentation on the website](https://docs.helix-editor.com/keymap.html).

[Troubleshooting](https://github.com/helix-editor/helix/wiki/Troubleshooting)

# Features

- Vim-like modal editing
- Multiple selections
- Built-in language server support
- Smart, incremental syntax highlighting and code editing via tree-sitter

Although it's primarily a terminal-based editor, I am interested in exploring
a custom renderer (similar to Emacs) using wgpu.

Note: Only certain languages have indentation definitions at the moment. Check
`runtime/queries/<lang>/` for `indents.scm`.

# Installation

[Installation documentation](https://docs.helix-editor.com/install.html).

[![Packaging status](https://repology.org/badge/vertical-allrepos/helix-editor.svg?exclude_unsupported=1)](https://repology.org/project/helix-editor/versions)

# Contributing

Contributing guidelines can be found [here](./docs/CONTRIBUTING.md).

# Getting help

Your question might already be answered on the [FAQ](https://github.com/helix-editor/helix/wiki/FAQ).

Discuss the project on the community [Matrix Space](https://matrix.to/#/#helix-community:matrix.org) (make sure to join `#helix-editor:matrix.org` if you're on a client that doesn't support Matrix Spaces yet).

# Credits

Thanks to [@jakenvac](https://github.com/jakenvac) for designing the logo!
