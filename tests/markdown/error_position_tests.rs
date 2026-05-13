use markdown2pdf::markdown::*;

#[test]
fn error_message_uses_line_and_column() {
    let lexer = Lexer::new("a\nb\nc".to_string());
    let (line, col) = lexer.pos_to_line_col(4);
    assert_eq!(line, 3);
    assert_eq!(col, 1);
}

#[test]
fn error_reports_correct_line() {
    let lexer = Lexer::new("first\nsecond\nthird".to_string());
    let pos = "first\nsecond\n".len();
    let (line, col) = lexer.pos_to_line_col(pos);
    assert_eq!(line, 3);
    assert_eq!(col, 1);
}

#[test]
fn lexer_error_variants_exist() {
    // Smoke-test that the LexerError enum still has its variants —
    // future code may surface `UnexpectedEndOfInput` for other inputs
    // even though the unclosed HTML comment now falls back to text.
    let _ = LexerError::UnexpectedEndOfInput;
    let _ = LexerError::UnknownToken("x".to_string());
}
