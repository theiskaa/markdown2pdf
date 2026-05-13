use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn atx_without_space_is_text() {
    let tokens = parse("#hello");
    assert_eq!(tokens, vec![Token::Text("#hello".to_string())]);
}

#[test]
fn atx_with_space_is_heading() {
    let tokens = parse("# hello");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
}

#[test]
fn atx_with_tab_after_hash_is_heading() {
    let tokens = parse("#\thello");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
}

#[test]
fn atx_seven_hashes_falls_back_to_text() {
    let tokens = parse("####### too deep");
    assert!(!matches!(tokens[0], Token::Heading(_, _)));
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("####### too deep"), "got {:?}", text);
}

#[test]
fn atx_six_hashes_is_h6() {
    let tokens = parse("###### six");
    assert!(matches!(tokens[0], Token::Heading(_, 6)));
}

#[test]
fn atx_trailing_hashes_stripped() {
    let tokens = parse("## Title ##");
    if let Token::Heading(content, 2) = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert_eq!(text, "Title");
    } else {
        panic!("expected H2, got {:?}", tokens);
    }
}

#[test]
fn atx_trailing_hashes_with_trailing_space_stripped() {
    let tokens = parse("## Title ## ");
    if let Token::Heading(content, 2) = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert_eq!(text, "Title");
    } else {
        panic!("expected H2, got {:?}", tokens);
    }
}

#[test]
fn atx_trailing_hash_without_preceding_space_kept() {
    // Regression — `## C#` must keep the `#` as content (no preceding space).
    let tokens = parse("## C#");
    if let Token::Heading(content, 2) = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert_eq!(text, "C#");
    } else {
        panic!("expected H2, got {:?}", tokens);
    }
}

#[test]
fn empty_atx_just_hashes() {
    let tokens = parse("##");
    assert!(matches!(tokens[0], Token::Heading(_, 2)));
    if let Token::Heading(content, _) = &tokens[0] {
        assert!(content.is_empty());
    }
}
