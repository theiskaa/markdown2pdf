//! End-to-end visual regression suite (Theme F).
//!
//! Each fixture is a complete markdown document combining several
//! renderer features — title page, TOC, headings, lists, tables,
//! images, footnotes, alignment modes, etc. Assertions are
//! deliberately *stable* (PDF magic + content presence + page count)
//! rather than byte-level so that font-metric drift, printpdf
//! revisions, or harmless layout adjustments don't cause false
//! failures. The point is to catch *cross-feature* regressions
//! (e.g. a layout change that breaks footnote anchors).
//!
//! Per-feature byte-level assertions live in `styling.rs` and
//! `fonts.rs` — those keep tighter invariants.

use super::common::*;

fn pdf_ok(bytes: &[u8]) {
    assert!(pdf_well_formed(bytes), "PDF malformed (magic/EOF)");
}

/// Full academic-style document.
/// Combines: title page, auto-generated TOC, multiple heading levels,
/// footnotes, justified body text, definition list, blockquote, table.
/// Catches regressions in cross-cutting machinery — heading anchor
/// resolution against shifted page indices, footnote numbering in
/// numbered-section context, TOC convergence loop.
#[test]
fn academic_document() {
    let md = r##"
# Introduction

Background paragraph. The lexer parses footnote references[^a] and the
renderer collects them at end of document. Cross-reference: see
[the conclusion](#conclusion) for the wrap-up.

## Definitions

Definition list:

Compiler
: Translates source code into a different representation.

Renderer
: A specialized compiler whose output is a visual format.

## Methodology

A small comparison table:

| Approach | Time | Notes |
| --- | --- | --- |
| Brute force | O(n²) | Easy to read |
| Knuth-Plass | O(n) | Optimal lines |

> Block quote: this is a single-paragraph blockquote with no nested
> structure, used here to verify the left rule still draws correctly
> in the presence of other features.

# Conclusion

Wrap-up text with another footnote[^b].

[^a]: The lexer parses footnote references and definitions.
[^b]: Definitions are collected at end of document.
"##;
    let cfg = r#"
[title_page]
title = "Academic Document"
subtitle = "A markdown2pdf regression fixture"
author = "Test Author"
date = "2026-05-14"

[toc]
enabled = true
title = "Contents"
max_depth = 3

[paragraph]
text_align = "justify"
"#;
    let bytes = render(md, cfg);
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, "Academic Document"));
    assert!(contains_text(&bytes, "Test Author"));
    assert!(contains_text(&bytes, "Contents"));
    assert!(contains_text(&bytes, "Introduction"));
    assert!(contains_text(&bytes, "Conclusion"));
    assert!(contains_text(&bytes, "Definitions"));
    assert!(contains_text(&bytes, "Methodology"));
    assert!(contains_text(&bytes, "Footnotes"));
    assert!(
        contains_text(&bytes, "/S/GoTo") || contains_text(&bytes, "/S /GoTo"),
        "cross-ref + TOC should emit GoTo actions"
    );
    assert!(
        contains_text(&bytes, " Tw"),
        "justified body should emit Tw"
    );
    assert!(
        page_count(&bytes) >= 3,
        "expected ≥3 pages, got {}",
        page_count(&bytes)
    );
}

/// Typography combo — small caps + super/sub + image caption. Catches
/// regressions in inline shape transformations.
#[test]
fn typography_combo() {
    let img = temp_jpeg_path();
    let md = format!(
        "\
# Typography Showcase

Einstein wrote E = mc<sup>2</sup>. Water is H<sub>2</sub>O. The body
of this paragraph also uses small caps to verify that the inline
super/sub transforms still work alongside the per-character case-class
split.

![hero]({} \"A caption beneath the image\")

Closing paragraph.
",
        img
    );
    let cfg = "[paragraph]\nsmall_caps = true\n[image]\nalign = \"center\"\nmax_width_pct = 60.0\n";
    let bytes = render(&md, cfg);
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, "(INSTEIN"));
    assert!(contains_text(&bytes, "(2)"));
    assert!(contains_text(&bytes, "(A caption beneath the image)"));
}

/// Deeply nested lists stressing block-padding + indent.
#[test]
fn nested_lists_three_deep() {
    let md = "\
# Lists

- Top level item one
  - Second level under one
    - Third level under one-A
    - Third level under one-B
  - Second level under one B
- Top level item two
  1. Ordered sublist
  2. Second ordered
     - Mixed unordered child
- Top level item three
";
    let bytes = render(md, "");
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, "Top level item one"));
    assert!(contains_text(&bytes, "Second level under one"));
    assert!(contains_text(&bytes, "Third level under one-A"));
    assert!(contains_text(&bytes, "Mixed unordered child"));
}

/// All four alignment modes interacting (paragraph justify +
/// blockquote center).
#[test]
fn alignment_combo() {
    let md = "\
# Alignment

Left-aligned paragraph. Lorem ipsum dolor sit amet, consectetur
adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore
magna aliqua.

Same content again, but the next blockquote should respect its own
alignment setting.

> Block-quoted body text demonstrates that nested alignment honors
> the block's own style when set.
";
    let cfg = "[paragraph]\ntext_align = \"justify\"\n[blockquote]\ntext_align = \"center\"\n";
    let bytes = render(md, cfg);
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, " Tw"));
}

/// Code blocks + inline code mixed with regular text + lists.
#[test]
fn code_and_text_mix() {
    let md = "\
# Code Mix

Inline code like `fn main()` should render in mono. Block:

```rust
fn main() {
    println!(\"hi\");
}
```

And inside a list:

- `let x = 1;` in monospace
- Regular item

Final paragraph.
";
    let bytes = render(md, "");
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, "Inline code like"));
    assert!(contains_text(&bytes, "should render in mono"));
    // The block code's `fn main()` is hex-encoded because of the `()`.
    // Hex codepoints `66 6E 20 6D 61 69 6E` = `fn main`.
    assert!(
        contains_text(&bytes, "666E206D61696E"),
        "expected hex-encoded `fn main` somewhere in stream"
    );
    assert!(contains_text(&bytes, "(let x = 1;)"));
}

/// Page-break + page-numbered footer combo. Two-pass header/footer
/// pipeline regression catch.
#[test]
fn page_break_with_footer() {
    let md = "\
# Page 1

Some content on page one.

<!-- pagebreak -->

# Page 2

Some content on page two.
";
    let cfg = r#"
[footer]
center = "Page {page} of {total_pages}"
"#;
    let bytes = render(md, cfg);
    pdf_ok(&bytes);
    assert!(
        page_count(&bytes) >= 2,
        "expected ≥2 pages, got {}",
        page_count(&bytes)
    );
    assert!(
        contains_text(&bytes, "Page 1 of 2") || contains_text(&bytes, "Page 2 of 2"),
        "footer should substitute {{page}}/{{total_pages}}"
    );
}

/// Long word + URL inside a narrow blockquote column. Catches
/// regressions in split_long_words against per-context indent.
#[test]
fn long_word_in_narrow_blockquote() {
    let long = "a".repeat(200);
    let md = format!(
        "# Narrow column\n\n> {}\n\n> Followed by [a long URL link](https://example.com/{}) text.\n",
        long, long
    );
    let bytes = render(&md, "");
    pdf_ok(&bytes);
    assert!(page_count(&bytes) >= 1);
}

#[test]
fn empty_document() {
    let bytes = render("", "");
    pdf_ok(&bytes);
    assert_eq!(
        page_count(&bytes),
        1,
        "empty markdown still produces 1 page"
    );
}

#[test]
fn whitespace_only_document() {
    let bytes = render("   \n\n\n   \n", "");
    pdf_ok(&bytes);
}

#[test]
fn only_headings_no_body() {
    let bytes = render("# A\n\n## B\n\n### C\n", "");
    pdf_ok(&bytes);
    assert!(contains_text(&bytes, "(A)"));
    assert!(contains_text(&bytes, "(B)"));
    assert!(contains_text(&bytes, "(C)"));
}
