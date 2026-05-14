//! PDF renderer for markdown2pdf.
//!
//! Owns the full path from a [`Token`] stream to PDF bytes. Built
//! directly on `printpdf 0.9` — no layout-engine abstraction between
//! us and the PDF backend.
//!
//! # Architecture
//!
//! ```text
//! Token (lexer output)
//!     │  lower::lower
//!     ▼
//! Vec<Block>            ← block IR (heading, paragraph, code, hr)
//!     │
//!     │  layout::lay_out_pages
//!     │      ↑ font::FontMetricsCache (glyph widths via ttf-parser)
//!     ▼
//! Vec<PdfPage>          ← printpdf 0.9 page operation streams
//!     │  PdfDocument::with_pages + save
//!     ▼
//! Vec<u8>               ← serialized PDF bytes
//! ```
//!
//! # Current scope — phase 1
//!
//! - Headings (levels 1–6), with size + bold from `StyleMatch`
//! - Paragraphs with real glyph-advance-based wrapping
//! - Code blocks in Courier (one PDF line per source line)
//! - Horizontal rules drawn as a horizontal line
//! - Inline emphasis (bold / italic / inline code) inside paragraphs
//!   and headings — each run uses the matching built-in font variant
//! - Page splits when content overflows the bottom margin
//!
//! # Out of scope for phase 1 (planned phases)
//!
//! - Lists (bullets, ordered, task) — phase 2
//! - Blockquotes (left rule + indent) — phase 2
//! - Hyperlinks (PDF annotations) — phase 2
//! - Tables — phase 3
//! - Images (decode + embed via printpdf images feature) — phase 4
//! - Justified alignment + hyphenation — phase 5
//!
//! Tokens whose dedicated layout hasn't landed yet (lists, blockquotes,
//! tables, html) degrade gracefully to plain paragraphs — they remain
//! visible in the output rather than disappearing.

mod font;
mod ir;
mod layout;
mod lower;

use crate::markdown::Token;
use crate::styling::StyleMatch;
use crate::{MdpError, fonts::FontConfig};

use printpdf::{PdfDocument, PdfSaveOptions};

/// Render a token stream to a PDF file at `path`.
pub fn render_to_file(
    tokens: Vec<Token>,
    style: StyleMatch,
    font_config: Option<&FontConfig>,
    path: &str,
) -> Result<(), MdpError> {
    let bytes = render_to_bytes(tokens, style, font_config)?;
    std::fs::write(path, bytes).map_err(|e| MdpError::PdfError {
        message: e.to_string(),
        path: Some(path.to_string()),
        suggestion: Some(
            "Check that the output directory exists and you have write permissions".to_string(),
        ),
    })
}

/// Render a token stream to PDF bytes.
pub fn render_to_bytes(
    tokens: Vec<Token>,
    style: StyleMatch,
    font_config: Option<&FontConfig>,
) -> Result<Vec<u8>, MdpError> {
    let mut doc = PdfDocument::new("markdown2pdf");

    // Collect every distinct character in the document so the font
    // loader can pre-populate its codepoint -> glyph table without
    // walking the font's full cmap.
    let body_text = Token::collect_all_text(&tokens);
    let used_codepoints: Vec<char> = {
        let mut chars: Vec<char> = body_text.chars().collect();
        chars.sort();
        chars.dedup();
        chars
    };

    let blocks = lower::lower(&tokens);
    let usage = ir::VariantUsage::analyze(&blocks);
    let font_set = font::FontSet::load(font_config, &used_codepoints, usage, &mut doc);
    let pages = layout::lay_out_pages(&blocks, &style, &font_set, &mut doc);

    // Always include at least one page so the resulting PDF is valid
    // even for an empty token stream.
    let pages = if pages.is_empty() {
        vec![printpdf::PdfPage::new(
            printpdf::Mm(210.0),
            printpdf::Mm(297.0),
            Vec::new(),
        )]
    } else {
        pages
    };

    let mut warnings = Vec::new();
    let bytes = doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut warnings);

    for w in &warnings {
        log::warn!("printpdf: {:?}", w);
    }

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Token;

    fn default_style() -> StyleMatch {
        StyleMatch::default()
    }

    #[test]
    fn empty_token_stream_produces_valid_pdf() {
        let bytes = render_to_bytes(vec![], default_style(), None).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn paragraph_produces_valid_pdf() {
        let tokens = vec![Token::Text("hello world".to_string())];
        let bytes = render_to_bytes(tokens, default_style(), None).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn heading_produces_valid_pdf() {
        let tokens = vec![Token::Heading(vec![Token::Text("Hi".into())], 1)];
        let bytes = render_to_bytes(tokens, default_style(), None).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn long_document_splits_pages() {
        let mut tokens = Vec::new();
        for i in 0..150 {
            tokens.push(Token::Text(format!("paragraph {}", i)));
            tokens.push(Token::Newline);
            tokens.push(Token::Newline);
        }
        let bytes = render_to_bytes(tokens, default_style(), None).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn render_to_file_creates_file() {
        let path = std::env::temp_dir().join("m2p_phase1.pdf");
        let path_s = path.to_str().unwrap();
        let tokens = vec![
            Token::Heading(vec![Token::Text("Hello".into())], 1),
            Token::Text("World".into()),
        ];
        render_to_file(tokens, default_style(), None, path_s).unwrap();
        assert!(path.exists());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let _ = std::fs::remove_file(&path);
    }
}
