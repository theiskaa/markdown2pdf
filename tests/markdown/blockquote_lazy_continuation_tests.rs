use markdown2pdf::markdown::*;

use super::common::parse;

fn body(t: &Token) -> &Vec<Token> {
    if let Token::BlockQuote(body) = t {
        body
    } else {
        panic!("expected BlockQuote, got {:?}", t);
    }
}

// a non-prefixed line that doesn't start a new block
// joins the open paragraph inside the quote.
#[test]
fn single_lazy_line_joins_paragraph() {
    let tokens = parse("> foo\nbar");
    assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
    let text = Token::collect_all_text(body(&tokens[0]));
    assert!(
        text.contains("foo") && text.contains("bar"),
        "got {:?}",
        text
    );
}

#[test]
fn multiple_lazy_lines_all_join() {
    let tokens = parse("> foo\nbar\nbaz");
    assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
    let text = Token::collect_all_text(body(&tokens[0]));
    for needle in &["foo", "bar", "baz"] {
        assert!(
            text.contains(needle),
            "{:?} missing from {:?}",
            needle,
            text
        );
    }
}

#[test]
fn lazy_mixed_with_marker_lines() {
    // Spec lazy lines can be interleaved with `>` lines.
    let tokens = parse("> foo\nbar\n> baz");
    assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
    let text = Token::collect_all_text(body(&tokens[0]));
    for needle in &["foo", "bar", "baz"] {
        assert!(
            text.contains(needle),
            "{:?} missing from {:?}",
            needle,
            text
        );
    }
}

#[test]
fn blank_line_terminates_lazy() {
    let tokens = parse("> foo\nbar\n\nbaz");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo") && q_text.contains("bar"));
    assert!(
        !q_text.contains("baz"),
        "blank line didn't stop quote: {:?}",
        q_text
    );
    // baz should appear as a separate top-level token.
    let after = Token::collect_all_text(&tokens[1..]);
    assert!(after.contains("baz"), "baz missing from rest {:?}", after);
}

#[test]
fn thematic_break_interrupts_lazy() {
    let tokens = parse("> foo\n---");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo"));
    assert!(!q_text.contains("---"), "thematic leaked in: {:?}", q_text);
    assert!(
        tokens[1..]
            .iter()
            .any(|t| matches!(t, Token::HorizontalRule)),
        "expected HR after quote, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn list_marker_interrupts_lazy() {
    let tokens = parse("> foo\n- bar");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo"));
    assert!(!q_text.contains("bar"), "marker leaked in: {:?}", q_text);
    assert!(
        tokens[1..]
            .iter()
            .any(|t| matches!(t, Token::ListItem { .. })),
        "expected ListItem after quote, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn atx_heading_interrupts_lazy() {
    let tokens = parse("> foo\n# heading");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo"));
    assert!(!q_text.contains("heading"));
    assert!(
        tokens[1..]
            .iter()
            .any(|t| matches!(t, Token::Heading(_, 1))),
        "expected H1 after quote, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn fenced_code_interrupts_lazy() {
    let tokens = parse("> foo\n```\ncode\n```");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo"));
    assert!(!q_text.contains("code"), "fence leaked in: {:?}", q_text);
    assert!(
        tokens[1..].iter().any(|t| matches!(t, Token::Code { .. })),
        "expected Code after quote, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn lazy_in_nested_blockquote_attaches_innermost() {
    // Per spec example 234: `>> foo\nbar` → bar joins the inner quote's
    // paragraph. We rely on the sub-lexer running the same lazy logic
    // recursively.
    let tokens = parse(">> foo\nbar");
    assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
    let outer = body(&tokens[0]);
    // The outer body must contain exactly one nested BlockQuote, whose
    // body contains both `foo` and `bar`.
    let inner = outer
        .iter()
        .find_map(|t| {
            if let Token::BlockQuote(b) = t {
                Some(b)
            } else {
                None
            }
        })
        .expect("expected nested BlockQuote");
    let inner_text = Token::collect_all_text(inner);
    assert!(
        inner_text.contains("foo") && inner_text.contains("bar"),
        "inner text: {:?}",
        inner_text
    );
}

#[test]
fn empty_quote_line_closes_paragraph_no_lazy() {
    // `> foo\n>\nbar` — the empty `>` line closes the open paragraph;
    // `bar` should NOT lazy-continue into the quote.
    let tokens = parse("> foo\n>\nbar");
    let q_text = Token::collect_all_text(body(&tokens[0]));
    assert!(q_text.contains("foo"));
    assert!(
        !q_text.contains("bar"),
        "bar must not be lazy after empty `>` line: {:?}",
        q_text
    );
}

#[test]
fn lazy_line_inline_formatting_is_parsed() {
    // The lazy line goes through the same sub-lexer pass — emphasis,
    // links, etc. must still be recognized inside it.
    let tokens = parse("> normal\n*lazy emphasis*");
    let quote = body(&tokens[0]);
    assert!(
        quote.iter().any(|t| matches!(t, Token::Emphasis { .. })),
        "expected emphasis in quote body, got {}",
        Token::slice_to_compact(quote)
    );
}

#[test]
fn nested_blockquote_marker_continues_quote() {
    // `> foo\n>> bar` — second line starts another quote inside the
    // first; should not be lazy. Both should be inside the outer quote.
    let tokens = parse("> foo\n>> bar");
    assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
    let outer = body(&tokens[0]);
    assert!(
        outer.iter().any(|t| matches!(t, Token::BlockQuote(_))),
        "expected nested BlockQuote inside outer, got {}",
        Token::slice_to_compact(outer)
    );
}
