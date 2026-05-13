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
