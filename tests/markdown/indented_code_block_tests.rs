use markdown2pdf::markdown::*;

use super::common::parse;

#[test]
fn four_space_indented_line_is_code() {
    let tokens = parse("    let x = 5;");
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "let x = 5;".to_string(),
            block: true,
        }]
    );
}

#[test]
fn tab_indent_is_code() {
    let tokens = parse("\tlet x = 5;");
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "let x = 5;".to_string(),
            block: true,
        }]
    );
}

#[test]
fn three_spaces_is_not_code() {
    // 3 spaces is not enough; should be regular paragraph text.
    let tokens = parse("   not code");
    let body = Token::collect_all_text(&tokens);
    assert_eq!(body, "not code");
    assert!(!tokens.iter().any(|t| matches!(t, Token::Code { .. })));
}

#[test]
fn multi_line_indented_code() {
    let input = "    fn main() {\n        println!(\"hi\");\n    }";
    let tokens = parse(input);
    if let Token::Code { content: body, .. } = &tokens[0] {
        assert!(body.contains("fn main()"), "got {:?}", body);
        assert!(body.contains("println!"), "got {:?}", body);
    } else {
        panic!("expected Code, got {:?}", tokens);
    }
}

#[test]
fn indented_code_inside_paragraph_does_not_apply() {
    // Indented line directly after a paragraph is treated as paragraph
    // continuation not code. We're more permissive: it
    // becomes code if separated by a blank line. Test the blank-line case.
    let input = "Some paragraph\n\n    code line";
    let tokens = parse(input);
    assert!(tokens.iter().any(|t| matches!(t, Token::Code { .. })));
}

#[test]
fn fenced_code_block_unaffected() {
    let input = "```\nfn main() {}\n```";
    let tokens = parse(input);
    assert_eq!(
        tokens,
        vec![Token::Code {
            language: "".to_string(),
            content: "fn main() {}".to_string(),
            block: true,
        }]
    );
}

#[test]
fn list_item_four_space_indent_is_nesting_not_code() {
    // 4 spaces under a list bullet is list-item continuation/nesting,
    // NOT an indented code block.
    let input = "- item one\n    nested\n- item two";
    let tokens = parse(input);
    let li_count = tokens
        .iter()
        .filter(|t| matches!(t, Token::ListItem { .. }))
        .count();
    assert!(
        li_count >= 2,
        "expected at least 2 list items, got {:?}",
        tokens
    );
    assert!(!tokens.iter().any(|t| matches!(t, Token::Code { .. })));
}
