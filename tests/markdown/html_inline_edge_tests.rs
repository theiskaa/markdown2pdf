//! Inline raw-HTML edge cases — the constructs that don't fit the
//! ordinary open/close tag shape but still pass through verbatim:
//!
//!   - Processing instructions: `<?…?>`
//!   - Declarations: `<!LETTER…>`
//!   - CDATA sections: `<![CDATA[…]]>`
//!   - Short HTML comment forms: `<!-->` (empty body) and `<!--->`
//!     (single-hyphen body)
//!   - Malformed attribute quoting that should reject as text rather
//!     than match as inline HTML.

use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn inline_processing_instruction() {
    let tokens = parse("foo <?php echo 'hi'; ?>");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s.contains("<?php"))),
        "got {:?}",
        tokens,
    );
}

#[test]
fn inline_xml_processing_instruction() {
    let tokens = parse(r#"foo <?xml version="1.0"?>"#);
    assert!(tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}

#[test]
fn inline_declaration() {
    let tokens = parse("foo <!ELEMENT br EMPTY>");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s.contains("ELEMENT"))),
        "got {:?}",
        tokens,
    );
}

#[test]
fn inline_cdata() {
    let tokens = parse("foo <![CDATA[>&<]]>");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s.contains("CDATA"))),
        "got {:?}",
        tokens,
    );
}

#[test]
fn short_comment_empty_body() {
    let tokens = parse("text <!--> tail");
    let Some(Token::HtmlComment(body)) = tokens.iter().find(|t| matches!(t, Token::HtmlComment(_)))
    else {
        panic!("expected HtmlComment, got {:?}", tokens);
    };
    assert_eq!(body, "");
}

#[test]
fn short_comment_single_hyphen_body() {
    let tokens = parse("text <!---> tail");
    let Some(Token::HtmlComment(body)) = tokens.iter().find(|t| matches!(t, Token::HtmlComment(_)))
    else {
        panic!("expected HtmlComment, got {:?}", tokens);
    };
    assert_eq!(body, "-");
}

#[test]
fn unquoted_attribute_value_with_single_quote_rejected() {
    // Per CommonMark §6.6 an unquoted value can't contain `'`, `"`,
    // `=`, `<`, `>`, `` ` ``. `<a href=hi'>` has `'` in the value and
    // should NOT be a valid inline tag — it falls through to text.
    let tokens = parse("<a href=hi'>");
    assert!(
        !tokens
            .iter()
            .any(|t| matches!(t, Token::HtmlInline(s) if s.contains("href=hi"))),
        "got {:?}",
        tokens,
    );
}

#[test]
fn unclosed_double_quoted_attribute_rejected() {
    // `<a href="hi>` — the value's opening `"` has no closing `"` on
    // the same input. Should NOT match as inline HTML.
    let tokens = parse("<a href=\"hi'>");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}

#[test]
fn pi_inside_inline_text() {
    // Surrounding text preserved as Text tokens.
    let tokens = parse("before <?php ?> after");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("before"));
    assert!(text.contains("after"));
    assert!(tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}

#[test]
fn unterminated_pi_falls_through_to_text() {
    let tokens = parse("foo <?php never closes");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}

#[test]
fn unterminated_declaration_falls_through_to_text() {
    let tokens = parse("foo <!ELEMENT no close");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}

#[test]
fn unterminated_cdata_falls_through_to_text() {
    let tokens = parse("foo <![CDATA[never closes");
    assert!(!tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))));
}
