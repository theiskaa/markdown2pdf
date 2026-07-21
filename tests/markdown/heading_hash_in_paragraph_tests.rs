use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn csharp_in_paragraph_is_text() {
    let tokens = parse("This uses C# heavily");
    assert_eq!(
        tokens,
        vec![Token::Text("This uses C# heavily".to_string())]
    );
}

#[test]
fn multiple_hashes_in_paragraph() {
    let tokens = parse("Compare C# and F# please");
    assert_eq!(
        tokens,
        vec![Token::Text("Compare C# and F# please".to_string())]
    );
}

#[test]
fn trailing_hash_in_paragraph() {
    let tokens = parse("ends with C#");
    assert_eq!(tokens, vec![Token::Text("ends with C#".to_string())]);
}

#[test]
fn line_start_heading_still_works() {
    let tokens = parse("# Real heading");
    assert_eq!(
        tokens,
        vec![Token::Heading(
            vec![Token::Text("Real heading".to_string())],
            1
        )]
    );
}

#[test]
fn heading_with_hash_in_content() {
    let tokens = parse("## Summary about C#");
    assert_eq!(
        tokens,
        vec![Token::Heading(
            vec![Token::Text("Summary about C#".to_string())],
            2
        )]
    );
}

#[test]
fn paragraph_then_heading() {
    let tokens = parse("first uses C#\n# heading");
    assert_eq!(
        tokens,
        vec![
            Token::Text("first uses C#".to_string()),
            Token::Newline,
            Token::Heading(vec![Token::Text("heading".to_string())], 1),
        ]
    );
}

#[test]
fn heading_then_paragraph_with_hash() {
    let tokens = parse("# Title\n\nbody mentions C# here");
    assert_eq!(
        tokens,
        vec![
            Token::Heading(vec![Token::Text("Title".to_string())], 1),
            Token::Newline,
            Token::Newline,
            Token::Text("body mentions C# here".to_string()),
        ]
    );
}

#[test]
fn full_csharp_issue_repro() {
    // Exact reproducer from issues/csharp.md
    let input = "## Summary\n\nThis monorepo is a coordination layer over four independent implementations of the same problem set. Clojure defines the Clojure algorithmic source, and C#, Rust, and Elixir mirror that source in their own idioms. The container repo keeps the system organized through ZSH-based orchestration, documentation, and repo-wide conventions.";
    let mut lexer = Lexer::new(input.to_string());
    let tokens = lexer.parse().expect("must not error on C# in paragraph");

    assert!(matches!(tokens[0], Token::Heading(_, 2)));
    let body = Token::collect_all_text(&tokens);
    assert!(body.contains("C#"));
    assert!(body.contains("Rust"));
    assert!(body.contains("Elixir"));
}
