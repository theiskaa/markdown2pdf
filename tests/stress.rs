//! Torture tests — inputs the CommonMark spec doesn't cover but that real
//! users (and malicious input) can produce. The bar here is robustness:
//! the lexer must not panic, stack-overflow, or hang. Output correctness is
//! a stretch goal in this file — we mostly check that `parse()` returns.

use markdown2pdf::markdown::{Lexer, LexerError};
use std::time::{Duration, Instant};

const PER_INPUT_BUDGET: Duration = Duration::from_secs(2);

fn run_within_budget(name: &str, input: String) {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    let input_for_thread = input.clone();
    // 8 MiB stack — the lexer's parse_link / blockquote paths are recursive
    // and the default test-thread stack (256 KiB) overflows on inputs that
    // a generous-stack thread handles fine.
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut lexer = Lexer::new(input_for_thread);
                lexer.parse()
            }));
            let _ = tx.send(result);
        })
        .expect("spawn stress thread");
    match rx.recv_timeout(PER_INPUT_BUDGET) {
        Ok(Ok(Ok(_))) => {
            let elapsed = start.elapsed();
            assert!(
                elapsed < PER_INPUT_BUDGET,
                "{}: took {:?} (over budget)",
                name,
                elapsed
            );
        }
        Ok(Ok(Err(e))) => panic!("{}: lexer error {:?}", name, e),
        Ok(Err(_)) => panic!("{}: panicked", name),
        Err(_) => panic!("{}: timed out (>{:?})", name, PER_INPUT_BUDGET),
    }
}

/// Like [`run_within_budget`], but a typed `Err` is an acceptable
/// outcome: inputs nested past `MAX_PARSE_DEPTH` are *contractually*
/// rejected with a `LexerError` rather than parsed. The bar is still
/// robustness — no panic, no stack-overflow abort, no timeout — so an
/// over-deep document fails gracefully instead of crashing the
/// process.
fn run_resilient(name: &str, input: String) {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut lexer = Lexer::new(input);
                lexer.parse()
            }));
            let _ = tx.send(result);
        })
        .expect("spawn stress thread");
    match rx.recv_timeout(PER_INPUT_BUDGET) {
        Ok(Ok(Ok(_))) | Ok(Ok(Err(_))) => {
            let elapsed = start.elapsed();
            assert!(
                elapsed < PER_INPUT_BUDGET,
                "{}: took {:?} (over budget)",
                name,
                elapsed
            );
        }
        Ok(Err(_)) => panic!("{}: panicked / stack-overflowed", name),
        Err(_) => panic!("{}: timed out (>{:?})", name, PER_INPUT_BUDGET),
    }
}

#[test]
fn deep_nested_blockquotes() {
    // Nesting at/under MAX_PARSE_DEPTH (32) parses normally; deeper
    // nesting is rejected with a typed error (see the
    // *_far_beyond_cap tests). 24 is a "deep but valid" document.
    let depth = 24;
    let input = ">".repeat(depth) + " foo\n";
    run_within_budget("deep_nested_blockquotes", input);
}

#[test]
fn deep_nested_emphasis() {
    let depth = 24;
    let input = "*".repeat(depth) + "x" + &"*".repeat(depth) + "\n";
    run_within_budget("deep_nested_emphasis", input);
}

#[test]
fn deep_nested_lists() {
    let depth = 24;
    let mut input = String::new();
    for i in 0..depth {
        input.push_str(&" ".repeat(i * 2));
        input.push_str("- item\n");
    }
    run_within_budget("deep_nested_lists", input);
}

#[test]
fn mass_backticks() {
    let n = 100_000;
    let input = "`".repeat(n) + "\n";
    run_within_budget("mass_backticks", input);
}

#[test]
fn mass_asterisks_line_start() {
    let n = 50_000;
    let input = "*".repeat(n) + "\n";
    run_within_budget("mass_asterisks_line_start", input);
}

#[test]
fn mass_open_brackets_no_close() {
    // `[` triggers parse_link, whose parse_nested_content recursion was
    // historically unbounded — ~10,000 brackets overflowed the stack.
    // It is now capped: a typed LexerError past MAX_PARSE_DEPTH, never
    // a crash, regardless of how many brackets are thrown at it.
    let n = 20_000;
    let input = "[".repeat(n) + "\n";
    run_resilient("mass_open_brackets_no_close", input);
}

#[test]
fn many_paragraphs() {
    let n = 5_000;
    let mut input = String::new();
    for i in 0..n {
        input.push_str(&format!("paragraph {}\n\n", i));
    }
    run_within_budget("many_paragraphs", input);
}

#[test]
fn mixed_line_endings() {
    // CRLF, LF, CR mid-file. Must all normalize cleanly.
    let input = "line1\r\nline2\nline3\rline4\r\n\r\nline5";
    run_within_budget("mixed_line_endings", input.to_string());
}

#[test]
fn null_bytes_and_control_chars() {
    let input = "foo\u{0000}bar\u{0001}\u{0007}\u{001F}baz\n";
    run_within_budget("null_bytes_and_control_chars", input.to_string());
}

#[test]
fn leading_bom() {
    let input = "\u{FEFF}# Heading\n";
    run_within_budget("leading_bom", input.to_string());
}

#[test]
fn unicode_in_headings_links_codespans() {
    let input = "# Iñtërnâtiônàlizætiøn 🦀\n\n[ภาษาไทย](https://example.com/ทดสอบ \"標題\")\n\n`日本語コード` and `emoji 🚀`\n";
    run_within_budget("unicode_in_headings_links_codespans", input.to_string());
}

#[test]
fn surrogate_and_oob_numeric_refs() {
    let input = "&#0; &#xD800; &#xDFFF; &#x110000; &#x99999999; &#xFFFD;\n";
    run_within_budget("surrogate_and_oob_numeric_refs", input.to_string());
}

#[test]
fn unicode_punctuation_flanking_boundaries() {
    // Each pair surrounds an emphasis-like run with a character from a
    // different Unicode punctuation category. None should panic.
    let cases = ["¡*x*!", "—*x*—", "«*x*»", "*x*。", "‘*x*’", "·*x*·"];
    for c in &cases {
        run_within_budget(c, format!("{}\n", c));
    }
}

#[test]
fn reference_self_cycle_does_not_loop() {
    let input = "[a][a]\n\n[a]: /u\n";
    run_within_budget("reference_self_cycle", input.to_string());
}

#[test]
fn reference_mutual_cycle_does_not_loop() {
    let input = "[a][b] [b][a]\n\n[a]: /a\n[b]: /b\n";
    run_within_budget("reference_mutual_cycle", input.to_string());
}

#[test]
fn unclosed_code_fence() {
    let input = "```rust\nfn main() {}\nno closer here\n";
    run_within_budget("unclosed_code_fence", input.to_string());
}

#[test]
fn unterminated_emphasis_at_eof() {
    let input = "**unterminated bold at end of file";
    run_within_budget("unterminated_emphasis_at_eof", input.to_string());
}

#[test]
fn extremely_long_single_line() {
    let n = 100_000;
    let input: String = "a".repeat(n);
    run_within_budget("extremely_long_single_line", input);
}

#[test]
fn many_link_definitions() {
    let n = 1_000;
    let mut input = String::new();
    for i in 0..n {
        input.push_str(&format!("[ref{}]: /u{}\n", i, i));
    }
    input.push_str("\n");
    for i in 0..n {
        input.push_str(&format!("[ref{}] ", i));
    }
    run_within_budget("many_link_definitions", input);
}

#[test]
fn mass_reference_definitions() {
    // 50k definitions + a use — the def HashMap has no count cap;
    // verify it stays linear (release ~32ms).
    let mut input = String::new();
    for i in 0..50_000 {
        input.push_str(&format!("[l{i}]: http://example/{i}\n"));
    }
    input.push_str("\nsee [l0].\n");
    run_within_budget("mass_reference_definitions", input);
}

#[test]
fn single_megabyte_reference_label() {
    // normalize_label allocates per char; a 1 MB label must not be
    // super-linear (release ~22ms).
    let big = "a".repeat(1_000_000);
    let input = format!("[{big}]: http://example\n\n[{big}]\n");
    run_within_budget("single_megabyte_reference_label", input);
}

#[test]
fn mass_shortcut_reference_uses() {
    let mut input = String::from("[x]: http://example\n\n");
    for _ in 0..50_000 {
        input.push_str("[x] ");
    }
    input.push('\n');
    run_within_budget("mass_shortcut_reference_uses", input);
}

#[test]
fn alternating_blockquote_paragraph() {
    let n = 500;
    let mut input = String::new();
    for i in 0..n {
        input.push_str(&format!("> quote {}\n\nparagraph {}\n\n", i, i));
    }
    run_within_budget("alternating_blockquote_paragraph", input);
}

#[test]
fn pathological_emphasis_pairs() {
    // A pattern that has historically caused O(n^2) or worse parsing in
    // markdown engines (the *_*_*_… alternation).
    let n = 200;
    let mut input = String::new();
    for _ in 0..n {
        input.push_str("*_");
    }
    for _ in 0..n {
        input.push_str("_*");
    }
    input.push('\n');
    run_within_budget("pathological_emphasis_pairs", input);
}

#[test]
fn nested_links_do_not_infinite_recurse() {
    let input = "[a [b [c [d [e](u5)](u4)](u3)](u2)](u1)\n";
    run_within_budget("nested_links", input.to_string());
}

#[test]
fn tab_only_long_line() {
    // A line of pure tabs — exercises the tab-expansion paths in
    // strip_leading_cols / indented-code detection.
    let n = 64_000;
    let input = "\t".repeat(n) + "\n";
    run_within_budget("tab_only_long_line", input);
}

#[test]
fn alternating_emphasis_and_code() {
    // `*` ` * ` … must not provoke quadratic emphasis matching.
    let n = 10_000;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("*`*`");
    }
    s.push('\n');
    run_within_budget("alternating_emphasis_and_code", s);
}

#[test]
fn mass_reference_definitions_unused() {
    // Defs with no usage — extract_definitions must scale.
    let n = 10_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("[d{}]: /u{}\n", i, i));
    }
    run_within_budget("mass_reference_definitions_unused", s);
}

#[test]
fn mass_image_references_unresolved() {
    let n = 5_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("![alt{}] ", i));
    }
    s.push('\n');
    run_within_budget("mass_image_references_unresolved", s);
}

#[test]
fn mass_links_with_titles() {
    let n = 2_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!(r#"[t{}](/u{} "title{}") "#, i, i, i));
    }
    s.push('\n');
    run_within_budget("mass_links_with_titles", s);
}

#[test]
fn mass_tables_back_to_back() {
    let n = 500;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("| a | b |\n| --- | --- |\n| 1 | 2 |\n\n");
    }
    run_within_budget("mass_tables_back_to_back", s);
}

#[test]
fn mixed_cr_in_code_block() {
    // CR-only line endings inside a fenced block. The lexer normalizes
    // before lexing; the block body must still come out non-empty.
    let input = "```\r\nbody one\rbody two\rend\r\n```\r\n";
    run_within_budget("mixed_cr_in_code_block", input.to_string());
}

#[test]
fn mass_html_comment_open_no_close() {
    // Each `<!--` opens a comment scan; without a closer the lexer must
    // fall back without looping.
    let n = 10_000;
    let input = "<!--".repeat(n) + "\n";
    run_within_budget("mass_html_comment_open_no_close", input);
}

#[test]
fn deeply_nested_image_in_link() {
    // `[![a](u)](u2)` chained 30 levels — nesting depth bounded by the
    // 8 MiB test stack.
    let mut s = String::new();
    let depth = 30;
    for i in 0..depth {
        s.push_str(&format!("[![a{}](u{})]", i, i));
    }
    s.push_str("(outer)\n");
    run_within_budget("deeply_nested_image_in_link", s);
}

#[test]
fn unicode_combining_marks_in_emphasis() {
    // Combining marks should not break flanking-run classification or
    // panic on grapheme boundaries.
    let cases = [
        "*á*",
        "*a\u{0301}*",
        "*a\u{200D}b*",     // ZWJ
        "*\u{FE0F}*",        // variation selector only
        "*test\u{0301}\u{0302}*",
    ];
    for c in &cases {
        run_within_budget(c, format!("{}\n", c));
    }
}

#[test]
fn mass_entity_references_unknown() {
    // Unknown entities short-circuit out of the table; loop must not
    // become quadratic.
    let n = 10_000;
    let input = "&xyzzy;".repeat(n) + "\n";
    run_within_budget("mass_entity_references_unknown", input);
}

#[test]
fn mass_setext_underlines() {
    // Alternating paragraph + underline must not overflow.
    let n = 5_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("para {}\n=\n\n", i));
    }
    run_within_budget("mass_setext_underlines", s);
}

#[test]
fn mass_thematic_breaks() {
    let n = 10_000;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("---\n");
    }
    run_within_budget("mass_thematic_breaks", s);
}

#[test]
fn pathological_table_unbalanced_pipes() {
    // Variable pipe counts per row — parse_table's row scanner must
    // tolerate every shape without panicking.
    let n = 1_000;
    let mut s = String::from("| a | b | c |\n| --- | --- | --- |\n");
    for i in 0..n {
        let pipes = (i % 7) + 1;
        let row: Vec<String> = (0..pipes).map(|j| format!("{}.{}", i, j)).collect();
        s.push_str(&format!("| {} |\n", row.join(" | ")));
    }
    run_within_budget("pathological_table_unbalanced_pipes", s);
}

#[test]
fn mass_html_block_div_openers() {
    // Many block-element opener lines followed by blank-line
    // terminators. Each pair is its own HtmlBlock; the scanner
    // must not become quadratic in the count.
    let n = 5_000;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("<div>\nx\n</div>\n\n");
    }
    run_within_budget("mass_html_block_div_openers", s);
}

#[test]
fn mass_unclosed_raw_html_blocks() {
    // Each `<script>` opener with no matching closer would individually
    // consume to EOF. Stacking many forces the lexer to recover after
    // each block claims a chunk; verify no quadratic behavior.
    let n = 1_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("<script>\nbody {}\n</script>\n\n", i));
    }
    run_within_budget("mass_unclosed_raw_html_blocks", s);
}

#[test]
fn deeply_nested_html_inside_blockquote() {
    // `> > > > <div>foo</div>` — deeply nested blockquote whose body
    // is an HTML block, at a valid depth (≤ MAX_PARSE_DEPTH).
    let depth = 24;
    let mut s = String::new();
    for _ in 0..depth {
        s.push_str("> ");
    }
    s.push_str("<div>\nbody\n</div>\n");
    run_within_budget("deeply_nested_html_inside_blockquote", s);
}

#[test]
fn pathological_html_attribute_storm() {
    // A single open tag with many attributes — the tag matcher's
    // attribute loop must handle this without slowing dramatically.
    let n = 1_000;
    let mut s = String::from("<a");
    for i in 0..n {
        s.push_str(&format!(" attr{}=\"value{}\"", i, i));
    }
    s.push_str(">\nbody\n</a>\n");
    run_within_budget("pathological_html_attribute_storm", s);
}

#[test]
fn html_tag_scanner_no_redos() {
    // The tag matcher must stay linear on adversarial shapes — no
    // catastrophic backtracking. Each ≤ 2ms in release; well under
    // budget even unoptimized.
    run_within_budget(
        "html_equals_storm",
        format!("<a {}>\n", "=".repeat(50_000)),
    );
    run_within_budget(
        "html_unquoted_value_boundaries",
        format!(
            "<a{}>\n",
            (0..20_000).map(|i| format!(" x=y{i}")).collect::<String>()
        ),
    );
    run_within_budget(
        "html_bare_lt_storm",
        "<".repeat(100_000) + "\n",
    );
    run_within_budget(
        "html_nested_tag_openers",
        "<a<a<a".repeat(20_000) + "\n",
    );
}

#[test]
fn mass_inline_processing_instructions() {
    // Many inline PIs in a single paragraph. The inline-special
    // matcher should handle each in O(its-own-length) without
    // re-scanning prior content.
    let n = 1_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("text <?php echo {}; ?> ", i));
    }
    s.push('\n');
    run_within_budget("mass_inline_processing_instructions", s);
}

#[test]
fn mass_html_comment_short_forms() {
    let n = 5_000;
    let mut s = String::new();
    for _ in 0..n {
        s.push_str("foo <!--> bar <!---> baz ");
    }
    s.push('\n');
    run_within_budget("mass_html_comment_short_forms", s);
}

#[test]
fn alternating_html_block_and_paragraph() {
    // Forces repeated paragraph-interrupt-by-Type-6 decisions.
    let n = 2_000;
    let mut s = String::new();
    for i in 0..n {
        s.push_str(&format!("paragraph {}\n<div>\nbody {}\n</div>\n\n", i, i));
    }
    run_within_budget("alternating_html_block_and_paragraph", s);
}

/// The recursion-depth contract (T4). Every recursive construct —
/// blockquote, nested list, link/image brackets, emphasis — driven
/// thousands of levels past `MAX_PARSE_DEPTH`. Before the cap each of
/// these overflowed the OS stack and aborted the test process; now
/// each must return a typed `LexerError` well within budget.
#[test]
fn pipe_heavy_non_table_is_not_quadratic() {
    // Each `|`-led line is its own paragraph (blank-line separated),
    // so the dispatcher calls is_table_start ~30k times while every
    // other per-line cost stays O(1). is_table_start used to collect
    // the entire remaining input into a String each call — O(n²),
    // far over the 2s budget. No line is followed by a delimiter
    // row, so nothing is a table.
    let mut input = String::new();
    for _ in 0..30_000 {
        input.push_str("| col a | col b | col c |\n\n");
    }
    run_within_budget("pipe_heavy_non_table", input);
}

#[test]
fn huge_single_paragraph_is_linear() {
    // One paragraph of many `|`-led lines with NO blank separators —
    // the whole thing accumulates into a single Text run. With the
    // is_table_start fix this is linear (verified: release time
    // doubles as n doubles); a reintroduced O(n²) here would blow
    // the 2s budget well before this size.
    let mut input = String::new();
    for _ in 0..15_000 {
        input.push_str("| col a | col b | col c |\n");
    }
    run_within_budget("huge_single_paragraph", input);
}

#[test]
fn deep_blockquote_far_beyond_cap_is_graceful() {
    let input = ">".repeat(5_000) + " foo\n";
    run_resilient("deep_blockquote_far_beyond_cap", input);
}

#[test]
fn deep_nested_list_far_beyond_cap_is_graceful() {
    let mut input = String::new();
    for i in 0..2_000 {
        input.push_str(&" ".repeat(i * 2));
        input.push_str("- item\n");
    }
    run_resilient("deep_nested_list_far_beyond_cap", input);
}

#[test]
fn deep_emphasis_far_beyond_cap_is_graceful() {
    let input = "*".repeat(5_000) + "x" + &"*".repeat(5_000) + "\n";
    run_resilient("deep_emphasis_far_beyond_cap", input);
}

#[test]
fn deep_link_nesting_far_beyond_cap_is_graceful() {
    // Genuinely nested (not sibling) link labels, 3,000 deep.
    let mut input = "[".repeat(3_000);
    input.push_str("text");
    input.push_str(&"](u)".repeat(3_000));
    input.push('\n');
    run_resilient("deep_link_nesting_far_beyond_cap", input);
}

#[test]
fn nesting_cap_returns_typed_error_not_crash() {
    // The contract is a *typed* error, not just "doesn't crash".
    // Parsed on the generous stack the other helpers use: reaching
    // the depth-64 check still consumes ~64 recursion frames, which a
    // 256 KiB default test stack can't hold — the cap bounds the
    // depth, it doesn't shrink each frame.
    let handle = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(|| {
            let mut lexer = Lexer::new(">".repeat(2_000) + " foo\n");
            lexer.parse()
        })
        .expect("spawn thread");
    match handle.join().expect("thread panicked") {
        Err(LexerError::UnknownToken { message, .. }) => {
            assert!(
                message.contains("maximum nesting depth"),
                "unexpected error message: {message}"
            );
        }
        other => panic!("expected a nesting-depth LexerError, got {:?}", other),
    }
}
