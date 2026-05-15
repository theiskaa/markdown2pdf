<img width="600px" src="https://github.com/user-attachments/assets/fe2e96b8-a0bd-43b4-9360-e6cce43693f2">

<p align="center">

[![Crates.io](https://img.shields.io/crates/v/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![Documentation](https://img.shields.io/docsrs/markdown2pdf)](https://docs.rs/markdown2pdf)
[![License](https://img.shields.io/crates/l/markdown2pdf)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/markdown2pdf)](https://crates.io/crates/markdown2pdf)

</p>

markdown2pdf converts Markdown to PDF with a lexical analyzer and an in-tree rendering engine built directly on `printpdf`. The library tokenizes Markdown into semantic elements, resolves styling from a TOML configuration, and lays out the PDF itself — no third-party document engine in between.

Both a binary and a library are provided. The binary offers CLI conversion from files, URLs, or strings. The library enables programmatic PDF generation with full control over styling and fonts. Configuration can be loaded at runtime or embedded at compile time for containerized deployments.

The lexer targets CommonMark 0.31.2 with the GitHub Flavored Markdown extensions and passes 100% of the CommonMark spec suite. The renderer covers headings with bookmarks and anchors, inline emphasis (bold, italic, monospace, strikethrough, underline, superscript, subscript, small-caps), ordered/unordered/task lists with arbitrary nesting, GFM tables with per-column alignment and header repeat, blockquotes, fenced and indented code, images (local, URL, and SVG), footnotes, definition lists, cross-references, and inline HTML. Document features include six bundled themes, per-block styling, configurable page setup, headers and footers, an auto-generated table of contents, a title page, YAML/TOML frontmatter, and PDF metadata. Multiple input sources; output to a file or to bytes for in-memory processing.

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
markdown2pdf = "1.0.0"

# Or with URL fetching + SVG rasterization
markdown2pdf = { version = "1.0.0", features = ["fetch", "svg"] }
```

See [docs/Library.md](docs/Library.md) for the programmatic API.

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
**[`docs/Configuration.md`](docs/Configuration.md)**; an annotated,
copy-and-tweak reference config is **[`docs/config.toml`](docs/config.toml)**.

## Usage

`markdown2pdf` converts a file (`-p`), a string (`-s`), or a URL
(`-u`, requires the `fetch` build feature) to a PDF (`-o`, default
`./output.pdf`).

```bash
markdown2pdf -p docs/resume.md -o resume.pdf
markdown2pdf -s "**bold** *italic*." -o out.pdf
markdown2pdf -p doc.md --theme academic --page-numbers -o out.pdf
```

`--verbose` / `--quiet` control output; `--dry-run` validates
without writing; `--print-effective-config` prints the resolved
style as TOML. Full flag reference, the config-override system, and
font selection: **[`docs/CLI.md`](docs/CLI.md)**.

## Library Usage

`parse_into_file` writes a PDF; `parse_into_bytes` returns a
`Vec<u8>` for web services. `ConfigSource` selects styling
(`Default`, `Theme("github")`, `File(path)`, `Embedded(toml)`).

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};

parse_into_file("# Hello".into(), "out.pdf", ConfigSource::Default, None)?;
parse_into_file("# Doc".into(), "out.pdf", ConfigSource::Theme("academic"), None)?;
```

Pre-resolved styles + runtime overrides, fonts (name / path /
embedded bytes), frontmatter, and the error model are covered in
**[`docs/Library.md`](docs/Library.md)**.

## Markdown Coverage

Targets [CommonMark 0.31.2](https://spec.commonmark.org/0.31.2/) + [GFM](https://github.github.com/gfm/). CommonMark spec pass rate: **100% (652/652)** — every section passes. Backed by ~800 inline lexer unit tests in `tests/markdown/`, the full spec runner in `tests/commonmark_spec.rs`, a robustness suite in `tests/stress.rs`, and an adversarial / structural renderer test suite in `tests/render/` (object-graph validation, malformed input, image-pipeline, and config-validation cases).

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.
