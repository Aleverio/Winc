<p align="center">
  <img src="./crates/winc-app/assets/icon.svg" alt="winc" width="128" />
</p>

<h1 align="center">Winct - Alverion's Fork</h1>

<p align="center">
  An Alverion-maintained fork of Winc, with independent features and customizations.
</p>

<p align="center">
  English | <a href="./README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="https://github.com/DEEIX-AI/Winc"><img alt="Upstream project" src="https://img.shields.io/badge/Upstream-DEEIX--AI%2FWinc-black" /></a>
  <a href="./LICENSE"><img alt="License" src="https://img.shields.io/badge/License-Apache%202.0-blue" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2024-b7410e" />
  <img alt="Platforms" src="https://img.shields.io/badge/Platforms-macOS%20%7C%20Linux%20%7C%20Windows-555" />
</p>

<p align="center">
  <a href="https://github.com/Aleverio/Winc/releases"><img alt="Download Alverion's Winc builds" src="./.github/assets/download-en.svg" /></a>
</p>

> **About this fork:** This is Alverion's independent fork of the original
> [Winc project by DEEIX-AI](https://github.com/DEEIX-AI/Winc). The upstream authors and project
> remain credited; this fork focuses on Alverion's additions and customizations.

## Overview

Based on Winc's Rust editor and rendering engine, this fork keeps the upstream editing experience
while developing additional features independently. The upstream project reports a 0.1-second
startup and 52 MB of memory with 100,000 lines open; those figures are upstream measurements, not
benchmarks of Alverion's changes.

| Measure | Result |
| --- | --- |
| Cold start to first frame | 119 ms (median of 20 runs) |
| Resident memory with 100,000 lines open | 52 MB |
| A 1 GB file, editable and highlighted | 220 ms |

Measured on macOS; results vary with hardware and system load.

## Alverion's additions

This fork adds a more configurable language-server workflow:

- Choose automatic or custom language-server commands per language.
- See LSP connection status in the status bar.
- View server diagnostics as underlines on the affected text, with line, column, and message details.

These are Alverion's fork changes; the original Winc project and its authors are credited above.

## Features

| Area | Capabilities |
| --- | --- |
| Editing | Multiple cursors, find and replace, native input methods, smooth scrolling in gigabyte-sized files, and an experimental Vim mode |
| Syntax | Highlighting for common programming, markup, data, and configuration languages, LSP hover and diagnostics, plus language-server or lightweight go-to-definition and usages |
| Formatting | Whole-file formatting with external tools such as rustfmt, Prettier, Black, Ruff, and gofmt; LSP formatting as a fallback |
| Preview | Markdown with math and Mermaid diagrams, images, SVG, PDF (macOS and Windows), Word documents (docx), and spreadsheets (xlsx, csv), with the source one click away |
| Git | Side-by-side diffs with word-level highlights, staging by hunk, commit and push, a commit graph, and blame |
| Project | A file tree, quick open, and project search that follows `.gitignore` |
| Terminal | An integrated terminal with automatic loading of project-local Python environments |
| Remote | Experimental SSH remote: open a folder on a Linux host and edit, search, format, use Git, and open terminals there. On Windows, open folders inside WSL distributions |
| Interface | Sixteen built-in themes, light and dark, and a complete interface in English and Simplified Chinese |
| Modules | Git, the terminal, and SSH remote can each be turned off. First-time setup on the start page offers the appearance, theme, language, and modules |
| Updates | Signed in-app updates on all three platforms, with the release notes shown in the app after each update |
| LSP | Experimental: incremental synchronization, diagnostics, hover, definition/declaration/type/implementation and reference navigation, manual completion, signature help, quick-fix code actions, and formatting fallback |

Touchscreens support tapping buttons and placing the caret, double-tapping to select a word,
swiping to scroll, and dragging scrollbars and panel dividers. Hold for 500 ms to open the context
menu. Swipes stay in the region where they started and stop on release. Additional fingers cancel
the current gesture; lift all fingers before starting another. On Windows, drag the title bar to
move the window, double-tap it to maximize or restore, and drag the window edges to resize.
Window dragging and edge snapping use the same native Windows behavior as mouse input.

Image and SVG previews zoom like the PDF preview: hold Ctrl (⌘ on macOS) and scroll, pinch, or
press Ctrl/⌘ with `+`, `-` or `0`; the point under the pointer stays in place. Scroll or drag a
zoomed image to pan, and double-click to fit it again. This also works in the SVG split preview.

## Platforms

| Platform | Requirements | Package |
| --- | --- | --- |
| macOS | macOS 11 or later, Apple silicon or Intel | Disk image |
| Linux | x86_64, X11 or Wayland | AppImage, deb, Arch repository |
| Windows | Windows 10 1809 or later, x86_64 | NSIS, MSI, portable ZIP |

The portable ZIP keeps settings and state in the `data` folder beside `winc.exe`; delete that
folder to keep them in the user profile instead.

## Language Servers

Install a language server separately. In **Settings → Language Servers**, enable automatic
selection for a built-in default, or enter a custom executable and arguments for a language. Servers
must already be installed and available on `PATH`. The same choices can be stored in the user
configuration file:

```json
{
  "lsp": {
    "auto": ["rust"],
    "servers": {
      "python": ["pyright-langserver", "--stdio"]
    }
  }
}
```

Winc sends opened and edited files to the selected server, underlines reported diagnostics, and
shows their line and column when you click the diagnostic count in the status bar. Use **Show
Hover** (Ctrl/Cmd+K, Ctrl/Cmd+I) to request hover information.
Navigation, signature help, completion, quick fixes, and formatting depend on the selected server's
advertised capabilities; when navigation returns no location, Winc falls back to its lightweight
local search.
Use **F12** for definition, **Shift+F12** for references, **Alt+F12** for declaration,
**Ctrl/Cmd+F12** for implementation, and **Ctrl/Cmd+Shift+F12** for type definition.

| Language | Automatic server command | Current LSP features |
| --- | --- | --- |
| Rust | `rust-analyzer` | Diagnostics, hover, definition/declaration/type/implementation, references, completion, signature help, quick fixes, formatting fallback |
| Python | `pyright-langserver --stdio` | Diagnostics, hover, definition/declaration/type/implementation, references, completion, signature help, quick fixes, formatting fallback |
| Go | `gopls` | Diagnostics, hover, definition/declaration/type/implementation, references, completion, signature help, quick fixes, formatting fallback |
| JSON | `vscode-json-language-server --stdio` | Diagnostics, hover, definition/declaration/type/implementation, references, completion, signature help, quick fixes, formatting fallback |
| Other languages | Configure a command in Settings | Server-dependent diagnostics, navigation, completion, signature help, quick fixes, formatting fallback |

Document changes are sent incrementally after the initial open. Servers are closed when documents
leave their workspace or are no longer eligible for synchronization. LSP is currently disabled for
documents Winc classifies as large (over 16 MiB); this preserves the editor's large-file memory
behavior. Whole-document LSP formatting is used when Winc cannot select an external formatter;
formatting selected text still requires an external formatter. Invoke signature help with
**Ctrl+Shift+Space**, or completion with **Ctrl+Space**; trigger-character completion is not
automatic yet.
Completion items are filtered and sorted, snippets are expanded, and item details/documentation are
shown in the popup. Workspace edits are applied only to open documents; file create/rename/delete
operations and edits requiring user confirmation are rejected.

The existing `cargo xtask bench` command measures app-level typing, highlighting, scrolling, startup,
and memory behavior. To compare cold and incremental Tree-sitter update latency on a generated Rust
source file, run:

```sh
cargo test -p winc-syntax --lib tree_sitter_update_benchmark -- --ignored --nocapture
```

This benchmark measures parse plus token collection, not allocations separately; timings depend on
build mode and machine. Two recent Windows release-mode runs on the generated 182,780-byte Rust
source measured cold medians of 266–276 ms and incremental medians of 203–221 ms. Tree-sitter
currently handles Rust, Python, JavaScript, TypeScript/TSX, and JSON up to 256 KiB; other languages
and larger files use Winc's line-based lexer. This single-language benchmark does not justify
expanding grammar coverage or changing the large-file cutoff; compare representative sources and
repeatable release-build measurements before doing either.

Alverion's builds and releases for this fork are published on
[this repository's GitHub Releases page](https://github.com/Aleverio/Winc/releases) when available.
For the original project, visit [DEEIX-AI/Winc](https://github.com/DEEIX-AI/Winc) or
[winc.si](https://winc.si).

## Install

This fork is named **Winct**, and installs through the `winctup` runtime. One command installs the
runtime and the newest stable release; later updates, rollback and self-update are handled by the
same runtime:

```bash
curl -fsSL https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.sh | bash
```

```powershell
irm https://raw.githubusercontent.com/Aleverio/Winc/dev/release/runtime/install.ps1 | iex
```

Add `~/.local/bin` (or `%LOCALAPPDATA%\winct\bin`) to `PATH`, then use `winctup status`,
`winctup update` and `winctup rollback`. See [release/runtime/README.md](release/runtime/README.md)
for the architecture, channels and security model. Native installers remain available on the
Releases page.

## Command Line

```bash
winc .                  # open the current folder
winc file.rs:12:5       # open a file at a line and column
winc --wait file        # wait until the tab closes, for example as the Git editor
cargo build 2>&1 | winc -   # read standard input
```

The Windows installers, the Linux deb and Arch packages, and `winctup` add `winc` to `PATH` (the runtime also installs the `winct` alias); the standalone Windows portable ZIP leaves `PATH` unchanged. On macOS, choose **Winct → Install winc Command…** from the menu bar or run **Shell Command: Install winc Command** from the command palette; with the Linux AppImage, use the command palette.

On macOS the app is started by the system, so it does not inherit the environment variables of your shell. Use `winc --new-instance` to keep them.

## Building from Source

winc builds with Rust 1.99.1; `rust-toolchain.toml` selects the toolchain automatically.

```bash
cargo run --release                 # build and run
cargo run --release -- <path>       # open a file or folder
```

For versioning, signing configuration and release workflows, see [Releasing winc](release/README.md).

## Development

```bash
cargo xtask test                    # test the crates changed since HEAD
cargo xtask test --all              # test the whole workspace
cargo clippy --workspace --all-targets -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo xtask bench                   # measure startup, memory, and scrolling against budgets
```

The interface is written in Rust. Windows come from winit, and rendering is done by winc's own
CPU rasterizer. On macOS, frames are written directly to an IOSurface.

The workspace is split into crates by responsibility:

| Crate | Responsibility |
| --- | --- |
| `winc-core` | Text, editing, undo, and diffing, independent of the interface |
| `winc-syntax` | Lexers for syntax highlighting |
| `winc-editor` | Editor logic: documents, file tree, search, Git, and formatting |
| `winc-vim` | Vim mode without the interface: key notation, the command table, parsing, and evaluation |
| `winc-backend` | File system access for local paths and for paths on remote hosts |
| `winc-server` | The server process that runs on a remote host and serves file operations over SSH |
| `winc-watch` | Watches folders on the machine that holds them (WSL distributions, remote hosts) |
| `winc-render` | Rendering: glyphs, rasterization, and presentation |
| `winc-preview` | Previews for images, SVG, Markdown, PDF, Word documents, and spreadsheets |
| `winc-typeset` | Layout for the Markdown preview |
| `winc-pty` | Terminal emulation and the pseudo-terminal helper process |
| `winc-config` | Configuration and state files |
| `winc-i18n` | Interface strings (Fluent bundles) |
| `winc-version` | The product version, read from the `VERSION` file |
| `winc-app` | The application: windows, events, and views |
| `winc` | The executable's entry point, which only calls into `winc-app` |
| `xtask` | Development automation: tests, benchmarks, and packaging |

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](.github/CONTRIBUTING.md) before you open a
pull request, and open it against the `dev` branch. All contributors sign the [Contributor License
Agreement](.github/CLA.md) and follow the [Code of Conduct](.github/CODE_OF_CONDUCT.md). To report a
vulnerability, see [SECURITY.md](.github/SECURITY.md).

## Maintainer and upstream

- Fork maintainer: **Alverion** ([GitHub repository](https://github.com/Aleverio/Winc))
- Original project: [DEEIX-AI/Winc](https://github.com/DEEIX-AI/Winc)

## Links

- [Rust](https://www.rust-lang.org)
- [Original Winc website](https://winc.si)

## License

winc is licensed under the [Apache License, Version 2.0](LICENSE). See [NOTICE](NOTICE).
