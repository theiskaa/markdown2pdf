//! Extended hard-line-break cases extending `hard_line_break_tests`.

use markdown2pdf::markdown::*;

use super::common::parse;

fn count_hard_breaks(tokens: &[Token]) -> usize {
    let mut n = 0;
    fn walk(tokens: &[Token], n: &mut usize) {
        for t in tokens {
            match t {
                Token::HardBreak => *n += 1,
                Token::Heading(body, _) => walk(body, n),
                Token::Emphasis { content, .. } => walk(content, n),
                Token::StrongEmphasis(body) => walk(body, n),
                Token::BlockQuote(body) => walk(body, n),
                Token::ListItem { content, .. } => walk(content, n),
                Token::Link { content, .. } => walk(content, n),
                Token::Image { alt, .. } => walk(alt, n),
                Token::Strikethrough(body) => walk(body, n),
                _ => {}
            }
        }
    }
    walk(tokens, &mut n);
    n
}

#[test]
fn two_trailing_spaces_creates_hard_break() {
    let tokens = parse("foo  \nbar\n");
    assert_eq!(count_hard_breaks(&tokens), 1);
}

#[test]
fn three_plus_trailing_spaces_still_hard_break() {
    let tokens = parse("foo   \nbar\n");
    assert_eq!(count_hard_breaks(&tokens), 1);
}

#[test]
fn backslash_at_eol_creates_hard_break() {
    let tokens = parse("foo\\\nbar\n");
    assert_eq!(count_hard_breaks(&tokens), 1);
}

#[test]
fn backslash_at_end_of_doc_is_not_hard_break() {
    // The trailing `\` with no following line is just literal text.
    let tokens = parse("foo\\");
    assert_eq!(count_hard_breaks(&tokens), 0);
}

#[test]
fn two_spaces_at_end_of_doc_is_not_hard_break() {
    let tokens = parse("foo  ");
    assert_eq!(count_hard_breaks(&tokens), 0);
}

#[test]
fn hard_break_inside_blockquote() {
    let tokens = parse("> foo  \n> bar\n");
    assert_eq!(count_hard_breaks(&tokens), 1);
}
