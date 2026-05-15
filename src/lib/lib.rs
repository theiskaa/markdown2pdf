//! The markdown2pdf library enables conversion of Markdown content into professionally styled PDF documents.
//! It provides a complete pipeline for parsing Markdown text, applying configurable styling rules, and
//! generating polished PDF output.
//!
//! The library handles the intricacies of Markdown parsing and PDF generation while giving users control
//! over the visual presentation through styling configuration. Users can customize fonts, colors, spacing,
//! and other visual properties via a TOML configuration file.
//!
//! Basic usage involves passing Markdown content as a string along with an output path:
//! ```rust
//! use markdown2pdf;
//! use markdown2pdf::config::ConfigSource;
//! use std::error::Error;
//!
//! // Convert Markdown string to PDF with proper error handling
//! fn example() -> Result<(), Box<dyn Error>> {
//!     let markdown = "# Hello World\nThis is a test.".to_string();
//!     markdown2pdf::parse_into_file(markdown, "output.pdf", ConfigSource::Default, None)?;
//!     Ok(())
//! }
//! ```
//!
//! For more control over the output styling, users can create a configuration file (markdown2pdfrc.toml)
//! to specify custom visual properties:
//! ```rust
//! use markdown2pdf;
//! use markdown2pdf::config::ConfigSource;
//! use std::fs;
//! use std::error::Error;
//!
//! // Read markdown file with proper error handling
//! fn example_with_styling() -> Result<(), Box<dyn Error>> {
//!     let markdown = fs::read_to_string("input.md")?;
//!     markdown2pdf::parse_into_file(markdown, "styled-output.pdf", ConfigSource::Default, None)?;
//!     Ok(())
//! }
//! ```
//!
//! The library also handles rich content like images and links seamlessly:
//! ```rust
//! use markdown2pdf;
//! use markdown2pdf::config::ConfigSource;
//! use std::error::Error;
//!
//! fn example_with_rich_content() -> Result<(), Box<dyn Error>> {
//!     let markdown = r#"
//!     # Document Title
//!
//!     ![Logo](./images/logo.png)
//!
//!     See our [website](https://example.com) for more info.
//!     "#.to_string();
//!
//!     markdown2pdf::parse_into_file(markdown, "doc-with-images.pdf", ConfigSource::Default, None)?;
//!     Ok(())
//! }
//! ```
//!
//! The styling configuration file supports comprehensive customization of the document appearance.
//! Page layout properties control the overall document structure:
//! ```toml
//! [page]
//! margins = { top = 72, right = 72, bottom = 72, left = 72 }
//! size = "a4"
//! orientation = "portrait"
//! ```
//!
//! Individual elements can be styled with precise control:
//! ```toml
//! [heading.1]
//! size = 24
//! textcolor = { r = 0, g = 0, b = 0 }
//! bold = true
//! afterspacing = 1.0
//!
//! [text]
//! size = 12
//! fontfamily = "roboto"
//! alignment = "left"
//!
//! [code]
//! backgroundcolor = { r = 245, g = 245, b = 245 }
//! fontfamily = "roboto-mono"
//! ```
//!
//! The conversion process follows a carefully structured pipeline. First, the Markdown text undergoes
//! lexical analysis to produce a stream of semantic tokens. These tokens then receive styling rules
//! based on the configuration. Finally, the styled elements are rendered into the PDF document.
//!
//! ## Token Processing Flow
//! ```text
//! +-------------+     +----------------+     +----------------+
//! |  Markdown   |     |  Tokens        |     |  PDF Elements  |
//! |  Input      |     |  # -> Heading  |     |  - Styled      |
//! |  # Title    | --> |  * -> List     | --> |    Heading     |
//! |  * Item     |     |  > -> Quote    |     |  - List with   |
//! |  > Quote    |     |                |     |    bullets     |
//! +-------------+     +----------------+     +----------------+
//!
//! +---------------+     +------------------+     +--------------+
//! | Styling       |     | Font Loading     |     | Output:      |
//! | - Font sizes  | --> | - Font families  | --> | Final        |
//! | - Colors      |     | - Weights        |     | Rendered     |
//! | - Margins     |     | - Styles         |     | PDF Document |
//! +---------------+     +------------------+     +--------------+
//! ```

pub mod config;
mod debug;
pub mod fonts;
pub mod markdown;
pub mod render;
pub mod styling;
pub mod validation;

use markdown::*;
use std::error::Error;
use std::fmt;

/// Represents errors that can occur during the markdown-to-pdf conversion process.
/// This includes both parsing failures and PDF generation issues.
#[derive(Debug)]
pub enum MdpError {
    /// Indicates an error occurred while parsing the Markdown content.
    /// `line` and `column` are 1-based when present and point at the
    /// source character that triggered the lexer failure.
    ParseError {
        message: String,
        line: Option<usize>,
        column: Option<usize>,
        suggestion: Option<String>,
    },
    /// Indicates an error occurred during PDF file generation
    PdfError {
        message: String,
        path: Option<String>,
        suggestion: Option<String>,
    },
    /// Indicates a font loading error
    FontError {
        font_name: String,
        message: String,
        suggestion: String,
    },
    /// Indicates an invalid configuration
    ConfigError { message: String, suggestion: String },
    /// Indicates an I/O error
    IoError {
        message: String,
        path: String,
        suggestion: String,
    },
}

impl Error for MdpError {}
impl fmt::Display for MdpError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MdpError::ParseError {
                message,
                line,
                column,
                suggestion,
            } => {
                write!(f, "❌ Markdown Parsing Error: {}", message)?;
                if let (Some(l), Some(c)) = (line, column) {
                    write!(f, " (at line {}, column {})", l, c)?;
                } else if let Some(l) = line {
                    write!(f, " (at line {})", l)?;
                }
                if let Some(hint) = suggestion {
                    write!(f, "\n💡 Suggestion: {}", hint)?;
                }
                Ok(())
            }
            MdpError::PdfError {
                message,
                path,
                suggestion,
            } => {
                write!(f, "❌ PDF Generation Error: {}", message)?;
                if let Some(p) = path {
                    write!(f, "\n📁 Path: {}", p)?;
                }
                if let Some(hint) = suggestion {
                    write!(f, "\n💡 Suggestion: {}", hint)?;
                }
                Ok(())
            }
            MdpError::FontError {
                font_name,
                message,
                suggestion,
            } => {
                write!(f, "❌ Font Error: Failed to load font '{}'", font_name)?;
                write!(f, "\n   Reason: {}", message)?;
                write!(f, "\n💡 Suggestion: {}", suggestion)?;
                Ok(())
            }
            MdpError::ConfigError {
                message,
                suggestion,
            } => {
                write!(f, "❌ Configuration Error: {}", message)?;
                write!(f, "\n💡 Suggestion: {}", suggestion)?;
                Ok(())
            }
            MdpError::IoError {
                message,
                path,
                suggestion,
            } => {
                write!(f, "❌ File Error: {}", message)?;
                write!(f, "\n📁 Path: {}", path)?;
                write!(f, "\n💡 Suggestion: {}", suggestion)?;
                Ok(())
            }
        }
    }
}

impl MdpError {
    /// Creates a simple parse error with just a message
    pub fn parse_error(message: impl Into<String>) -> Self {
        MdpError::ParseError {
            message: message.into(),
            line: None,
            column: None,
            suggestion: Some(
                "Check your Markdown syntax for unclosed brackets, quotes, or code blocks"
                    .to_string(),
            ),
        }
    }

    /// Creates a simple PDF error with just a message
    pub fn pdf_error(message: impl Into<String>) -> Self {
        MdpError::PdfError {
            message: message.into(),
            path: None,
            suggestion: Some(
                "Check that the output directory exists and you have write permissions".to_string(),
            ),
        }
    }
}

/// Transforms Markdown content into a styled PDF document and saves it to the specified path.
/// This function provides a high-level interface for converting Markdown to PDF with configurable
/// styling through TOML configuration files.
///
/// The process begins by parsing the Markdown content into a structured token representation.
/// It then applies styling rules, either from a configuration file if present or using defaults.
/// Finally, it generates the PDF document with the appropriate styling and structure.
///
/// # Arguments
/// * `markdown` - The Markdown content to convert
/// * `path` - The output file path for the generated PDF
/// * `config` - Configuration source (Default, File path, or Embedded TOML)
///
/// # Returns
/// * `Ok(())` on successful PDF generation and save
/// * `Err(MdpError)` if errors occur during parsing, styling, or file operations
///
/// # Example
/// ```rust
/// use std::error::Error;
/// use markdown2pdf::config::ConfigSource;
/// use markdown2pdf::fonts::FontConfig;
///
/// fn example() -> Result<(), Box<dyn Error>> {
///     let markdown = "# Hello World\nThis is a test.".to_string();
///
///     // Use default configuration
///     markdown2pdf::parse_into_file(markdown.clone(), "output1.pdf", ConfigSource::Default, None)?;
///
///     // Use file-based configuration
///     markdown2pdf::parse_into_file(markdown.clone(), "output2.pdf", ConfigSource::File("config.toml"), None)?;
///
///     // Use embedded configuration with custom font
///     const EMBEDDED: &str = r#"
///         [heading.1]
///         size = 18
///         bold = true
///     "#;
///     let font_config = FontConfig::new()
///         .with_default_font("Georgia");
///     markdown2pdf::parse_into_file(markdown, "output3.pdf", ConfigSource::Embedded(EMBEDDED), Some(&font_config))?;
///
///     Ok(())
/// }
/// ```
/// Variant of [`parse_into_file`] that takes a pre-resolved style
/// instead of a `ConfigSource`. Useful when the caller has already
/// loaded the config (e.g. to also serialize it for
/// `--print-effective-config`) and doesn't want to load it again.
pub fn parse_into_file_with_style(
    markdown: String,
    path: impl AsRef<std::path::Path>,
    style: styling::ResolvedStyle,
    font_config: Option<&fonts::FontConfig>,
) -> Result<(), MdpError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(MdpError::IoError {
                message: "Output directory does not exist".to_string(),
                path: parent.display().to_string(),
                suggestion: format!("Create the directory first: mkdir -p {}", parent.display()),
            });
        }
    }

    let tokens = parse_markdown(markdown)?;
    render::render_to_file(tokens, style, font_config, path)
}

pub fn parse_into_file(
    markdown: String,
    path: impl AsRef<std::path::Path>,
    config: config::ConfigSource,
    font_config: Option<&fonts::FontConfig>,
) -> Result<(), MdpError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(MdpError::IoError {
                message: "Output directory does not exist".to_string(),
                path: parent.display().to_string(),
                suggestion: format!("Create the directory first: mkdir -p {}", parent.display()),
            });
        }
    }

    let tokens = parse_markdown(markdown)?;
    let style = config::load_config_from_source(config);
    render::render_to_file(tokens, style, font_config, path)
}

/// Lex markdown and map lexer errors to `MdpError::ParseError`. Used
/// by every public entry point.
fn parse_markdown(markdown: String) -> Result<Vec<markdown::Token>, MdpError> {
    let mut lexer = Lexer::new(markdown);
    lexer.parse().map_err(|e| {
        let (line, column) = e.position();
        let (message, suggestion) = match &e {
            markdown::LexerError::UnexpectedEndOfInput { .. } => (
                "Unexpected end of input".to_string(),
                "Check for unclosed code blocks (```), links, or image tags".to_string(),
            ),
            markdown::LexerError::UnknownToken { message, .. } => (
                message.clone(),
                "Verify your Markdown syntax is valid. Try testing with a simpler document first."
                    .to_string(),
            ),
        };
        MdpError::ParseError {
            message,
            line: Some(line),
            column: Some(column),
            suggestion: Some(suggestion),
        }
    })
}

/// Transforms Markdown content into a styled PDF document and returns the PDF data as bytes.
/// This function provides the same conversion pipeline as `parse_into_file` but returns
/// the PDF content directly as a byte vector instead of writing to a file.
///
/// The process begins by parsing the Markdown content into a structured token representation.
/// It then applies styling rules based on the provided configuration source.
/// Finally, it generates the PDF document with the appropriate styling and structure.
///
/// # Arguments
/// * `markdown` - The Markdown content to convert
/// * `config` - Configuration source (Default, File path, or Embedded TOML)
///
/// # Returns
/// * `Ok(Vec<u8>)` containing the PDF data on successful conversion
/// * `Err(MdpError)` if errors occur during parsing or PDF generation
///
/// # Example
/// ```rust
/// use std::fs;
/// use std::error::Error;
/// use markdown2pdf::config::ConfigSource;
/// use markdown2pdf::fonts::FontConfig;
///
/// fn example() -> Result<(), Box<dyn Error>> {
///     let markdown = "# Hello World\nThis is a test.".to_string();
///
///     // Use embedded configuration
///     const EMBEDDED: &str = r#"
///         [heading.1]
///         size = 18
///         bold = true
///     "#;
///     let pdf_bytes = markdown2pdf::parse_into_bytes(markdown, ConfigSource::Embedded(EMBEDDED), None)?;
///
///     // Save to file or send over network
///     fs::write("output.pdf", pdf_bytes)?;
///     Ok(())
/// }
/// ```
pub fn parse_into_bytes(
    markdown: String,
    config: config::ConfigSource,
    font_config: Option<&fonts::FontConfig>,
) -> Result<Vec<u8>, MdpError> {
    let tokens = parse_markdown(markdown)?;
    let style = config::load_config_from_source(config);
    render::render_to_bytes(tokens, style, font_config)
}

/// Variant of [`parse_into_bytes`] that takes a pre-resolved style
/// instead of a `ConfigSource`. Mirrors [`parse_into_file_with_style`]
/// for callers that already have a `ResolvedStyle` in hand (web
/// services, in-memory pipelines).
pub fn parse_into_bytes_with_style(
    markdown: String,
    style: styling::ResolvedStyle,
    font_config: Option<&fonts::FontConfig>,
) -> Result<Vec<u8>, MdpError> {
    let tokens = parse_markdown(markdown)?;
    render::render_to_bytes(tokens, style, font_config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_basic_markdown_conversion() {
        let markdown = "# Test\nHello world".to_string();
        let result = parse_into_file(
            markdown,
            "test_output.pdf",
            config::ConfigSource::Default,
            None,
        );
        assert!(result.is_ok());
        fs::remove_file("test_output.pdf").unwrap();
    }

    #[test]
    fn test_invalid_output_path_does_not_swallow_real_errors() {
        // The lexer is intentionally permissive — historically malformed
        // inputs like `![Invalid` or unterminated `<!--` would surface as
        // a ParseError. Both now gracefully fall back to literal text, so
        // a parse failure is no longer a useful integration test for the
        // error path. Other paths (font load, file write, etc.) still
        // surface errors via MdpError, exercised by other tests below.
        let markdown = "<!--never closes".to_string();
        let result = parse_into_file(
            markdown,
            "error_output.pdf",
            config::ConfigSource::Default,
            None,
        );
        // The render either succeeds or fails for a non-parse reason; the
        // important contract is that a parse error is NOT raised.
        if let Err(MdpError::ParseError { .. }) = result {
            panic!("lexer should treat unclosed comment as literal text, not a parse error");
        }
        let _ = std::fs::remove_file("error_output.pdf");
    }

    #[test]
    fn test_invalid_output_path() {
        let markdown = "# Test".to_string();
        let result = parse_into_file(
            markdown,
            "/nonexistent/directory/output.pdf",
            config::ConfigSource::Default,
            None,
        );
        assert!(matches!(
            result,
            Err(MdpError::IoError { .. }) | Err(MdpError::PdfError { .. })
        ));
    }

    #[test]
    fn test_basic_markdown_to_bytes() {
        let markdown = "# Test\nHello world".to_string();
        let result = parse_into_bytes(markdown, config::ConfigSource::Default, None);
        assert!(result.is_ok());
        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn parse_into_bytes_with_style_renders() {
        let markdown = "# Test\nBody".to_string();
        let style = styling::ResolvedStyle::default();
        let bytes = parse_into_bytes_with_style(markdown, style, None).expect("render");
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn parse_error_display_includes_line_and_column_when_present() {
        let err = MdpError::ParseError {
            message: "Bad token".to_string(),
            line: Some(7),
            column: Some(3),
            suggestion: None,
        };
        let s = format!("{}", err);
        assert!(
            s.contains("line 7") && s.contains("column 3"),
            "expected line/column in display, got: {}",
            s
        );
    }

    #[test]
    fn parse_error_display_omits_position_when_absent() {
        let err = MdpError::ParseError {
            message: "Bad token".to_string(),
            line: None,
            column: None,
            suggestion: None,
        };
        let s = format!("{}", err);
        assert!(!s.contains("line"), "unexpected position in display: {}", s);
    }

    #[test]
    fn parse_into_file_accepts_pathbuf_and_str() {
        let markdown = "# Hi".to_string();
        let pathbuf = std::env::temp_dir().join("m2p_asref_test.pdf");
        parse_into_file(
            markdown.clone(),
            &pathbuf,
            config::ConfigSource::Default,
            None,
        )
        .expect("PathBuf path works");
        assert!(pathbuf.exists());
        let _ = fs::remove_file(&pathbuf);

        let path_str = pathbuf.to_str().unwrap();
        parse_into_file(markdown, path_str, config::ConfigSource::Default, None)
            .expect("&str path works");
        let _ = fs::remove_file(&pathbuf);
    }

    #[test]
    fn test_embedded_config_file_output() {
        const EMBEDDED_CONFIG: &str = r#"
            [margin]
            top = 20.0
            right = 20.0
            bottom = 20.0
            left = 20.0

            [heading.1]
            size = 20
            bold = true
            alignment = "center"
        "#;

        let markdown = "# Test Heading\nThis is test content.".to_string();
        let result = parse_into_file(
            markdown,
            "test_embedded_output.pdf",
            config::ConfigSource::Embedded(EMBEDDED_CONFIG),
            None,
        );
        assert!(result.is_ok());

        assert!(std::path::Path::new("test_embedded_output.pdf").exists());
        fs::remove_file("test_embedded_output.pdf").unwrap();
    }

    #[test]
    fn test_embedded_config_bytes_output() {
        const EMBEDDED_CONFIG: &str = r#"
            [text]
            size = 14
            alignment = "justify"
            fontfamily = "helvetica"

            [heading.1]
            size = 18   
            textcolor = { r = 100, g = 100, b = 100 }
        "#;

        let markdown =
            "# Hello World\nThis is a test document with embedded configuration.".to_string();
        let result = parse_into_bytes(
            markdown,
            config::ConfigSource::Embedded(EMBEDDED_CONFIG),
            None,
        );
        assert!(result.is_ok());

        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn test_embedded_config_invalid_toml() {
        const INVALID_CONFIG: &str = "this is not valid toml {{{";

        let markdown = "# Test\nContent".to_string();
        let result = parse_into_bytes(
            markdown,
            config::ConfigSource::Embedded(INVALID_CONFIG),
            None,
        );
        assert!(result.is_ok());

        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
    }

    #[test]
    fn test_embedded_config_empty() {
        const EMPTY_CONFIG: &str = "";

        let markdown = "# Test\nContent".to_string();
        let result = parse_into_bytes(markdown, config::ConfigSource::Embedded(EMPTY_CONFIG), None);
        assert!(result.is_ok());

        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
    }

    #[test]
    fn test_config_source_variants() {
        let markdown = "# Test\nContent".to_string();

        let result = parse_into_bytes(markdown.clone(), config::ConfigSource::Default, None);
        assert!(result.is_ok());

        const EMBEDDED: &str = r#"
            [heading.1]
            size = 16
            bold = true
        "#;
        let result = parse_into_bytes(
            markdown.clone(),
            config::ConfigSource::Embedded(EMBEDDED),
            None,
        );
        assert!(result.is_ok());

        let result = parse_into_bytes(
            markdown,
            config::ConfigSource::File("nonexistent.toml"),
            None,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_complex_markdown_to_bytes() {
        let markdown = r#"
# Document Title

This is a paragraph with **bold** and *italic* text.

## Subheading

- List item 1
- List item 2
  - Nested item

1. Ordered item 1
2. Ordered item 2

```rust
fn hello() {
    println!("Hello, world!");
}
```

[Link example](https://example.com)

---

Final paragraph.
        "#
        .to_string();

        let result = parse_into_bytes(markdown, config::ConfigSource::Default, None);
        assert!(result.is_ok());
        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn test_empty_markdown_to_bytes() {
        let markdown = "".to_string();
        let result = parse_into_bytes(markdown, config::ConfigSource::Default, None);
        assert!(result.is_ok());
        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn test_partial_html_comment_is_not_a_parse_error() {
        // The lexer falls back to literal text for unterminated HTML
        // comments; the byte-level entrypoint should mirror that.
        let markdown = "<!--never closes".to_string();
        let result = parse_into_bytes(markdown, config::ConfigSource::Default, None);
        if let Err(MdpError::ParseError { .. }) = result {
            panic!("lexer should treat unclosed comment as literal text, not a parse error");
        }
    }

    #[test]
    fn test_link_styling_with_underline() {
        const LINK_STYLE_CONFIG: &str = r#"
            [link]
            size = 10
            textcolor = { r = 0, g = 0, b = 200 }
            bold = true
            italic = false
            underline = true
            strikethrough = false
        "#;

        let markdown = r#"
# Links Test

- [Styled link](https://example.com)
- [Another styled link](https://example.org)
        "#
        .to_string();

        let result = parse_into_bytes(
            markdown,
            config::ConfigSource::Embedded(LINK_STYLE_CONFIG),
            None,
        );
        assert!(result.is_ok());
        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn test_link_styling_with_strikethrough() {
        const LINK_STYLE_CONFIG: &str = r#"
            [link]
            size = 10
            textcolor = { r = 200, g = 0, b = 0 }
            bold = false
            italic = true
            underline = false
            strikethrough = true
        "#;

        let markdown = "[Strikethrough link](https://example.com)".to_string();

        let result = parse_into_bytes(
            markdown,
            config::ConfigSource::Embedded(LINK_STYLE_CONFIG),
            None,
        );
        assert!(result.is_ok());
        let pdf_bytes = result.unwrap();
        assert!(!pdf_bytes.is_empty());
        assert!(pdf_bytes.starts_with(b"%PDF-"));
    }
}
