# markdown2pdf

markdown2pdf is a command-line tool and library for converting Markdown content into styled PDF documents. It uses a lexical analyzer to parse Markdown and a PDF module to generate documents based on the parsed tokens.

The library employs a pipeline that tokenizes Markdown text into semantic elements, then processes these tokens through a styling module that applies configurable visual formatting. The styling engine supports customization of fonts, colors, spacing, and other typographic properties through TOML configuration. For containerized deployments and self-contained binaries, configurations can be embedded directly at compile time, eliminating runtime file dependencies.

This project includes both a binary and a library. The binary provides a command-line interface for converting Markdown files, URLs, or direct string input into styled PDF documents. The library offers programmatic Markdown parsing and PDF generation with fine-grained control over the conversion process, styling rules, and document formatting, including embedded configuration support for Docker, Nix, and containerized deployments.

The library is fast and reliable, built in Rust for performance and memory safety. It handles comprehensive Markdown syntax including headings, lists, code blocks, links, and images. Configuration is flexible through TOML files that can be loaded from disk or embedded at compile time. The library supports multiple input sources and can generate files or return PDF bytes for in-memory processing.

## Install

Install the binary globally using cargo:

```bash
cargo install markdown2pdf
```

For the latest git version:

```bash
cargo install --git https://github.com/theiskaa/markdown2pdf
```

## Install as library

Add to your project:

```bash
cargo add markdown2pdf
```

Or add to your Cargo.toml:

```toml
markdown2pdf = "0.1.5"
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

## Library Usage

The library exposes two main functions: `parse_into_file()` and `parse_into_bytes()`. Both accept raw Markdown text and handle all intermediate processing steps internally. They leverage the lexer to build an abstract syntax tree, apply styling rules from configuration, and render the final PDF output.

The `parse_into_file()` function saves the PDF directly to a file. The `parse_into_bytes()` function returns the PDF data as a byte vector for scenarios requiring more flexibility, such as web services, API responses, or network transmission.

The library uses a `ConfigSource` enum to specify how styling configuration should be loaded. This supports three approaches: default built-in styling, file-based configuration loaded at runtime, or embedded configuration compiled directly into the binary.

Configuration sources include `ConfigSource::Default` for built-in styling with no external dependencies, `ConfigSource::File("path")` for runtime loading from TOML files, and `ConfigSource::Embedded(content)` for compile-time embedded configuration strings.

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};

// Default styling
parse_into_file(markdown, "output.pdf", ConfigSource::Default)?;

// File-based configuration
parse_into_file(markdown, "output.pdf", ConfigSource::File("config.toml"))?;

// Embedded configuration
const CONFIG: &str = include_str!("../config.toml");
parse_into_file(markdown, "output.pdf", ConfigSource::Embedded(CONFIG))?;
```

For embedded configuration, TOML content can be included at compile time using `include_str!()` or defined as string literals. This approach eliminates runtime file dependencies and creates truly portable executables suitable for containerized deployments.

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
