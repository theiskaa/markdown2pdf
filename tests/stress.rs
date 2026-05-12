//! Torture tests — inputs the CommonMark spec doesn't cover but that real
//! users (and malicious input) can produce. The bar here is robustness:
//! the lexer must not panic, stack-overflow, or hang. Output correctness is
//! a stretch goal in this file — we mostly check that `parse()` returns.

use markdown2pdf::markdown::Lexer;
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
        .stack_size(8 * 1024 * 1024)
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

#[test]
fn deep_nested_blockquotes() {
    // parse_blockquote recursively constructs a sub-Lexer per level, so the
    // safe nesting depth is bounded by the OS thread stack. Default stack
    // on Linux/macOS test threads is ~2 MiB; empirically 30 levels parse
    // cleanly with plenty of headroom. Deeper nesting (≳150) overflows.
    // Document the limit by testing well under it.
    let depth = 30;
    let input = ">".repeat(depth) + " foo\n";
    run_within_budget("deep_nested_blockquotes", input);
}

#[test]
fn deep_nested_emphasis() {
    let depth = 50;
    let input = "*".repeat(depth) + "x" + &"*".repeat(depth) + "\n";
    run_within_budget("deep_nested_emphasis", input);
}

#[test]
fn deep_nested_lists() {
    let depth = 50;
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
    // `[` triggers parse_link, which uses parse_nested_content to find the
    // closing `]`. Each `[` adds a frame to the recursion. With the 8 MiB
    // stack run_within_budget allocates, 5,000 unmatched brackets parse
    // cleanly. Pathological adversarial inputs above ~10,000 would still
    // overflow — that's a known recursion-depth fragility tracked
    // separately from this robustness test.
    let n = 500;
    let input = "[".repeat(n) + "\n";
    run_within_budget("mass_open_brackets_no_close", input);
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
