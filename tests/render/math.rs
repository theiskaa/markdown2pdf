//! LaTeX math end-to-end. Both inline `$…$` and display `$$…$$` are
//! typeset by the in-tree TeX engine and drawn as filled glyph
//! *outlines* (no embedded font, nothing selectable). `\$` is a
//! literal dollar; unterminated `$` degrades to literal text.

use super::common::*;

#[test]
fn inline_math_strips_delimiters_and_typesets() {
    let bytes = render("Mass-energy: $E = mc^2$ holds.", "");
    let plain = render("Mass-energy:  holds.", "");
    assert!(pdf_well_formed(&bytes));
    // Surrounding prose stays selectable text...
    assert!(contains_text(&bytes, "Mass-energy:"));
    assert!(contains_text(&bytes, "holds."));
    // ...the math is drawn as vector outlines (extra fill ops)...
    assert!(
        count_rect_ops(&bytes) > count_rect_ops(&plain),
        "inline math must emit filled glyph outlines"
    );
    // ...and the `$` delimiters never reach the output as text.
    assert!(!contains_text(&bytes, "$E"), "opening $ leaked");
    assert!(!contains_text(&bytes, "mc^2"), "math must not be text");
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
    let plain = render("Before the equation.\n\nAfter the equation.", "");
    assert!(pdf_well_formed(&bytes));
    // The display span flushes the paragraph before it and starts a
    // fresh one after — both surrounding paragraphs survive intact.
    assert!(contains_text(&bytes, "Before the equation."));
    assert!(contains_text(&bytes, "After the equation."));
    // Math is drawn as filled glyph outlines, so the equation adds
    // vector fill ops the plain document doesn't have...
    assert!(
        count_rect_ops(&bytes) > count_rect_ops(&plain),
        "display math must emit filled glyph outlines"
    );
    // ...and nothing about it is selectable text: no embedded math
    // font, and the `$$` delimiters never reach the stream.
    assert!(!contains_text(&bytes, "$$"), "display delimiters leaked");
    assert!(
        !bytes.windows(8).any(|w| w == b"FontFile"),
        "math must not embed a font (outlines only)"
    );
}

#[test]
fn display_math_with_tex_backslashes_still_renders() {
    let bytes = render("Lead.\n\n$$\\int_0^1 x\\,dx$$\n\nTail.", "");
    let plain = render("Lead.\n\nTail.", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "Lead."));
    assert!(contains_text(&bytes, "Tail."));
    assert!(
        count_rect_ops(&bytes) > count_rect_ops(&plain),
        "\\int_0^1 x\\,dx must typeset to filled outlines"
    );
}

#[test]
fn display_math_is_not_selectable_text() {
    // The whole point of outline rendering: math contributes vector
    // fills, never glyph text. A doc whose only content is display
    // math must have far more fill ops than text-show ops.
    let bytes = render("$$\\frac{a+b}{c-d} = \\sqrt{x^2+y^2}$$", "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        count_rect_ops(&bytes) > 5,
        "a fraction + radical should emit many filled outlines"
    );
    assert!(
        !bytes.windows(8).any(|w| w == b"FontFile"),
        "no font should be embedded for math"
    );
}

#[test]
fn inline_math_inside_emphasis_and_heading_renders() {
    let bytes = render(
        "# The $E=mc^2$ result\n\nText with *the $a+b$ term* inside.",
        "",
    );
    let plain = render("# The  result\n\nText with *the  term* inside.", "");
    assert!(pdf_well_formed(&bytes));
    // Surrounding heading / emphasis text is unaffected; the two
    // inline formulae add vector fills.
    assert!(contains_text(&bytes, "result"));
    assert!(
        count_rect_ops(&bytes) > count_rect_ops(&plain),
        "inline math in heading/emphasis must typeset to outlines"
    );
}

#[test]
fn math_inside_lists_and_blockquotes_renders() {
    let bytes = render("- first $x_1$\n- second $x_2$\n\n> quoted $y^2$", "");
    let plain = render("- first \n- second \n\n> quoted ", "");
    assert!(pdf_well_formed(&bytes));
    assert!(contains_text(&bytes, "first"));
    assert!(contains_text(&bytes, "quoted"));
    assert!(
        count_rect_ops(&bytes) > count_rect_ops(&plain),
        "math in lists/blockquotes must typeset to outlines"
    );
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

#[test]
fn math_config_color_reaches_the_stream() {
    // `[math] color` is emitted as the polygon fill colour. printpdf
    // normalizes components, so pure green is `0 1 0 rg`.
    let cfg = "[math]\ncolor = \"#00FF00\"\n";
    let bytes = render("$$x^2 + y^2$$", cfg);
    assert!(pdf_well_formed(&bytes));
    assert!(
        contains_text(&bytes, "0 1 0 rg") || contains_text(&bytes, "0.0 1.0 0.0 rg"),
        "custom [math] color must reach the content stream"
    );
}

#[test]
fn math_config_align_shifts_the_block() {
    // Left-aligned display math starts at the left margin; centered
    // starts further right. The first glyph polygon's leftmost X
    // therefore differs. We approximate by comparing the byte offset
    // of the first fill op's coordinates is fragile, so instead just
    // assert both render well-formed and differently sized streams.
    let left = render("$$X = 1$$", "[math]\nalign = \"left\"\n");
    let center = render("$$X = 1$$", "[math]\nalign = \"center\"\n");
    let right = render("$$X = 1$$", "[math]\nalign = \"right\"\n");
    for b in [&left, &center, &right] {
        assert!(pdf_well_formed(b));
        assert!(count_rect_ops(b) > 0, "equation must still render");
    }
    // The three alignments place the same glyphs at different x — the
    // content streams must not be byte-identical.
    assert!(left != center && center != right && left != right);
}

#[test]
fn math_config_scale_changes_size() {
    let small = render("$$\\frac{a}{b}$$", "[math]\nscale = 0.8\n");
    let big = render("$$\\frac{a}{b}$$", "[math]\nscale = 2.0\n");
    assert!(pdf_well_formed(&small) && pdf_well_formed(&big));
    // Glyph outlines are stored once, size-independent, as Form
    // XObjects — only the per-use `cm` scale differs — so the streams
    // must differ but byte length is *not* a size proxy any more.
    assert_ne!(small, big, "scale must change the rendered geometry");
    assert!(
        count_rect_ops(&small) > 0 && count_rect_ops(&big) > 0,
        "the fraction must still render at both scales"
    );
    // The larger scale must put a bigger uniform-scale `cm` into the
    // stream than the smaller one (scale·body vs 0.8·body).
    let max_scale = |b: &[u8]| -> i64 {
        let s = String::from_utf8_lossy(b);
        s.lines()
            .filter(|l| l.trim_end().ends_with(" cm"))
            .filter_map(|l| l.split_whitespace().next())
            .filter_map(|t| t.parse::<f64>().ok())
            .map(|v| v as i64)
            .max()
            .unwrap_or(0)
    };
    assert!(
        max_scale(&big) > max_scale(&small),
        "scale=2.0 must emit a larger transform than scale=0.8 ({} vs {})",
        max_scale(&big),
        max_scale(&small)
    );
}
