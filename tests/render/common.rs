//! Shared helpers for renderer integration tests. Lives under
//! `tests/render/` and is included by every per-feature test file via
//! the `#[path]` machinery in `tests/render.rs`, mirroring the layout
//! used by the lexer tests in `tests/markdown/`.

#![allow(dead_code)] // not every test file uses every helper

use markdown2pdf::config::ConfigSource;
use markdown2pdf::fonts::{FontConfig, FontSource};
use markdown2pdf::parse_into_bytes;

/// Render markdown + an embedded TOML config to PDF bytes. Panics on
/// any error so individual tests don't have to unwrap.
///
/// Forces the renderer onto the built-in Type 1 Helvetica/Courier
/// path: every text op is emitted as a parenthesized WinAnsi string
/// (`(hello) Tj`), which the test assertions (`contains_text`,
/// `count_substr`, byte-level scans) can search verbatim. The
/// production default — auto-detect a system Unicode font when the
/// caller passes `None` — uses Identity-H glyph IDs, which would make
/// every text-content scan fail. Tests that specifically exercise the
/// auto-detect path (in `tests/render/fonts.rs`) call
/// `parse_into_bytes` directly with `None`.
pub fn render(md: &str, cfg_toml: &str) -> Vec<u8> {
    let cfg = FontConfig::new().with_default_font_source(FontSource::Builtin("Helvetica"));
    let bytes =
        parse_into_bytes(md.to_string(), ConfigSource::Embedded(cfg_toml), Some(&cfg))
            .expect("render must succeed");
    // The renderer Flate-compresses streams (printpdf 0.9 never does,
    // so we deflate in post-process). Tests inspect drawing operators
    // and visible text in the byte stream, so expand it back to the
    // (still 100%-valid) uncompressed form printpdf used to emit —
    // size doesn't matter in tests and this keeps every assertion,
    // helper-based or inline, working unchanged. Lossless and a no-op
    // if already uncompressed.
    if let Ok(mut doc) = lopdf::Document::load_mem(&bytes) {
        doc.decompress();
        let mut out = Vec::new();
        if doc.save_to(&mut out).is_ok() && out.starts_with(b"%PDF-") {
            return out;
        }
    }
    bytes
}

/// The PDF flattened back to the plain, fully-expanded shape printpdf
/// originally emitted: every stream Flate-*decompressed* in place and
/// every object-stream-packed object written back out as an
/// individual classic object (the post-process now ships PDF 1.5
/// object + cross-reference streams). `Document::load_mem` resolves
/// object streams into the object map and the classic `save_to`
/// re-serializes each object individually, so structural scans for
/// `/Type/Page`, `/Ascent`, … keep working unchanged. Each stream's
/// content appears exactly once (streams are *replaced*, not
/// appended). Idempotent: a no-op on an already-plain PDF (so it
/// composes safely with `render`, which also expands), and falls back
/// to the input on any parse / serialize failure.
pub fn scan(bytes: &[u8]) -> Vec<u8> {
    if let Ok(mut doc) = lopdf::Document::load_mem(bytes) {
        doc.decompress();
        let mut out = Vec::new();
        if doc.save_to(&mut out).is_ok() && out.starts_with(b"%PDF-") {
            return out;
        }
    }
    bytes.to_vec()
}

/// `true` if `needle` appears anywhere in the PDF (raw structure or
/// decompressed content).
pub fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    scan(bytes).windows(needle.len()).any(|w| w == needle)
}

/// Count filled rectangles in the content stream. Block backgrounds
/// are emitted as a closed 4-corner polygon path terminated by the
/// PDF fill operator `f` on its own line (printpdf 0.9's
/// `Op::DrawRectangle` is broken — it discards the path with `n` —
/// so the renderer uses `Op::DrawPolygon`, whose serializer ends a
/// non-zero-winding fill with `h` then `f`). We count standalone
/// `f` fill ops, which only the background-rect path emits.
pub fn count_rect_ops(bytes: &[u8]) -> usize {
    let bytes = scan(bytes);
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
    let bytes = scan(bytes);
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
    let bytes = scan(bytes);
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
    let bytes = scan(bytes);
    let s = String::from_utf8_lossy(&bytes);
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
    String::from_utf8_lossy(&scan(bytes)).contains(needle)
}

/// First installed system font from a cross-platform candidate list,
/// or `None` if the host has none (some minimal CI images). Tests
/// that exercise the *external font* path use this instead of
/// hardcoding `"Georgia"` (which only exists on macOS/Windows) so
/// they run on Linux CI too and skip cleanly only when truly no
/// system font is available.
pub fn any_system_font() -> Option<String> {
    const CANDIDATES: &[&str] = &[
        "Georgia",
        "DejaVu Sans",
        "DejaVuSans",
        "Liberation Sans",
        "LiberationSans",
        "Noto Sans",
        "NotoSans",
        "Arial",
        "Helvetica",
        "Verdana",
        "FreeSans",
    ];
    CANDIDATES
        .iter()
        .find(|name| markdown2pdf::fonts::find_system_font(name).is_some())
        .map(|s| s.to_string())
}

/// Path to a small real JPEG generated on demand in the system temp
/// dir. Image tests use this instead of an `examples/` fixture so
/// they have no dependency on an uncommitted file — CI checkouts
/// don't carry the untracked `examples/` directory.
pub fn temp_jpeg_path() -> String {
    use image::{DynamicImage, ImageFormat, RgbImage};
    use std::sync::atomic::{AtomicU64, Ordering};
    // Unique path per call. Image tests run in parallel; a fixed
    // shared filename races — `fs::write` truncates then fills, so a
    // concurrent test can read a half-written JPEG, the image silently
    // fails to decode, and it takes its caption with it (flaky under
    // full-suite load, fine in isolation). pid + counter isolates
    // every call.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "m2p_test_fixture_image_{}_{n}.jpg",
        std::process::id()
    ));
    // Wide enough that a short caption renders on one line — captions
    // are wrap-constrained to the rendered image width, so a narrow
    // fixture would split `(This is a caption)` across multiple `Tj`
    // operands and break caption tests.
    let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
        1400,
        900,
        image::Rgb([88, 110, 150]),
    ));
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Jpeg)
        .expect("encode fixture jpeg");
    std::fs::write(&path, buf).expect("write fixture jpeg");
    path.to_string_lossy().to_string()
}
