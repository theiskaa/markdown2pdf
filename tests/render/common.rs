//! Shared helpers for renderer integration tests. Lives under
//! `tests/render/` and is included by every per-feature test file via
//! the `#[path]` machinery in `tests/render.rs`, mirroring the layout
//! used by the lexer tests in `tests/markdown/`.

#![allow(dead_code)] // not every test file uses every helper

use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;

/// Render markdown + an embedded TOML config to PDF bytes. Panics on
/// any error so individual tests don't have to unwrap.
pub fn render(md: &str, cfg_toml: &str) -> Vec<u8> {
    parse_into_bytes(md.to_string(), ConfigSource::Embedded(cfg_toml), None)
        .expect("render must succeed")
}

/// `true` if `needle` appears anywhere in `bytes`.
pub fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

/// Count filled rectangles in the content stream. Block backgrounds
/// are emitted as a closed 4-corner polygon path terminated by the
/// PDF fill operator `f` on its own line (printpdf 0.9's
/// `Op::DrawRectangle` is broken — it discards the path with `n` —
/// so the renderer uses `Op::DrawPolygon`, whose serializer ends a
/// non-zero-winding fill with `h` then `f`). We count standalone
/// `f` fill ops, which only the background-rect path emits.
pub fn count_rect_ops(bytes: &[u8]) -> usize {
    let mut hits = 0usize;
    let mut i = 0usize;
    // Match a line that is exactly `f` (preceded and followed by a
    // line break). Text/`Tf`/`rg` never produce a bare `f` line.
    while i + 3 <= bytes.len() {
        let prev = bytes[i];
        let mid = bytes[i + 1];
        let next = bytes[i + 2];
        if matches!(prev, b'\n' | b'\r')
            && mid == b'f'
            && matches!(next, b'\n' | b'\r')
        {
            hits += 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    hits
}

/// `true` if the content stream contains a path-stroke op (`S` or `s`
/// preceded by whitespace) — borders and HR lines emit one.
pub fn bytes_have_stroke_op(bytes: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 2 <= bytes.len() {
        let c = bytes[i];
        let n = bytes[i + 1];
        if (c == b' ' || c == b'\n' || c == b'\r')
            && (n == b'S' || n == b's')
            && bytes
                .get(i + 2)
                .map(|b| matches!(b, b'\n' | b' ' | b'\r'))
                .unwrap_or(true)
        {
            return true;
        }
        i += 1;
    }
    false
}

/// Generate a markdown document with `n` paragraphs of filler text.
/// Used by header / footer / TOC tests that need predictable
/// page-count behavior.
pub fn multi_page_markdown(n_paragraphs: usize) -> String {
    let mut out = String::new();
    for i in 0..n_paragraphs {
        out.push_str(&format!(
            "Paragraph {} with body text long enough to take up real space on the page. {}\n\n",
            i,
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(8)
        ));
    }
    out
}

/// Count case-sensitive occurrences of `needle` in `bytes`. Used by
/// tests that assert a string appears N times (e.g. a heading appears
/// once in TOC and once in body).
pub fn count_substr(bytes: &[u8], needle: &[u8]) -> usize {
    let mut count = 0usize;
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            count += 1;
            i += needle.len();
        } else {
            i += 1;
        }
    }
    count
}

/// `true` if the PDF byte stream starts with `%PDF-` and ends with
/// `%%EOF` (within the last 16 bytes). Useful as a "render didn't
/// crash" sanity check.
pub fn pdf_well_formed(bytes: &[u8]) -> bool {
    if !bytes.starts_with(b"%PDF-") {
        return false;
    }
    let tail = &bytes[bytes.len().saturating_sub(16)..];
    String::from_utf8_lossy(tail).contains("%%EOF")
}

/// Count how many content pages the PDF advertises. printpdf 0.9
/// emits `/Type/Page` (no inner whitespace) per page and a single
/// `/Type/Pages` for the page tree root; this only counts the
/// singular form.
pub fn page_count(bytes: &[u8]) -> usize {
    let s = String::from_utf8_lossy(bytes);
    let needle = "/Type/Page";
    let mut total = 0usize;
    let mut start = 0usize;
    while let Some(pos) = s[start..].find(needle) {
        let abs = start + pos;
        let after = s.as_bytes().get(abs + needle.len()).copied();
        match after {
            Some(b's') => {} // `/Type/Pages` — skip
            _ => total += 1,
        }
        start = abs + needle.len();
    }
    total
}

/// `true` if `bytes` (PDF content stream) contains `needle` as raw
/// text — uses lossy UTF-8 decoding which is fine since we only ever
/// search for ASCII fragments in the printpdf-emitted content.
pub fn contains_text(bytes: &[u8], needle: &str) -> bool {
    String::from_utf8_lossy(bytes).contains(needle)
}
