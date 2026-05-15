use super::common::parse;

#[test]
fn crlf_paragraph_then_heading() {
    let lf = parse("first line\n# Heading");
    let crlf = parse("first line\r\n# Heading");
    assert_eq!(lf, crlf);
}

#[test]
fn crlf_blockquote_continuation() {
    let lf = parse("> first\n> second");
    let crlf = parse("> first\r\n> second");
    assert_eq!(lf, crlf);
}

#[test]
fn crlf_setext_heading() {
    let lf = parse("Title\n===");
    let crlf = parse("Title\r\n===");
    assert_eq!(lf, crlf);
}

#[test]
fn crlf_thematic_break() {
    let lf = parse("Para\n\n---\n\nBody");
    let crlf = parse("Para\r\n\r\n---\r\n\r\nBody");
    assert_eq!(lf, crlf);
}

#[test]
fn bare_cr_old_mac_normalized() {
    let lf = parse("first\nsecond");
    let cr = parse("first\rsecond");
    assert_eq!(lf, cr);
}

#[test]
fn mixed_line_endings_in_one_doc() {
    let mixed = parse("# A\r\nbody one\nbody two\rbody three");
    let lf = parse("# A\nbody one\nbody two\nbody three");
    assert_eq!(mixed, lf);
}

// CommonMark 0.31.2 §2.3: U+0000 must be treated as U+FFFD.

#[test]
fn nul_becomes_replacement_char_in_text() {
    assert_eq!(parse("a\u{0}b"), parse("a\u{FFFD}b"));
}

#[test]
fn nul_becomes_replacement_char_in_code_span() {
    assert_eq!(parse("`a\u{0}b`"), parse("`a\u{FFFD}b`"));
}

#[test]
fn lone_nul_is_replacement_char() {
    assert_eq!(parse("\u{0}"), parse("\u{FFFD}"));
}

#[test]
fn nul_not_kept_verbatim() {
    // Guard against a regression that passes NUL through unchanged.
    let joined = format!("{:?}", parse("x\u{0}y"));
    assert!(
        !joined.contains('\u{0}'),
        "NUL leaked into tokens: {joined:?}"
    );
}
