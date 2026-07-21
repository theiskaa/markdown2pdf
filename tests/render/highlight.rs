//! `==highlight==` end-to-end. The renderer paints a filled rectangle
//! (the `[mark]` background) behind the run's glyphs. Filled rects
//! serialize as a standalone `f` op, so `count_rect_ops` rises by one
//! per highlighted segment versus the same text unmarked.

use super::common::*;

#[test]
fn highlight_paints_a_background_rect() {
    let plain = render("Some important text.", "");
    let marked = render("Some ==important== text.", "");
    assert!(pdf_well_formed(&marked));
    assert!(
        count_rect_ops(&marked) > count_rect_ops(&plain),
        "a highlighted run must add a filled rectangle (plain={}, marked={})",
        count_rect_ops(&plain),
        count_rect_ops(&marked),
    );
}

#[test]
fn unterminated_highlight_adds_no_rect() {
    let plain = render("Some important text.", "");
    let unterminated = render("Some ==important text.", "");
    assert!(pdf_well_formed(&unterminated));
    assert_eq!(
        count_rect_ops(&unterminated),
        count_rect_ops(&plain),
        "an unterminated == must not paint a highlight"
    );
}

#[test]
fn setext_heading_with_equals_underline_still_renders() {
    let bytes = render("Intro paragraph.\n===\n\nBody text here.", "");
    assert!(pdf_well_formed(&bytes));
}

#[test]
fn nested_bold_highlight_renders() {
    let bytes = render("A ==**bold mark**== here.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(count_rect_ops(&bytes) >= 1);
}

#[test]
fn highlight_works_in_lists_and_blockquotes() {
    let bytes = render("- item ==one==\n\n> quote ==two==", "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        count_rect_ops(&bytes) >= 2,
        "expected a rect for each of the two highlighted runs"
    );
}

#[test]
fn custom_mark_background_color_is_honored() {
    let cfg = r##"
[mark]
background_color = "#00FF00"
"##;
    let bytes = render("Green ==marker== here.", cfg);
    assert!(pdf_well_formed(&bytes));
    assert!(count_rect_ops(&bytes) >= 1);
    // printpdf emits fill colour as an `rg` op with normalized
    // components; pure green is `0 1 0 rg`.
    assert!(
        contains_text(&bytes, "0 1 0 rg") || contains_text(&bytes, "0.0 1.0 0.0 rg"),
        "custom [mark] background colour should reach the content stream"
    );
}

#[test]
fn multipage_document_with_highlights_is_well_formed() {
    let mut md = String::new();
    for i in 0..60 {
        md.push_str(&format!(
            "Paragraph {i} with a ==marked span== inside it.\n\n"
        ));
    }
    let bytes = render(&md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(page_count(&bytes) > 1, "test should span multiple pages");
}
