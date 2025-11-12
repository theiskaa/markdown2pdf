<img width="600px" src="https://github.com/user-attachments/assets/fe2e96b8-a0bd-43b4-9360-e6cce43693f2">

<p align="center">

[![Crates.io](https://img.shields.io/crates/v/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![Documentation](https://img.shields.io/docsrs/markdown2pdf)](https://docs.rs/markdown2pdf)
[![License](https://img.shields.io/crates/l/markdown2pdf)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![GitHub Stars](https://img.shields.io/github/stars/theiskaa/markdown2pdf)](https://github.com/theiskaa/markdown2pdf/stargazers)

</p>

markdown2pdf converts Markdown to PDF using a lexical analyzer and PDF rendering engine. The library tokenizes Markdown into semantic elements, applies styling rules from TOML configuration, and generates styled PDF output.

Both binary and library are provided. The binary offers CLI conversion from files, URLs, or strings. The library enables programmatic PDF generation with full control over styling and fonts. Configuration can be loaded at runtime or embedded at compile time for containerized deployments.

Built in Rust for performance and memory safety. Handles standard Markdown syntax including headings, lists, code blocks, links, and images. Supports multiple input sources and outputs to files or bytes for in-memory processing.

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

Add to your project:

```bash
cargo add markdown2pdf
```

Or add to your Cargo.toml:

```toml
markdown2pdf = "0.1.9"
```

## Usage

The tool accepts file paths (`-p`), string content (`-s`), or URLs (`-u`) as input. Output path is specified with `-o`. Input precedence: path > url > string. Defaults to 'output.pdf'.

Convert a Markdown file:
```bash
markdown2pdf -p "docs/resume.md" -o "resume.pdf"
```

Convert string content:
```bash
markdown2pdf -s "**bold text** *italic text*." -o "output.pdf"
```

Convert from URL:
```bash
markdown2pdf -u "https://raw.githubusercontent.com/user/repo/main/README.md" -o "readme.pdf"
```

Use `--verbose` for detailed font selection output, `--quiet` for CI/CD pipelines, or `--dry-run` to validate syntax without generating PDF.

## Font Handling and Unicode Support

The library automatically detects Unicode characters and selects system fonts with good coverage. Font subsetting reduces PDF size by 98% by including only used glyphs. A document with Noto Sans embeds ~31 KB instead of 1.2 MB.

Fallback font chains specify multiple fonts tried in sequence for missing characters. Useful for mixed-script documents with Latin, Cyrillic, Greek, or CJK. The system analyzes each character and selects the best font from the chain.

When non-ASCII characters are detected, the library prioritizes Noto Sans, DejaVu Sans, and Liberation Sans. Coverage percentages are reported with suggestions if fonts lack support.

The system loads actual Bold, Italic, and Bold-Italic font files rather than synthetic rendering. Font name resolution includes fuzzy matching and aliasing for cross-platform compatibility. "Arial" automatically maps to Helvetica on macOS.

Custom fonts load from directories via `--font-path` with recursive search for TrueType and OpenType fonts.

```bash
# Unicode with fallback chain
markdown2pdf -p international.md --default-font "Noto Sans" \
  --fallback-font "Arial Unicode MS" -o output.pdf

# Custom fonts with subsetting
markdown2pdf -p document.md --font-path "./fonts" \
  --default-font "Roboto" --code-font "Fira Code" -o output.pdf
```

## Library Usage

Two main functions: `parse_into_file()` saves PDF to disk, `parse_into_bytes()` returns bytes for web services. Both parse Markdown, apply styling, and render output.

Configuration uses `ConfigSource`: `Default` for built-in styling, `File("path")` for runtime loading, or `Embedded(content)` for compile-time embedding.

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};

// Default styling
parse_into_file(markdown, "output.pdf", ConfigSource::Default, None)?;

// File-based configuration
parse_into_file(markdown, "output.pdf", ConfigSource::File("config.toml"), None)?;

// Embedded configuration
const CONFIG: &str = include_str!("../config.toml");
parse_into_file(markdown, "output.pdf", ConfigSource::Embedded(CONFIG), None)?;
```

Embedded configuration uses `include_str!()` at compile time, eliminating runtime file dependencies.

Font configuration uses `FontConfig` for programmatic control over fonts, fallback chains, and subsetting.

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource, fonts::FontConfig};
use std::path::PathBuf;

// Configure fonts for international document
let font_config = FontConfig {
    custom_paths: vec![PathBuf::from("./fonts")],
    default_font: Some("Noto Sans".to_string()),
    code_font: Some("Fira Code".to_string()),
    fallback_fonts: vec![
        "Arial Unicode MS".to_string(),
        "DejaVu Sans".to_string(),
    ],
    enable_subsetting: true,
};

parse_into_file(
    markdown,
    "output.pdf",
    ConfigSource::Default,
    Some(&font_config),
)?;
```

Font subsetting is enabled by default, analyzing text to create minimal subsets while maintaining full fidelity.

For advanced usage, work directly with the lexer and PDF components via `load_config_from_source()`.

## Configuration

TOML configuration customizes fonts, colors, spacing, and visual properties. Configuration translates to a `StyleMatch` instance. Three loading methods: default styles, runtime files, or compile-time embedding.

Embedded configuration creates self-contained binaries for Docker and containers with compile-time validation. Error handling falls back to default styling if files are missing or invalid.

For binary usage, create a config file at `~/markdown2pdfrc.toml` and copy the example configuration from `markdown2pdfrc.example.toml`. For library usage with embedded config, create your configuration file and embed it using `include_str!()` or define it as a string literal, then use it with `ConfigSource::Embedded(content)`.

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.

## Donations
For information regarding donations please refer to [DONATE.md](DONATE.md)
