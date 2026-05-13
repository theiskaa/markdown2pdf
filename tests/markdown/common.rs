//! Shared helpers for the lexer integration tests. Every test file in
//! `tests/lexer/` had its own copy of the `parse` helper — this module
//! centralizes them. Per the Rust Book, this is the canonical place for
//! integration-test helper code (the `tests/common/mod.rs` pattern).

#![allow(dead_code)] // not every test file uses every helper

use markdown2pdf::markdown::{Lexer, Token};

/// Parses `input` and unwraps the result. Panics on lexer errors — the
/// integration test should treat any error as a failure to investigate.
pub fn parse(input: &str) -> Vec<Token> {
    let mut lexer = Lexer::new(input.to_string());
    lexer.parse().unwrap()
}
