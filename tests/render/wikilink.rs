//! WikiLinks end-to-end through the renderer. A `[[Target]]` resolves
//! against heading anchors via the same deferred internal-link path as
//! `[text](#slug)`: a matching heading yields a `GoTo` action; a miss
//! degrades to styled text (the renderer logs a warning and drops only
//! the annotation, never the text) so a partial export still renders.

use super::common::*;

fn goto_count(bytes: &[u8]) -> usize {
    count_substr(bytes, b"GoTo")
}

#[test]
fn resolved_wikilink_emits_goto_action() {
    let md = "\
# Introduction

Body text.

Jump to the [[Introduction]].
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 1, "matching heading should yield one GoTo");
}

#[test]
fn labeled_wikilink_resolves_to_target() {
    let md = "\
# Introduction

Body.

See [[introduction|see the intro]] for context.
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 1);
}

#[test]
fn missing_target_degrades_without_panic() {
    let bytes = render("A link to [[Missing Page]] that has no heading.", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 0, "unresolved wikilink must not emit a GoTo");
}

#[test]
fn unclosed_wikilink_renders_as_text() {
    let bytes = render("This [[Unclosed never closes so it is literal.", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 0);
}

#[test]
fn escaped_wikilink_is_not_a_link() {
    let bytes = render(r"Escaped \[\[Not a wikilink\]\] stays literal.", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 0);
}

#[test]
fn forward_reference_resolves_to_later_heading() {
    let md = "\
Read the [[Conclusion]] first.

# Conclusion

The end.
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(goto_count(&bytes), 1, "a link before its heading must still resolve");
}

#[test]
fn two_links_to_same_target_emit_two_gotos() {
    let md = "\
# Target

Go [[Target]] once and [[Target]] twice.
";
    let bytes = render(md, "");
    assert_eq!(goto_count(&bytes), 2);
}

#[test]
fn explicit_crossref_to_duplicate_heading_suffix_resolves() {
    let md = "\
# Dup

First.

# Dup

Second.

Jump to [the second one](#dup-2).
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        goto_count(&bytes),
        1,
        "link to the -2 suffix slug of a duplicate heading must resolve"
    );
}

#[test]
fn wikilink_and_explicit_crossref_coexist() {
    let md = "\
# Section One

Body.

A wikilink [[Section One]] and an explicit ref [back](#section-one).
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        goto_count(&bytes),
        2,
        "sharing slugify must not break explicit #slug cross-references"
    );
}
