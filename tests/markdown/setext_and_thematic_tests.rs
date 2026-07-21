use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn setext_h1_basic() {
    let tokens = parse("Title\n===");
    assert!(
        matches!(tokens[0], Token::Heading(_, 1)),
        "expected H1, got {:?}",
        tokens
    );
    if let Token::Heading(content, 1) = &tokens[0] {
        assert_eq!(Token::collect_all_text(content), "Title");
    }
}

#[test]
fn setext_h1_long_underline() {
    let tokens = parse("Title\n=======");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
}

#[test]
fn setext_h1_with_inline_emphasis() {
    let tokens = parse("Title with *emphasis*\n===");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
    if let Token::Heading(content, 1) = &tokens[0] {
        assert!(content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    }
}

#[test]
fn setext_h2_basic() {
    let tokens = parse("Title\n---");
    assert!(
        matches!(tokens[0], Token::Heading(_, 2)),
        "expected H2 (NOT a HorizontalRule), got {:?}",
        tokens
    );
    if let Token::Heading(content, 2) = &tokens[0] {
        assert_eq!(Token::collect_all_text(content), "Title");
    }
}

#[test]
fn setext_h2_long_underline() {
    let tokens = parse("Title\n----------");
    assert!(matches!(tokens[0], Token::Heading(_, 2)));
}

#[test]
fn thematic_break_dashes() {
    let tokens = parse("---");
    assert_eq!(tokens, vec![Token::HorizontalRule]);
}

#[test]
fn thematic_break_asterisks() {
    let tokens = parse("***");
    assert_eq!(tokens, vec![Token::HorizontalRule]);
}

#[test]
fn thematic_break_underscores() {
    let tokens = parse("___");
    assert_eq!(tokens, vec![Token::HorizontalRule]);
}

#[test]
fn thematic_break_long_runs() {
    for input in ["-------", "*******", "_______"] {
        assert_eq!(
            parse(input),
            vec![Token::HorizontalRule],
            "input {:?}",
            input
        );
    }
}

#[test]
fn paragraph_followed_by_dashes_is_setext_h2_not_hr() {
    let tokens = parse("Some content\n---");
    // Must be Heading, not Text + HorizontalRule
    let has_hr = tokens.iter().any(|t| matches!(t, Token::HorizontalRule));
    assert!(!has_hr, "should not have produced an HR, got {:?}", tokens);
    assert!(matches!(tokens[0], Token::Heading(_, 2)));
}

#[test]
fn lone_dashes_after_blank_line_is_hr() {
    let tokens = parse("Some content\n\n---");
    // Blank line means dashes are a true HR, not a setext underline.
    assert!(tokens.iter().any(|t| matches!(t, Token::HorizontalRule)));
}

#[test]
fn regression_atx_h1_still_works() {
    let tokens = parse("# H1");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
}

#[test]
fn regression_atx_h2_still_works() {
    let tokens = parse("## H2");
    assert!(matches!(tokens[0], Token::Heading(_, 2)));
}

#[test]
fn regression_list_item_after_paragraph() {
    // Make sure setext detection doesn't eat list markers.
    let tokens = parse("paragraph\n- item");
    let has_li = tokens.iter().any(|t| matches!(t, Token::ListItem { .. }));
    assert!(has_li, "expected list item, got {:?}", tokens);
}

#[test]
fn display_math_opener_skips_setext_walker() {
    // `$$` starts a display-math block — setext detection must not
    // fold the math body into an H1 just because an `=` line appears
    // inside it (common in matrix-multiplication equations).
    let tokens = parse(
        "$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n\\begin{pmatrix} x \\\\ y \\end{pmatrix}\n=\n\\begin{pmatrix} ax+by \\\\ cx+dy \\end{pmatrix}\n$$\n",
    );
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Math { inline: false, .. })),
        "expected display math token, got {tokens:?}"
    );
    assert!(
        !tokens.iter().any(|t| matches!(t, Token::Heading(_, _))),
        "must not produce a heading for `$$ … = … $$`"
    );
}
