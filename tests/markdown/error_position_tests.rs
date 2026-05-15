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
    let eof = LexerError::UnexpectedEndOfInput { line: 1, column: 1 };
    let unk = LexerError::UnknownToken {
        message: "x".to_string(),
        line: 2,
        column: 3,
    };
    assert_eq!(eof.position(), (1, 1));
    assert_eq!(unk.position(), (2, 3));
}

#[test]
fn lexer_error_display_includes_line_col() {
    let unk = LexerError::UnknownToken {
        message: "Unexpected character".to_string(),
        line: 5,
        column: 12,
    };
    let s = format!("{}", unk);
    assert!(
        s.contains("line 5") && s.contains("column 12"),
        "display string missing line/column: {}",
        s
    );
}
