# Library usage

The crate exposes the same conversion pipeline the binary uses, so any styling achievable from the command line is achievable programmatically. The library parses Markdown into a token stream, resolves a style from a theme and optional configuration, and renders the PDF with its own in-tree engine. It is designed for embedding in web services that return PDF bytes, in build tooling that writes files, and in GUI or sandboxed applications that supply fonts and configuration as compile-time data rather than reading from disk.

Add the crate with Cargo. The default build has no network or SVG support; the optional `fetch` feature enables fetching remote images over a pure-Rust TLS stack, and the optional `svg` feature enables rasterizing SVG images:

```toml
markdown2pdf = "1.5.1"

# with URL fetching + SVG rasterization
markdown2pdf = { version = "1.5.1", features = ["fetch", "svg"] }
```

## Entry points

There are four conversion functions, forming a two-by-two grid: output to a file or to a byte buffer, and styling from a `ConfigSource` or from an already-resolved style. The file variants accept anything that implements `AsRef<Path>`, so a `&str`, `String`, `PathBuf`, or `&Path` all work. The final argument of every function is an optional reference to a `FontConfig`; passing `None` uses the built-in fonts.

`parse_into_file` parses, styles, and writes a PDF to the given path. `parse_into_bytes` does the same but returns the PDF as a `Vec<u8>`, which is the right choice for an HTTP handler or any in-memory pipeline. The two `*_with_style` variants take a pre-resolved `ResolvedStyle` instead of a `ConfigSource`, which avoids re-resolving the configuration on every call when the style is fixed or is being reused across many documents.

A minimal conversion to a file uses the default theme:

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};
use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
    let markdown = "# Hello\n\nSome **bold** text.".to_string();
    parse_into_file(markdown, "out.pdf", ConfigSource::Default, None)?;
    Ok(())
}
```

A web service typically wants bytes rather than a file so it can stream the PDF back in a response without touching the filesystem:

```rust
use markdown2pdf::{parse_into_bytes, config::ConfigSource};

fn render(markdown: String) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let pdf = parse_into_bytes(markdown, ConfigSource::Theme("github"), None)?;
    Ok(pdf)
}
```

## Selecting a style

The `ConfigSource` enum chooses where styling comes from. `Default` uses the bundled `default` theme with no overrides. `Theme(name)` selects one of the bundled presets — `default`, `github`, `academic`, `minimal`, `compact`, or `modern` — by name, which lets library code pick a known-good look without carrying any TOML. `File(path)` reads and parses a TOML configuration at runtime. `Embedded(toml)` treats a string as the configuration body, which combined with `include_str!` bakes the configuration into the binary at compile time — the standard approach for containerized or read-only deployments.

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource};

// A bundled preset, no TOML to carry
parse_into_file(md.clone(), "a.pdf", ConfigSource::Theme("academic"), None)?;

// A configuration embedded at compile time
const CFG: &str = include_str!("../brand.toml");
parse_into_file(md, "b.pdf", ConfigSource::Embedded(CFG), None)?;
```

Every field these sources can set is documented in [configuration.md](configuration.md), with an annotated reference configuration in [config.toml](config.toml).

## Pre-resolved styles and runtime overrides

When the same style is applied to many documents, or when per-request overrides are needed, resolve the style once and reuse it. `load_config_strict` turns a `ConfigSource` and an optional theme name into a concrete `ResolvedStyle`, returning a typed error rather than falling back silently. The resolved style is then handed to `parse_into_bytes_with_style` or `parse_into_file_with_style` as many times as needed:

```rust
use markdown2pdf::{parse_into_bytes_with_style,
                    config::{ConfigSource, load_config_strict}};

let style = load_config_strict(ConfigSource::File("brand.toml"), Some("github"))?;
let pdf = parse_into_bytes_with_style(markdown, style, None)?;
```

`load_config_strict_with_overrides` adds a highest-priority override layer expressed as a TOML fragment. The keys mirror the configuration schema exactly, and the layer wins over both the configuration file and the theme — this is the same mechanism the binary's `-V` flag uses, exposed for programmatic callers that want, for example, to inject a per-request title or color:

```rust
use markdown2pdf::config::{ConfigSource, load_config_strict_with_overrides};

let style = load_config_strict_with_overrides(
    ConfigSource::Theme("github"),
    None,
    Some("paragraph.text_align = \"justify\"\nheadings.h1.font_size_pt = 28"),
)?;
let pdf = markdown2pdf::parse_into_bytes_with_style(markdown, style, None)?;
```

These strict functions return a `ResolveError` describing exactly what went wrong: malformed TOML, an unknown theme, a cyclic `inherits` chain, or an I/O failure, with unknown keys carrying a closest-match suggestion. When a silent fallback is preferable to an error — for instance when a missing optional config should simply yield the default look — `load_config_from_source` logs the problem and returns the default theme instead of failing.

## Fonts

`FontConfig` selects the body and code fonts and is built fluently. A font may be named — resolving to a built-in (`Helvetica`, `Times`, `Courier`) or a system font — or supplied as raw bytes through `FontSource`, which is the right choice for GUI applications and sandboxed environments that cannot read the filesystem. Glyph subsetting is enabled by default, so only the glyphs used in the document are embedded.

Selecting fonts by name covers the common case:

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource, fonts::FontConfig};

let fonts = FontConfig::new()
    .with_default_font("Georgia")
    .with_code_font("Courier");
parse_into_file(md, "out.pdf", ConfigSource::Default, Some(&fonts))?;
```

Supplying fonts as embedded bytes removes any filesystem dependency entirely, which matters for single-binary GUI distribution:

```rust
use markdown2pdf::{parse_into_file, config::ConfigSource,
                    fonts::{FontConfig, FontSource}};

static BODY: &[u8] = include_bytes!("../fonts/Inter.ttf");
static CODE: &[u8] = include_bytes!("../fonts/JetBrainsMono.ttf");

let fonts = FontConfig::new()
    .with_default_font_source(FontSource::bytes(BODY))
    .with_code_font_source(FontSource::bytes(CODE));
parse_into_file(md, "out.pdf", ConfigSource::Default, Some(&fonts))?;
```

## Frontmatter

A YAML block delimited by `---` or a TOML block delimited by `+++` at the very top of the Markdown is consumed before lexing and folded into the document metadata. The recognized keys are `title`, `author`, `subject`, `keywords`, `creator`, and `language` (also accepted as `lang`); they override the configuration's `[metadata]` section. This requires no change at the call site — every `parse_into_*` entry point handles frontmatter transparently — so a document can carry its own title and author without the caller knowing them:

```markdown
---
title: My Document
author: Jane Doe
keywords: [rust, pdf]
---

# Body starts here
```

## Errors

Every entry point returns `Result<_, MdpError>`. The variants distinguish where the failure originated: `ParseError` carries a message and a one-based line and column for a lexer failure, `PdfError` covers generation and write failures and includes the offending path, `FontError` names the font that could not be loaded, `ConfigError` reports an invalid configuration, and `IoError` reports a filesystem failure with its path. Every variant also carries a human-readable suggestion. `MdpError` implements `std::error::Error` and `Display` — the `Display` output includes the suggestion — so it composes directly with `?` and `Box<dyn Error>` without any manual mapping.

## Logging

The library logs through the [`log`](https://crates.io/crates/log) facade and is silent unless a backend is installed. Enabling any `log`-compatible backend such as `env_logger` and setting `RUST_LOG=markdown2pdf=info`, or `debug` for more detail, surfaces diagnostics like font fallback decisions, configuration fallbacks, and validation notes without changing any code.
