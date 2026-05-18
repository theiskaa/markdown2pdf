//! LaTeX math end-to-end. Inline `$…$` renders as an italic
//! monospace run (the `$` delimiters are stripped); display `$$…$$`
//! renders as its own centered block. `\$` is a literal dollar.
//! Unterminated `$` degrades to literal text without panicking.

use super::common::*;

#[test]
fn inline_math_strips_delimiters_and_keeps_content() {
    let bytes = render("Mass-energy: $E = mc^2$ holds.", "");
    assert!(pdf_well_formed(&bytes));
    // The formula body reaches the content stream...
    assert!(contains_text(&bytes, "mc^2"), "math content missing");
    // ...but the `$` delimiters do not survive as literal text.
    assert!(
        !contains_text(&bytes, "$E"),
        "the opening $ delimiter leaked into the output"
    );
}

#[test]
fn escaped_dollar_renders_as_a_literal_amount() {
    let bytes = render(r"Coffee costs \$5.00 each.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        contains_text(&bytes, "$5.00"),
        "escaped \\$ must render as a literal dollar amount"
    );
}

#[test]
fn unterminated_dollar_does_not_panic_and_keeps_text() {
    let bytes = render("The budget is $5 with no closing delimiter.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        contains_text(&bytes, "$5"),
        "an unterminated $ should stay literal text"
    );
}

#[test]
fn price_pair_is_not_treated_as_math() {
    // "$5 and $6" is the classic Pandoc false-positive guard.
    let bytes = render("Items cost $5 and $6 respectively.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "$5"));
    assert!(contains_text(&bytes, "$6"));
}

#[test]
fn display_math_renders_as_its_own_block() {
    let bytes = render(
        "Before the equation.\n\n$$E = mc^2$$\n\nAfter the equation.",
        "",
    );
    assert!(pdf_well_formed(&bytes));
    // The display span flushes the paragraph before it and starts a
    // fresh one after — both surrounding paragraphs survive intact.
    assert!(contains_text(&bytes, "Before the equation."));
    assert!(contains_text(&bytes, "After the equation."));
    // The formula body is rendered.
    assert!(contains_text(&bytes, "mc^2"), "display math body missing");
    // The `$$` delimiters do not survive as literal text.
    assert!(!contains_text(&bytes, "$$"), "display delimiters leaked");
}

#[test]
fn display_math_with_tex_backslashes_still_renders() {
    // `\int_0^1 x\,dx` reaches the content stream via an embedded
    // monospace face (hex-encoded glyphs), so we assert robustness
    // and block separation rather than the literal body text.
    let bytes = render(
        "Lead.\n\n$$\\int_0^1 x\\,dx$$\n\nTail.",
        "",
    );
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "Lead."));
    assert!(contains_text(&bytes, "Tail."));
}

#[test]
fn display_math_is_centered_one_absolute_move_per_line() {
    // A left-aligned multi-line paragraph emits a single absolute
    // `Td` then relative line motion. Centered display math emits one
    // absolute `Td` per source line, so a 3-line equation yields
    // strictly more `Td` ops than an equivalent plain paragraph.
    let plain = render(
        "alpha line one here\nbeta line two here\ngamma line three",
        "",
    );
    let mathy = render("$$\na = 1\nb = 2\nc = 3\n$$", "");
    assert!(pdf_well_formed(&mathy));
    let td_plain = count_substr(&plain, b" Td");
    let td_math = count_substr(&mathy, b" Td");
    assert!(
        td_math >= 3 && td_math > td_plain,
        "centered display math must place each line absolutely \
         (plain={td_plain}, math={td_math})"
    );
}

#[test]
fn inline_math_inside_emphasis_and_heading_renders() {
    let bytes = render("# The $E=mc^2$ result\n\nText with *the $a+b$ term* inside.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "mc^2"));
    assert!(contains_text(&bytes, "a+b"));
}

#[test]
fn math_inside_lists_and_blockquotes_renders() {
    let bytes = render("- first $x_1$\n- second $x_2$\n\n> quoted $y^2$", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "x_1"));
    assert!(contains_text(&bytes, "x_2"));
    assert!(contains_text(&bytes, "y^2"));
}

#[test]
fn empty_display_math_is_dropped_without_panic() {
    let bytes = render("Lead in.\n\n$$$$\n\nLead out.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "Lead in."));
    assert!(contains_text(&bytes, "Lead out."));
}

#[test]
fn multipage_document_with_math_is_well_formed() {
    let mut md = String::new();
    for i in 0..60 {
        md.push_str(&format!(
            "Paragraph {i} discusses $a_{{{i}}} + b^2$ and then:\n\n$$\\sum_{{k=0}}^{{{i}}} k$$\n\n"
        ));
    }
    let bytes = render(&md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(page_count(&bytes) > 1, "test should span multiple pages");
}

#[test]
fn adversarial_dollar_documents_never_panic() {
    for src in [
        "$",
        "$$",
        "$$$",
        "$$$$$",
        "lone $ in prose",
        "$x",
        "trailing dollar x$",
        "$$\nunclosed display\n\nnext para",
        r"escaped \$\$\$ run",
        "mix $a$ and \\$ and $b$ and $7",
        "$$ $$",
    ] {
        let bytes = render(src, "");
        assert!(pdf_well_formed(&bytes), "{src:?} produced a malformed PDF");
    }
}
