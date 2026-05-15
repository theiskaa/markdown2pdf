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
//! What the renderer covers today:
//!
//! - Headings (levels 1–6), paragraphs with glyph-advance wrapping,
//!   fenced code blocks, horizontal rules with per-style dash + width
//! - Inline emphasis (bold / italic / inline code / strike / underline)
//!   with per-variant external Unicode font selection
//! - Unordered, ordered, and task lists with configurable bullets and
//!   loose-vs-tight spacing
//! - Block backgrounds, per-side borders, padding, line-height —
//!   all reading from the resolved style schema
//! - Blockquotes with configurable border (replacing the old hardcoded
//!   left rule)
//! - GFM tables with per-column alignment, header repeat across pages
//! - Local-file images (PNG / JPEG); URL fetch is gated under the
//!   `fetch` feature
//! - Hyperlinks as PDF link annotations
//! - Block-level HTML treated as monospace; comment-only blocks are
//!   invisible per CommonMark §4.6
//! - Page splits with header repeat for tables
//!
//! Known gaps:
//!
//! - Real justified text (Knuth-Plass) — `text_align = justify`
//!   silently degrades to left
//! - Cross-page block background fragments — a paragraph that spans
//!   pages paints its background only on the starting page
//! - URL image fetching, inline link tooltips, footnotes, headers /
//!   footers, page numbers, TOC, bookmarks — all roadmap items

mod font;
mod hyphenate;
mod ir;
mod layout;
mod lower;
mod postprocess;

use crate::markdown::Token;
use crate::styling::ResolvedStyle;
use crate::{MdpError, fonts::FontConfig};

use printpdf::{PdfDocument, PdfSaveOptions};

/// Render a token stream to a PDF file at `path`.
pub fn render_to_file(
    tokens: Vec<Token>,
    style: ResolvedStyle,
    font_config: Option<&FontConfig>,
    path: impl AsRef<std::path::Path>,
) -> Result<(), MdpError> {
    let path = path.as_ref();
    let bytes = render_to_bytes(tokens, style, font_config)?;
    std::fs::write(path, bytes).map_err(|e| MdpError::PdfError {
        message: e.to_string(),
        path: Some(path.display().to_string()),
        suggestion: Some(
            "Check that the output directory exists and you have write permissions".to_string(),
        ),
    })
}

/// Render a token stream to PDF bytes.
pub fn render_to_bytes(
    tokens: Vec<Token>,
    style: ResolvedStyle,
    font_config: Option<&FontConfig>,
) -> Result<Vec<u8>, MdpError> {
    let doc_title = style
        .metadata
        .title
        .clone()
        .unwrap_or_else(|| "markdown2pdf".to_string());
    let mut doc = PdfDocument::new(&doc_title);

    {
        let info = &mut doc.metadata.info;
        info.document_title = doc_title.clone();
        if let Some(a) = &style.metadata.author {
            info.author = a.clone();
        }
        if let Some(s) = &style.metadata.subject {
            info.subject = s.clone();
        }
        if let Some(c) = &style.metadata.creator {
            info.creator = c.clone();
        }
        if !style.metadata.keywords.is_empty() {
            info.keywords = style.metadata.keywords.clone();
        }
    }

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

    let (fallback_w, fallback_h) = layout::page_dimensions_mm(&style.page);
    let pages = if pages.is_empty() {
        vec![printpdf::PdfPage::new(
            printpdf::Mm(fallback_w),
            printpdf::Mm(fallback_h),
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

    // Inject `/Contents` (tooltip) entries on link annotations using
    // titles from `[text](url "title")`. printpdf 0.9 doesn't expose
    // `/Contents` on its `LinkAnnotation` struct, so we parse the
    // serialized bytes back with lopdf and patch them in.
    let tooltips = postprocess::collect_link_tooltips(&tokens);
    let bytes = postprocess::inject_link_tooltips(bytes, &tooltips);

    // Catalog `/Lang` for accessibility — printpdf 0.9 doesn't expose
    // it. No-op when no language is configured.
    let bytes = match &style.metadata.language {
        Some(lang) => postprocess::inject_lang(bytes, lang),
        None => bytes,
    };

    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Token;

    fn default_style() -> ResolvedStyle {
        ResolvedStyle::default()
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
