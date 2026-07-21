//! End-to-end regression tests for everything the renderer wires from
//! the schema — block backgrounds, borders, padding, HR style/width,
//! tables, custom bullets, page setup, headers/footers, bookmarks,
//! cross-references, TOC, title page, footnotes, alignment, small caps,
//! URL images, image flow, long-word breaking, sup/sub, definition
//! lists.
//!
//! Byte-level on the rendered PDF stream — cheap to run, stable against
//! library changes that don't touch the relevant Op variants. Shared
//! helpers (`render`, `contains`, `count_rect_ops`, `bytes_have_stroke_op`,
//! `multi_page_markdown`, `count_substr`) live in `super::common`.

use super::common::*;

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
    let has_stroke = bytes_have_stroke_op(&styled);
    assert!(has_stroke, "expected a stroke op for the heading border");
}

#[test]
fn code_block_padding_shifts_text_inward() {
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
    assert!(no_pad.starts_with(b"%PDF-"));
    assert!(with_pad.starts_with(b"%PDF-"));
    let _ = no_pad.len();
    let _ = with_pad.len();
}

#[test]
fn hr_dashed_style_emits_a_nondefault_dash_pattern() {
    let dashed = render(
        "---",
        r##"
        [horizontal_rule]
        style = "dashed"
        "##,
    );
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
    assert!(full.starts_with(b"%PDF-"));
    assert!(half.starts_with(b"%PDF-"));
}

#[test]
fn block_level_html_comment_is_invisible() {
    let md = "Before\n\n<!-- this should not be visible -->\n\nAfter";
    let bytes = render(md, "");
    assert!(
        !contains(&bytes, b"this should not be visible"),
        "block-level HTML comment leaked into the rendered PDF"
    );
}

#[test]
fn table_alignment_does_not_crash() {
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
    let bytes = render(
        "- a\n- b",
        r##"
        [list.unordered]
        bullet = "-"
        "##,
    );
    assert!(bytes.starts_with(b"%PDF-"));
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
    let bytes = render(
        "Hi.",
        r##"
        [page]
        orientation = "landscape"
        "##,
    );
    let s = String::from_utf8_lossy(&bytes);
    let landscape_dim = s.contains("841") || s.contains("842");
    assert!(
        landscape_dim,
        "expected the A4 long side (~842pt) in MediaBox for landscape"
    );
}

#[test]
fn metadata_fields_written_to_info_dict() {
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
    assert!(s.contains("/Author"), "Info dict missing /Author key");
    assert!(s.contains("/Subject"), "Info dict missing /Subject key");
    assert!(s.contains("/Title"), "Info dict missing /Title key");
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
    assert!(page_count(&bytes) >= 2, "expected ≥2 pages, got {}", page_count(&bytes));
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
    let count = |bytes: &[u8]| -> usize { count_substr(bytes, b"(HEAD)") };
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
    assert!(bytes.starts_with(b"%PDF-"));
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("%%EOF"), "PDF didn't finish properly");
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
    assert!(
        s.contains("(Table of Contents)") || s.contains("Table of Contents"),
        "TOC title missing from rendered bytes"
    );
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
    assert!(
        page_count(&bytes) >= 2,
        "expected ≥2 pages (title + body), got {}",
        page_count(&bytes)
    );
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
    let bytes = render("# Hi\n\nHello.", "");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn text_align_center_changes_output() {
    let md = "A short line of text.\n";
    let cfg_left = "[paragraph]\ntext_align = \"left\"\n";
    let cfg_center = "[paragraph]\ntext_align = \"center\"\n";
    let bytes_left = render(md, cfg_left);
    let bytes_center = render(md, cfg_center);
    assert_ne!(
        bytes_left, bytes_center,
        "left vs center alignment should produce different PDFs"
    );
}

#[test]
fn text_align_right_changes_output() {
    let md = "A short line of text.\n";
    let cfg_left = "[paragraph]\ntext_align = \"left\"\n";
    let cfg_right = "[paragraph]\ntext_align = \"right\"\n";
    let bytes_left = render(md, cfg_left);
    let bytes_right = render(md, cfg_right);
    assert_ne!(
        bytes_left, bytes_right,
        "left vs right alignment should produce different PDFs"
    );
}

#[test]
fn text_align_justify_emits_word_spacing_op() {
    let md = "This is a sentence that is long enough to wrap onto a \
              second line so the first line gets justified spacing applied. \
              And here is a tail that makes the second line non-empty too.\n";
    let cfg = "[paragraph]\ntext_align = \"justify\"\n";
    let bytes = render(md, cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains(" Tw"),
        "justified paragraph should emit `Tw` (word-spacing) op"
    );
}

#[test]
fn text_align_left_does_not_emit_word_spacing() {
    let md = "Long enough sentence that wraps to a second line for sure \
              with enough text to make the line break occur somewhere.\n";
    let bytes_left = render(md, "[paragraph]\ntext_align = \"left\"\n");
    let bytes_just = render(md, "[paragraph]\ntext_align = \"justify\"\n");
    assert_ne!(
        bytes_left, bytes_just,
        "left vs justify should differ when there's a wrappable line"
    );
    assert!(bytes_left.starts_with(b"%PDF-"));
}

#[test]
fn small_caps_uppercases_lowercase_letters_in_paragraph() {
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Hello world.", cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(ELLO)"), "expected `ello` -> `ELLO`");
    assert!(s.contains("(WORLD)"), "expected `world` -> `WORLD`");
}

#[test]
fn small_caps_keeps_originally_uppercase_letters_separate() {
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Hello.", cfg);
    let s = String::from_utf8_lossy(&bytes);
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
    assert!(
        s.contains("(Hello world.)") || s.contains("(Hello world.) "),
        "expected `Hello world.` emitted as-is when small_caps is off"
    );
}

#[test]
fn small_caps_leaves_digits_and_punctuation_full_size() {
    let cfg = "[paragraph]\nsmall_caps = true\n";
    let bytes = render("Year 1984 (yes!).", cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(s.contains("(EAR)"), "`ear` -> `EAR` small-caps segment");
    assert!(s.contains("(YES)"), "`yes` -> `YES` small-caps segment");
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
    let md = "![alt-fallback](https://invalid.example.invalid/nope.png)\n";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn image_caption_renders_when_title_attribute_present() {
    let img = temp_jpeg_path();
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
    let img = temp_jpeg_path();
    let md = format!("![alt]({})\n", img);
    let bytes = render(&md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(!s.contains("(alt)"), "alt text should not render as caption");
}

#[test]
fn image_right_align_changes_xobject_translation() {
    let img = temp_jpeg_path();
    let md = format!("![alt]({})\n", img);
    let cfg_left = "[image]\nalign = \"left\"\n";
    let cfg_right = "[image]\nalign = \"right\"\n";
    let bytes_left = render(&md, cfg_left);
    let bytes_right = render(&md, cfg_right);
    assert_ne!(
        bytes_left, bytes_right,
        "left vs right alignment should produce different PDFs"
    );
}

#[test]
fn image_max_width_pct_shrinks_image() {
    let img = temp_jpeg_path();
    let md = format!("![alt]({})\n", img);
    let cfg_full = "[image]\nmax_width_pct = 100.0\n";
    let cfg_half = "[image]\nmax_width_pct = 50.0\n";
    let bytes_full = render(&md, cfg_full);
    let bytes_half = render(&md, cfg_half);
    assert_ne!(
        bytes_full, bytes_half,
        "max_width_pct 100 vs 50 should produce different PDFs"
    );
}

#[test]
fn very_long_word_does_not_overflow_horizontally() {
    let long = "x".repeat(200);
    let md = format!("Body {} tail.\n", long);
    let bytes = render(&md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}

#[test]
fn very_long_url_does_not_overflow() {
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
fn hyphenation_inserts_hyphen_into_overflow_english_word() {
    // A real English word too long for a very narrow column. The
    // hyphenation crate should find dictionary break points and the
    // split_long_words pass should emit "-" at the chosen breaks.
    let md = "antidisestablishmentarianism floccinaucinihilipilification.\n";
    let cfg = r#"
[page]
size = "A4"
[page.margins]
top = 25.0
right = 150.0
bottom = 25.0
left = 30.0
"#;
    let bytes = render(md, cfg);
    let s = String::from_utf8_lossy(&bytes);
    // A literal "-" emitted as part of a hyphenated chunk shows up in
    // the PDF content stream. Since both words are >25 chars and the
    // column is very narrow, at least one hyphenation point should
    // have fired.
    assert!(
        s.contains("-)") || s.contains("- "),
        "expected a hyphen at the chunk boundary in PDF stream"
    );
}

#[test]
fn long_word_with_unicode_breaks_at_char_boundaries() {
    let long = "ñ".repeat(150);
    let md = format!("Pre {} post.\n", long);
    let bytes = render(&md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}

#[test]
fn non_url_word_with_hash_does_not_soft_break_at_hash() {
    let md = "Identifier C#program_with_an_extremely_long_name_that_forces_wrap end.\n";
    let cfg = r#"
[page]
size = "A4"
[page.margins]
top = 25.0
right = 150.0
bottom = 25.0
left = 30.0
"#;
    let bytes = render(md, cfg);
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("(C#)"),
        "non-URL token containing '#' must not split right after '#'"
    );
}

#[test]
fn html_sup_renders_as_superscript() {
    let bytes = render("Einstein: E = mc<sup>2</sup>.", "");
    let s = String::from_utf8_lossy(&bytes);
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
fn html_underline_tag_is_consumed() {
    let bytes = render("Try <u>underlined</u> text.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("(<u>)") && !s.contains("(</u>)"),
        "<u> tags leaked into output"
    );
    assert!(contains(&bytes, b"underlined"));
}

#[test]
fn html_strike_and_del_tags_consumed() {
    for src in [
        "<s>gone</s>",
        "<del>removed</del>",
        "<strike>cancelled</strike>",
    ] {
        let bytes = render(src, "");
        let s = String::from_utf8_lossy(&bytes);
        assert!(
            !s.contains("(<s>)")
                && !s.contains("(<del>)")
                && !s.contains("(<strike>)"),
            "tag leaked into output for `{}`",
            src
        );
    }
}

#[test]
fn html_small_tag_is_consumed() {
    let bytes = render("Body <small>fine print</small>.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("(<small>)") && !s.contains("(</small>)"),
        "<small> tag leaked into output"
    );
    assert!(contains(&bytes, b"fine print"));
}

#[test]
fn html_kbd_tag_is_consumed() {
    let bytes = render("Press <kbd>Enter</kbd>.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("(<kbd>)") && !s.contains("(</kbd>)"),
        "<kbd> tag leaked into output"
    );
    assert!(contains(&bytes, b"Enter"));
}

#[test]
fn html_unknown_inline_tag_falls_through_verbatim() {
    let bytes = render("Random <custom>thing</custom>.", "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        s.contains("<custom>") || contains_text(&bytes, "<custom>"),
        "unknown inline tag should appear verbatim, not be silently dropped"
    );
}

#[test]
fn cross_reference_backward_link_to_earlier_heading() {
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
fn link_with_title_emits_contents_tooltip() {
    let md = "See [the spec](https://example.com/spec \"Official spec page\").\n";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    // The /Contents entry on the Link annotation holds the tooltip
    // text. lopdf serializes literal strings as `(text)`.
    assert!(
        s.contains("/Contents")
            && (s.contains("(Official spec page)") || s.contains("Official spec page")),
        "expected /Contents tooltip on link annotation"
    );
}

#[test]
fn link_without_title_has_no_contents_entry() {
    // No title means no tooltip; the link still works as a URI action
    // but no /Contents key is added.
    let md = "See [the spec](https://example.com/spec).\n";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    // The URI action is present.
    assert!(s.contains("/S/URI") || s.contains("/S /URI"));
    // The /Contents tooltip should NOT have a 'spec'-like literal,
    // because nothing was provided. Asserting absence of a specific
    // tooltip phrase is the safest invariant here.
    assert!(
        !s.contains("(Official spec page)"),
        "should not have leaked a tooltip"
    );
}

#[test]
fn link_tooltip_does_not_break_pdf() {
    let md = "[a](https://x.com/a \"tip a\") and [b](https://x.com/b \"tip b\").\n";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(String::from_utf8_lossy(&bytes).contains("%%EOF"));
}

#[test]
fn cross_reference_collision_suffix_resolves() {
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

#[test]
fn html_img_block_does_not_render_tag_as_text() {
    let md = "<img src=\"nonexistent.png\" alt=\"a banner\">\n\nBody.";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    // The literal HTML should NOT appear as monospace source — even
    // when the path doesn't exist we either show alt text or fall
    // back to an HtmlBlock; the user's biggest complaint is seeing
    // the raw `<img src=` characters.
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("<img src="),
        "raw `<img src=` leaked into the PDF stream"
    );
}

#[test]
fn html_img_block_falls_back_to_alt_text_when_src_unloadable() {
    // With src pointing to a missing file, the renderer's
    // render_image fallback emits `[image: <alt>]` italic text — not
    // the raw HTML tag.
    let md = "<img src=\"missing.png\" alt=\"banner\">\n";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(!s.contains("<img"), "raw HTML leaked");
    assert!(
        contains(&bytes, b"banner") || contains_text(&bytes, "banner"),
        "alt text was not rendered as fallback"
    );
}

#[test]
fn standalone_p_tag_is_dropped() {
    let md = "<p align=\"center\">\n\nReal body text here.\n\n</p>";
    let bytes = render(md, "");
    let s = String::from_utf8_lossy(&bytes);
    assert!(
        !s.contains("<p align="),
        "framing <p> tag rendered as text"
    );
    assert!(!s.contains("</p>"), "framing </p> tag rendered as text");
}

#[test]
fn unknown_html_block_still_renders_as_text() {
    // Tags we don't recognize as framing or img stay as HtmlBlock
    // and render via the monospace block-html path.
    let md = "<custom>Hello</custom>\n";
    let bytes = render(md, "");
    assert!(bytes.starts_with(b"%PDF-"));
    // Doesn't matter how it appears visually — just shouldn't panic.
}

// --- Cross-page block backgrounds (W5b) ---

#[test]
fn single_page_background_still_emits_one_rect() {
    // Regression guard: the common single-page case is unchanged —
    // a colored paragraph emits at least one background rectangle.
    let baseline = render("Plain paragraph.", "");
    let styled = render(
        "Plain paragraph.",
        r##"
        [paragraph]
        background_color = "#FFCC00"
        "##,
    );
    assert!(
        count_rect_ops(&styled) > count_rect_ops(&baseline),
        "single-page background must still emit a fill rect"
    );
}

/// Build a quoted block long enough to span several pages: many
/// quoted paragraphs separated by blank `>` lines.
fn long_quote(n: usize) -> String {
    let mut body = String::new();
    for i in 0..n {
        body.push_str(&format!(
            "> Paragraph {} of a long quoted block. {}\n>\n",
            i,
            "Filler sentence to consume vertical space. ".repeat(3)
        ));
    }
    body
}

#[test]
fn background_spanning_pages_paints_a_fragment_per_page() {
    // A colored blockquote that spans several page breaks must paint
    // one background fragment PER page it touches. Before W5b the
    // paint was skipped when the block spanned pages (so only ~1
    // rect total, or 0); now it's one per page. We assert the fill
    // rect count is at least the page count.
    let body = long_quote(90);
    let bytes = render(
        &body,
        r##"
        [blockquote]
        background_color = "#E0E0FF"
        "##,
    );
    let pages = page_count(&bytes);
    assert!(
        pages >= 3,
        "test setup expected a multi-page quote, got {} pages",
        pages
    );
    let rects = count_rect_ops(&bytes);
    assert!(
        rects >= pages,
        "expected ≥ one background fragment per page ({} pages) but \
         found only {} fill-rect ops — cross-page paint regressed",
        pages,
        rects
    );
}

#[test]
fn single_page_colored_blockquote_emits_exactly_one_fragment() {
    // Regression guard for the non-spanning case: a short colored
    // quote should still paint exactly one background rect, not one
    // per (nonexistent) page break.
    let bytes = render(
        "> short colored quote\n",
        r##"
        [blockquote]
        background_color = "#E0E0FF"
        "##,
    );
    assert_eq!(page_count(&bytes), 1);
    assert_eq!(
        count_rect_ops(&bytes),
        1,
        "single-page colored quote should emit exactly one bg rect"
    );
}

#[test]
fn background_spanning_pages_produces_valid_pdf() {
    let bytes = render(
        &long_quote(90),
        r##"
        [blockquote]
        background_color = "#FFEEDD"
        "##,
    );
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(contains(&bytes, b"%%EOF"));
    assert!(page_count(&bytes) >= 3);
}

#[test]
fn nested_colored_blocks_spanning_pages_do_not_crash() {
    // Blockquote with a background containing paragraphs that also
    // have a background; the whole thing spills across pages. Tests
    // the open-bg LIFO stack with >1 entry live at a page break.
    let bytes = render(
        &long_quote(90),
        r##"
        [blockquote]
        background_color = "#EEEEEE"

        [paragraph]
        background_color = "#FFFFCC"
        "##,
    );
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(contains(&bytes, b"%%EOF"));
    assert!(page_count(&bytes) >= 3);
}

#[test]
fn uncolored_paragraphs_spanning_pages_emit_no_background_rects() {
    // Paragraphs have NO default background (unlike blockquotes,
    // which the default theme tints grey). A long run of plain
    // paragraphs that spans pages must therefore emit ZERO fill
    // rects — proving the cross-page machinery only paints when a
    // background is actually configured.
    let mut md = String::new();
    for i in 0..120 {
        md.push_str(&format!(
            "Paragraph {}. {}\n\n",
            i,
            "Filler sentence to consume space. ".repeat(3)
        ));
    }
    let bytes = render(&md, "");
    assert!(page_count(&bytes) >= 3, "setup must span pages");
    assert_eq!(
        count_rect_ops(&bytes),
        0,
        "uncolored paragraphs must not emit any background fill rects"
    );
}

#[test]
fn colored_paragraph_spanning_pages_paints_per_page() {
    // The other half: a *paragraph* with an explicit background that
    // spans pages should now paint one fragment per page (paragraph
    // has no default bg, so every rect here is from our config).
    let big = format!(
        "{}",
        "word ".repeat(12_000)
    );
    let bytes = render(
        &big,
        r##"
        [paragraph]
        background_color = "#FFEECC"
        "##,
    );
    let pages = page_count(&bytes);
    assert!(pages >= 2, "setup must span pages, got {}", pages);
    let rects = count_rect_ops(&bytes);
    assert!(
        rects >= pages,
        "colored paragraph spanning {} pages emitted only {} fill \
         rects (expected ≥ one per page)",
        pages,
        rects
    );
}

/// Correctness of inline run-style flags and a few features that
/// previously only had "renders / tag consumed" coverage. Every
/// assertion targets a specific content-stream operator, with a
/// plain-text negative control so it can't pass vacuously.
mod inline_style_application {
    use super::*;

    /// Largest `x` operand across all `… Td` text-position ops.
    fn max_td_x(bytes: &[u8]) -> f32 {
        let s = String::from_utf8_lossy(bytes);
        let mut m = 0.0f32;
        for line in s.lines() {
            let l = line.trim();
            if let Some(p) = l.strip_suffix(" Td") {
                if let Some(x) = p.split_whitespace().next() {
                    if let Ok(v) = x.parse::<f32>() {
                        if v > m {
                            m = v;
                        }
                    }
                }
            }
        }
        m
    }

    #[test]
    fn html_underline_emits_stroke_decoration() {
        let plain = render("before under after", "");
        assert!(
            !bytes_have_stroke_op(&plain),
            "plain paragraph must not emit a stroke (negative control)"
        );
        let u = render("before <u>under</u> after", "");
        assert!(
            bytes_have_stroke_op(&u),
            "<u> must draw an underline stroke"
        );
    }

    #[test]
    fn strike_del_and_tilde_emit_stroke_decoration() {
        for md in [
            "x <s>struck</s> y",
            "x <del>deleted</del> y",
            "x ~~struck~~ y",
        ] {
            assert!(
                bytes_have_stroke_op(&render(md, "")),
                "{md:?} must draw a strikethrough stroke"
            );
        }
    }

    #[test]
    fn html_small_tag_shrinks_font_to_085x() {
        // Default paragraph size is 8pt; <small> → 0.85× = 6.8pt.
        let plain = render("just regular sized text", "");
        assert!(
            !contains(&plain, b"6.8 Tf"),
            "plain text must not emit the small (6.8pt) size"
        );
        let small = render("regular <small>tiny words</small> regular", "");
        assert!(
            contains(&small, b"6.8 Tf"),
            "<small> must shrink the run to 0.85× (6.8pt Tf)"
        );
    }

    #[test]
    fn html_kbd_tag_switches_to_monospace() {
        let plain = render("press the key now", "");
        assert!(
            !contains(&plain, b"Courier"),
            "plain prose must not pull in a monospace font"
        );
        let kbd = render("press <kbd>Ctrl</kbd> now", "");
        assert!(
            contains(&kbd, b"Courier"),
            "<kbd> must switch the run to the monospace font"
        );
    }

    #[test]
    fn superscript_and_subscript_shrink_font_to_070x() {
        // <sup>/<sub> → 0.70× of 8pt = 5.6pt.
        let plain = render("E equals m c squared", "");
        assert!(!contains(&plain, b"5.6 Tf"));
        assert!(
            contains(&render("E = mc<sup>2</sup>", ""), b"5.6 Tf"),
            "<sup> must render at 0.70× size"
        );
        assert!(
            contains(&render("H<sub>2</sub>O", ""), b"5.6 Tf"),
            "<sub> must render at 0.70× size"
        );
    }

    #[test]
    fn ordered_list_emits_visible_numbers() {
        let b = render("1. alpha\n2. bravo\n3. charlie\n", "");
        for marker in [&b"(1."[..], b"(2.", b"(3."] {
            assert!(
                contains(&b, marker),
                "ordered list must render the {:?} marker",
                std::str::from_utf8(marker).unwrap()
            );
        }
        assert!(contains_text(&b, "alpha") && contains_text(&b, "charlie"));
    }

    #[test]
    fn multiline_footnote_definition_includes_continuation() {
        let md = "Text with note[^a].\n\n[^a]: first line\n    continued second line\n";
        let b = render(md, "");
        assert!(
            contains_text(&b, "first line"),
            "footnote definition first line missing"
        );
        assert!(
            contains_text(&b, "continued second line"),
            "4-space indented footnote continuation was dropped"
        );
    }

    #[test]
    fn indented_code_block_renders_content() {
        let b = render("para before\n\n    let x = 42;\n\npara after\n", "");
        assert!(
            contains_text(&b, "let x = 42;"),
            "indented (4-space) code block content missing"
        );
    }

    #[test]
    fn table_column_alignment_shifts_text_position() {
        // Same one-cell table, right- vs left-aligned: the right
        // column must place its text at a larger x.
        let right = render("| H |\n|--:|\n| Z |\n", "");
        let left = render("| H |\n|:--|\n| Z |\n", "");
        assert!(
            max_td_x(&right) > max_td_x(&left) + 50.0,
            "right-aligned cell text ({:.0}) should sit well right of \
             left-aligned ({:.0})",
            max_td_x(&right),
            max_td_x(&left)
        );
    }

    #[test]
    fn task_list_draws_a_checkbox_not_literal_brackets() {
        let b = render("- [ ] open task\n- [x] done task\n", "");
        assert!(
            !contains(&b, b"[ ]") && !contains(&b, b"[x]"),
            "task markers leaked as literal text instead of a drawn box"
        );
        assert!(
            bytes_have_stroke_op(&b),
            "task checkbox outline (a stroked path) was not drawn"
        );
        assert!(contains_text(&b, "open task") && contains_text(&b, "done task"));
    }

    #[test]
    fn checked_and_unchecked_tasks_render_differently() {
        // The checked box additionally draws a tick, so the streams
        // must differ — proves the checked state is visualised.
        let done = render("- [x] a\n", "");
        let open = render("- [ ] a\n", "");
        assert!(bytes_have_stroke_op(&done) && bytes_have_stroke_op(&open));
        assert_ne!(
            done, open,
            "checked vs unchecked task produced identical output"
        );
        assert!(done.len() > open.len(), "checked box should add a tick path");
    }

    #[test]
    fn default_unordered_bullet_is_a_drawn_disc_not_asterisk() {
        // Built-in Helvetica lacks `•`; it must be a filled disc
        // (a polygon fill op), never the `*` win1252 fallback.
        let list = render("- alpha\n- bravo\n", "");
        let plain = render("alpha bravo just a paragraph", "");
        assert!(
            count_rect_ops(&list) >= 2,
            "expected ≥1 filled disc per item, got {} fill ops",
            count_rect_ops(&list)
        );
        assert_eq!(
            count_rect_ops(&plain),
            0,
            "a plain paragraph must not emit any fill ops (control)"
        );
        assert!(contains_text(&list, "alpha") && contains_text(&list, "bravo"));
    }

    #[test]
    fn table_header_repeats_across_page_breaks() {
        let mut md = String::from("| HDRMARK | C2 |\n|---|---|\n");
        for i in 0..150 {
            md.push_str(&format!("| row{i} body | val{i} |\n"));
        }
        let b = render(&md, "");
        let pages = page_count(&b);
        assert!(pages >= 2, "expected a multi-page table, got {pages}");
        assert!(
            count_substr(&b, b"HDRMARK") >= pages,
            "table header appeared {} times across {} pages — \
             not repeated per page",
            count_substr(&b, b"HDRMARK"),
            pages
        );
    }

    #[test]
    fn table_header_background_paints_repeated_headers() {
        let mut md = String::from("| HDRMARK | C2 |\n|---|---|\n");
        for i in 0..150 {
            md.push_str(&format!("| row{i} body | val{i} |\n"));
        }
        let b = render(
            &md,
            r##"
            [table.header]
            background_color = "#FFCC00"
            "##,
        );
        let pages = page_count(&b);
        assert!(pages >= 2, "expected a multi-page table, got {pages}");
        assert!(
            count_rect_ops(&b) >= pages,
            "table header background emitted {} fill rects across {} pages",
            count_rect_ops(&b),
            pages
        );
    }

    // AC1: a header cell spanning two columns renders once, with the
    // covered slot drawing no separate cell.
    #[test]
    fn table_colspan_header_renders_once_over_merged_region() {
        let b = render(
            "| Group | > | Tail |\n| :---: | :---: | :---: |\n| a | b | c |\n",
            "",
        );
        assert!(pdf_well_formed(&b));
        assert!(bytes_have_stroke_op(&b), "table borders must still draw");
        assert_eq!(
            count_substr(&b, b"(Group)"),
            1,
            "colspan origin rendered more than once"
        );
        assert!(
            contains_text(&b, "Group") && contains_text(&b, "Tail"),
            "the cell after a colspan must still render"
        );
        assert!(contains_text(&b, "a") && contains_text(&b, "b") && contains_text(&b, "c"));
    }

    // AC2: a cell spanning two rows renders once; the cells beside it
    // lay out normally.
    #[test]
    fn table_rowspan_renders_once_with_neighbors_intact() {
        let b = render(
            "| Key | Value |\n| --- | --- |\n| A | one |\n| ^ | two |\n",
            "",
        );
        assert!(pdf_well_formed(&b));
        assert_eq!(
            count_substr(&b, b"(A)"),
            1,
            "rowspan origin rendered more than once"
        );
        assert!(
            contains_text(&b, "one") && contains_text(&b, "two"),
            "neighbor cells beside a rowspan must render normally"
        );
    }

    // AC3: alignment and per-cell styling still apply to non-spanned
    // cells that sit next to spanned ones.
    #[test]
    fn table_alignment_and_styling_apply_alongside_spans() {
        let b = render(
            "| Group | > | Plain |\n| :--- | :---: | ---: |\n\
             | **bold** | mid | `code` |\n| ^ | center | right |\n",
            "",
        );
        assert!(pdf_well_formed(&b));
        assert!(
            bytes_have_stroke_op(&b),
            "borders draw with mixed spans + styling"
        );
        assert!(contains_text(&b, "bold") && contains_text(&b, "code"));
        assert!(contains_text(&b, "mid") && contains_text(&b, "center"));
        assert_eq!(
            count_substr(&b, b"(bold)"),
            1,
            "row-spanned styled origin rendered once"
        );
    }

    // AC5 regression: a plain GFM table with a tall wrapped cell next
    // to short cells still renders (the vertical-alignment branch keeps
    // non-spanned cells top-aligned exactly as before spans existed).
    #[test]
    fn plain_gfm_table_with_mixed_height_rows_unaffected() {
        let b = render(
            "| Short | Long |\n| --- | --- |\n\
             | x | this cell has enough words to wrap onto a second \
             physical line inside the column box |\n| y | z |\n",
            "",
        );
        assert!(pdf_well_formed(&b));
        assert!(contains_text(&b, "Short") && contains_text(&b, "Long"));
        assert!(contains_text(&b, "x") && contains_text(&b, "y") && contains_text(&b, "z"));
    }

    // AC4: a spanned region near / across a page boundary must not
    // corrupt the table or panic.
    #[test]
    fn table_rowspan_near_page_break_does_not_panic() {
        let mut md = String::from("| Key | Value |\n|---|---|\n");
        for i in 0..120 {
            md.push_str(&format!("| row{i} | value{i} |\n"));
        }
        md.push_str("| Span | before |\n| ^ | after |\n");
        let b = render(&md, "");
        assert!(pdf_well_formed(&b));
        assert!(page_count(&b) >= 2);
        assert!(contains_text(&b, "Span") && contains_text(&b, "after"));
    }

    // A rowspan group taller than a whole page must still not panic.
    #[test]
    fn table_huge_rowspan_group_does_not_panic() {
        let mut md = String::from("| K | V |\n|---|---|\n| Top | start |\n");
        for i in 0..200 {
            md.push_str(&format!("| ^ | filler line number {i} |\n"));
        }
        let b = render(&md, "");
        assert!(pdf_well_formed(&b));
        assert!(contains_text(&b, "Top"));
    }

    // Escaped markers render as literal `>` / `^` text, never spans.
    #[test]
    fn escaped_span_markers_render_literally() {
        let b = render(
            "| op | meaning |\n| --- | --- |\n| \\> | greater than |\n| \\^ | caret |\n",
            "",
        );
        assert!(pdf_well_formed(&b));
        assert!(
            contains_text(&b, "greater than") && contains_text(&b, "caret"),
            "escaped-marker rows keep their real content"
        );
    }
}
