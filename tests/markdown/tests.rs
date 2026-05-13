use genpdfi::Alignment;
use markdown2pdf::markdown::*;

use super::common::parse;

// Helper function to create a lexer and parse input

#[test]
fn test_basic_text() {
    let tokens = parse("Hello world");
    assert_eq!(tokens, vec![Token::Text("Hello world".to_string())]);
}

#[test]
fn test_headings() {
    let tests = vec![
        (
            "# H1",
            vec![Token::Heading(vec![Token::Text("H1".to_string())], 1)],
        ),
        (
            "## H2",
            vec![Token::Heading(vec![Token::Text("H2".to_string())], 2)],
        ),
        (
            "### H3",
            vec![Token::Heading(vec![Token::Text("H3".to_string())], 3)],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_emphasis() {
    // After removing the spurious trailing-space push in parse_emphasis,
    // emphasis content is exactly the inner text — no extra " " token.
    let tests = vec![
        (
            "*italic*",
            vec![Token::Emphasis {
                level: 1,
                content: vec![Token::Text("italic".to_string())],
            }],
        ),
        (
            "**bold**",
            vec![Token::Emphasis {
                level: 2,
                content: vec![Token::Text("bold".to_string())],
            }],
        ),
        (
            "_also italic_",
            vec![Token::Emphasis {
                level: 1,
                content: vec![Token::Text("also italic".to_string())],
            }],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_code_blocks() {
    let tests = vec![
        (
            "`inline code`",
            vec![Token::Code { language: "".to_string(), content: "inline code".to_string(), block: false }],
        ),
        (
            "```rust\nfn main() {}\n```",
            vec![Token::Code { language: "rust".to_string(), content: "fn main() {}".to_string(), block: true }],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_blockquotes() {
    let tokens = parse("> This is a quote");
    assert_eq!(tokens.len(), 1);
    if let Token::BlockQuote(body) = &tokens[0] {
        let text = Token::collect_all_text(body);
        assert_eq!(text, "This is a quote");
    } else {
        panic!("expected BlockQuote, got {:?}", tokens);
    }
}

#[test]
fn test_lists() {
    let tests = vec![
        (
            "- Item 1\n- Item 2",
            vec![
                Token::ListItem {
                    content: vec![Token::Text("Item 1".to_string())],
                    ordered: false,
                    number: None,
                    marker: '-',
                    checked: None,
            loose: false,
                },
                Token::ListItem {
                    content: vec![Token::Text("Item 2".to_string())],
                    ordered: false,
                    number: None,
                    marker: '-',
                    checked: None,
            loose: false,
                },
            ],
        ),
        (
            "1. First\n2. Second",
            vec![
                Token::ListItem {
                    content: vec![Token::Text("First".to_string())],
                    ordered: true,
                    number: Some(1),
                    marker: '.',
                    checked: None,
            loose: false,
                },
                Token::ListItem {
                    content: vec![Token::Text("Second".to_string())],
                    ordered: true,
                    number: Some(2),
                    marker: '.',
                    checked: None,
            loose: false,
                },
            ],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_nested_lists() {
    let input = "- Item 1\n  - Nested 1\n  - Nested 2\n- Item 2";
    let expected = vec![
        Token::ListItem {
            content: vec![
                Token::Text("Item 1".to_string()),
                Token::ListItem {
                    content: vec![Token::Text("Nested 1".to_string())],
                    ordered: false,
                    number: None,
                    marker: '-',
                    checked: None,
                    loose: false,
                },
                Token::ListItem {
                    content: vec![Token::Text("Nested 2".to_string())],
                    ordered: false,
                    number: None,
                    marker: '-',
                    checked: None,
                    loose: false,
                },
            ],
            ordered: false,
            number: None,
            marker: '-',
            checked: None,
            loose: false,
        },
        Token::ListItem {
            content: vec![Token::Text("Item 2".to_string())],
            ordered: false,
            number: None,
            marker: '-',
            checked: None,
            loose: false,
        },
    ];
    assert_eq!(parse(input), expected);
}

#[test]
fn test_links() {
    let tests = vec![
        (
            "[Link](https://example.com)",
            vec![Token::Link { content: vec![Token::Text("Link".to_string())], url: "https://example.com".to_string(), title: None }],
        ),
        (
            "![Image](image.jpg)",
            vec![Token::Image { alt: vec![Token::Text("Image".to_string())], url: "image.jpg".to_string(), title: None }],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_horizontal_rule() {
    let tests = vec!["---", "----", "-----"];
    for input in tests {
        assert_eq!(parse(input), vec![Token::HorizontalRule]);
    }
}
#[test]
fn test_complex_document() {
    let input = r#"# Main Title

This is a paragraph with *italic* and **bold** text.

## Subsection

- List item 1
  - Nested item with `code`
- List item 2

> A blockquote

---

[Link](https://example.com)"#;

    let tokens = parse(input);
    assert!(tokens.len() > 0);
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
    // Add more specific assertions as needed
}

#[test]
fn test_error_cases() {
    // Unclosed HTML comment falls back to literal text (the lexer
    // emits the partial `<!--…` chars as `Text` rather than bubbling
    // an error up). The robustness contract is: lexer.parse() returns
    // Ok for any input that doesn't hit a hard panic.
    let mut lexer = Lexer::new("<!--never closes".to_string());
    let tokens = lexer.parse().expect("partial HTML comment should not error");
    let dbg = format!("{:?}", tokens);
    assert!(
        dbg.contains("Text") && dbg.contains("<!--"),
        "expected literal `<!--…` text, got {}",
        dbg
    );
}

#[test]
fn test_code_block_edge_cases() {
    let tests = vec![
        (
            "```\nempty language\n```",
            vec![Token::Code {
                language: "".to_string(),
                content: "empty language".to_string(),
                block: true,
            }],
        ),
        (
            "`code with *asterisk*`",
            vec![Token::Code {
                language: "".to_string(),
                content: "code with *asterisk*".to_string(),
                block: false,
            }],
        ),
        (
            "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```",
            vec![Token::Code { language: "rust".to_string(), content: "fn main() {\n    println!(\"Hello\");\n}".to_string(), block: true }],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_nested_list_combinations() {
    let input = r#"1. First level
   - Nested unordered
   - Another unordered
2. Second level
   1. Nested ordered
   2. Another ordered
   - Mixed with unordered"#;

    let tokens = parse(input);
    assert_eq!(tokens.len(), 2); // Two top-level items
    assert!(matches!(
        tokens[0],
        Token::ListItem {
            ordered: true,
            number: Some(1),
            ..
        }
    ));
    assert!(matches!(
        tokens[1],
        Token::ListItem {
            ordered: true,
            number: Some(2),
            ..
        }
    ));
}

#[test]
fn test_blockquote_variations() {
    // After the blockquote shape change, the body is a Vec<Token> and
    // inline formatting inside a quote is parsed (so *emphasis* becomes
    // an Emphasis token, [link](url) becomes a Link, etc.).
    let cases: &[(&str, &dyn Fn(&[Token])) ] = &[
        (
            "> Simple quote",
            &|body| {
                assert_eq!(Token::collect_all_text(body), "Simple quote");
            },
        ),
        (
            "> Quote with *emphasis*",
            &|body| {
                assert!(body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
            },
        ),
        (
            "> Quote with [link](url)",
            &|body| {
                assert!(body.iter().any(|t| matches!(t, Token::Link { .. })));
            },
        ),
    ];

    for (input, check) in cases {
        let tokens = parse(input);
        assert_eq!(tokens.len(), 1, "input was {:?}", input);
        if let Token::BlockQuote(body) = &tokens[0] {
            check(body);
        } else {
            panic!("expected BlockQuote for {:?}, got {:?}", input, tokens);
        }
    }
}

#[test]
fn test_link_and_image_edge_cases() {
    let tests = vec![
        (
            // Plain URLs may not contain spaces — the URL ends at the
            // first whitespace and the rest is text.
            "[Link with spaces](<https://example.com/path with spaces>)",
            vec![Token::Link {
                content: vec![Token::Text("Link with spaces".to_string())],
                url: "https://example.com/path with spaces".to_string(),
                title: None,
            }],
        ),
        (
            "![Image with *emphasis* in alt](image.jpg)",
            vec![Token::Image {
                alt: vec![
                    Token::Text("Image with ".to_string()),
                    Token::Emphasis {
                        level: 1,
                        content: vec![Token::Text("emphasis".to_string())],
                    },
                    Token::Text(" in alt".to_string()),
                ],
                url: "image.jpg".to_string(),
                title: None,
            }],
        ),
        (
            "[Empty]()",
            vec![Token::Link { content: vec![Token::Text("Empty".to_string())], url: "".to_string(), title: None }],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_whitespace_handling() {
    // Trailing whitespace after a closing emphasis delimiter is preserved
    // as a separate Text token rather than swallowed. Validate that the
    // emphasis itself parses cleanly; trailing whitespace tokens are OK.
    let tokens = parse("*emphasis with space after*  ");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    if let Token::Emphasis { content, .. } = &tokens[0] {
        let inner = Token::collect_all_text(content);
        assert!(
            inner.contains("emphasis with space after"),
            "got {:?}",
            inner
        );
    }
}

#[test]
fn test_mixed_content() {
    let input = r#"# Title with *emphasis*

A paragraph with `code` and [link](url).

- List with **bold**
  1. Nested with *italic*
  2. And `code`

> Quote with [link](url)"#;

    let tokens = parse(input);
    assert!(tokens.len() > 0);

    // Verify first token is a heading with emphasis
    if let Token::Heading(content, 1) = &tokens[0] {
        assert!(content
            .iter()
            .any(|token| matches!(token, Token::Emphasis { .. })));
    } else {
        panic!("Expected heading with emphasis");
    }
}

#[test]
fn test_html_comment_variations() {
    // `<!--…-->` at line start now produces a block-level HtmlBlock
    // (CommonMark §4.6 type 2) carrying the verbatim block including
    // delimiters. Inline comments (mid-paragraph) still produce
    // HtmlComment — see html_comment_block_tests.rs and
    // parse_html_comment_tests.rs respectively.
    let tests = vec![
        (
            "<!-- Simple -->",
            vec![Token::HtmlBlock("<!-- Simple -->".to_string())],
        ),
        (
            "<!--Multi\nline\ncomment-->",
            vec![Token::HtmlBlock("<!--Multi\nline\ncomment-->".to_string())],
        ),
    ];

    for (input, expected) in tests {
        assert_eq!(parse(input), expected);
    }
}

#[test]
fn test_standalone_exclamation() {
    let tokens = parse("Hello! World");
    assert_eq!(tokens, vec![Token::Text("Hello! World".to_string())]);

    let tokens = parse("This is exciting!");
    assert_eq!(tokens, vec![Token::Text("This is exciting!".to_string())]);

    let tokens = parse("Multiple marks!!");
    assert_eq!(tokens, vec![Token::Text("Multiple marks!!".to_string())]);

    let tokens = parse("![Alt text](image.png)");
    assert_eq!(
        tokens,
        vec![Token::Image { alt: vec![Token::Text("Alt text".to_string())], url: "image.png".to_string(), title: None }]
    );
}

#[test]
fn test_tables() {
    let input = r#"| Name | Age | City |
|:-----|:---:|----:|
| Alice | 30 | Paris |
| Bob | 25 | Lyon |"#;

    let tokens = parse(input);
    assert_eq!(
        tokens,
        vec![Token::Table {
            headers: vec![
                vec![Token::Text("Name".to_string())],
                vec![Token::Text("Age".to_string())],
                vec![Token::Text("City".to_string())],
            ],
            aligns: vec![Alignment::Left, Alignment::Center, Alignment::Right],
            rows: vec![
                vec![
                    vec![Token::Text("Alice".to_string())],
                    vec![Token::Text("30".to_string())],
                    vec![Token::Text("Paris".to_string())],
                ],
                vec![
                    vec![Token::Text("Bob".to_string())],
                    vec![Token::Text("25".to_string())],
                    vec![Token::Text("Lyon".to_string())],
                ],
            ],
        }]
    );
}
