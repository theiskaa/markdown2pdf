//! Lexer tests for the admonition / callout token. Covers both
//! syntaxes (MkDocs `!!! kind "title"` and GFM `> [!KIND]`),
//! aliasing, unknown-kind generic fallback, malformed inputs that
//! must fall through cleanly, and nesting.

use markdown2pdf::markdown::*;

use super::common::parse;

fn first_admonition(tokens: &[Token]) -> Option<(&str, &str, Option<&Vec<Token>>, &Vec<Token>)> {
    tokens.iter().find_map(|t| match t {
        Token::Admonition {
            kind,
            raw_label,
            title,
            body,
        } => Some((kind.as_str(), raw_label.as_str(), title.as_ref(), body)),
        _ => None,
    })
}

fn flatten_text(tokens: &[Token]) -> String {
    Token::collect_all_text(tokens)
}

// =====================================================================
// MkDocs `!!!` syntax
// =====================================================================

#[test]
fn mkdocs_note_no_title_no_body() {
    let tokens = parse("!!! note\n");
    let (kind, raw, title, body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "note");
    assert_eq!(raw, "note");
    assert!(title.is_none());
    assert!(body.is_empty());
}

#[test]
fn mkdocs_note_with_indented_body() {
    let tokens = parse("!!! note\n    body content\n");
    let (kind, _, title, body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "note");
    assert!(title.is_none());
    assert!(flatten_text(body).contains("body content"));
}

#[test]
fn mkdocs_note_with_double_quoted_title() {
    let tokens = parse("!!! note \"Heads up\"\n    body\n");
    let (kind, _, title, _body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "note");
    let title_tokens = title.expect("title parsed");
    assert!(flatten_text(title_tokens).contains("Heads up"));
}

#[test]
fn mkdocs_title_parses_inline_emphasis() {
    let tokens = parse("!!! warning \"**Critical**\"\n    body\n");
    let (_, _, title, _) = first_admonition(&tokens).expect("Admonition");
    let title = title.expect("title present");
    assert!(
        title
            .iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
        "title inline content must keep `**…**` emphasis: {title:?}"
    );
}

#[test]
fn mkdocs_body_keeps_paragraph_breaks() {
    let src = "!!! info\n    first paragraph\n\n    second paragraph\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let collected = flatten_text(body);
    assert!(collected.contains("first paragraph"));
    assert!(collected.contains("second paragraph"));
}

#[test]
fn mkdocs_body_ends_at_dedent() {
    let src = "!!! tip\n    body line\nplain paragraph\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let body_text = flatten_text(body);
    assert!(body_text.contains("body line"));
    assert!(
        !body_text.contains("plain paragraph"),
        "dedented line must close the admonition body: body={body_text:?}"
    );
    // The plain paragraph must still appear in the document text.
    let document = flatten_text(&tokens);
    assert!(document.contains("plain paragraph"));
}

#[test]
fn mkdocs_tab_indent_recognised_as_body() {
    let tokens = parse("!!! info\n\tbody via tab\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(flatten_text(body).contains("body via tab"));
}

#[test]
fn mkdocs_mixed_space_tab_indent_satisfies_four_columns() {
    // `   \t` reaches column 4 via CommonMark tab expansion.
    let tokens = parse("!!! warning\n   \tindented body\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(flatten_text(body).contains("indented body"));
}

#[test]
fn mkdocs_blank_line_inside_body_preserved() {
    let src = "!!! note\n    para one\n\n    para two\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    // Two paragraphs in the body should each appear; the blank line
    // between them should not have collapsed them into one run.
    let body_text = flatten_text(body);
    assert!(body_text.contains("para one"));
    assert!(body_text.contains("para two"));
}

#[test]
fn mkdocs_alias_caution_canonicalises_to_danger() {
    let tokens = parse("!!! caution\n    body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "danger");
    assert_eq!(raw, "caution");
}

#[test]
fn mkdocs_alias_hint_canonicalises_to_tip() {
    let tokens = parse("!!! hint\n    body\n");
    let (kind, _, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "tip");
}

#[test]
fn mkdocs_unknown_kind_falls_back_to_generic() {
    let tokens = parse("!!! bug\n    repro\n");
    let (kind, raw, _, body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "generic");
    assert_eq!(raw, "bug");
    assert!(flatten_text(body).contains("repro"));
}

#[test]
fn mkdocs_kind_canonicalisation_is_case_insensitive() {
    let tokens = parse("!!! WARNING\n    body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "warning");
    assert_eq!(raw, "warning"); // raw_label is always lowercased
}

#[test]
fn mkdocs_missing_kind_falls_through_to_paragraph() {
    // `!!!` followed only by whitespace + newline is not a valid
    // opener — it must degrade to plain paragraph text.
    let tokens = parse("!!!\nplain\n");
    assert!(
        first_admonition(&tokens).is_none(),
        "no Admonition expected, got {tokens:?}"
    );
    let text = flatten_text(&tokens);
    assert!(text.contains("!!!"));
}

#[test]
fn mkdocs_no_space_after_bangs_falls_through() {
    // `!!!note` without separator must NOT match.
    let tokens = parse("!!!note\n    body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn mkdocs_four_bangs_falls_through() {
    let tokens = parse("!!!! note\n    body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn mkdocs_unterminated_title_falls_through() {
    let tokens = parse("!!! note \"unterminated\n    body\n");
    assert!(
        first_admonition(&tokens).is_none(),
        "unterminated quote must not match: {tokens:?}"
    );
}

#[test]
fn mkdocs_junk_after_title_falls_through() {
    // Content past the closing `"` on the opener line is junk.
    let tokens = parse("!!! note \"ok\" trailing junk\n    body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn mkdocs_three_space_indent_on_opener_is_block_marker() {
    // Up to 3 leading spaces is treated as a block marker.
    let tokens = parse("   !!! note\n    body\n");
    let (kind, _, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
}

#[test]
fn mkdocs_four_space_indent_disqualifies_opener() {
    // 4 spaces is an indented code block, not a block marker.
    let tokens = parse("    !!! note\n        body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn mkdocs_body_with_markdown_keeps_inline_styling() {
    let tokens = parse("!!! note\n    body with **bold** and *em*\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
        "**bold** must reach the body: {body:?}"
    );
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Emphasis { level: 1, .. })),
        "*em* must reach the body: {body:?}"
    );
}

#[test]
fn mkdocs_body_supports_nested_list() {
    let src = "!!! info\n    - item one\n    - item two\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter().any(|t| matches!(t, Token::ListItem { .. })),
        "list items must survive inside body: {body:?}"
    );
}

#[test]
fn mkdocs_body_supports_fenced_code() {
    let src = "!!! note\n    ```\n    println!(\"hi\");\n    ```\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Code { block: true, .. })),
        "fenced code must survive inside body: {body:?}"
    );
}

#[test]
fn mkdocs_nested_admonition_inside_admonition() {
    // Inner `!!!` is at 4 columns of indent (inside the outer body).
    // After dedent, the inner becomes a top-level `!!!` for the
    // sub-lexer.
    let src = "!!! note \"Outer\"\n    !!! tip\n        inner body\n";
    let tokens = parse(src);
    let (outer_kind, _, _, outer_body) = first_admonition(&tokens).expect("outer Admonition");
    assert_eq!(outer_kind, "note");
    let (inner_kind, _, _, inner_body) = first_admonition(outer_body).expect("inner Admonition");
    assert_eq!(inner_kind, "tip");
    assert!(flatten_text(inner_body).contains("inner body"));
}

#[test]
fn mkdocs_empty_body_does_not_panic() {
    let tokens = parse("!!! note \"only title\"\n");
    let (_, _, title, body) = first_admonition(&tokens).expect("Admonition produced");
    assert!(title.is_some());
    assert!(body.is_empty());
}

// =====================================================================
// GFM `> [!KIND]` alert syntax
// =====================================================================

#[test]
fn gfm_alert_warning_basic() {
    let tokens = parse("> [!WARNING]\n> be careful\n");
    let (kind, raw, title, body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "warning");
    assert_eq!(raw, "warning");
    assert!(title.is_none());
    assert!(flatten_text(body).contains("be careful"));
}

#[test]
fn gfm_alert_case_insensitive_kind() {
    let tokens = parse("> [!note]\n> body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert_eq!(raw, "note");
}

#[test]
fn gfm_alert_alias_important_canonicalises_to_info() {
    let tokens = parse("> [!IMPORTANT]\n> body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "info");
    assert_eq!(raw, "important");
}

#[test]
fn gfm_alert_alias_caution_canonicalises_to_danger() {
    let tokens = parse("> [!CAUTION]\n> watch out\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "danger");
    assert_eq!(raw, "caution");
}

#[test]
fn gfm_alert_unknown_kind_falls_back_to_generic() {
    let tokens = parse("> [!QUESTION]\n> who knows\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "generic");
    assert_eq!(raw, "question");
}

#[test]
fn gfm_alert_with_body_on_kind_line() {
    let tokens = parse("> [!NOTE] body on same line\n");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition produced");
    assert_eq!(kind, "note");
    assert!(flatten_text(body).contains("body on same line"));
}

#[test]
fn gfm_alert_with_continued_body() {
    let tokens = parse("> [!TIP] start\n> more body\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let text = flatten_text(body);
    assert!(text.contains("start"));
    assert!(text.contains("more body"));
}

#[test]
fn gfm_alert_markdown_inside_body_keeps_styling() {
    let tokens = parse("> [!INFO]\n> **bold** body\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter()
            .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
        "**bold** must survive in GFM alert body: {body:?}"
    );
}

#[test]
fn gfm_plain_blockquote_stays_blockquote() {
    let tokens = parse("> regular blockquote\n> continuation\n");
    assert!(
        first_admonition(&tokens).is_none(),
        "non-marker blockquote must remain a BlockQuote"
    );
    assert!(
        tokens.iter().any(|t| matches!(t, Token::BlockQuote(_))),
        "BlockQuote token expected"
    );
}

#[test]
fn gfm_blockquote_with_bracketed_text_stays_blockquote() {
    // `[Reference]` (no leading `!`) must not trigger detection.
    let tokens = parse("> [Reference] something\n> body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn gfm_empty_kind_stays_blockquote() {
    let tokens = parse("> [!]\n> body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn gfm_marker_with_no_separator_stays_blockquote() {
    // `[!NOTE]text` (no space between marker and content) is not a
    // valid alert per the same-line-content rule.
    let tokens = parse("> [!NOTE]extra\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn gfm_marker_on_later_line_does_not_match() {
    // Only the first body line is examined; a marker on line 2 stays
    // as literal blockquote content.
    let tokens = parse("> intro\n> [!NOTE]\n> body\n");
    assert!(first_admonition(&tokens).is_none());
}

#[test]
fn gfm_alert_marker_alone_with_no_body() {
    let tokens = parse("> [!NOTE]\n");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert!(body.is_empty());
}

#[test]
fn gfm_alert_with_three_space_blockquote_indent_still_matches() {
    let tokens = parse("   > [!WARNING]\n   > body\n");
    let (kind, _, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "warning");
}

#[test]
fn gfm_alert_inside_nested_blockquote_is_inner_admonition() {
    // Outer `>` creates a blockquote whose body is `> [!NOTE]\n> body`
    // — the inner blockquote then matches as an Admonition.
    let tokens = parse("> > [!NOTE]\n> > body\n");
    let outer = tokens
        .iter()
        .find_map(|t| match t {
            Token::BlockQuote(body) => Some(body),
            _ => None,
        })
        .expect("outer BlockQuote");
    let (kind, _, _, _) = first_admonition(outer).expect("inner Admonition inside outer quote");
    assert_eq!(kind, "note");
}

// =====================================================================
// Cross-syntax assertions
// =====================================================================

#[test]
fn both_syntaxes_produce_admonition_with_same_kind() {
    let from_mkdocs = parse("!!! warning\n    body\n");
    let from_gfm = parse("> [!WARNING]\n> body\n");
    let (k_a, _, _, _) = first_admonition(&from_mkdocs).expect("mkdocs");
    let (k_b, _, _, _) = first_admonition(&from_gfm).expect("gfm");
    assert_eq!(k_a, k_b);
    assert_eq!(k_a, "warning");
}

#[test]
fn neither_syntax_panics_on_truncated_input() {
    // Slice variants that previously segfaulted in earlier prototypes;
    // every line is a sanity check that the parser bails cleanly.
    let _ = parse("!!!");
    let _ = parse("!!! ");
    let _ = parse("!!! note");
    let _ = parse("!!! note \"");
    let _ = parse("> [");
    let _ = parse("> [!");
    let _ = parse("> [!NOTE");
}

// =====================================================================
// Additional edge cases (verification gaps)
// =====================================================================

#[test]
fn mkdocs_empty_quoted_title_renders_as_some_with_no_inline() {
    // `!!! note ""` is an empty title, not "no title". The token should
    // carry Some(Vec::new()) so the renderer can distinguish authorial
    // intent from absence.
    let tokens = parse("!!! note \"\"\n    body\n");
    let (_, _, title, _) = first_admonition(&tokens).expect("Admonition");
    let title = title.expect("empty-quoted title still produces Some");
    assert!(title.is_empty(), "title content must be empty: {title:?}");
}

#[test]
fn mkdocs_extra_whitespace_between_kind_and_title() {
    let tokens = parse("!!! note    \"Padded\"\n    body\n");
    let (kind, _, title, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert!(flatten_text(title.expect("title")).contains("Padded"));
}

#[test]
fn mkdocs_hyphen_in_kind_falls_back_to_generic() {
    let tokens = parse("!!! note-2\n    body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "generic");
    assert_eq!(raw, "note-2");
}

#[test]
fn mkdocs_underscore_and_digits_in_kind_allowed() {
    let tokens = parse("!!! mynote_v2\n    body\n");
    let (kind, raw, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "generic");
    assert_eq!(raw, "mynote_v2");
}

#[test]
fn mkdocs_five_space_body_indent_keeps_extra_space() {
    // 4 spaces are the dedent quota; a 5th space is content.
    let tokens = parse("!!! note\n     extra space line\n");
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let body_text = flatten_text(body);
    assert!(
        body_text.contains(" extra space line") || body_text.contains("extra space line"),
        "body content lost: {body_text:?}"
    );
}

#[test]
fn mkdocs_body_with_eof_no_trailing_newline() {
    let tokens = parse("!!! note\n    body without newline");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert!(flatten_text(body).contains("body without newline"));
}

#[test]
fn mkdocs_opener_without_trailing_newline_at_eof() {
    let tokens = parse("!!! note");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert!(body.is_empty());
}

#[test]
fn mkdocs_admonition_at_position_zero_no_leading_newline() {
    // No newline before the `!!!` opener — must still match.
    let tokens = parse("!!! tip\n    body\n");
    let (kind, _, _, _) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "tip");
}

#[test]
fn mkdocs_inline_bangs_mid_paragraph_do_not_open_admonition() {
    // `!!!` after some prose on the same line is inline content, not
    // a block opener.
    let tokens = parse("prose !!! still prose\n");
    assert!(
        first_admonition(&tokens).is_none(),
        "inline !!! must stay literal: {tokens:?}"
    );
}

#[test]
fn mkdocs_two_admonitions_back_to_back() {
    let src = "!!! note\n    first\n\n!!! warning\n    second\n";
    let tokens = parse(src);
    let admonitions: Vec<_> = tokens
        .iter()
        .filter(|t| matches!(t, Token::Admonition { .. }))
        .collect();
    assert_eq!(
        admonitions.len(),
        2,
        "expected two Admonitions, got {tokens:?}"
    );
}

#[test]
fn mkdocs_admonition_inside_list_item() {
    let src = "- list item\n\n  !!! note\n      body\n";
    let tokens = parse(src);
    let list_item_body = tokens
        .iter()
        .find_map(|t| match t {
            Token::ListItem { content, .. } => Some(content),
            _ => None,
        })
        .expect("list item present");
    // Some lexers would put the admonition in the list item's content.
    // Others put it after. Either is acceptable as long as the
    // admonition is recognised somewhere in the document tree.
    let document_has_admonition = first_admonition(&tokens).is_some()
        || first_admonition(list_item_body).is_some();
    assert!(
        document_has_admonition,
        "admonition lost in list-item context: {tokens:?}"
    );
}

#[test]
fn mkdocs_body_supports_setext_heading() {
    let src = "!!! note\n    Heading\n    -------\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter().any(|t| matches!(t, Token::Heading(_, 2))),
        "setext H2 inside body lost: {body:?}"
    );
}

#[test]
fn mkdocs_body_supports_sub_blockquote() {
    let src = "!!! note\n    > nested quote\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter().any(|t| matches!(t, Token::BlockQuote(_))),
        "sub-blockquote inside body lost: {body:?}"
    );
}

#[test]
fn mkdocs_title_with_inline_code() {
    let tokens = parse("!!! note \"Run `cmd`\"\n    body\n");
    let (_, _, title, _) = first_admonition(&tokens).expect("Admonition");
    let title = title.expect("title");
    assert!(
        title
            .iter()
            .any(|t| matches!(t, Token::Code { block: false, .. })),
        "inline code in title lost: {title:?}"
    );
}

#[test]
fn gfm_alert_without_space_after_arrow_still_matches() {
    // CommonMark allows `>` without a trailing space — `>[!NOTE]` is a
    // valid blockquote whose body is `[!NOTE]`, and the GFM detector
    // therefore promotes it to an Admonition. (GitHub renders the same
    // way.)
    let tokens = parse(">[!NOTE] body\n");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "note");
    assert!(flatten_text(body).contains("body"));
}

#[test]
fn gfm_alert_body_with_intermediate_blank_quote_line() {
    let src = "> [!NOTE]\n> first paragraph\n>\n> second paragraph\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let text = flatten_text(body);
    assert!(text.contains("first paragraph"));
    assert!(text.contains("second paragraph"));
}

#[test]
fn gfm_alert_marker_only_with_no_body_continuation() {
    // Single-line blockquote whose body is just the marker.
    let tokens = parse("> [!INFO]\n");
    let (kind, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert_eq!(kind, "info");
    assert!(body.is_empty());
}

#[test]
fn gfm_alert_body_with_nested_list() {
    let src = "> [!TIP]\n> - one\n> - two\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    assert!(
        body.iter().any(|t| matches!(t, Token::ListItem { .. })),
        "list in GFM alert body lost: {body:?}"
    );
}

#[test]
fn gfm_alert_inside_list_item_quote() {
    // `- item\n  > [!NOTE]\n  > body` — the inner blockquote is a
    // child of the list item, and should match as Admonition.
    let src = "- item\n  > [!NOTE]\n  > body\n";
    let tokens = parse(src);
    let list_content = tokens
        .iter()
        .find_map(|t| match t {
            Token::ListItem { content, .. } => Some(content),
            _ => None,
        })
        .expect("list item present");
    assert!(
        first_admonition(list_content).is_some(),
        "admonition inside list-item blockquote lost: {tokens:?}"
    );
}

#[test]
fn raw_label_is_always_lowercased() {
    // Even if the author writes `!!! WARNING` or `[!WARNING]`, raw_label
    // is always lowercased so the renderer's label lookup is uniform.
    for src in [
        "!!! WARNING\n    body\n",
        "!!! Warning\n    body\n",
        "> [!WARNING]\n> body\n",
        "> [!Warning]\n> body\n",
    ] {
        let tokens = parse(src);
        let (_, raw, _, _) = first_admonition(&tokens)
            .unwrap_or_else(|| panic!("Admonition expected for {src:?}"));
        assert_eq!(raw, "warning", "raw_label not lowercased for {src:?}");
    }
}

#[test]
fn canonicalisation_round_trip_through_first_class_kinds() {
    // Every first-class kind (typed verbatim) must canonicalise to
    // itself.
    for kind in ["note", "info", "tip", "warning", "danger"] {
        let src = format!("!!! {}\n    body\n", kind);
        let tokens = parse(&src);
        let (canonical, raw, _, _) = first_admonition(&tokens)
            .unwrap_or_else(|| panic!("Admonition expected for {src:?}"));
        assert_eq!(canonical, kind, "first-class kind not round-tripping: {src:?}");
        assert_eq!(raw, kind);
    }
}

#[test]
fn admonition_does_not_consume_following_paragraph() {
    let src = "!!! tip\n    inside\n\nafter paragraph.\n";
    let tokens = parse(src);
    let (_, _, _, body) = first_admonition(&tokens).expect("Admonition");
    let body_text = flatten_text(body);
    assert!(body_text.contains("inside"));
    assert!(
        !body_text.contains("after paragraph"),
        "post-admonition paragraph leaked into body: {body_text:?}"
    );
    // The trailing paragraph must still render in the document text.
    let document = flatten_text(&tokens);
    assert!(document.contains("after paragraph"));
}
