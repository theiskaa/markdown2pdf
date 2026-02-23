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

Built in Rust for performance and memory safety. Handles standard Markdown syntax including headings, lists, code blocks, links, tables, and images. Supports multiple input sources and outputs to files or bytes for in-memory processing.

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
markdown2pdf = "0.2.1"
```

### Feature Flags

The library provides optional feature flags to control dependencies:

- **Default (no features)**: Core PDF generation from files and strings. No network dependencies.
- **`fetch`**: Enables URL fetching support (requires one of the TLS features below).
- **`native-tls`**: Enables URL fetching with native TLS/OpenSSL (recommended for most users).
- **`rustls-tls`**: Enables URL fetching with pure-Rust TLS implementation (useful for static linking or avoiding OpenSSL).

```toml
# Minimal installation (no network dependencies)
markdown2pdf = "0.2.1"

# With URL fetching support (native TLS)
markdown2pdf = { version = "0.2.1", features = ["native-tls"] }

# With URL fetching support (rustls)
markdown2pdf = { version = "0.2.1", features = ["rustls-tls"] }
```

**Note**: Binary installations via cargo or prebuilt downloads do not include URL fetching by default. To build the binary with URL support:

```bash
# Install with URL fetching support
cargo install markdown2pdf --features native-tls

# Or build from source
cargo build --release --features native-tls
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

Convert from URL (requires `native-tls` or `rustls-tls` feature):
```bash
markdown2pdf -u "https://raw.githubusercontent.com/user/repo/main/README.md" -o "readme.pdf"
```

Use `--verbose` for detailed output, `--quiet` for CI/CD pipelines, or `--dry-run` to validate syntax without generating PDF.

## Fonts

The font system supports four modes:

- **Built-in fonts**: Helvetica, Times, Courier (fastest, no file I/O)
- **System fonts**: Searches standard OS font directories
- **File paths**: Load directly from a TTF/OTF file
- **Embedded bytes**: Load from compile-time included font data (great for GUI apps)

```bash
# Use built-in font (fastest)
markdown2pdf -p document.md -o output.pdf

# Use system font
markdown2pdf -p document.md --default-font Georgia -o output.pdf

# Use specific font file
markdown2pdf -p document.md --default-font "/path/to/font.ttf" -o output.pdf
```

Font subsetting is enabled by default, reducing PDF size by embedding only the glyphs used in the document. A Unicode document with Arial Unicode MS produces ~45KB instead of 23MB.

Performance is ~20ms for standard documents with built-in fonts.

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

Font configuration uses `FontConfig` for programmatic control:

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource, fonts::FontConfig};

let font_config = FontConfig::new()
    .with_default_font("Georgia")
    .with_code_font("Courier");

parse_into_file(
    markdown,
    "output.pdf",
    ConfigSource::Default,
    Some(&font_config),
)?;
```

You can also load fonts directly from embedded bytes using `FontSource`, which is useful for GUI applications or environments without filesystem access:

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource, fonts::{FontConfig, FontSource}};

static BODY_FONT: &[u8] = include_bytes!("path/to/body_font.ttf");
static CODE_FONT: &[u8] = include_bytes!("path/to/code_font.ttf");

let font_config = FontConfig::new()
    .with_default_font_source(FontSource::bytes(BODY_FONT))
    .with_code_font_source(FontSource::bytes(CODE_FONT));

parse_into_file(
    markdown,
    "output.pdf",
    ConfigSource::Default,
    Some(&font_config),
)?;
```

## Logging

The library uses the [`log`](https://crates.io/crates/log) crate. No output by default. Enable with any `log`-compatible backend (e.g., `env_logger`) and set `RUST_LOG=markdown2pdf=info` or `debug` for diagnostics.

## Configuration

TOML configuration customizes fonts, colors, spacing, and visual properties. Three loading methods: default styles, runtime files, or compile-time embedding.

For binary usage, create a config file at `~/markdown2pdfrc.toml` and copy the example configuration from `markdown2pdfrc.example.toml`. For library usage with embedded config, create your configuration file and embed it using `include_str!()`.

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.
