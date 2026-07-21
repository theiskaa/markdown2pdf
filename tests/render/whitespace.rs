//! Whitespace & line-break correctness (W7f). Asserts the rendered
//! PDF preserves the right break/no-break behavior for spaces, hard
//! breaks, non-breaking spaces, and control whitespace.
//!
//! Helpers extract the `(...) Tj` show-text operands and the `Td`
//! text-position ops from the content stream so we can reason about
//! how many lines the text occupied and where breaks landed.

use super::common::*;

/// Render `md` with `cfg` (embedded TOML) and return each `Tj`
/// show-text operand in document order. `render` returns the
/// stream-expanded PDF — the renderer Flate-compresses content, so
/// the operators are only visible after expansion.
fn show_text_lines(md: &str, cfg: &str) -> Vec<String> {
    let bytes = render(md, cfg);
    let s = String::from_utf8_lossy(&bytes);
    s.lines()
        .map(|l| l.trim())
        .filter(|l| l.ends_with(") Tj"))
        .map(|l| {
            // strip the leading `(` and trailing `) Tj`
            l[1..l.len() - 4].to_string()
        })
        .collect()
}

/// Count `Td` text-position ops — a proxy for "number of laid-out
/// lines / segments".
fn td_count(md: &str, cfg: &str) -> usize {
    let bytes = render(md, cfg);
    let s = String::from_utf8_lossy(&bytes);
    s.lines().filter(|l| l.trim().ends_with(" Td")).count()
}

mod hard_breaks {
    use super::*;

    #[test]
    fn two_trailing_spaces_breaks_the_line() {
        // `line one  \nline two` is a CommonMark hard break: two
        // distinct laid-out lines, not one joined line.
        let lines = show_text_lines("line one  \nline two", "");
        assert!(
            lines.iter().any(|l| l.contains("line one"))
                && lines.iter().any(|l| l.contains("line two")),
            "hard-break content lost: {:?}",
            lines
        );
        assert!(
            !lines.iter().any(|l| l.contains("line one line two")),
            "two-space hard break did NOT split the line: {:?}",
            lines
        );
    }

    #[test]
    fn backslash_breaks_the_line() {
        let lines = show_text_lines("line one\\\nline two", "");
        assert!(
            !lines.iter().any(|l| l.contains("line one line two")),
            "backslash hard break did NOT split the line: {:?}",
            lines
        );
    }

    #[test]
    fn soft_break_joins_with_space() {
        // A single `\n` (no trailing spaces / backslash) is a soft
        // break: the two lines join into one flow with a space.
        let lines = show_text_lines("line one\nline two", "");
        let joined = lines.join("");
        assert!(
            joined.contains("line one line two"),
            "soft break should join with a single space: {:?}",
            lines
        );
    }
}

mod non_breaking_space {
    use super::*;

    // Narrow column + big font so the wrap is forced; tuned so a
    // line holds about two 9-char tokens.
    const NARROW: &str = "[page]\nsize = { width_mm = 70.0, height_mm = 200.0 }\nmargins = { top = 5.0, right = 5.0, bottom = 5.0, left = 5.0 }\n[paragraph]\nfont_size_pt = 20.0\n";

    #[test]
    fn nbsp_pair_does_not_break_where_a_space_would() {
        let space_lines = show_text_lines("AAAAAAAAA BBBBBBBBB CCCCCCCCC DDDDDDDDD", NARROW);
        let nbsp_lines = show_text_lines(
            "AAAAAAAAA\u{00A0}BBBBBBBBB CCCCCCCCC\u{00A0}DDDDDDDDD",
            NARROW,
        );
        // With regular spaces every 9-char token wraps to its own
        // line. With nbsp, each glued pair stays together so a line
        // carries the *combined* (overflowing) pair instead of a
        // single token — the two layouts MUST differ.
        assert_ne!(
            space_lines, nbsp_lines,
            "nbsp produced the same breaks as a regular space — \
             non-breaking space not respected"
        );
    }

    #[test]
    fn nbsp_entity_behaves_like_literal_nbsp() {
        let from_entity = show_text_lines("A&nbsp;B", "");
        let from_literal = show_text_lines("A\u{00A0}B", "");
        assert_eq!(
            from_entity, from_literal,
            "`&nbsp;` and U+00A0 must render identically"
        );
    }

    #[test]
    fn nbsp_renders_as_a_space_glyph() {
        // It should *look* like a space (not vanish, not a `?`).
        let lines = show_text_lines("A\u{00A0}B", "");
        let joined = lines.join("");
        assert!(
            joined.contains("A B"),
            "nbsp should render as a visible space: {:?}",
            lines
        );
    }

    #[test]
    fn narrow_nbsp_and_figure_space_also_non_breaking() {
        // U+202F (narrow NBSP) and U+2007 (figure space) join too.
        let space = show_text_lines(
            "AAAAAAAAA BBBBBBBBB CCCCCCCCC DDDDDDDDD",
            super::non_breaking_space::NARROW,
        );
        let narrow = show_text_lines(
            "AAAAAAAAA\u{202F}BBBBBBBBB CCCCCCCCC\u{2007}DDDDDDDDD",
            super::non_breaking_space::NARROW,
        );
        assert_ne!(
            space, narrow,
            "U+202F / U+2007 must be treated as non-breaking"
        );
    }
}

mod space_collapsing {
    use super::*;

    #[test]
    fn interior_multiple_spaces_are_preserved_in_one_run() {
        // Documents current behavior: the renderer keeps the author's
        // interior spaces in a single show-text run (it does not
        // HTML-collapse them). Regression guard — if we ever choose
        // to collapse, this test is the deliberate breaking point.
        let lines = show_text_lines("a      b", "");
        assert!(
            lines.iter().any(|l| l.contains("a      b")),
            "interior multi-space run not preserved as-is: {:?}",
            lines
        );
    }

    #[test]
    fn trailing_whitespace_does_not_crash_or_corrupt() {
        // Trailing spaces at EOF are invisible in the PDF anyway;
        // the contract is just "valid PDF, content intact".
        let bytes = render("word          ", "");
        assert!(pdf_well_formed(&bytes));
        assert!(contains(&bytes, b"word"));
    }
}

mod control_whitespace {
    use super::*;

    #[test]
    fn zero_width_space_does_not_crash() {
        let bytes = render("wide\u{200B}word\u{200B}here", "");
        assert!(pdf_well_formed(&bytes));
    }

    #[test]
    fn zero_width_joiner_sequence_does_not_crash() {
        let bytes = render("a\u{200D}b", "");
        assert!(pdf_well_formed(&bytes));
    }

    #[test]
    fn crlf_line_endings_treated_like_lf() {
        let crlf = show_text_lines("para one\r\n\r\npara two", "");
        let lf = show_text_lines("para one\n\npara two", "");
        assert_eq!(crlf, lf, "CRLF should produce the same layout as LF");
    }

    #[test]
    fn line_and_paragraph_separators_do_not_crash() {
        let bytes = render("before\u{2028}mid\u{2029}after", "");
        assert!(pdf_well_formed(&bytes));
    }

    #[test]
    fn tabs_in_body_text_do_not_crash() {
        let bytes = render("col a\tcol b\tcol c", "");
        assert!(pdf_well_formed(&bytes));
    }
}

mod regression_guards {
    use super::*;

    #[test]
    fn plain_paragraph_still_one_line() {
        // Sanity: a short paragraph that fits is still a single
        // laid-out line (the nbsp predicate change must not have
        // altered ordinary wrapping).
        assert_eq!(td_count("hello world", ""), 1);
    }

    #[test]
    fn ordinary_spaces_still_break_normally() {
        let narrow = "[page]\nsize = { width_mm = 60.0, height_mm = 200.0 }\nmargins = { top = 5.0, right = 5.0, bottom = 5.0, left = 5.0 }\n[paragraph]\nfont_size_pt = 18.0\n";
        let lines = show_text_lines("alpha bravo charlie delta echo foxtrot", narrow);
        assert!(
            lines.len() >= 2,
            "ordinary spaces should still allow wrapping: {:?}",
            lines
        );
    }
}
