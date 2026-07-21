use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn single_intra_word_underscore() {
    let tokens = parse("foo_bar");
    assert_eq!(tokens, vec![Token::Text("foo_bar".to_string())]);
}

#[test]
fn double_intra_word_underscore() {
    let tokens = parse("foo__bar");
    assert_eq!(tokens, vec![Token::Text("foo__bar".to_string())]);
}

#[test]
fn triple_intra_word_underscore() {
    let tokens = parse("foo___bar");
    assert_eq!(tokens, vec![Token::Text("foo___bar".to_string())]);
}

#[test]
fn multiple_intra_word_underscores() {
    let tokens = parse("foo_bar_baz_qux");
    assert_eq!(tokens, vec![Token::Text("foo_bar_baz_qux".to_string())]);
}

#[test]
fn snake_case_identifier() {
    let tokens = parse("snake_case_variable");
    assert_eq!(tokens, vec![Token::Text("snake_case_variable".to_string())]);
}

#[test]
fn upper_snake_case() {
    let tokens = parse("UPPER_CASE_CONSTANT");
    assert_eq!(tokens, vec![Token::Text("UPPER_CASE_CONSTANT".to_string())]);
}

#[test]
fn path_with_underscore() {
    let tokens = parse("phpmyadmin/localized_docs");
    assert_eq!(
        tokens,
        vec![Token::Text("phpmyadmin/localized_docs".to_string())]
    );
}

#[test]
fn underscore_path_in_sentence() {
    let tokens = parse("blabla phpmyadmin/localized_docs blabla");
    assert_eq!(
        tokens,
        vec![Token::Text(
            "blabla phpmyadmin/localized_docs blabla".to_string()
        )]
    );
}

#[test]
fn heading_with_intra_word_underscore() {
    let tokens = parse("## phpmyadmin/localized_docs (GitHub)");
    assert_eq!(
        tokens,
        vec![Token::Heading(
            vec![Token::Text(
                "phpmyadmin/localized_docs (GitHub)".to_string()
            )],
            2
        )]
    );
}

#[test]
fn heading_with_code_containing_underscore() {
    let tokens = parse("## `phpmyadmin/localized_docs` (GitHub)");
    if let Token::Heading(content, 2) = &tokens[0] {
        assert!(matches!(content[0], Token::Code { .. }));
        if let Token::Code { content: code, .. } = &content[0] {
            assert_eq!(code, "phpmyadmin/localized_docs");
        }
    } else {
        panic!("expected H2 heading, got {:?}", tokens);
    }
}

// Emphasis still works (regression)

#[test]
fn single_underscore_emphasis_still_works() {
    let tokens = parse("_italic_");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
}

#[test]
fn double_underscore_strong_still_works() {
    let tokens = parse("__bold__");
    assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
}

#[test]
fn underscore_emphasis_with_space_flank() {
    let tokens = parse("foo _bar_ baz");
    // foo_<space> Text, then _bar_ Emphasis, then baz Text
    // (existing whitespace handling collapses the space after closing `_`)
    assert!(matches!(&tokens[0], Token::Text(s) if s.starts_with("foo")));
    assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
    assert!(matches!(&tokens[2], Token::Text(s) if s.contains("baz")));
    if let Token::Emphasis { content, .. } = &tokens[1] {
        let inner = Token::collect_all_text(content);
        assert!(inner.contains("bar"));
    }
}

#[test]
fn underscore_emphasis_in_parens() {
    let tokens = parse("(_foo_)");
    assert!(matches!(&tokens[0], Token::Text(s) if s == "("));
    assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
    assert!(matches!(&tokens[2], Token::Text(s) if s == ")"));
}

// CommonMark-tricky: outer _ open/close, inner _ is intra-word
#[test]
fn outer_emphasis_with_inner_intra_word_underscore() {
    let tokens = parse("_foo_bar_");
    // Should be one emphasis with text "foo_bar"
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    let inner_text = Token::collect_all_text(&[tokens[0].clone()]);
    assert!(
        inner_text.contains("foo_bar"),
        "expected emphasis to contain 'foo_bar', got {:?}",
        tokens
    );
}

// Star emphasis must remain unchanged

#[test]
fn star_emphasis_intra_word_still_emphasis() {
    // * is allowed intra-word
    let tokens = parse("a*b*c");
    assert!(matches!(&tokens[0], Token::Text(s) if s == "a"));
    assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
    assert!(matches!(&tokens[2], Token::Text(s) if s == "c"));
}

#[test]
fn star_emphasis_basic() {
    let tokens = parse("*italic*");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
}

#[test]
fn star_strong() {
    let tokens = parse("**bold**");
    assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
}

// Cross-context

#[test]
fn list_item_with_intra_word_underscore() {
    let tokens = parse("- foo_bar item");
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo_bar"));
    } else {
        panic!("expected list item, got {:?}", tokens);
    }
}

#[test]
fn blockquote_with_intra_word_underscore() {
    let tokens = parse("> Quote with foo_bar inside");
    assert_eq!(tokens.len(), 1);
    if let Token::BlockQuote(body) = &tokens[0] {
        assert_eq!(Token::collect_all_text(body), "Quote with foo_bar inside");
        // intra-word `_` must not produce emphasis here either
        assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    } else {
        panic!("expected BlockQuote, got {:?}", tokens);
    }
}

#[test]
fn link_with_intra_word_underscore() {
    let tokens = parse("[link_text](https://example.com)");
    assert_eq!(
        tokens,
        vec![Token::Link {
            content: vec![Token::Text("link_text".to_string())],
            url: "https://example.com".to_string(),
            title: None
        }]
    );
}

#[test]
fn code_with_underscore() {
    let tokens = parse("`foo_bar`");
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "foo_bar".to_string(),
            block: false
        }]
    );
}

#[test]
fn image_alt_with_underscore() {
    let tokens = parse("![alt_text](img.png)");
    assert_eq!(
        tokens,
        vec![Token::Image {
            alt: vec![Token::Text("alt_text".to_string())],
            url: "img.png".to_string(),
            title: None
        }]
    );
}

// Real-world reproducer from issues/unmatching.md
#[test]
fn full_unmatching_issue_repro() {
    let input = "## `phpmyadmin/localized_docs` (GitHub)\n## phpmyadmin/localized_docs (GitHub)";
    let mut lexer = Lexer::new(input.to_string());
    let tokens = lexer.parse().expect("must not error on intra-word _");

    // Two headings, separated by Newline
    assert!(matches!(tokens[0], Token::Heading(_, 2)));
    let last_heading = tokens
        .iter()
        .rev()
        .find(|t| matches!(t, Token::Heading(_, 2)))
        .unwrap();
    if let Token::Heading(content, _) = last_heading {
        let text = Token::collect_all_text(content);
        assert!(text.contains("phpmyadmin/localized_docs"));
    }
}
