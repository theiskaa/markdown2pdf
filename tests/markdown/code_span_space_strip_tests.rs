use markdown2pdf::markdown::*;

use super::common::parse;


#[test]
fn single_surrounding_space_stripped() {
    let tokens = parse("a ` foo ` b");
    let codes: Vec<_> = tokens
        .iter()
        .filter_map(|t| {
            if let Token::Code { content: body, .. } = t {
                Some(body.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(codes, vec!["foo"]);
}

#[test]
fn double_surrounding_space_only_one_stripped() {
    let tokens = parse("a `  foo  ` b");
    if let Some(Token::Code { content: body, .. }) =
        tokens.iter().find(|t| matches!(t, Token::Code { .. }))
    {
        assert_eq!(body, " foo ");
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}

#[test]
fn all_spaces_not_stripped() {
    let tokens = parse("a `   ` b");
    if let Some(Token::Code { content: body, .. }) =
        tokens.iter().find(|t| matches!(t, Token::Code { .. }))
    {
        assert_eq!(body, "   ");
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}

#[test]
fn no_surrounding_space_unchanged() {
    let tokens = parse("`foo`");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: "foo".to_string(), block: false }]
    );
}

#[test]
fn one_sided_space_unchanged() {
    // Only strip when BOTH sides have a space.
    let tokens = parse("a ` foo` b");
    if let Some(Token::Code { content: body, .. }) =
        tokens.iter().find(|t| matches!(t, Token::Code { .. }))
    {
        assert_eq!(body, " foo");
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}
