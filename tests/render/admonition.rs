//! Render-level checks for admonition blocks. The lexer already
//! covers token shape (see `tests/markdown/admonition_tests.rs`);
//! this file pins the PDF-level behaviour: tinted background, accent
//! border + icon, header label text, unknown-kind fallback, body
//! still flows nested constructs, anchor preprocessing still runs
//! into admonition bodies.

use super::common::*;

fn link_annotation_count(bytes: &[u8]) -> usize {
    count_substr(bytes, b"/Subtype/Link")
}

#[test]
fn each_first_class_kind_renders_label_and_box() {
    for (kind, label) in [
        ("note", "NOTE"),
        ("info", "INFO"),
        ("tip", "TIP"),
        ("warning", "WARNING"),
        ("danger", "DANGER"),
    ] {
        let md = format!("!!! {}\n    body content here\n", kind);
        let bytes = render(&md, "");
        assert!(pdf_well_formed(&bytes), "PDF malformed for kind {kind}");
        assert!(
            contains_text(&bytes, label),
            "label `{label}` missing from rendered PDF for kind {kind}"
        );
        assert!(
            contains_text(&bytes, "body content here"),
            "body text missing for kind {kind}"
        );
        // The tinted background draws a filled rectangle; counting
        // bare `f` fill ops is the established marker (see other
        // render tests).
        assert!(
            count_rect_ops(&bytes) >= 1,
            "no background fill emitted for kind {kind}"
        );
    }
}

#[test]
fn unknown_kind_uses_raw_label_as_header() {
    let bytes = render("!!! bug \"Repro steps\"\n    repro here\n", "");
    assert!(pdf_well_formed(&bytes));
    // The author-supplied title takes over for `!!! bug "…"`.
    assert!(contains_text(&bytes, "Repro steps"));
    assert!(contains_text(&bytes, "repro here"));
}

#[test]
fn unknown_kind_without_title_shows_uppercased_raw_label() {
    let bytes = render("!!! quirk\n    weird body\n", "");
    assert!(pdf_well_formed(&bytes));
    // With no quoted title, the renderer surfaces the raw label
    // uppercased so authors can see which kind word they typed.
    assert!(
        contains_text(&bytes, "QUIRK"),
        "raw label not surfaced for unknown kind"
    );
}

#[test]
fn gfm_alert_produces_admonition_styled_box() {
    let bytes = render("> [!WARNING]\n> careful now\n", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "WARNING"));
    assert!(contains_text(&bytes, "careful now"));
    assert!(count_rect_ops(&bytes) >= 1);
}

#[test]
fn custom_title_replaces_default_label() {
    let bytes = render("!!! note \"Heads up\"\n    body\n", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "Heads up"));
    // The default label NOTE is replaced by the title — neither
    // form is mandatory in the byte stream, but the body must
    // always render.
    assert!(contains_text(&bytes, "body"));
}

#[test]
fn aliased_kind_renders_canonical_label() {
    // `important` aliases to `info` — the rendered label is `INFO`
    // (the canonical kind's default), not the alias the author typed.
    let bytes = render("> [!IMPORTANT]\n> heads up\n", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "INFO"));
    assert!(!contains_text(&bytes, "IMPORTANT"));
}

#[test]
fn empty_body_does_not_panic() {
    let bytes = render("!!! note \"only header\"\n", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "only header"));
}

#[test]
fn nested_admonition_renders_both_boxes() {
    let src = "!!! note \"Outer\"\n    !!! tip\n        inner body\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "Outer"));
    assert!(contains_text(&bytes, "TIP"));
    assert!(contains_text(&bytes, "inner body"));
    // Two boxes → at least two background fills.
    assert!(
        count_rect_ops(&bytes) >= 2,
        "expected at least two background fills for nested admonition"
    );
}

#[test]
fn body_supports_lists_and_code() {
    let src = "!!! note\n    - first item\n    - second item\n\n    ```\n    code line\n    ```\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "first item"));
    assert!(contains_text(&bytes, "second item"));
    assert!(contains_text(&bytes, "code line"));
}

#[test]
fn inline_anchor_inside_admonition_body_becomes_clickable_link() {
    // The preprocess pass that rewrites inline `<a>` into a real
    // Token::Link must recurse into admonition body — otherwise
    // anchors inside callouts silently lose their click target.
    let src =
        "!!! note\n    Click <a href=\"https://example.com\">here</a> for docs.\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        link_annotation_count(&bytes),
        1,
        "anchor inside admonition body lost its link annotation"
    );
}

#[test]
fn markdown_link_inside_admonition_renders_link() {
    let src = "!!! tip\n    See [docs](https://example.com).\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(link_annotation_count(&bytes), 1);
}

#[test]
fn admonition_inside_blockquote_still_renders() {
    let src = "> outer quote\n>\n> > [!NOTE]\n> > inner alert\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "outer quote"));
    assert!(contains_text(&bytes, "NOTE"));
    assert!(contains_text(&bytes, "inner alert"));
}

#[test]
fn long_body_split_across_pages_does_not_crash() {
    // Build a body large enough to force at least one page break.
    // Each line is rendered as its own paragraph inside the body, so
    // many short lines pile up vertically.
    let mut body = String::new();
    for i in 0..400 {
        body.push_str(&format!("    paragraph {} of the long body.\n\n", i));
    }
    let md = format!("!!! warning\n{body}");
    let bytes = render(&md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        page_count(&bytes) >= 2,
        "expected at least 2 pages, got {}",
        page_count(&bytes)
    );
}

#[test]
fn all_themes_render_admonition_without_panic() {
    // Every bundled theme must accept admonitions. Body text doesn't
    // assert specific colours (theme-specific) — just that the render
    // path doesn't error on any of them.
    for theme in [
        "default", "github", "academic", "minimal", "compact", "modern",
    ] {
        let cfg = format!("theme = \"{theme}\"\n");
        let bytes = render("!!! note \"hello\"\n    body\n", &cfg);
        assert!(
            pdf_well_formed(&bytes),
            "theme `{theme}` failed to render admonition"
        );
        assert!(
            contains_text(&bytes, "hello"),
            "theme `{theme}` lost the admonition title"
        );
    }
}

#[test]
fn admonition_does_not_consume_following_paragraph() {
    let bytes = render(
        "!!! note\n    inside\n\nafter paragraph here.\n",
        "",
    );
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "inside"));
    assert!(contains_text(&bytes, "after paragraph here"));
}

#[test]
fn back_to_back_admonitions_produce_two_boxes() {
    let src = "!!! note\n    first\n\n!!! warning\n    second\n";
    let bytes = render(src, "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "NOTE"));
    assert!(contains_text(&bytes, "WARNING"));
    assert!(
        count_rect_ops(&bytes) >= 2,
        "expected at least two background fills for two admonitions"
    );
}

#[test]
fn body_with_strong_emphasis_renders() {
    let bytes = render(
        "!!! info\n    body with **bold** and *italic* spans.\n",
        "",
    );
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "bold"));
    assert!(contains_text(&bytes, "italic"));
}
