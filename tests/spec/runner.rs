//! Spec runner — iterates assets/commonmark_spec.json, parses each example
//! through the lexer, renders the result via the test-only HTML renderer,
//! normalizes both sides, and compares as strings. Reports per-section
//! pass/fail totals and detailed diffs for failures.

use crate::spec::html_render;
use crate::spec::normalize::normalize;
use markdown2pdf::markdown::{Lexer, Token};
use std::collections::BTreeMap;

const SPEC_JSON: &str = include_str!("../../assets/commonmark_spec.json");
const KNOWN_FAILURES_TXT: &str = include_str!("known_failures.txt");

#[derive(Debug, Clone)]
pub struct Example {
    pub example: u32,
    pub section: String,
    pub markdown: String,
    pub html: String,
}

#[derive(Debug)]
pub struct Failure {
    pub example: u32,
    pub section: String,
    pub markdown: String,
    pub expected: String,
    pub actual: String,
    pub ast: String,
}

#[derive(Debug, Default)]
pub struct SuiteResult {
    pub per_section: BTreeMap<String, (u32, u32)>, // section → (pass, fail)
    pub passed: u32,
    pub failed: u32,
    pub failures: Vec<Failure>,
    pub regressed: Vec<u32>,         // failed examples NOT in known_failures
    pub unexpected_passes: Vec<u32>, // example numbers in known_failures that now pass
}

pub fn load_examples() -> Vec<Example> {
    parse_spec_json(SPEC_JSON)
}

fn parse_spec_json(text: &str) -> Vec<Example> {
    // Minimal hand-rolled JSON parsing for the known shape (array of flat
    // objects with the four fields we need). Avoids adding serde_json as a
    // dev-dep for this single file.
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    skip_ws(&chars, &mut i);
    expect(&chars, &mut i, '[');
    skip_ws(&chars, &mut i);
    while i < chars.len() && chars[i] != ']' {
        out.push(parse_object(&chars, &mut i));
        skip_ws(&chars, &mut i);
        if i < chars.len() && chars[i] == ',' {
            i += 1;
            skip_ws(&chars, &mut i);
        }
    }
    out
}

fn skip_ws(chars: &[char], i: &mut usize) {
    while *i < chars.len() && chars[*i].is_whitespace() {
        *i += 1;
    }
}

fn expect(chars: &[char], i: &mut usize, c: char) {
    assert_eq!(chars[*i], c, "expected {:?} at position {}", c, i);
    *i += 1;
}

fn parse_object(chars: &[char], i: &mut usize) -> Example {
    skip_ws(chars, i);
    expect(chars, i, '{');
    let mut ex = Example {
        example: 0,
        section: String::new(),
        markdown: String::new(),
        html: String::new(),
    };
    loop {
        skip_ws(chars, i);
        if chars[*i] == '}' {
            *i += 1;
            break;
        }
        let key = parse_string(chars, i);
        skip_ws(chars, i);
        expect(chars, i, ':');
        skip_ws(chars, i);
        match key.as_str() {
            "markdown" => ex.markdown = parse_string(chars, i),
            "html" => ex.html = parse_string(chars, i),
            "section" => ex.section = parse_string(chars, i),
            "example" => ex.example = parse_number(chars, i),
            _ => {
                // Skip other fields (start_line, end_line, etc.).
                skip_value(chars, i);
            }
        }
        skip_ws(chars, i);
        if chars[*i] == ',' {
            *i += 1;
        }
    }
    ex
}

fn parse_string(chars: &[char], i: &mut usize) -> String {
    expect(chars, i, '"');
    let mut out = String::new();
    while *i < chars.len() && chars[*i] != '"' {
        if chars[*i] == '\\' && *i + 1 < chars.len() {
            *i += 1;
            match chars[*i] {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\u{0008}'),
                'f' => out.push('\u{000C}'),
                'u' => {
                    *i += 1;
                    let hex: String = chars[*i..*i + 4].iter().collect();
                    *i += 3; // we'll +1 below
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(c) = char::from_u32(code) {
                            out.push(c);
                        }
                    }
                }
                c => out.push(c),
            }
            *i += 1;
        } else {
            out.push(chars[*i]);
            *i += 1;
        }
    }
    expect(chars, i, '"');
    out
}

fn parse_number(chars: &[char], i: &mut usize) -> u32 {
    let start = *i;
    while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '-') {
        *i += 1;
    }
    let s: String = chars[start..*i].iter().collect();
    s.parse().unwrap_or(0)
}

fn skip_value(chars: &[char], i: &mut usize) {
    skip_ws(chars, i);
    match chars[*i] {
        '"' => {
            let _ = parse_string(chars, i);
        }
        '{' => {
            *i += 1;
            let mut depth = 1;
            while *i < chars.len() && depth > 0 {
                if chars[*i] == '"' {
                    let _ = parse_string(chars, i);
                    continue;
                }
                if chars[*i] == '{' {
                    depth += 1;
                } else if chars[*i] == '}' {
                    depth -= 1;
                }
                *i += 1;
            }
        }
        '[' => {
            *i += 1;
            let mut depth = 1;
            while *i < chars.len() && depth > 0 {
                if chars[*i] == '"' {
                    let _ = parse_string(chars, i);
                    continue;
                }
                if chars[*i] == '[' {
                    depth += 1;
                } else if chars[*i] == ']' {
                    depth -= 1;
                }
                *i += 1;
            }
        }
        _ => {
            while *i < chars.len()
                && chars[*i] != ','
                && chars[*i] != '}'
                && chars[*i] != ']'
            {
                *i += 1;
            }
        }
    }
}

fn load_known_failures() -> std::collections::BTreeSet<u32> {
    KNOWN_FAILURES_TXT
        .lines()
        .filter_map(|l| {
            let trimmed = l.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            // Allow `<num>` or `<num> # comment`
            let num_part = trimmed.split('#').next().unwrap_or("").trim();
            num_part.parse::<u32>().ok()
        })
        .collect()
}

/// Per-example parse cap. The lexer is expected to handle every spec input
/// in well under a second; anything longer is a hang we want to surface as
/// a failure rather than block the whole suite on.
const PER_EXAMPLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug)]
enum ParseOutcome {
    Ok(Vec<Token>),
    Err(String),
    Panic(#[allow(dead_code)] String),
    Timeout,
}

fn parse_with_timeout(markdown: String) -> ParseOutcome {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut lexer = Lexer::new(markdown);
            lexer.parse()
        }));
        let _ = tx.send(result);
    });
    match rx.recv_timeout(PER_EXAMPLE_TIMEOUT) {
        Ok(Ok(Ok(tokens))) => {
            let _ = handle.join();
            ParseOutcome::Ok(tokens)
        }
        Ok(Ok(Err(e))) => {
            let _ = handle.join();
            ParseOutcome::Err(format!("{:?}", e))
        }
        Ok(Err(_panic)) => {
            let _ = handle.join();
            ParseOutcome::Panic("panicked".to_string())
        }
        Err(_) => {
            // Thread leaks — that's intentional; it's better than blocking the
            // suite. Process exit will reap it.
            ParseOutcome::Timeout
        }
    }
}

pub fn run() -> SuiteResult {
    let examples = load_examples();
    let known_failures = load_known_failures();
    let mut result = SuiteResult::default();
    for ex in &examples {
        let outcome = parse_with_timeout(ex.markdown.clone());
        let (actual, ast) = match &outcome {
            ParseOutcome::Ok(tokens) => (
                normalize(&html_render::render(tokens)),
                Token::slice_to_compact(tokens),
            ),
            ParseOutcome::Err(e) => (
                format!("<<lexer error: {}>>", e),
                format!("<lexer error: {}>", e),
            ),
            ParseOutcome::Panic(_) => (
                String::from("<<panic>>"),
                String::from("<panic>"),
            ),
            ParseOutcome::Timeout => (
                String::from("<<timeout>>"),
                String::from("<timeout>"),
            ),
        };
        let expected = normalize(&ex.html);
        let entry = result.per_section.entry(ex.section.clone()).or_default();
        if actual == expected {
            entry.0 += 1;
            result.passed += 1;
            if known_failures.contains(&ex.example) {
                result.unexpected_passes.push(ex.example);
            }
        } else {
            entry.1 += 1;
            result.failed += 1;
            if !known_failures.contains(&ex.example) {
                result.regressed.push(ex.example);
            }
            result.failures.push(Failure {
                example: ex.example,
                section: ex.section.clone(),
                markdown: ex.markdown.clone(),
                expected,
                actual,
                ast,
            });
        }
    }
    result
}

pub fn print_report(result: &SuiteResult) {
    println!();
    println!("=== CommonMark spec coverage ===");
    println!();
    println!("{:<48} {:>8} {:>8} {:>8}", "Section", "Pass", "Fail", "Total");
    println!("{}", "-".repeat(76));
    for (section, (pass, fail)) in &result.per_section {
        let total = pass + fail;
        let pct = if total > 0 { 100.0 * (*pass as f64) / (total as f64) } else { 0.0 };
        println!(
            "{:<48} {:>8} {:>8} {:>5}/{:<3} ({:>3.0}%)",
            section, pass, fail, pass, total, pct
        );
    }
    println!("{}", "-".repeat(76));
    let total = result.passed + result.failed;
    let pct = if total > 0 {
        100.0 * (result.passed as f64) / (total as f64)
    } else {
        0.0
    };
    println!(
        "{:<48} {:>8} {:>8} {:>5}/{:<3} ({:>3.1}%)",
        "TOTAL", result.passed, result.failed, result.passed, total, pct
    );
    println!();
    if !result.regressed.is_empty() {
        println!(
            "REGRESSIONS: {} examples failed that are NOT in known_failures.txt:",
            result.regressed.len()
        );
        for ex in &result.regressed {
            println!("  - example {}", ex);
        }
        println!();
    }
    if !result.unexpected_passes.is_empty() {
        println!(
            "UNEXPECTED PASSES: {} examples in known_failures.txt now pass and should be removed:",
            result.unexpected_passes.len()
        );
        for ex in &result.unexpected_passes {
            println!("  - example {}", ex);
        }
        println!();
    }
}

pub fn print_failure_details(result: &SuiteResult, limit: usize) {
    if result.failures.is_empty() {
        return;
    }
    println!("=== Failure details (first {}) ===", limit);
    println!();
    for f in result.failures.iter().take(limit) {
        println!("Example {} [{}]:", f.example, f.section);
        println!("  markdown: {:?}", f.markdown);
        println!("  expected: {:?}", f.expected);
        println!("  actual:   {:?}", f.actual);
        println!("  ast:      {}", f.ast);
        println!();
    }
}
