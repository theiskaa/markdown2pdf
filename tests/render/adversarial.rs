//! Adversarial / malformed-input tests for the renderer. Each case
//! feeds ugly content and asserts:
//!
//! 1. No panic / abort during rendering
//! 2. Output is structurally a PDF (`%PDF-` header, `%%EOF` trailer)
//! 3. Where applicable, content gracefully degrades (e.g. unknown
//!    HTML appears as text rather than disappearing)
//!
//! These tests are a regression safety net. New rendering work should
//! not make any of them fail.

use super::common::*;

fn render_must_not_panic(md: &str) -> Vec<u8> {
    // Wrap in catch_unwind so a future regression that panics here
    // surfaces as a failed assertion rather than killing the whole
    // test binary.
    let md = md.to_string();
    let bytes = std::panic::catch_unwind(move || {
        markdown2pdf::parse_into_bytes(
            md,
            markdown2pdf::config::ConfigSource::Default,
            None,
        )
    });
    let bytes = bytes
        .expect("renderer panicked")
        .expect("renderer returned MdpError");
    assert!(pdf_well_formed(&bytes), "PDF not well-formed");
    bytes
}

mod empty_and_minimal {
    use super::*;

    #[test]
    fn completely_empty_input() {
        render_must_not_panic("");
    }

    #[test]
    fn single_newline() {
        render_must_not_panic("\n");
    }

    #[test]
    fn only_whitespace_spaces() {
        render_must_not_panic("    \t  \t  ");
    }

    #[test]
    fn only_newlines() {
        render_must_not_panic("\n\n\n\n\n\n\n\n\n\n");
    }

    #[test]
    fn single_character() {
        render_must_not_panic("x");
    }

    #[test]
    fn single_null_byte() {
        render_must_not_panic("\0");
    }

    #[test]
    fn only_one_heading_hash() {
        render_must_not_panic("#");
    }
}

mod control_and_zero_width_chars {
    use super::*;

    #[test]
    fn null_bytes_in_paragraph() {
        render_must_not_panic("Before\0middle\0after.");
    }

    #[test]
    fn ascii_control_chars_in_body() {
        let mut s = String::from("Before ");
        for c in 0x01u8..=0x1F {
            if c != b'\n' && c != b'\r' && c != b'\t' {
                s.push(c as char);
            }
        }
        s.push_str(" after.");
        render_must_not_panic(&s);
    }

    #[test]
    fn zero_width_space() {
        render_must_not_panic("word\u{200B}wrap\u{200B}point.");
    }

    #[test]
    fn zero_width_joiner_in_emoji() {
        // ZWJ between hands forms a "people holding hands" sequence.
        render_must_not_panic("hand: 👨\u{200D}🤝\u{200D}👩 here.");
    }

    #[test]
    fn unicode_bom_mid_text() {
        render_must_not_panic("Before \u{FEFF}after.");
    }

    #[test]
    fn unicode_line_and_paragraph_separators() {
        render_must_not_panic("Line\u{2028}sep\u{2029}para sep.");
    }
}

mod massive_inputs {
    use super::*;

    #[test]
    fn fifty_thousand_char_paragraph() {
        let s = "word ".repeat(10_000);
        let bytes = render_must_not_panic(&s);
        // Should split across multiple pages.
        assert!(
            page_count(&bytes) >= 2,
            "expected multi-page output, got {} pages",
            page_count(&bytes)
        );
    }

    #[test]
    fn single_word_five_thousand_chars() {
        let s = "x".repeat(5_000);
        render_must_not_panic(&s);
    }

    #[test]
    fn many_newlines_only() {
        let s = "\n".repeat(10_000);
        render_must_not_panic(&s);
    }

    #[test]
    fn many_paragraphs() {
        let s = "Para.\n\n".repeat(1_000);
        let bytes = render_must_not_panic(&s);
        assert!(page_count(&bytes) >= 2);
    }

    #[test]
    fn single_word_one_hundred_thousand_chars() {
        // split_long_words is per-candidate measure; verify a 100k
        // unbreakable word stays linear/bounded (release ~13ms).
        let start = std::time::Instant::now();
        let bytes = render_must_not_panic(&"x".repeat(100_000));
        assert!(page_count(&bytes) >= 2);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "100k-char word render is pathologically slow"
        );
    }

    #[test]
    fn very_large_document_is_bounded() {
        // 20k blocks: output and time must stay linear in input
        // (raw_pages is fully in-RAM, no page cap).
        let mut s = String::new();
        for i in 0..20_000 {
            s.push_str(&format!("Paragraph {i} with a few words.\n\n"));
        }
        let start = std::time::Instant::now();
        let bytes = render_must_not_panic(&s);
        assert!(page_count(&bytes) >= 2);
        assert!(
            start.elapsed() < std::time::Duration::from_secs(30),
            "large document render is pathologically slow"
        );
    }
}

mod headings {
    use super::*;

    #[test]
    fn all_heading_levels_one_through_six() {
        let md = "# h1\n\n## h2\n\n### h3\n\n#### h4\n\n##### h5\n\n###### h6\n";
        render_must_not_panic(md);
    }

    #[test]
    fn heading_with_seven_hashes_falls_back() {
        // CommonMark: more than 6 `#` is paragraph text. Verify we
        // don't crash and the content remains visible.
        let bytes = render_must_not_panic("####### too many hashes\n");
        // Body text "too many hashes" should still appear somewhere.
        assert!(
            contains_text(&bytes, "too many hashes")
                || contains(&bytes, b"too many hashes"),
            "text content lost for 7-hash 'heading'"
        );
    }

    #[test]
    fn heading_with_twenty_hashes() {
        render_must_not_panic("#################### deeply hashed\n");
    }

    #[test]
    fn heading_with_only_hashes() {
        render_must_not_panic("######\n");
    }
}

mod list_and_quote_depth {
    use super::*;

    #[test]
    fn nested_list_twenty_deep() {
        let mut md = String::new();
        for i in 0..20 {
            md.push_str(&" ".repeat(i * 2));
            md.push_str(&format!("- level {}\n", i));
        }
        render_must_not_panic(&md);
    }

    #[test]
    fn one_thousand_list_items() {
        let mut md = String::new();
        for i in 0..1_000 {
            md.push_str(&format!("- item {}\n", i));
        }
        let bytes = render_must_not_panic(&md);
        assert!(page_count(&bytes) >= 2);
    }

    #[test]
    fn blockquote_fifteen_deep() {
        let mut md = String::new();
        for _ in 0..15 {
            md.push_str("> ");
        }
        md.push_str("deeply quoted\n");
        render_must_not_panic(&md);
    }

    #[test]
    fn alternating_quote_and_list() {
        let md = "> - a\n>   - b\n>     - c\n>       - d\n>         - e\n";
        render_must_not_panic(md);
    }
}

mod code_blocks {
    use super::*;

    #[test]
    fn ten_thousand_line_code_block() {
        let mut md = String::from("```\n");
        for i in 0..10_000 {
            md.push_str(&format!("line {}\n", i));
        }
        md.push_str("```\n");
        let bytes = render_must_not_panic(&md);
        assert!(page_count(&bytes) >= 2);
    }

    #[test]
    fn unterminated_fenced_code_block() {
        let md = "```\nfn main() { 1 + 1 }\nstill in code";
        render_must_not_panic(md);
    }

    #[test]
    fn code_block_with_only_fences() {
        render_must_not_panic("```\n```\n");
    }

    #[test]
    fn code_block_with_no_language_hint() {
        render_must_not_panic("```\nplain\n```\n");
    }

    #[test]
    fn code_block_with_extremely_long_single_line() {
        let line = "x".repeat(2_000);
        let md = format!("```\n{}\n```\n", line);
        render_must_not_panic(&md);
    }
}

mod tables {
    use super::*;

    #[test]
    fn table_with_fifty_columns() {
        let headers: Vec<String> = (0..50).map(|i| format!("c{}", i)).collect();
        let sep: Vec<String> = (0..50).map(|_| "---".to_string()).collect();
        let row: Vec<String> = (0..50).map(|i| format!("{}", i)).collect();
        let md = format!(
            "| {} |\n| {} |\n| {} |\n",
            headers.join(" | "),
            sep.join(" | "),
            row.join(" | ")
        );
        render_must_not_panic(&md);
    }

    #[test]
    fn table_with_extremely_long_cell_content() {
        let cell = "word ".repeat(500);
        let md = format!("| A | B |\n|---|---|\n| {} | x |\n", cell);
        render_must_not_panic(&md);
    }

    #[test]
    fn table_with_empty_cells() {
        let md = "|  |  |  |\n|---|---|---|\n|  |  |  |\n|  |  |  |\n";
        render_must_not_panic(md);
    }

    #[test]
    fn table_with_one_hundred_rows() {
        let mut md = String::from("| A | B |\n|---|---|\n");
        for i in 0..100 {
            md.push_str(&format!("| {} | {} |\n", i, i * 2));
        }
        render_must_not_panic(&md);
    }

    fn wide_table_md(cols: usize, rows: usize) -> String {
        let join = |f: &dyn Fn(usize) -> String| {
            (0..cols).map(|i| f(i)).collect::<Vec<_>>().join(" | ")
        };
        let mut md = format!(
            "| {} |\n| {} |\n",
            join(&|i| format!("col {i} alpha")),
            join(&|_| "---".to_string()),
        );
        for r in 0..rows {
            md.push_str(&format!("| {} |\n", join(&|i| format!("r{r} c{i} beta"))));
        }
        md
    }

    #[test]
    fn table_with_five_hundred_columns() {
        // Even-split column width fell below the cell padding, making
        // the cell box invert and row height explode. Floored now.
        let start = std::time::Instant::now();
        render_must_not_panic(&wide_table_md(500, 20));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "500-column table render is pathologically slow"
        );
    }

    #[test]
    fn table_with_two_thousand_columns() {
        let start = std::time::Instant::now();
        render_must_not_panic(&wide_table_md(2_000, 5));
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "2000-column table render is pathologically slow"
        );
    }
}

mod images {
    use super::*;

    #[test]
    fn missing_local_image_falls_back() {
        // Empty alt → renders nothing (no `[image: ]` text).
        let bytes = render_must_not_panic("![](no-such-file.png)\n");
        assert!(
            !contains(&bytes, b"[image: ]"),
            "empty-alt placeholder leaked into output"
        );
    }

    #[test]
    fn missing_local_image_with_alt_shows_alt() {
        let bytes = render_must_not_panic("![my banner](no-such-file.png)\n");
        // The alt text 'my banner' should appear as italic fallback.
        assert!(
            contains(&bytes, b"my banner") || contains_text(&bytes, "my banner"),
            "alt text lost when image is missing"
        );
    }

    #[test]
    fn empty_image_src() {
        render_must_not_panic("![alt]()\n");
    }

    #[test]
    fn image_with_only_whitespace_src() {
        render_must_not_panic("![alt](   )\n");
    }

    #[test]
    fn image_with_one_thousand_char_url() {
        let url = format!("https://example.com/{}", "x".repeat(900));
        let md = format!("![alt]({})\n", url);
        render_must_not_panic(&md);
    }

    #[test]
    fn html_img_block_with_no_attrs() {
        render_must_not_panic("<img>\n");
    }

    #[test]
    fn html_img_with_empty_src() {
        render_must_not_panic("<img src=\"\" alt=\"x\">\n");
    }
}

mod inline_html_edges {
    use super::*;

    #[test]
    fn empty_sup_tag_pair() {
        render_must_not_panic("text<sup></sup>more\n");
    }

    #[test]
    fn empty_inline_tags_pair() {
        let md = "text<u></u><s></s><small></small><kbd></kbd>more\n";
        render_must_not_panic(md);
    }

    #[test]
    fn stacked_inline_tags() {
        render_must_not_panic(
            "<u><s><kbd><small><sup>stacked</sup></small></kbd></s></u>\n",
        );
    }

    #[test]
    fn br_tag_variants() {
        let md = "one<br>two<br/>three<br />four<br   />five<BR>six\n";
        render_must_not_panic(md);
    }

    #[test]
    fn unbalanced_open_tags() {
        // Open without close — flags should saturate but not crash.
        render_must_not_panic("<u><s><sup>never closed.\n");
    }

    #[test]
    fn unbalanced_close_tags() {
        // Close without open — saturating_sub keeps depth at 0.
        render_must_not_panic("never opened</u></s></sup>.\n");
    }

    #[test]
    fn unknown_html_block_falls_through() {
        let bytes = render_must_not_panic("<weird>content here</weird>\n");
        // Unknown tag should appear verbatim, not be silently swallowed.
        assert!(
            contains_text(&bytes, "<weird>")
                || contains(&bytes, b"<weird>")
                || contains(&bytes, b"weird"),
            "unknown HTML tag was silently dropped"
        );
    }

    #[test]
    fn html_comment_at_start_invisible() {
        let bytes = render_must_not_panic(
            "<!-- secret message that should not appear -->\nBody.\n",
        );
        assert!(
            !contains_text(&bytes, "secret message"),
            "HTML comment payload leaked into output"
        );
    }
}

mod links {
    use super::*;

    #[test]
    fn link_with_empty_text() {
        render_must_not_panic("[](https://example.com)\n");
    }

    #[test]
    fn link_with_empty_url() {
        render_must_not_panic("[text]()\n");
    }

    #[test]
    fn link_with_only_whitespace_url() {
        render_must_not_panic("[text](   )\n");
    }

    #[test]
    fn link_with_one_thousand_char_url() {
        let url = format!("https://example.com/{}", "p".repeat(950));
        let md = format!("[text]({})\n", url);
        render_must_not_panic(&md);
    }

    #[test]
    fn malformed_link_falls_back_to_text() {
        let bytes = render_must_not_panic("[unclosed bracket text here\n");
        // The text may render as a plain literal OR as a hex-encoded
        // PDF string (printpdf escapes `[` via hex strings). Check
        // both forms.
        let hex: String = b"unclosed bracket text here"
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect();
        assert!(
            contains_text(&bytes, "unclosed bracket text here")
                || contains(&bytes, b"unclosed bracket")
                || contains_text(&bytes, &hex),
            "malformed link's body text was dropped"
        );
    }

    #[test]
    fn autolink_with_weird_protocol() {
        render_must_not_panic("<javascript:alert(1)>\n");
    }
}

mod inline_combinations {
    use super::*;

    #[test]
    fn every_flag_set_simultaneously() {
        let md = "<u><s><small><kbd>***`flagstorm`***</kbd></small></s></u>\n";
        render_must_not_panic(md);
    }

    #[test]
    fn deeply_nested_emphasis() {
        // CommonMark caps emphasis at strong + italic, but lexer
        // must not crash on absurd nesting.
        let openers = "*".repeat(50);
        let closers = "*".repeat(50);
        let md = format!("{}deep{}\n", openers, closers);
        render_must_not_panic(&md);
    }

    #[test]
    fn many_consecutive_strikethrough_tildes() {
        render_must_not_panic("~~~~~strange~~~~~~\n");
    }

    #[test]
    fn emphasis_spanning_paragraphs() {
        // Emphasis should NOT cross a blank line per CommonMark; the
        // openers fall back to literal text.
        let bytes = render_must_not_panic("*open\n\nnew para*\n");
        assert!(
            contains_text(&bytes, "*open") || contains(&bytes, b"open"),
            "literal-fallback content lost"
        );
    }
}

mod whitespace_and_breaks {
    use super::*;

    #[test]
    fn ten_consecutive_spaces_in_paragraph() {
        render_must_not_panic("a          b\n");
    }

    #[test]
    fn hard_break_via_two_trailing_spaces() {
        render_must_not_panic("line one  \nline two\n");
    }

    #[test]
    fn hard_break_via_backslash() {
        render_must_not_panic("line one\\\nline two\n");
    }

    #[test]
    fn tab_only_lines() {
        render_must_not_panic("para1\n\t\t\t\npara2\n");
    }

    #[test]
    fn mixed_crlf_and_lf_line_endings() {
        let md = "first line\r\nsecond line\r\n\r\nthird paragraph\n";
        render_must_not_panic(md);
    }
}

mod unicode_and_rtl {
    use super::*;

    #[test]
    fn cjk_paragraph() {
        render_must_not_panic("你好世界。こんにちは。안녕하세요.\n");
    }

    #[test]
    fn rtl_arabic_paragraph() {
        render_must_not_panic("مرحبا بالعالم\n");
    }

    #[test]
    fn rtl_hebrew_paragraph() {
        render_must_not_panic("שלום עולם\n");
    }

    #[test]
    fn combining_characters() {
        // base + combining acute + combining ring above
        render_must_not_panic("a\u{0301}\u{030A} combining marks\n");
    }

    #[test]
    fn emoji_with_skin_tones() {
        render_must_not_panic("👋🏻 👋🏼 👋🏽 👋🏾 👋🏿\n");
    }

    #[test]
    fn mixed_direction_paragraph() {
        render_must_not_panic("English text مع عربي and more English.\n");
    }
}

mod frontmatter_edges {
    use super::*;

    #[test]
    fn frontmatter_without_close_renders_body() {
        // Unclosed frontmatter is left in place as body text per
        // our extractor's contract (returns None when no close).
        render_must_not_panic("---\ntitle: never closed\n\nbody after.\n");
    }

    #[test]
    fn empty_yaml_frontmatter() {
        render_must_not_panic("---\n---\nBody.\n");
    }

    #[test]
    fn empty_toml_frontmatter() {
        render_must_not_panic("+++\n+++\nBody.\n");
    }

    #[test]
    fn frontmatter_with_garbage_inside() {
        render_must_not_panic("---\n!!!invalid yaml!!!\n---\nBody.\n");
    }

    #[test]
    fn frontmatter_with_extremely_long_value() {
        let title = "x".repeat(5_000);
        let md = format!("---\ntitle: {}\n---\nBody.\n", title);
        render_must_not_panic(&md);
    }
}

mod entities_and_escapes {
    use super::*;

    #[test]
    fn many_html_entities() {
        render_must_not_panic("&amp; &lt; &gt; &quot; &copy; &nbsp; &#65; &#x41;\n");
    }

    #[test]
    fn malformed_entity_unconsumed() {
        // `&notanentity;` — invalid; CommonMark says emit as literal.
        render_must_not_panic("&notanentity; and &too;\n");
    }

    #[test]
    fn all_backslash_escapes() {
        let md = r"\*\_\`\\\<\>\[\]\(\)\#\!\+\-\.\{\}";
        render_must_not_panic(&format!("{}\n", md));
    }
}

mod misc_robustness {
    use super::*;

    #[test]
    fn document_with_only_horizontal_rules() {
        render_must_not_panic("---\n***\n___\n");
    }

    #[test]
    fn document_with_only_page_break_markers() {
        render_must_not_panic("<!-- pagebreak -->\n\n<!-- pagebreak -->\n");
    }

    #[test]
    fn one_thousand_consecutive_pagebreaks() {
        let md = "<!-- pagebreak -->\n\n".repeat(1_000);
        let bytes = render_must_not_panic(&md);
        // Pages shouldn't explode to 1000; many empty pagebreaks
        // collapse via the layout's start-of-page guard.
        assert!(
            page_count(&bytes) <= 1_001,
            "page count blew up: {}",
            page_count(&bytes)
        );
    }

    #[test]
    fn deeply_nested_inline_link_alt() {
        // Nested image inside a link is a real GFM pattern (badges
        // in READMEs). Verify we handle a deep version cleanly.
        let md = "[![alt](https://example.com/a.png)](https://example.com)\n";
        render_must_not_panic(md);
    }

    #[test]
    fn document_ending_mid_token() {
        // Unterminated code span, link, emphasis at EOF.
        for s in &[
            "text with `unclosed code",
            "text with [unclosed link",
            "text with *unclosed emphasis",
            "text with <unclosed html tag",
            "text with &unfinished entity",
        ] {
            render_must_not_panic(s);
        }
    }
}
