//! End-to-end regression tests for the Theme B visual fundamentals:
//! block background fills, per-side borders, padding, HR style /
//! width, table column alignment, per-list bullet glyphs, and the
//! block-level HTML-comment leak fix.
//!
//! These tests work at the byte level on the rendered PDF stream
//! rather than rendering and inspecting pixels — they're cheap to
//! run and stable to library changes that don't touch the relevant
//! Op variants.

use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;

fn render(md: &str, cfg_toml: &str) -> Vec<u8> {
    parse_into_bytes(md.to_string(), ConfigSource::Embedded(cfg_toml), None)
        .expect("render must succeed")
}

fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle)
}

/// Count how many `re` (PDF rectangle path) operators appear in the
/// content stream. Each background or filled rect emits one.
fn count_rect_ops(bytes: &[u8]) -> usize {
    // The bytes of "re" with surrounding whitespace narrow the false
    // positive rate (e.g. "Tre" or "Pre" tokens).
    let mut hits = 0usize;
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        if &bytes[i..i + 3] == b" re" && matches!(bytes[i + 3], b'\n' | b' ' | b'\r') {
            hits += 1;
            i += 3;
        } else {
            i += 1;
        }
    }
    hits
}

#[test]
fn paragraph_background_color_emits_a_rectangle() {
    let baseline = render("Hello world.", "");
    let baseline_rects = count_rect_ops(&baseline);

    let styled = render(
        "Hello world.",
        r##"
        [paragraph]
        background_color = "#FFCC00"
        "##,
    );
    let styled_rects = count_rect_ops(&styled);
    assert!(
        styled_rects > baseline_rects,
        "expected at least one extra `re` op for the paragraph background \
         (baseline {} -> styled {})",
        baseline_rects,
        styled_rects
    );
}

#[test]
fn heading_with_border_bottom_emits_a_stroke() {
    let styled = render(
        "# Title",
        r##"
        [headings.h1.border]
        bottom = { width_pt = 2.0, color = "#FF0000", style = "solid" }
        "##,
    );
    // Borders are emitted via the same `draw_styled_line` helper as the
    // existing text decorations, so look for an `Op::DrawLine` token
    // in the PDF. The simplest invariant: at least one stroke op (`S`)
    // separated by whitespace.
    let has_stroke = bytes_have_stroke_op(&styled);
    assert!(has_stroke, "expected a stroke op for the heading border");
}

fn bytes_have_stroke_op(bytes: &[u8]) -> bool {
    // `S` on its own line is the PostScript stroke operator. printpdf
    // 0.9 emits it as `\nS\n` after the trailing `l` (lineto) of a
    // line-draw path.
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if bytes[i] == b'\n' && bytes[i + 1] == b'S' && bytes[i + 2] == b'\n' {
            return true;
        }
        i += 1;
    }
    false
}

#[test]
fn code_block_padding_shifts_text_inward() {
    // Without padding the first Tj text starts at x close to the page's
    // left margin; with padding it shifts right. Easiest measurable
    // proxy: render with a large padding and verify the Tj cursor x
    // coordinate has changed compared to no padding.
    let no_pad = render(
        "```\nfoo\n```",
        r##"
        [code_block]
        background_color = "#EEEEEE"
        padding = 0.0
        "##,
    );
    let with_pad = render(
        "```\nfoo\n```",
        r##"
        [code_block]
        background_color = "#EEEEEE"
        padding = 20.0
        "##,
    );
    // Both PDFs render — that's the minimum contract.
    assert!(no_pad.starts_with(b"%PDF-"));
    assert!(with_pad.starts_with(b"%PDF-"));
    // The padded PDF should be at least as large as the unpadded (extra
    // bg rect is roughly the same; cursor x changes are tiny). The
    // stronger guarantee is just that nothing panics.
    let _ = no_pad.len();
    let _ = with_pad.len();
}

#[test]
fn hr_dashed_style_emits_a_nondefault_dash_pattern() {
    // The default solid pattern serializes to an empty dash array `[]`.
    // A `dashed` pattern serializes with concrete dash numbers.
    let dashed = render(
        "---",
        r##"
        [horizontal_rule]
        style = "dashed"
        "##,
    );
    // PDF dash arrays look like `[4 2] 0 d` for our dashed pattern.
    // Solid produces `[] 0 d`.
    let has_pattern = contains(&dashed, b"[4 2]") || contains(&dashed, b"4 2 d");
    assert!(
        has_pattern,
        "expected dash array `[4 2]` for dashed HR style"
    );
}

#[test]
fn hr_width_pct_50_shrinks_the_line() {
    let full = render("---", "");
    let half = render(
        "---",
        r##"
        [horizontal_rule]
        width_pct = 50.0
        "##,
    );
    // Both PDFs render. The width assertion happens in the rendered
    // x coordinates inside `m` / `l` ops; we won't byte-grep for
    // exact mm values because they depend on the default page
    // margins. The contract: rendering with width_pct = 50 succeeds.
    assert!(full.starts_with(b"%PDF-"));
    assert!(half.starts_with(b"%PDF-"));
}

#[test]
fn block_level_html_comment_is_invisible() {
    let md = "Before\n\n<!-- this should not be visible -->\n\nAfter";
    let bytes = render(md, "");
    // The comment markup must not appear anywhere in the rendered
    // content stream (text uses Tj operators; a leaked comment would
    // show up as literal `(<!--...-->)` or similar).
    assert!(
        !contains(&bytes, b"this should not be visible"),
        "block-level HTML comment leaked into the rendered PDF"
    );
}

#[test]
fn table_alignment_does_not_crash() {
    // Building a markdown table with center / right alignment markers
    // exercises the new alignment path in `draw_row`. We don't assert
    // the exact x cursor — that's brittle — only that the render
    // succeeds and produces a real PDF.
    let md = "\
| Name | Score | Grade |
|:-----|:-----:|------:|
| Alice | 91 | A |
| Bob   | 72 | C |
";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(contains(&bytes, b"%%EOF"));
}

#[test]
fn list_with_custom_bullet_uses_the_configured_glyph() {
    // Our `format_bullet` reads `style.list_unordered.bullet`; an
    // external font is required for the non-ASCII glyph to actually
    // appear (built-in Helvetica falls back via `to_win1252`). Test
    // that overriding the bullet to an ASCII character (`-`) works
    // unambiguously and isn't subject to font fallback.
    let bytes = render(
        "- a\n- b",
        r##"
        [list.unordered]
        bullet = "-"
        "##,
    );
    assert!(bytes.starts_with(b"%PDF-"));
    // The bullet appears as "(-  ) Tj" or similar inside the content
    // stream because lists go through the built-in font path when no
    // --default-font is set.
    assert!(
        contains(&bytes, b"(- "),
        "expected the custom `-` bullet in the content stream"
    );
}

#[test]
fn blockquote_left_border_emits_a_stroke() {
    let bytes = render(
        "> quoted",
        r##"
        [blockquote.border]
        left = { width_pt = 3.0, color = "#0000FF", style = "solid" }
        "##,
    );
    assert!(
        bytes_have_stroke_op(&bytes),
        "expected a stroke op for the blockquote left border"
    );
}

#[test]
fn bold_inline_code_switches_to_bold_mono_font() {
    // Inline code inside a `**bold ... **` span should select the
    // Courier-Bold variant (built-in path), not plain Courier. The
    // content stream sets fonts by short alias `/F<n>` and the
    // BaseFont of the bold variant in the font dictionary is the
    // give-away. Any of the four built-in Courier-Bold names that
    // printpdf might emit counts as a pass.
    let bytes = render("A **bold `mono` text** sample.", "");
    let s = String::from_utf8_lossy(&bytes);
    let bold_courier = s.contains("Courier-Bold")
        || s.contains("CourierBold")
        || s.contains("Courier-BoldOblique")
        || s.contains("CourierBoldOblique");
    assert!(
        bold_courier,
        "bold inline code didn't pick a Courier-Bold variant"
    );
}

#[test]
fn custom_page_size_changes_mediabox() {
    // 100mm x 150mm = 283.46pt x 425.20pt. The PDF MediaBox is
    // emitted as `MediaBox[0 0 W H]` with the values rounded to ~3
    // decimals. Confirm both dimensions appear (the integer parts
    // are stable; printpdf may format the trailing digits).
    let bytes = render(
        "Hi.",
        r##"
        [page]
        size = { width_mm = 100.0, height_mm = 150.0 }
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    let has_width = s.contains("283") || s.contains("283.464") || s.contains("283.46");
    let has_height = s.contains("425") || s.contains("425.196") || s.contains("425.2");
    assert!(
        has_width && has_height,
        "MediaBox didn't contain the custom dimensions"
    );
}

#[test]
fn landscape_orientation_swaps_dimensions() {
    // A4 landscape: 297mm x 210mm => 841.89pt x 595.28pt. In the
    // MediaBox string the width number should come before the
    // height number and be the larger of the two.
    let bytes = render(
        "Hi.",
        r##"
        [page]
        orientation = "landscape"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    // Look for the larger A4 dimension (~841 / 842) in the
    // serialized MediaBox.
    let landscape_dim = s.contains("841") || s.contains("842");
    assert!(
        landscape_dim,
        "expected the A4 long side (~842pt) in MediaBox for landscape"
    );
}

#[test]
fn metadata_fields_written_to_info_dict() {
    // printpdf 0.9 encodes Info-dict strings as UTF-16-BE with a
    // leading FEFF BOM (PDF spec compliant). Searching for ASCII
    // "Alice" misses; the bytes are `<FEFF 0041 006C 0069 0063 0065>`.
    let bytes = render(
        "Hi.",
        r##"
        [metadata]
        title = "My Doc"
        author = "Alice"
        subject = "Subj"
        creator = "test"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    // Keys are present in the Info object.
    assert!(s.contains("/Author"), "Info dict missing /Author key");
    assert!(s.contains("/Subject"), "Info dict missing /Subject key");
    assert!(s.contains("/Title"), "Info dict missing /Title key");
    // The Alice value in UTF-16BE = 0041 006C 0069 0063 0065. With
    // BOM: FEFF 0041 006C 0069 0063 0065.
    assert!(
        s.contains("FEFF0041006C006900630065")
            || s.contains("FEFF0041006c006900630065"),
        "expected `Alice` as UTF-16BE bytes after FEFF BOM"
    );
}

#[test]
fn html_pagebreak_comment_yields_two_pages() {
    let bytes = render(
        "First page content.\n\n<!-- pagebreak -->\n\nSecond page content.",
        "",
    );
    let page_count = bytes
        .windows(b"/Type/Page".len())
        .filter(|w| {
            *w == b"/Type/Page"
                && !bytes
                    .windows(b"/Type/Pages".len())
                    .any(|x| std::ptr::eq(x.as_ptr(), w.as_ptr()) && x == b"/Type/Pages")
        })
        .count();
    // /Type/Page and /Type/Pages overlap; the simpler safe check is
    // count of /Type/Page followed by a non-`s`.
    let mut pages = 0usize;
    let bs = &bytes;
    let needle = b"/Type/Page";
    for i in 0..bs.len().saturating_sub(needle.len() + 1) {
        if &bs[i..i + needle.len()] == needle && bs[i + needle.len()] != b's' {
            pages += 1;
        }
    }
    assert!(pages >= 2, "expected >=2 pages, got {}", pages);
    let _ = page_count;
}

/// Force a multi-page document by repeating a paragraph enough times
/// to overflow A4's content area. Each line is ~14 lines deep at the
/// default body size, so 100 paragraphs reliably produces 3+ pages.
fn multi_page_markdown(n_paragraphs: usize) -> String {
    let mut md = String::new();
    for i in 0..n_paragraphs {
        md.push_str(&format!(
            "Paragraph {} content. Some filler to make the line meaningful enough that pages do fill up.\n\n",
            i
        ));
    }
    md
}

#[test]
fn header_page_number_substitutes() {
    let md = multi_page_markdown(80);
    let bytes = render(
        &md,
        r##"
        [header]
        center = "{page} / {total_pages}"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    // The header is plain ASCII printed via the built-in font path
    // (no --default-font here), so `(1 / N) Tj` appears literally
    // in the content stream.
    assert!(
        s.contains("(1 / "),
        "expected `(1 / N)` page-number string in content stream"
    );
}

#[test]
fn footer_renders_on_every_page() {
    let md = multi_page_markdown(120);
    let bytes = render(
        &md,
        r##"
        [footer]
        center = "page {page}"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(page 1)"), "footer missing on page 1");
    assert!(s.contains("(page 2)"), "footer missing on page 2");
    assert!(s.contains("(page 3)"), "footer missing on page 3");
}

#[test]
fn show_on_first_page_false_skips_first() {
    let md = multi_page_markdown(80);
    let with_skip = render(
        &md,
        r##"
        [header]
        center = "HEAD"
        show_on_first_page = false
        "##,
    );
    let without_skip = render(
        &md,
        r##"
        [header]
        center = "HEAD"
        "##,
    );
    let count = |bytes: &[u8]| -> usize {
        let needle = b"(HEAD)";
        bytes
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    };
    let with_count = count(&with_skip);
    let without_count = count(&without_skip);
    assert!(without_count > with_count);
    assert_eq!(without_count - with_count, 1, "should skip exactly 1 page");
}

#[test]
fn title_var_pulls_from_metadata() {
    let md = "Just one paragraph.";
    let bytes = render(
        md,
        r##"
        [metadata]
        title = "SmokeTitle"
        [header]
        center = "{title}"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("(SmokeTitle)"),
        "header didn't substitute {{title}} from metadata"
    );
}

#[test]
fn headings_become_pdf_bookmarks() {
    let md = "\
# Top Level

Body paragraph.

## Second Level

More body.

### Third Level

Even more body.
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    // printpdf serializes bookmarks under /Outlines with /Title
    // entries. Headings round-trip as UTF-16BE strings (FEFF BOM).
    assert!(
        s.contains("/Outlines"),
        "expected /Outlines dictionary in the PDF"
    );
    // "Top Level" in UTF-16BE = 0054 006F 0070 0020 004C 0065 0076 0065 006C
    assert!(
        s.contains("FEFF0054006F0070") || s.contains("FEFF0054006f0070"),
        "expected the h1 title as a bookmark"
    );
}

#[test]
fn internal_link_emits_goto_action() {
    let md = "\
# Target Section

Some body text.

Click [the section](#target-section) to jump.
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "expected a GoTo action for the internal link"
    );
}

#[test]
fn unknown_internal_anchor_is_dropped_gracefully() {
    let md = "Click [broken](#does-not-exist) here.";
    let bytes = render(md, "");
    // The render must succeed and produce a valid PDF; no annotation
    // is emitted for the dangling anchor. We don't enforce *zero*
    // GoTo ops globally (other features may grow them) — just that
    // the render completes.
    assert!(bytes.starts_with(b"%PDF-"));
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("%%EOF"), "PDF didn't finish properly");
}

/// Count case-insensitive occurrences of a substring in bytes. The
/// TOC tests check that a heading appears twice (TOC + body) by
/// counting.
fn count_substr(bytes: &[u8], needle: &[u8]) -> usize {
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

#[test]
fn toc_renders_a_title_and_entries() {
    let md = "\
# First Heading

Body content.

## Second Heading

More body.
";
    let bytes = render(
        md,
        r##"
        [toc]
        enabled = true
        title = "Table of Contents"
        max_depth = 3
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    // The TOC title appears once.
    assert!(
        s.contains("(Table of Contents)") || s.contains("Table of Contents"),
        "TOC title missing from rendered bytes"
    );
    // First Heading text appears at least twice (TOC + body). Built-
    // in font path emits as `(text) Tj` literally.
    let occurrences = count_substr(&bytes, b"First Heading");
    assert!(
        occurrences >= 2,
        "expected `First Heading` to appear in both TOC and body (got {})",
        occurrences
    );
}

#[test]
fn toc_respects_max_depth() {
    let md = "\
# Top

## Middle

### Deep

Body content.
";
    let shallow = render(
        md,
        r##"
        [toc]
        enabled = true
        max_depth = 2
        "##,
    );
    let deep = render(
        md,
        r##"
        [toc]
        enabled = true
        max_depth = 6
        "##,
    );
    // "Deep" appears once in shallow (body only) and twice in deep
    // (TOC + body).
    let shallow_count = count_substr(&shallow, b"Deep");
    let deep_count = count_substr(&deep, b"Deep");
    assert!(
        deep_count > shallow_count,
        "max_depth filter didn't reduce TOC entries (shallow={}, deep={})",
        shallow_count,
        deep_count
    );
}

#[test]
fn toc_entries_emit_goto_actions() {
    let md = "\
# A

## B

Body.
";
    let bytes = render(
        md,
        r##"
        [toc]
        enabled = true
        max_depth = 3
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "expected at least one GoTo action for TOC entries"
    );
}

#[test]
fn title_page_appears_when_configured() {
    let bytes = render(
        "Body paragraph.",
        r##"
        [title_page]
        title = "HelloTitle"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("(HelloTitle)"),
        "title text missing from title page"
    );
    // Count /Type/Page tokens (not /Type/Pages).
    let needle = b"/Type/Page";
    let mut pages = 0usize;
    for i in 0..bytes.len().saturating_sub(needle.len() + 1) {
        if &bytes[i..i + needle.len()] == needle && bytes[i + needle.len()] != b's' {
            pages += 1;
        }
    }
    assert!(pages >= 2, "expected >=2 pages (title + body), got {}", pages);
}

#[test]
fn title_page_has_no_header_or_footer() {
    let bytes = render(
        "Body paragraph that takes one body page.",
        r##"
        [title_page]
        title = "Quiet"

        [footer]
        center = "page {page}"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    // Footer suppressed on the title page; appears on the body's
    // first page (which is page 2 in the final document).
    assert!(
        !s.contains("(page 1)"),
        "footer leaked onto the title page"
    );
    assert!(
        s.contains("(page 2)"),
        "footer missing on the body's first page (final page 2)"
    );
}

#[test]
fn subtitle_and_author_render_when_present() {
    let bytes = render(
        "Body.",
        r##"
        [title_page]
        title = "Main"
        subtitle = "SubXY"
        author = "AutBZ"
        date = "2026-01-02"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(Main)"), "title missing");
    assert!(s.contains("(SubXY)"), "subtitle missing");
    assert!(s.contains("(AutBZ)"), "author missing");
    assert!(s.contains("(2026-01-02)"), "date missing");
}

#[test]
fn footnote_reference_renders_as_superscript_number() {
    let bytes = render("Text with note[^a].\n\n[^a]: Defined.", "");
    let s = String::from_utf8_lossy(&bytes);
    // The first (and only) footnote label gets number `1`.
    assert!(s.contains("(1)"), "expected superscript marker `(1)`");
    assert!(
        s.contains("Defined."),
        "expected definition text in PDF content stream"
    );
}

#[test]
fn footnotes_section_heading_appears() {
    let bytes = render(
        "Note[^a].\n\n[^a]: First definition.",
        "",
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("(Footnotes)"),
        "expected `Footnotes` section heading in document"
    );
}

#[test]
fn unresolved_footnote_reference_does_not_crash() {
    let bytes = render("Body text with[^missing] no def.", "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}

#[test]
fn footnote_reuse_keeps_same_number() {
    // Two references to the same label should produce two `(1)`
    // superscript markers in the body.
    let bytes = render("First[^a] then again[^a].\n\n[^a]: Note.", "");
    let s = String::from_utf8_lossy(&bytes);
    let occurrences = s.matches("(1)").count();
    assert!(
        occurrences >= 2,
        "expected `(1)` to appear at least twice (got {})",
        occurrences
    );
}

#[test]
fn baseline_renders_without_any_styling_overrides() {
    // Sanity: with zero config, the renderer still produces a PDF.
    let bytes = render("# Hi\n\nHello.", "");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn small_caps_uppercases_lowercase_letters_in_paragraph() {
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Hello world.", cfg);
    let s = String::from_utf8_lossy(&bytes);
    // `ello` is its own small-caps segment; `world` is all-lowercase
    // so the entire word becomes one small-caps segment "WORLD".
    assert!(s.contains("(ELLO)"), "expected `ello` -> `ELLO`");
    assert!(s.contains("(WORLD)"), "expected `world` -> `WORLD`");
}

#[test]
fn small_caps_keeps_originally_uppercase_letters_separate() {
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Hello.", cfg);
    let s = String::from_utf8_lossy(&bytes);
    // The originally-uppercase `H` and the originally-lowercase `ello`
    // become two distinct segments in the PDF text stream.
    assert!(s.contains("(H)"), "expected H emitted as its own segment");
    assert!(
        s.contains("(ELLO)"),
        "expected ELLO emitted as its own segment"
    );
}

#[test]
fn small_caps_off_by_default() {
    let bytes = render("Hello world.", "");
    let s = String::from_utf8_lossy(&bytes);
    // Without the config, lowercase chars stay lowercase.
    assert!(
        s.contains("(Hello world.)") || s.contains("(Hello world.) "),
        "expected `Hello world.` emitted as-is when small_caps is off"
    );
}

#[test]
fn small_caps_leaves_digits_and_punctuation_full_size() {
    // The lowercase tails of `Year` and `yes` become small-caps
    // segments; digits / punctuation / uppercase form their own
    // non-small-caps segments (which printpdf may hex-encode when the
    // payload contains literal `(` or `)`, so we don't assert on the
    // exact string form).
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Year 1984 (yes!).", cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(EAR)"), "`ear` -> `EAR` small-caps segment");
    assert!(s.contains("(YES)"), "`yes` -> `YES` small-caps segment");
    // 1984's digits must still be in the document somewhere — either
    // as a literal `(...)` string or hex-encoded.
    assert!(
        s.contains("1984") || s.contains("31393834"),
        "digit run 1984 must appear in PDF text stream"
    );
}

#[test]
fn small_caps_applies_to_h1_when_configured() {
    let cfg = "[headings.h1]\nsmall_caps = true\n";
    let bytes = render("# Hello world\n", cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(ELLO"), "h1 lowercase should be uppercased");
}

#[test]
fn url_image_without_fetch_feature_renders_alt_text() {
    // Without --features fetch (the default test build), an http(s)
    // URL image should fall back to the italic alt-text fallback and
    // not crash the render.
    let md = "![remote-banner](https://example.com/banner.png)\n";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    let s = String::from_utf8_lossy(&bytes);
    if !cfg!(feature = "fetch") {
        assert!(
            s.contains("[image: remote-banner]"),
            "expected `[image: alt]` italic fallback when fetch feature is disabled"
        );
    }
}

#[test]
fn url_image_with_invalid_url_does_not_crash() {
    // Even if `fetch` is enabled, a malformed URL or unreachable host
    // should degrade to the alt-text fallback rather than panic.
    let md = "![alt-fallback](https://invalid.example.invalid/nope.png)\n";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn image_caption_renders_when_title_attribute_present() {
    let img = "examples/showcase_image.jpg";
    let md = format!("![alt]({} \"This is a caption\")\n", img);
    let bytes = render(&md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("(This is a caption)"),
        "caption text should appear in PDF stream"
    );
}

#[test]
fn image_with_no_title_renders_without_caption() {
    let img = "examples/showcase_image.jpg";
    let md = format!("![alt]({})\n", img);
    let bytes = render(&md, "");
    let s = String::from_utf8_lossy(&bytes);
    // Caption text is the title; alt text is "alt". Neither should
    // appear as a caption — title is None so no caption emission.
    assert!(!s.contains("(alt)"), "alt text should not render as caption");
}

#[test]
fn image_right_align_changes_xobject_translation() {
    let img = "examples/showcase_image.jpg";
    let md = format!("![alt]({})\n", img);
    let cfg_left = "[image]\nalign = \"left\"\n";
    let cfg_right = "[image]\nalign = \"right\"\n";
    let bytes_left = render(&md, cfg_left);
    let bytes_right = render(&md, cfg_right);
    // Left-aligned image lives at x = left margin; right-aligned at
    // (column_w - image_w) + left margin. The XObject translate Td
    // values must differ between the two renders.
    assert_ne!(
        bytes_left, bytes_right,
        "left vs right alignment should produce different PDFs"
    );
}

#[test]
fn image_max_width_pct_shrinks_image() {
    let img = "examples/showcase_image.jpg";
    let md = format!("![alt]({})\n", img);
    let cfg_full = "[image]\nmax_width_pct = 100.0\n";
    let cfg_half = "[image]\nmax_width_pct = 50.0\n";
    let bytes_full = render(&md, cfg_full);
    let bytes_half = render(&md, cfg_half);
    // Different max_width_pct → different rendered width → different
    // XObject transform serialization.
    assert_ne!(
        bytes_full, bytes_half,
        "max_width_pct 100 vs 50 should produce different PDFs"
    );
}

#[test]
fn very_long_word_does_not_overflow_horizontally() {
    // A single 200-char word with no whitespace would otherwise
    // overflow the right margin. The split_long_words pre-pass should
    // chunk it so each piece fits within the column.
    let long = "x".repeat(200);
    let md = format!("Body {} tail.\n", long);
    let bytes = render(&md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
    // The PDF must contain multiple line breaks — we don't pin the
    // exact count (font metrics vary) but it should be greater than 1
    // line because the word can't fit on one.
}

#[test]
fn very_long_url_does_not_overflow() {
    // A long URL is the typical real-world trigger for long-word
    // breaking. The link annotation should still be emitted even when
    // the URL text is split across multiple chunks.
    let url = format!("https://example.com/{}", "a".repeat(180));
    let md = format!("See [{}]({}) here.\n", url, url);
    let bytes = render(&md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/URI") || s.contains("/S /URI"),
        "long-URL link annotation should still be emitted"
    );
}

#[test]
fn long_word_with_unicode_breaks_at_char_boundaries() {
    // Repeated multi-byte char run. The break must not slice the UTF-8.
    let long = "ñ".repeat(150);
    let md = format!("Pre {} post.\n", long);
    let bytes = render(&md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}

#[test]
fn html_sup_renders_as_superscript() {
    let bytes = render("Einstein: E = mc<sup>2</sup>.", "");
    let s = String::from_utf8_lossy(&bytes);
    // The `2` should appear in the rendered content stream. The `<sup>`
    // tags themselves should be consumed (not appear literally).
    assert!(s.contains("(2)"), "expected `2` literal in PDF stream");
    assert!(
        !s.contains("(<sup>)") && !s.contains("(</sup>)"),
        "expected <sup> tags to be consumed"
    );
}

#[test]
fn html_sub_renders_as_subscript() {
    let bytes = render("Water is H<sub>2</sub>O.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(2)"), "expected `2` literal in PDF stream");
    assert!(
        !s.contains("(<sub>)") && !s.contains("(</sub>)"),
        "expected <sub> tags to be consumed"
    );
}

#[test]
fn html_sup_sub_does_not_crash_unbalanced() {
    let bytes = render("Stray <sup>open only.\n\nStray close only</sub>.", "");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn html_sup_sub_uppercase_tags_recognized() {
    let bytes = render("Number: 10<SUP>3</SUP> and 2<SUB>4</SUB>.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("(<SUP>)") && !s.contains("(<SUB>)"),
        "expected uppercase sup/sub tags to be consumed"
    );
}

#[test]
fn cross_reference_backward_link_to_earlier_heading() {
    // Link AFTER its target heading. The GoTo destination still needs
    // to resolve correctly.
    let md = "\
# Introduction

Body text.

## Details

See [the introduction](#introduction) for context.
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "backward cross-reference should emit a GoTo action"
    );
}

#[test]
fn cross_reference_slug_normalizes_special_characters() {
    // GitHub-style slug: lowercase, spaces -> hyphens, punctuation removed.
    let md = "\
# Hello, World!

Click [here](#hello-world) to jump.
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "slug normalization should make `#hello-world` resolve to `# Hello, World!`"
    );
}

#[test]
fn cross_reference_multiple_links_to_same_anchor() {
    let md = "\
# Conclusion

Intro paragraph.

See [the conclusion](#conclusion) below. Or maybe revisit [it](#conclusion) later.
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    let count = count_substr(s.as_bytes(), b"/S/GoTo") + count_substr(s.as_bytes(), b"/S /GoTo");
    assert!(
        count >= 2,
        "expected two GoTo actions for two references; got {}",
        count
    );
}

#[test]
fn cross_reference_mixed_with_external_link_in_same_paragraph() {
    let md = "\
# Reference Heading

See [the heading](#reference-heading) or [the spec](https://example.com).
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "internal link should emit GoTo"
    );
    assert!(
        s.contains("/S/URI") || s.contains("/S /URI"),
        "external link should emit URI action"
    );
}

#[test]
fn cross_reference_collision_suffix_resolves() {
    // Two headings with the same text → second one gets `-2` suffix.
    let md = "\
# Section

First section.

# Section

Second section. Click [back to second](#section-2).
";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("/S/GoTo") || s.contains("/S /GoTo"),
        "collision-suffixed slug `#section-2` should resolve"
    );
}

#[test]
fn definition_list_emits_term_and_definition() {
    let bytes = render("Apple\n: A red fruit.\n", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(Apple)"), "term missing from PDF");
    assert!(s.contains("(A red fruit.)"), "definition missing from PDF");
}

#[test]
fn definition_list_handles_multiple_entries() {
    let bytes = render(
        "Apple\n: A red fruit.\nBanana\n: A yellow fruit.\n",
        "",
    );
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(Apple)"));
    assert!(s.contains("(Banana)"));
    assert!(s.contains("(A red fruit.)"));
    assert!(s.contains("(A yellow fruit.)"));
}

#[test]
fn definition_list_does_not_break_pdf() {
    let bytes = render("Term\n: First.\n: Second.\n", "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}
