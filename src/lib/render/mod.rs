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
mod image_policy;
mod ir;
mod layout;
mod lower;
mod math;
#[cfg(feature = "fetch")]
mod net_guard;
#[cfg(feature = "fetch")]
mod net_read;
mod postprocess;
mod preprocess;

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
    mut tokens: Vec<Token>,
    style: ResolvedStyle,
    font_config: Option<&FontConfig>,
) -> Result<Vec<u8>, MdpError> {
    // Recognise inline `<a href="…">…</a>` HTML up front so the
    // renderer's normal link path (and the tooltip post-pass below)
    // handles it like any markdown link.
    preprocess::rewrite_html_anchors(&mut tokens);

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
    let blocks = lower::lower(&tokens);
    // Codepoint set seeded from the source body, then extended with
    // every string the layout pass synthesizes (admonition kind
    // labels, the auto "Footnotes" heading, TOC title, title-page
    // text, header/footer furniture templates) so the external-font
    // subset includes the glyphs needed to render them. Without this
    // seeding e.g. `[!IMPORTANT]` would emit `.notdef` boxes for
    // every letter in `IMPORTANT` that didn't happen to appear in
    // the source body.
    let used_codepoints: Vec<char> = {
        let mut chars: Vec<char> = body_text.chars().collect();
        collect_synthesized_codepoints(&blocks, &style, &mut chars);
        collect_style_codepoints(&style, &mut chars);
        chars.sort();
        chars.dedup();
        chars
    };

    let mut usage = ir::VariantUsage::analyze(&blocks);
    // Headings and blockquotes get their weight / slant from the
    // theme, not from per-run flags, so the IR walk above can't see
    // them. Without this, an external font would skip loading the
    // bold (or italic) face and these blocks would render regular.
    for block_style in style.headings.iter().chain([&style.blockquote]) {
        if block_style.is_bold() && block_style.is_italic() {
            usage.body_bold_italic = true;
        } else if block_style.is_bold() {
            usage.body_bold = true;
        } else if block_style.is_italic() {
            usage.body_italic = true;
        }
    }
    let cb = &style.code_block;
    if cb.is_bold() && cb.is_italic() {
        usage.mono_bold_italic = true;
    } else if cb.is_bold() {
        usage.mono_bold = true;
    } else if cb.is_italic() {
        usage.mono_italic = true;
    }
    // Load a distinct inline-code family only when `[code_inline]
    // font_family` is set AND differs from `[code_block] font_family`.
    // Otherwise inline and block code share the same path (so the
    // default theme, which spells both `"Courier"`, stays byte-
    // identical to the pre-feature output).
    let code_inline_font = match (
        style.code_inline.font_family.as_deref(),
        style.code_block.font_family.as_deref(),
    ) {
        (Some(ci), Some(cb)) if ci.eq_ignore_ascii_case(cb) => None,
        (ci, _) => ci,
    };
    let font_set = font::FontSet::load_with_style_fallbacks(
        font_config,
        &style.fallback_fonts,
        code_inline_font,
        &used_codepoints,
        usage,
        &mut doc,
    );
    let known_heading_slugs = collect_heading_slugs(&blocks);
    let pages = layout::lay_out_pages(&blocks, &style, &font_set, &known_heading_slugs, &mut doc);

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

    // printpdf 0.9 never compresses streams; deflate them ourselves
    // (math vector outlines make raw page streams very large).
    let bytes = postprocess::compress(bytes);

    Ok(bytes)
}

/// Collect every heading's slug from the lowered IR so the layout
/// pass can distinguish resolved internal links from unresolved
/// ones. Walks in document order and mirrors `render_heading`'s
/// `-2`, `-3`, … suffix policy so a link like `#dup-2` to the
/// second of two same-text headings still resolves.
fn collect_heading_slugs(blocks: &[ir::Block]) -> std::collections::HashSet<String> {
    use crate::markdown::slugify;
    let mut out = std::collections::HashSet::new();
    fn walk(blocks: &[ir::Block], out: &mut std::collections::HashSet<String>) {
        for b in blocks {
            match b {
                ir::Block::Heading { runs, .. } => {
                    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    let base = {
                        let s = slugify(&text);
                        if s.is_empty() { "section".to_string() } else { s }
                    };
                    let mut slug = base.clone();
                    let mut n = 2usize;
                    while out.contains(&slug) {
                        slug = format!("{}-{}", base, n);
                        n += 1;
                    }
                    out.insert(slug);
                }
                ir::Block::BlockQuote { body } | ir::Block::Admonition { body, .. } => {
                    walk(body, out);
                }
                ir::Block::List { entries } => {
                    for e in entries {
                        walk(&e.children, out);
                    }
                }
                _ => {}
            }
        }
    }
    walk(blocks, &mut out);
    out
}

/// Append every character that flows from `style` straight into the
/// rendered output without ever passing through the source markdown:
/// the TOC title, the title page's title / subtitle / author / date
/// fields, and the header / footer furniture templates on each of
/// the three (left / center / right) anchors. These are
/// user-configurable strings the body text need not contain, so an
/// external font's subset has to be told about them up front.
fn collect_style_codepoints(style: &ResolvedStyle, out: &mut Vec<char>) {
    if let Some(toc) = &style.toc {
        out.extend(toc.title.chars());
    }
    if let Some(tp) = &style.title_page {
        out.extend(tp.title.chars());
        if let Some(s) = &tp.subtitle {
            out.extend(s.chars());
        }
        if let Some(a) = &tp.author {
            out.extend(a.chars());
        }
        if let Some(d) = &tp.date {
            out.extend(d.chars());
        }
    }
    for f in [style.header.as_ref(), style.footer.as_ref()].into_iter().flatten() {
        for slot in [f.left.as_ref(), f.center.as_ref(), f.right.as_ref()] {
            if let Some(t) = slot {
                out.extend(t.chars());
            }
        }
    }
}

/// Walk the lowered IR and append every character the layout pass
/// might emit on its own — admonition kind labels, the auto
/// "Footnotes" section heading, etc. Recurses into containers so
/// labels from an admonition inside a blockquote are also captured.
fn collect_synthesized_codepoints(
    blocks: &[ir::Block],
    style: &ResolvedStyle,
    out: &mut Vec<char>,
) {
    for block in blocks {
        match block {
            ir::Block::Admonition { kind, raw_label, title, body } => {
                // The renderer uses raw_label.to_ascii_uppercase() when
                // no user title is set; fall back to the per-kind label
                // when raw_label is empty.
                if title.is_none() || title.as_deref().map(|r| r.is_empty()).unwrap_or(true) {
                    if !raw_label.is_empty() {
                        out.extend(raw_label.to_ascii_uppercase().chars());
                    } else {
                        out.extend(style.admonition.for_kind(kind).label.chars());
                    }
                }
                collect_synthesized_codepoints(body, style, out);
            }
            ir::Block::BlockQuote { body } => {
                collect_synthesized_codepoints(body, style, out);
            }
            ir::Block::List { entries } => {
                for entry in entries {
                    collect_synthesized_codepoints(&entry.children, style, out);
                }
            }
            ir::Block::FootnoteDefinitions { .. } => {
                // render_footnote_definitions auto-emits "Footnotes"
                // as the section heading text.
                out.extend("Footnotes".chars());
            }
            _ => {}
        }
    }
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
