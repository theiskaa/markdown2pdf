use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn simple_inline_comment() {
    // INLINE comments need a non-comment prefix on the same line —
    // a `<!--` at line start is now a block-level HtmlBlock (CommonMark
    // §4.6 type 2), tested separately in html_comment_block_tests.rs.
    let tokens = parse("text <!-- hello -->");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::HtmlComment(_))),
        "expected HtmlComment, got {:?}",
        tokens
    );
}

#[test]
fn empty_comment_body_inline() {
    let tokens = parse("text <!---->");
    // Either parses as an empty HtmlComment or falls back to text —
    // both are acceptable; the bar is "must not panic".
    let _ = tokens;
}

#[test]
fn multi_line_inline_comment() {
    // A multi-line `<!--…-->` that starts mid-paragraph stays inline.
    let tokens = parse("text <!--\nline one\nline two\n-->");
    assert!(tokens.iter().any(|t| matches!(t, Token::HtmlComment(_))));
}

#[test]
fn comment_with_text_after_in_same_paragraph() {
    let tokens = parse("<!-- c --> tail");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("tail"));
}

#[test]
fn unterminated_comment_does_not_panic() {
    // Pin behavior at the "must not panic / hang" level — exact
    // fallback shape is allowed to evolve.
    let _tokens = parse("<!-- never closed");
}

#[test]
fn comment_with_text_before_in_same_paragraph() {
    let tokens = parse("head <!-- c -->");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("head"));
}
