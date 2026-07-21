use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn leading_tab_is_indented_code_block() {
    // A leading tab expands to 4 columns → indented code block.
    let tokens = parse("\tfoo");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
        "expected indented code block, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn two_spaces_plus_tab_is_indented_code_block() {
    // 2 spaces, then tab → tab expands to next multiple of 4 = col 4
    // total = 4 columns of indent → indented code block.
    let tokens = parse("  \tfoo");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
        "expected indented code block, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn three_spaces_plus_tab_is_indented_code_block() {
    // 3 spaces + tab → tab fills cols 3-4 → 4 columns → indented code.
    let tokens = parse("   \tfoo");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
        "expected indented code block, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn three_leading_spaces_no_tab_keeps_heading() {
    // 3 spaces of indent before `#` is still a heading.
    let tokens = parse("   # heading");
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Heading(_, 1))),
        "expected Heading, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn one_space_plus_tab_before_hash_is_indented_code() {
    // 1 space + tab → 4 columns → indented code, NOT heading.
    let tokens = parse(" \t# not a heading");
    assert!(
        !tokens.iter().any(|t| matches!(t, Token::Heading(_, _))),
        "unexpected Heading, got {}",
        Token::slice_to_compact(&tokens)
    );
    assert!(
        tokens.iter().any(|t| matches!(t, Token::Code { .. })),
        "expected indented code, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn tab_after_blockquote_marker_is_content_padding() {
    // `>\tfoo` — tab after `>` is content-side padding, so the body is
    // paragraph "foo", not indented code.
    let tokens = parse(">\tfoo");
    if let Token::BlockQuote(body) = &tokens[0] {
        let text = Token::collect_all_text(body);
        assert!(text.contains("foo"), "got {:?}", text);
        // The quote body should NOT contain a code block.
        assert!(
            !body.iter().any(|t| matches!(t, Token::Code { .. })),
            "unexpected code in quote body: {}",
            Token::slice_to_compact(body)
        );
    } else {
        panic!(
            "expected BlockQuote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }
}

#[test]
fn tab_after_list_marker_is_content_padding() {
    // `-\tfoo` — tab after the bullet is item-content padding, content="foo".
    let tokens = parse("-\tfoo");
    if let Token::ListItem { content, .. } = &tokens[0] {
        let text = Token::collect_all_text(content);
        assert!(text.contains("foo"), "got {:?}", text);
    } else {
        panic!(
            "expected ListItem, got {}",
            Token::slice_to_compact(&tokens)
        );
    }
}

#[test]
fn four_spaces_is_indented_code() {
    let tokens = parse("    foo");
    assert!(
        tokens
            .iter()
            .any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
        "expected indented code, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn three_spaces_no_tab_is_paragraph() {
    // 3 spaces, no tab → only 3 columns of indent → still a paragraph.
    let tokens = parse("   foo");
    assert!(
        !tokens.iter().any(|t| matches!(t, Token::Code { .. })),
        "unexpected code, got {}",
        Token::slice_to_compact(&tokens)
    );
}

#[test]
fn tab_inside_paragraph_preserved() {
    // A tab not at line start is just literal text content.
    let tokens = parse("a\tb");
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("a"), "got {:?}", text);
    assert!(text.contains("b"), "got {:?}", text);
}

// Tab-expansion edge cases (T10): each must parse without error
// (the `parse` helper unwraps) and produce the expected structure.

#[test]
fn tab_as_unordered_list_marker_separator() {
    let tokens = parse("-\tcode line");
    assert!(
        matches!(tokens[0], Token::ListItem { ordered: false, .. }),
        "got {:?}",
        tokens
    );
}

#[test]
fn tab_as_ordered_list_marker_separator() {
    let tokens = parse("1.\titem");
    if let Token::ListItem {
        ordered, number, ..
    } = &tokens[0]
    {
        assert!(*ordered);
        assert_eq!(*number, Some(1));
    } else {
        panic!("got {:?}", tokens);
    }
}

#[test]
fn tab_after_blockquote_marker() {
    let tokens = parse(">\tquoted");
    assert!(
        matches!(tokens[0], Token::BlockQuote(_)),
        "got {:?}",
        tokens
    );
    assert!(Token::collect_all_text(&tokens).contains("quoted"));
}

#[test]
fn lone_tab_indent_is_code_block() {
    let tokens = parse("\tcode");
    assert!(
        matches!(tokens[0], Token::Code { block: true, .. }),
        "got {:?}",
        tokens
    );
}

#[test]
fn mixed_tab_and_space_list_continuation() {
    // Must not error; the continuation line is folded into the item.
    let tokens = parse("- a\n \tb");
    assert!(
        matches!(tokens[0], Token::ListItem { .. }),
        "got {:?}",
        tokens
    );
    let text = Token::collect_all_text(&tokens);
    assert!(text.contains("a") && text.contains("b"), "got {text:?}");
}
