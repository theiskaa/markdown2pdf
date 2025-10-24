<img width="600px" src="https://github.com/user-attachments/assets/fe2e96b8-a0bd-43b4-9360-e6cce43693f2">

<p align="center">

[![Crates.io](https://img.shields.io/crates/v/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![Documentation](https://img.shields.io/docsrs/markdown2pdf)](https://docs.rs/markdown2pdf)
[![License](https://img.shields.io/crates/l/markdown2pdf)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/markdown2pdf)](https://crates.io/crates/markdown2pdf)
[![GitHub Stars](https://img.shields.io/github/stars/theiskaa/markdown2pdf)](https://github.com/theiskaa/markdown2pdf/stargazers)

</p>

markdown2pdf is a command-line tool and library for converting Markdown content into styled PDF documents. It uses a lexical analyzer to parse Markdown and a PDF module to generate documents based on the parsed tokens.

The library employs a pipeline that tokenizes Markdown text into semantic elements, then processes these tokens through a styling module that applies configurable visual formatting. The styling engine supports customization of fonts, colors, spacing, and other typographic properties through TOML configuration. For containerized deployments and self-contained binaries, configurations can be embedded directly at compile time, eliminating runtime file dependencies.

This project includes both a binary and a library. The binary provides a command-line interface for converting Markdown files, URLs, or direct string input into styled PDF documents. The library offers programmatic Markdown parsing and PDF generation with fine-grained control over the conversion process, styling rules, and document formatting, including embedded configuration support for Docker, Nix, and containerized deployments.

The library is fast and reliable, built in Rust for performance and memory safety. It handles comprehensive Markdown syntax including headings, lists, code blocks, links, and images. Configuration is flexible through TOML files that can be loaded from disk or embedded at compile time. The library supports multiple input sources and can generate files or return PDF bytes for in-memory processing.

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

The markdown2pdf tool accepts Markdown file paths, direct content strings, or URLs as input. Options include `-p` for file paths, `-s` for direct string content, `-u` for URLs, and `-o` for output file specification. If multiple input options are provided, precedence follows: path > url > string. Default output is 'output.pdf' if not specified.

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

The command-line interface includes validation and output control options to support different workflows and debugging needs. The `--verbose` flag enables detailed output showing font selection decisions, coverage percentages, and file sizes, which is particularly useful when troubleshooting font rendering issues or optimizing document generation. Conversely, the `--quiet` flag suppresses all output except errors, making it ideal for use in CI/CD pipelines and automated scripts where clean output is preferred.

Pre-flight validation can be performed using the `--dry-run` flag, which analyzes your Markdown content for potential issues without generating a PDF. This validation checks for common syntax problems like unclosed code blocks or brackets, detects Unicode characters that might require special font configuration, and verifies that referenced image files exist. The dry-run mode is significantly faster than full PDF generation and provides immediate feedback about potential rendering issues, making it valuable for pre-commit hooks and rapid iteration during document development.

```bash
# Verbose output for debugging
markdown2pdf -p document.md --verbose -o output.pdf

# Quiet mode for scripts
markdown2pdf -p document.md --quiet -o output.pdf

# Validate without generating PDF
markdown2pdf -p document.md --dry-run

# Combined: verbose validation
markdown2pdf -p document.md --verbose --dry-run
```

## Font Handling and Unicode Support

The markdown2pdf library includes sophisticated font handling capabilities designed for international documents and diverse character sets. The system automatically detects Unicode characters in your content and selects appropriate fonts to ensure proper rendering across multiple scripts and languages.

Font subsetting technology is built into the core engine, dramatically reducing PDF file sizes by including only the glyphs actually used in your document. This optimization typically achieves a 98% size reduction compared to embedding complete font files, making the generated PDFs compact and efficient for distribution. For example, a document using Noto Sans might embed only 31 KB of font data instead of the full 1.2 MB font file, with no loss in rendering quality or character coverage.

The library supports fallback font chains, allowing you to specify multiple fonts that will be tried in sequence when a character is missing from the primary font. This is particularly valuable for documents containing mixed scripts such as Latin, Cyrillic, Greek, or Asian characters. The system analyzes each character and automatically selects the most appropriate font from your fallback chain, ensuring complete coverage without manual intervention. You can specify fallback fonts using the `--fallback-font` argument, which can be provided multiple times to build a comprehensive fallback chain.

Unicode support is automatic and intelligent. When the library detects non-ASCII characters in your content, it searches system fonts for those with good Unicode coverage, prioritizing fonts like Noto Sans, DejaVu Sans, and Liberation Sans. The system reports coverage percentages and provides helpful suggestions if your chosen font cannot render all characters in the document. For documents requiring extensive international character support, you can explicitly specify a Unicode-capable font with `--default-font "Noto Sans"` combined with appropriate fallbacks.

Font variant loading has been enhanced to support true typographic styles rather than relying on synthetic rendering. The system searches for actual Bold, Italic, and Bold-Italic font files when you use emphasis in your Markdown, trying multiple naming conventions automatically. If variant files are found, they are loaded and used for proper typography; if not, the system gracefully falls back to the regular font face with a helpful notification. This works seamlessly with custom font directories specified through `--font-path` arguments.

The font name resolution system includes fuzzy matching and aliasing to handle cross-platform font availability. When you specify a font like "Arial" on a system where Helvetica is available instead, the library automatically tries common aliases and alternatives. This ensures your documents render consistently across Windows, macOS, and Linux platforms without requiring platform-specific configuration.

Custom fonts can be loaded from directories or individual font files by providing paths through the `--font-path` option. The system recursively searches these directories for TrueType and OpenType fonts, making them available for selection. Combined with the fallback chain system, this allows you to bundle fonts with your application or specify organization-standard typefaces while maintaining fallback coverage for any edge cases.

```bash
# Unicode document with fallback chain
markdown2pdf -p international.md \
  --default-font "Noto Sans" \
  --fallback-font "Arial Unicode MS" \
  --fallback-font "DejaVu Sans" \
  -o output.pdf

# Custom fonts with automatic subsetting
markdown2pdf -p document.md \
  --font-path "./fonts" \
  --default-font "Roboto" \
  --code-font "Fira Code" \
  -o output.pdf
```

## Library Usage

The library exposes two main functions: `parse_into_file()` and `parse_into_bytes()`. Both accept raw Markdown text and handle all intermediate processing steps internally. They leverage the lexer to build an abstract syntax tree, apply styling rules from configuration, and render the final PDF output.

The `parse_into_file()` function saves the PDF directly to a file. The `parse_into_bytes()` function returns the PDF data as a byte vector for scenarios requiring more flexibility, such as web services, API responses, or network transmission.

The library uses a `ConfigSource` enum to specify how styling configuration should be loaded. This supports three approaches: default built-in styling, file-based configuration loaded at runtime, or embedded configuration compiled directly into the binary.

Configuration sources include `ConfigSource::Default` for built-in styling with no external dependencies, `ConfigSource::File("path")` for runtime loading from TOML files, and `ConfigSource::Embedded(content)` for compile-time embedded configuration strings.

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

For embedded configuration, TOML content can be included at compile time using `include_str!()` or defined as string literals. This approach eliminates runtime file dependencies and creates truly portable executables suitable for containerized deployments.

Font configuration at the library level is accomplished through the `FontConfig` structure, which provides programmatic control over font selection, fallback chains, and subsetting behavior. The configuration allows you to specify custom font directories, set default and code fonts, establish fallback font chains, and control whether font subsetting is enabled. When working with international content or documents requiring specific typography, you can construct a font configuration and pass it to the parsing functions as an optional parameter.

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

The font subsetting feature is enabled by default and works transparently to reduce PDF file sizes. When enabled, the library analyzes all text content in your document to determine which glyphs are actually needed, then creates a minimal font subset containing only those characters. This process maintains full rendering fidelity while dramatically reducing file size, making it particularly effective for large documents or when using comprehensive Unicode fonts.

For advanced usage, you can work directly with the lexer and PDF generation components. Create a lexer instance to parse Markdown content into tokens, then create a PDF renderer with styling rules through a `StyleMatch` instance loaded via `load_config_from_source()`. Finally, render the document to a file or convert to bytes.

## Configuration

The markdown2pdf tool and library support extensive customization through TOML configuration. Configuration can be provided through three methods: default built-in styles, file-based configuration loaded at runtime, or embedded configuration compiled into the binary.

The configuration is translated to a `StyleMatch` instance which determines how different Markdown elements are rendered in the final PDF. Configuration supports customization of fonts, colors, spacing, and other visual properties for all Markdown elements.

Default configuration uses built-in styling with sensible defaults and requires no external files. File-based configuration loads TOML files at runtime and supports both relative and absolute paths. Embedded configuration compiles TOML content directly into the binary, creating self-contained executables perfect for Docker, Nix, and containerized environments.

Embedded configuration provides several advantages: self-contained binaries with no external dependencies, container compatibility with no filesystem concerns, version control integration where configuration changes are tracked with code, compile-time validation where errors are caught early, and production reliability that eliminates missing file errors.

Error handling is graceful - if a specified configuration file cannot be found or contains invalid syntax, the library falls back to default styling without crashing, ensuring PDF generation always succeeds.

For binary usage, create a config file at `~/markdown2pdfrc.toml` and copy the example configuration from `markdown2pdfrc.example.toml`. For library usage with embedded config, create your configuration file and embed it using `include_str!()` or define it as a string literal, then use it with `ConfigSource::Embedded(content)`.

## Contributing
For information regarding contributions, please refer to [CONTRIBUTING.md](CONTRIBUTING.md) file.

## Donations
For information regarding donations please refer to [DONATE.md](DONATE.md)
