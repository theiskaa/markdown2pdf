use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn plain_text() {
    let tokens = vec![Token::Text("hello world".to_string())];
    assert_eq!(Token::collect_all_text(&tokens), "hello world");
}

#[test]
fn heading_descends_into_children() {
    let tokens = parse("# Hello *world*\n");
    assert!(Token::collect_all_text(&tokens).contains("Hello"));
    assert!(Token::collect_all_text(&tokens).contains("world"));
}

#[test]
fn emphasis_and_strong_descend() {
    let tokens = parse("*one* **two**");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("one"));
    assert!(text.contains("two"));
}

#[test]
fn code_content_included() {
    let tokens = parse("`inline` and ```fenced```");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("inline"));
}

#[test]
fn blockquote_and_listitem_descend() {
    let tokens = parse("> in quote\n\n- item one\n- item two\n");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("in quote"));
    assert!(text.contains("item one"));
    assert!(text.contains("item two"));
}

#[test]
fn link_content_included_but_url_is_not() {
    let tokens = parse("[shown](https://example.com)");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("shown"));
    assert!(!text.contains("https://example.com"));
}

#[test]
fn image_alt_included_but_url_is_not() {
    let tokens = parse("![alt-text](pic.png)");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("alt-text"));
    assert!(!text.contains("pic.png"));
}

#[test]
fn strikethrough_descends() {
    let tokens = parse("~~deleted~~");
    assert!(Token::collect_all_text(&tokens).contains("deleted"));
}

#[test]
fn structural_tokens_yield_empty() {
    // Newline, HardBreak, HorizontalRule, TableAlignment carry no
    // text. Each should leave collect_all_text untouched.
    let tokens = vec![
        Token::Newline,
        Token::HardBreak,
        Token::HorizontalRule,
        Token::TableAlignment(markdown2pdf::markdown::TableAlignment::Left),
    ];
    assert_eq!(Token::collect_all_text(&tokens), "");
}

#[test]
fn table_cells_included() {
    let tokens = parse("| a | b |\n| --- | --- |\n| x | y |\n");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("a"));
    assert!(text.contains("b"));
    assert!(text.contains("x"));
    assert!(text.contains("y"));
}

#[test]
fn document_round_trip_includes_all_visible_chars() {
    // Every visible char in this document must appear in the
    // collected text — guards against silent variant additions that
    // forget to descend.
    let doc = "# Title\n\n*emph* and **strong** with `code`\n\n> quote\n\n- one\n- two\n";
    let tokens = parse(doc);
    let text = Token::collect_all_text(&tokens);
    for needle in &["Title", "emph", "strong", "code", "quote", "one", "two"] {
        assert!(text.contains(needle), "missing {:?} in {:?}", needle, text);
    }
}
