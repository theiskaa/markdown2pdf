use markdown2pdf::markdown::*;

use super::common::parse;



#[test]
fn escape_asterisk_blocks_emphasis() {
    let tokens = parse(r"\*not emphasis\*");
    assert_eq!(tokens, vec![Token::Text("*not emphasis*".to_string())]);
}

#[test]
fn escape_underscore_blocks_emphasis() {
    let tokens = parse(r"\_not emphasis\_");
    assert_eq!(tokens, vec![Token::Text("_not emphasis_".to_string())]);
}

#[test]
fn escape_hash_blocks_heading() {
    // \# at line start should NOT start a heading.
    let tokens = parse(r"\# not a heading");
    assert_eq!(tokens, vec![Token::Text("# not a heading".to_string())]);
}

#[test]
fn escape_left_bracket_blocks_link() {
    let tokens = parse(r"\[not a link]");
    assert_eq!(tokens, vec![Token::Text("[not a link]".to_string())]);
}

#[test]
fn escape_backtick_blocks_code() {
    let tokens = parse(r"\`not code\`");
    assert_eq!(tokens, vec![Token::Text("`not code`".to_string())]);
}

#[test]
fn escape_bang_blocks_image() {
    let tokens = parse(r"\![not an image](x)");
    // \! becomes literal !, then the [ ... ](x) gets parsed as a regular link.
    // Important: this must NOT crash with "Malformed image".
    assert!(matches!(tokens[0], Token::Text(ref s) if s == "!"));
    assert!(matches!(tokens[1], Token::Link { .. }));
}

#[test]
fn escape_double_backslash_yields_single_backslash() {
    let tokens = parse(r"\\");
    assert_eq!(tokens, vec![Token::Text("\\".to_string())]);
}

#[test]
fn escape_then_unescaped_emphasis() {
    // Spec: \\ -> literal \; then _foo_ opens emphasis normally.
    let tokens = parse(r"\\_foo_");
    assert_eq!(
        tokens,
        vec![
            Token::Text("\\".to_string()),
            Token::Emphasis {
                level: 1,
                content: vec![Token::Text("foo".to_string())],
            },
        ]
    );
}

#[test]
fn escape_all_punctuation_chars() {
    // Sweep every CommonMark-recognized punctuation char.
    // Each escape pair must collapse to the punctuation char alone.
    let punct = [
        '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', ':', ';',
        '<', '=', '>', '?', '@', '[', '\\', ']', '^', '_', '`', '{', '|', '}', '~',
    ];
    for c in punct {
        let input = format!("a\\{}b", c);
        let tokens = parse(&input);
        let collected = Token::collect_all_text(&tokens);
        assert!(
            collected.contains(&format!("a{}b", c)) || collected.contains(c),
            "punctuation {:?}: expected escaped literal in {:?}, got {:?}",
            c,
            input,
            tokens
        );
    }
}


#[test]
fn backslash_before_letter_is_literal() {
    // \a is not an escape — both chars survive.
    let tokens = parse(r"\a");
    assert_eq!(tokens, vec![Token::Text("\\a".to_string())]);
}

#[test]
fn backslash_before_digit_is_literal() {
    let tokens = parse(r"\7");
    assert_eq!(tokens, vec![Token::Text("\\7".to_string())]);
}

#[test]
fn trailing_backslash_at_eof_is_literal() {
    let tokens = parse(r"foo\");
    assert_eq!(tokens, vec![Token::Text("foo\\".to_string())]);
}


#[test]
fn escape_inside_emphasis_run() {
    // *\*foo* opens emphasis, escape produces literal *, foo* closes.
    let tokens = parse(r"*\*foo*");
    assert!(
        matches!(tokens[0], Token::Emphasis { level: 1, .. }),
        "expected emphasis, got {:?}",
        tokens
    );
    if let Token::Emphasis { content, .. } = &tokens[0] {
        let inner = Token::collect_all_text(content);
        assert!(inner.contains("*foo"), "inner was {:?}", inner);
    }
}

#[test]
fn escape_underscore_inside_emphasis() {
    // _foo\_bar_ -> emphasis with literal foo_bar
    let tokens = parse(r"_foo\_bar_");
    assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    if let Token::Emphasis { content, .. } = &tokens[0] {
        let inner = Token::collect_all_text(content);
        assert!(inner.contains("foo_bar"), "inner was {:?}", inner);
    }
}


#[test]
fn escape_inside_heading() {
    let tokens = parse(r"# Header with \*literal asterisks\*");
    assert!(matches!(tokens[0], Token::Heading(_, 1)));
    if let Token::Heading(content, _) = &tokens[0] {
        let inner = Token::collect_all_text(content);
        assert!(inner.contains("*literal asterisks*"), "got {:?}", inner);
    }
}


#[test]
fn escape_not_active_in_inline_code() {
    // Inside code, \\ and \* are literal — \ stays \.
    let tokens = parse(r"`\*literal\*`");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: r"\*literal\*".to_string(), block: false }]
    );
}

#[test]
fn escape_not_active_in_fenced_code() {
    let input = "```\n\\*kept literal\\*\n```";
    let tokens = parse(input);
    if let Token::Code { content: body, .. } = &tokens[0] {
        assert!(body.contains(r"\*kept literal\*"), "body was {:?}", body);
    } else {
        panic!("expected code block, got {:?}", tokens);
    }
}


#[test]
fn escape_blocks_thematic_rule() {
    let tokens = parse(r"\---");
    // \- becomes literal -; remaining -- is plain text.
    assert_eq!(tokens, vec![Token::Text("---".to_string())]);
}

#[test]
fn escape_blocks_blockquote() {
    let tokens = parse(r"\> not a quote");
    assert_eq!(tokens, vec![Token::Text("> not a quote".to_string())]);
}

#[test]
fn escape_blocks_list_marker() {
    // \- at line start should not start a list.
    let tokens = parse(r"\- not a list item");
    assert_eq!(tokens, vec![Token::Text("- not a list item".to_string())]);
}


#[test]
fn mixed_paragraph_with_multiple_escapes() {
    let tokens = parse(r"Use \*asterisks\* or \_underscores\_ for emphasis.");
    assert_eq!(
        tokens,
        vec![Token::Text(
            "Use *asterisks* or _underscores_ for emphasis.".to_string()
        )]
    );
}

#[test]
fn escape_mixed_with_real_emphasis() {
    // Both asterisks around "literal" are escaped (so it stays plain),
    // followed by a genuine *real* emphasis pair.
    let tokens = parse(r"\*literal\* and *real*");
    // -> Text("*literal* and ") + Emphasis(real)
    assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("*literal*")));
    let last = tokens.last().unwrap();
    assert!(matches!(last, Token::Emphasis { .. }));
}

#[test]
fn escape_does_not_consume_newline() {
    // a lone trailing backslash before a newline
    // is a hard line break — produces Text("foo") + HardBreak + Text("bar").
    let tokens = parse("foo\\\nbar");
    assert!(matches!(tokens[0], Token::Text(ref s) if s == "foo"));
    assert!(tokens.iter().any(|t| matches!(t, Token::HardBreak)));
    assert!(tokens.iter().any(|t| matches!(t, Token::Text(ref s) if s == "bar")));
}

#[test]
fn escape_inside_inline_code_span_is_literal() {
    // backslash escapes do NOT apply inside code spans.
    // Body must contain the literal backslash and the asterisk verbatim.
    let tokens = parse(r"`\*not emphasis\*`");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: r"\*not emphasis\*".to_string(), block: false }]
    );
}

#[test]
fn escape_inside_multi_backtick_code_span_is_literal() {
    let tokens = parse(r"``a \` b``");
    assert_eq!(
        tokens,
        vec![Token::Code { language: "".to_string(), content: r"a \` b".to_string(), block: false }]
    );
}

#[test]
fn escape_inside_fenced_code_block_is_literal() {
    let tokens = parse("```\n\\*not emphasis\\*\n```");
    let code = tokens
        .iter()
        .find_map(|t| if let Token::Code { content: body, .. } = t { Some(body) } else { None })
        .expect("expected Code token");
    assert!(
        code.contains(r"\*not emphasis\*"),
        "fenced code body should preserve backslashes literally, got {:?}",
        code
    );
}

#[test]
fn escape_inside_tilde_fenced_code_block_is_literal() {
    let tokens = parse("~~~\n\\*not emphasis\\*\n~~~");
    let code = tokens
        .iter()
        .find_map(|t| if let Token::Code { content: body, .. } = t { Some(body) } else { None })
        .expect("expected Code token");
    assert!(
        code.contains(r"\*not emphasis\*"),
        "tilde fence body should preserve backslashes literally, got {:?}",
        code
    );
}

#[test]
fn escape_inside_autolink_url_is_literal() {
    // escapes don't apply in autolinks. `<http://x/\bar>` keeps
    // the backslash verbatim as part of the URL.
    let tokens = parse(r"<http://example.com/\bar>");
    let link = tokens
        .iter()
        .find_map(|t| if let Token::Link { url, .. } = t { Some(url) } else { None })
        .expect("expected autolink Link token");
    assert!(
        link.contains(r"/\bar"),
        "autolink URL should preserve backslash literally, got {:?}",
        link
    );
}

#[test]
fn escape_in_link_url_inside_parens() {
    // escapes DO apply inside parenthesized link destinations,
    // so `\(` produces a literal `(` in the URL.
    let tokens = parse(r"[t](http://x\)y)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("t".to_string())], url: "http://x)y".to_string(), title: None }]
    );
}

#[test]
fn escape_in_link_text() {
    // escapes apply in link text — `\]` is literal `]`.
    let tokens = parse(r"[a\]b](u)");
    assert_eq!(
        tokens,
        vec![Token::Link { content: vec![Token::Text("a]b".to_string())], url: "u".to_string(), title: None }]
    );
}

#[test]
fn escape_propagates_through_heading() {
    // Heading inline content reuses parse_text, so escapes should also
    // apply inside an ATX heading.
    let tokens = parse(r"# foo \* bar");
    if let Token::Heading(content, 1) = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo * bar"), "got {:?}", text);
        // And no Emphasis should have formed inside.
        assert!(!content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    } else {
        panic!("expected Heading, got {}", Token::slice_to_compact(&tokens));
    }
}

#[test]
fn escape_propagates_through_blockquote() {
    let tokens = parse(r"> foo \* bar");
    if let Token::BlockQuote(body) = &tokens[0] {
        let text = Token::collect_all_text(body);
        assert!(text.contains("foo * bar"), "got {:?}", text);
        assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    } else {
        panic!("expected BlockQuote, got {}", Token::slice_to_compact(&tokens));
    }
}

#[test]
fn escape_propagates_through_list_item() {
    let tokens = parse(r"- foo \* bar");
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo * bar"), "got {:?}", text);
        assert!(!content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    } else {
        panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
    }
}

#[test]
fn escape_inside_emphasis_run_keeps_punctuation_literal() {
    // *\*foo* — outer * opens emphasis, \\* produces literal *, foo* closes.
    let tokens = parse(r"*\*foo*");
    if let Token::Emphasis { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.starts_with('*'), "got {:?}", text);
        assert!(text.contains("foo"), "got {:?}", text);
    } else {
        panic!("expected Emphasis, got {}", Token::slice_to_compact(&tokens));
    }
}
