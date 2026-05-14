//! Knuth-Liang hyphenation. Backed by the `hyphenation` crate with
//! the English (US) dictionary embedded at compile time. The
//! `Standard` dictionary loads on first call (lazy `OnceLock`); every
//! caller reuses the same instance.
//!
//! v1 scope: only used by the long-word-breaking pre-pass to choose
//! aesthetically defensible split points when a single word exceeds
//! the column width. The greedy wrap algorithm itself doesn't yet
//! consult hyphenation — that needs Knuth-Plass cost modeling to look
//! good and is a follow-up.

use hyphenation::{Hyphenator, Language, Load, Standard};
use std::sync::OnceLock;

fn dictionary() -> Option<&'static Standard> {
    static DICT: OnceLock<Option<Standard>> = OnceLock::new();
    DICT.get_or_init(|| Standard::from_embedded(Language::EnglishUS).ok())
        .as_ref()
}

/// Byte offsets where the word may be split with a hyphen inserted.
/// Returns the empty vec if the word is too short to break (3-letter
/// minimum: hyphenation rules generally avoid breaking after the first
/// two or before the last two letters), if the language dictionary
/// couldn't load, or if the word contains non-letter characters that
/// the dictionary doesn't model (digits, punctuation).
pub fn break_points(word: &str) -> Vec<usize> {
    if word.len() < 5 {
        return Vec::new();
    }
    if !word.chars().all(|c| c.is_alphabetic()) {
        return Vec::new();
    }
    let Some(dict) = dictionary() else {
        return Vec::new();
    };
    let hyphenated = dict.hyphenate(word);
    hyphenated.breaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyphenation_returns_break_points_for_long_word() {
        // "hyphenation" itself has well-known breaks (hy-phen-ation).
        let breaks = break_points("hyphenation");
        assert!(!breaks.is_empty(), "expected at least one break point");
    }

    #[test]
    fn short_words_have_no_break_points() {
        assert!(break_points("cat").is_empty());
        assert!(break_points("dog").is_empty());
        // 4-letter words below the 5-letter cutoff also skip.
        assert!(break_points("home").is_empty());
    }

    #[test]
    fn words_with_digits_have_no_break_points() {
        // The dictionary is letter-only; words like "ab12cd" return
        // an empty break-point list so the caller falls back to its
        // char-boundary algorithm.
        assert!(break_points("ab12cd").is_empty());
    }

    #[test]
    fn breaks_are_within_word_length() {
        let word = "extraordinary";
        for &b in &break_points(word) {
            assert!(b > 0 && b < word.len(), "break {} out of range", b);
        }
    }
}
