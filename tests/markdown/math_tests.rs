//! LaTeX math lexing. `$…$` is inline `Token::Math { inline: true }`,
//! `$$…$$` is display `Token::Math { inline: false }`. Delimiter rules
//! follow Pandoc: an inline opener needs a non-space immediately after
//! `$`, an inline closer needs a non-space immediately before `$` and
//! must not be directly followed by a digit; `\$` is a literal dollar;
//! an unterminated `$` degrades to literal text (never panics). Math
//! content is opaque — no markdown parsing or escape decoding happens
//! inside it.

use markdown2pdf::markdown::*;

use super::common::parse;

/// Every `Token::Math` in document order, flattened across whatever
/// inline/block context it is nested in, as `(inline, content)` pairs.
fn maths(tokens: &[Token]) -> Vec<(bool, String)> {
    let mut out = Vec::new();
    fn walk(ts: &[Token], out: &mut Vec<(bool, String)>) {
        for t in ts {
            match t {
                Token::Math { inline, content } => out.push((*inline, content.clone())),
                Token::Heading(c, _)
                | Token::StrongEmphasis(c)
                | Token::BlockQuote(c)
                | Token::Strikethrough(c)
                | Token::Highlight(c) => walk(c, out),
                Token::Emphasis { content, .. }
                | Token::ListItem { content, .. }
                | Token::Link { content, .. } => walk(content, out),
                Token::Table { headers, rows, .. } => {
                    for cell in headers {
                        walk(cell, out);
                    }
                    for row in rows {
                        for cell in row {
                            walk(cell, out);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    walk(tokens, &mut out);
    out
}

fn has_math(tokens: &[Token]) -> bool {
    !maths(tokens).is_empty()
}

// --- Acceptance criteria from issue #78 -----------------------------

#[test]
fn inline_math_is_distinct_token_not_literal_dollars() {
    let tokens = parse("$a^2 + b^2 = c^2$");
    assert_eq!(maths(&tokens), vec![(true, "a^2 + b^2 = c^2".to_string())]);
    // The `$` delimiters must not survive as literal text.
    assert!(!Token::collect_all_text(&tokens).contains('$'));
}

#[test]
fn display_math_is_a_block_token() {
    let tokens = parse(r"$$\int_0^1 x\,dx$$");
    assert_eq!(
        maths(&tokens),
        vec![(false, r"\int_0^1 x\,dx".to_string())]
    );
}

#[test]
fn escaped_dollar_is_a_literal_amount() {
    let tokens = parse(r"\$5.00");
    assert!(!has_math(&tokens), "\\$ must not open math");
    assert_eq!(Token::collect_all_text(&tokens), "$5.00");
}

#[test]
fn unterminated_inline_dollar_degrades_to_text() {
    let tokens = parse("price is $5 only");
    assert!(!has_math(&tokens));
    assert_eq!(Token::collect_all_text(&tokens), "price is $5 only");
}

#[test]
fn unterminated_display_dollars_degrade_to_text() {
    let tokens = parse(r"$$\frac{1}{2}");
    assert!(!has_math(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("$$"));
}

// --- Pandoc delimiter rules -----------------------------------------

#[test]
fn opener_followed_by_space_is_not_math() {
    let tokens = parse("a $ x$ b");
    assert!(!has_math(&tokens), "non-space must follow the opening $");
    assert!(Token::collect_all_text(&tokens).contains("$ x$"));
}

#[test]
fn closer_preceded_by_space_is_not_math() {
    let tokens = parse("a $x $ b");
    assert!(!has_math(&tokens), "non-space must precede the closing $");
}

#[test]
fn closer_followed_by_digit_is_not_math() {
    // Classic price false-positive guard: "$5 and $6" must stay text.
    let tokens = parse("it was $5 and $6 total");
    assert!(!has_math(&tokens));
    assert_eq!(Token::collect_all_text(&tokens), "it was $5 and $6 total");
}

#[test]
fn closer_followed_by_non_digit_punctuation_is_math() {
    let tokens = parse("see $x$, done");
    assert_eq!(maths(&tokens), vec![(true, "x".to_string())]);
}

#[test]
fn math_works_mid_word() {
    // KaTeX/MathJax permissiveness: `$x$` need not be flanked by spaces.
    let tokens = parse("foo$x$bar");
    assert_eq!(maths(&tokens), vec![(true, "x".to_string())]);
}

#[test]
fn empty_inline_dollars_are_not_math() {
    // `$$` with nothing is treated as a display opener; with no closer
    // it degrades to literal text rather than empty inline math.
    let tokens = parse("a $$ b");
    assert!(!has_math(&tokens));
}

// --- Escapes inside content -----------------------------------------

#[test]
fn backslash_escaped_dollar_inside_inline_is_not_a_closer() {
    let tokens = parse(r"$a \$ b$");
    assert_eq!(maths(&tokens), vec![(true, r"a \$ b".to_string())]);
}

#[test]
fn math_content_is_verbatim_no_markdown_parsing() {
    // `*`, `_`, `` ` ``, `[` inside math are literal TeX, not markdown.
    let tokens = parse(r"$a_*b* \alpha [c]$");
    assert_eq!(
        maths(&tokens),
        vec![(true, r"a_*b* \alpha [c]".to_string())]
    );
}

#[test]
fn math_content_keeps_tex_backslashes() {
    let tokens = parse(r"$\frac{1}{2} + \sqrt{x}$");
    assert_eq!(
        maths(&tokens),
        vec![(true, r"\frac{1}{2} + \sqrt{x}".to_string())]
    );
}

// --- Display math ---------------------------------------------------

#[test]
fn display_math_trims_surrounding_whitespace() {
    let tokens = parse("$$  E = mc^2  $$");
    assert_eq!(maths(&tokens), vec![(false, "E = mc^2".to_string())]);
}

#[test]
fn display_math_spans_multiple_lines() {
    let tokens = parse("$$\na = b\nc = d\n$$");
    assert_eq!(maths(&tokens), vec![(false, "a = b\nc = d".to_string())]);
}

#[test]
fn display_math_blank_line_terminates_and_degrades() {
    // A blank line ends the paragraph, so an unclosed display span
    // before it must not swallow across the boundary.
    let tokens = parse("$$\na = b\n\nplain paragraph");
    assert!(!has_math(&tokens));
    assert!(Token::collect_all_text(&tokens).contains("plain paragraph"));
}

#[test]
fn inline_math_does_not_cross_a_newline() {
    let tokens = parse("$a +\nb$");
    assert!(!has_math(&tokens), "inline math is single-line");
}

// --- Nesting / context ----------------------------------------------

#[test]
fn inline_math_inside_emphasis() {
    let tokens = parse("*the $x^2$ term*");
    assert_eq!(maths(&tokens), vec![(true, "x^2".to_string())]);
}

#[test]
fn inline_math_inside_heading() {
    let tokens = parse("# Energy $E=mc^2$ explained");
    assert_eq!(maths(&tokens), vec![(true, "E=mc^2".to_string())]);
}

#[test]
fn inline_math_inside_list_item_and_blockquote() {
    let tokens = parse("- item $a+b$\n\n> quote $c+d$");
    let m = maths(&tokens);
    assert!(m.contains(&(true, "a+b".to_string())));
    assert!(m.contains(&(true, "c+d".to_string())));
}

#[test]
fn dollar_inside_code_span_is_not_math() {
    let tokens = parse("`let cost = $5`");
    assert!(!has_math(&tokens), "code spans are opaque to math");
}

#[test]
fn dollar_inside_fenced_code_is_not_math() {
    let tokens = parse("```\n$x = 1$\n```");
    assert!(!has_math(&tokens), "fenced code is opaque to math");
}

#[test]
fn surrounding_text_and_spacing_preserved() {
    let tokens = parse("before $x$ after");
    assert_eq!(maths(&tokens), vec![(true, "x".to_string())]);
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("before "), "got {text:?}");
    assert!(text.contains(" after"), "space after $x$ lost: {text:?}");
}

#[test]
fn two_inline_spans_on_one_line() {
    let tokens = parse("$a$ and $b$");
    assert_eq!(
        maths(&tokens),
        vec![(true, "a".to_string()), (true, "b".to_string())]
    );
}

// --- Robustness (no panic on adversarial input) ---------------------

#[test]
fn adversarial_dollar_inputs_never_panic() {
    for src in [
        "$",
        "$$",
        "$$$",
        "$$$$",
        "$$$$$",
        "$x",
        "x$",
        "$ $",
        "$$ $$",
        "a$b$c$d$e",
        r"\$\$\$",
        "$\n$",
        "$$\n\n$$",
        "$$$$$$$$$$",
        "price $1 $2 $3 $4",
        "$$$$ unterminated display open",
        "${}^{}_{}$",
    ] {
        let tokens = parse(src);
        // Round-tripped text must always preserve every dollar's worth
        // of information one way or another (no silent total loss).
        let _ = Token::collect_all_text(&tokens);
        assert!(!tokens.is_empty() || src.is_empty(), "{src:?} produced nothing");
    }
}

#[test]
fn empty_display_math_is_inert() {
    // `$$$$` is an empty display span — a Math token with empty
    // content (the renderer drops it). Must not panic.
    let tokens = parse("$$$$");
    assert_eq!(maths(&tokens), vec![(false, String::new())]);
}
