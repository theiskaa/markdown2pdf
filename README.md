<img width="600px" src="https://github.com/user-attachments/assets/fe2e96b8-a0bd-43b4-9360-e6cce43693f2">

<p align="center">

[![Crates.io](https://img.shields.io/crates/v/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![Documentation](https://img.shields.io/docsrs/markdown2pdf)](https://docs.rs/markdown2pdf)
[![License](https://img.shields.io/crates/l/markdown2pdf)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/markdown2pdf)](https://crates.io/crates/markdown2pdf)

</p>

markdown2pdf converts Markdown to PDF through a fully in-tree pipeline: a CommonMark/GFM lexer, a TOML-driven style resolver, and a layout engine built directly on `printpdf`. There is no intermediate HTML and no third-party document engine.

It ships as both a binary and a library. The binary converts from a file, URL, or string on the command line; the library exposes programmatic generation with full control over styling and fonts. Configuration is loaded at runtime or embedded at compile time for containerized deployments.

The lexer targets CommonMark 0.31.2 plus GFM and note-tool extensions such as WikiLinks, `==highlight==`, and LaTeX math, and passes 649 of the 652 CommonMark spec examples. The exceptions are deliberate, where the WikiLink syntax reclaims `[[…]]` (which CommonMark treats as nested brackets). Conformance is held in place by the full CommonMark spec runner alongside the lexer-unit, stress, and adversarial renderer suites.

The renderer covers headings with bookmarks and anchors, the full inline-emphasis set (bold, italic, monospace, strikethrough, underline, highlight, super/subscript, small-caps), nested ordered/unordered/task lists, GFM tables with per-column alignment and header repeat, blockquotes, fenced and indented code, images (local, URL, SVG), footnotes, definition lists, cross-references, and inline HTML. Mathematics is typeset by a built-in TeX engine — real fraction bars, radicals, script stacks, big operators with limits, growing delimiters, matrices, and accents — drawn as vector outlines and configurable through a `[math]` style block.

Documents are styled per block with six bundled themes, configurable page setup, running headers and footers, an auto-generated table of contents, a title page, YAML/TOML frontmatter, and PDF metadata. Output is written to a file or returned as an in-memory byte buffer.

## Install binary

### Homebrew

```sh
brew install theiskaa/tap/markdown2pdf
```

### Cargo

Install the binary globally using cargo:

```bash
cargo install markdown2pdf
```

For the latest git version:

```bash
cargo install --git https://github.com/theiskaa/markdown2pdf
```

URL input (`-u`) and SVG images are behind optional features; pass
them to `cargo install` (see [Feature flags](#feature-flags)):

```bash
cargo install markdown2pdf --features fetch,svg
```

### Prebuilt binaries

Prebuilt versions are available in our [GitHub releases](https://github.com/theiskaa/markdown2pdf/releases/latest):

|  File  | Platform | Checksum |
|--------|----------|----------|
| [markdown2pdf-aarch64-apple-darwin.tar.xz](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-aarch64-apple-darwin.tar.xz) | Apple Silicon macOS | [checksum](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-aarch64-apple-darwin.tar.xz.sha256) |
| [markdown2pdf-x86_64-apple-darwin.tar.xz](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-apple-darwin.tar.xz) | Intel macOS | [checksum](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-apple-darwin.tar.xz.sha256) |
| [markdown2pdf-x86_64-pc-windows-msvc.zip](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-pc-windows-msvc.zip) | x64 Windows | [checksum](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-pc-windows-msvc.zip.sha256) |
| [markdown2pdf-aarch64-unknown-linux-gnu.tar.xz](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-aarch64-unknown-linux-gnu.tar.xz) | ARM64 Linux | [checksum](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-aarch64-unknown-linux-gnu.tar.xz.sha256) |
| [markdown2pdf-x86_64-unknown-linux-gnu.tar.xz](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-unknown-linux-gnu.tar.xz) | x64 Linux | [checksum](https://github.com/theiskaa/markdown2pdf/releases/latest/download/markdown2pdf-x86_64-unknown-linux-gnu.tar.xz.sha256) |

## Install as library

Add the crate to your project:

```bash
cargo add markdown2pdf
```

Or, in `Cargo.toml`:

```toml
# Minimal — local files only, no network, no SVG
markdown2pdf = "1.1.0"

# Or with URL fetching + SVG rasterization
markdown2pdf = { version = "1.1.0", features = ["fetch", "svg"] }
```

See [docs/library.md](docs/library.md) for the programmatic API.

## Feature flags

Two optional features, both off by default and shared by the binary
and the library. The library enables them in `Cargo.toml`
(`features = [...]`); the binary enables them at install or build
time (`cargo install markdown2pdf --features fetch,svg`).

- **`fetch`** — URL input (the `-u`/`--url` flag) and remote images.
  Uses pure-Rust TLS (rustls), so no system OpenSSL is needed; works
  in `rust:slim` and Alpine. If you need native TLS for corporate
  certificate stores, depend on `reqwest` directly with your
  preferred backend and Cargo will unify the features.
- **`svg`** — SVG image rasterization via `resvg`, for SVG embedded
  through `![](path.svg)` or `<img src="...svg">`.

## Configuration

Every visual choice — fonts, colors, page setup, headers / footers,
table of contents, title page, alignment, per-block typography — lives
in a TOML configuration. Six bundled themes (`default`, `github`,
`academic`, `minimal`, `compact`, `modern`) give one-line styling;
per-block overrides handle the long tail; and **any value can be
overridden per-run from the command line**, winning over the config
file and theme.

```sh
# A theme
markdown2pdf -p input.md --theme github -o out.pdf

# Your own config file
markdown2pdf -p input.md -c my-config.toml -o out.pdf

# Override individual values at runtime (highest priority)
markdown2pdf -p input.md --title "Report" --font-size 11 --margin 2.5cm \
  --page-numbers -V headings.h1.font_size_pt=28 -o out.pdf
```

The full schema with every field explained is in
**[`docs/configuration.md`](docs/configuration.md)**; an annotated,
copy-and-tweak reference config is **[`docs/config.toml`](docs/config.toml)**.

## Usage

The binary converts a file (`-p`), a string (`-s`), or a URL (`-u`,
with the `fetch` feature) to a PDF (`-o`, default `./output.pdf`).

```bash
markdown2pdf -p docs/resume.md -o resume.pdf
markdown2pdf -s "**bold** *italic*." -o out.pdf
markdown2pdf -p doc.md --theme academic --page-numbers -o out.pdf
```

`--dry-run` validates input without writing; `--print-effective-config`
emits the resolved style as TOML; `--verbose` and `--quiet` adjust
logging. The complete flag reference, config-override precedence, and
font selection are in **[`docs/cli.md`](docs/cli.md)**.

## Library Usage

`parse_into_file` writes a PDF to disk; `parse_into_bytes` returns the
document as a `Vec<u8>` for in-memory use. `ConfigSource` selects
styling — `Default`, `Theme("github")`, `File(path)`, or
`Embedded(toml)`.

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};

parse_into_file("# Hello".into(), "out.pdf", ConfigSource::Default, None)?;
parse_into_file("# Doc".into(), "out.pdf", ConfigSource::Theme("academic"), None)?;
```

Pre-resolved styles and runtime overrides, font selection (by name,
path, or embedded bytes), frontmatter, and the error model are
covered in **[`docs/library.md`](docs/library.md)**.

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.
