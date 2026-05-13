use markdown2pdf::markdown::*;

#[test]
fn tab_at_column_one_is_four_spaces() {
    let lexer = Lexer::new("\tx".to_string());
    assert_eq!(lexer.get_current_indent(), 4);
}

#[test]
fn two_spaces_then_tab_is_four_columns() {
    // 2 spaces + \t → tab fills to next column-4 boundary, total = 4.
    let lexer = Lexer::new("  \tx".to_string());
    assert_eq!(lexer.get_current_indent(), 4);
}

#[test]
fn three_spaces_then_tab_is_four_columns() {
    // 3 spaces + \t → tab fills col 4 only, total = 4.
    let lexer = Lexer::new("   \tx".to_string());
    assert_eq!(lexer.get_current_indent(), 4);
}

#[test]
fn one_space_then_tab_is_four_columns() {
    let lexer = Lexer::new(" \tx".to_string());
    assert_eq!(lexer.get_current_indent(), 4);
}

#[test]
fn two_tabs_is_eight_columns() {
    let lexer = Lexer::new("\t\tx".to_string());
    assert_eq!(lexer.get_current_indent(), 8);
}

#[test]
fn tab_then_spaces() {
    // \t + 2 spaces → 4 + 2 = 6
    let lexer = Lexer::new("\t  x".to_string());
    assert_eq!(lexer.get_current_indent(), 6);
}
