//! Validation and warning system for markdown2pdf
//!
//! This module provides pre-flight checks that warn users about potential issues
//! without blocking PDF generation.

use crate::fonts::{FontConfig, default_body_source};
use std::path::Path;

/// Represents a non-critical warning that doesn't prevent PDF generation
#[derive(Debug, Clone)]
pub struct ValidationWarning {
    pub kind: WarningKind,
    pub message: String,
    pub suggestion: String,
}

/// Types of warnings that can be issued
#[derive(Debug, Clone, PartialEq)]
pub enum WarningKind {
    /// Font specified but not found
    MissingFont,
    /// Image path referenced but file not found
    MissingImage,
    /// Configuration file not found
    MissingConfig,
    /// Document contains Unicode but no Unicode font specified
    UnicodeWithoutFont,
    /// Large document may take time to process
    LargeDocument,
    /// Potentially problematic markdown syntax
    SyntaxWarning,
}

impl ValidationWarning {
    pub fn missing_font(font_name: &str) -> Self {
        Self {
            kind: WarningKind::MissingFont,
            message: format!("Font '{}' not found on system", font_name),
            suggestion: format!(
                "Install '{}' or specify fallback fonts. System will use fallback font.",
                font_name
            ),
        }
    }

    pub fn missing_image(path: &str) -> Self {
        Self {
            kind: WarningKind::MissingImage,
            message: format!("Image not found: {}", path),
            suggestion: "Check the image path is correct and the file exists".to_string(),
        }
    }

    pub fn unicode_without_font(chars: Vec<char>) -> Self {
        let sample: String = chars.iter().take(5).collect();
        Self {
            kind: WarningKind::UnicodeWithoutFont,
            message: format!(
                "Document contains Unicode characters (e.g., '{}') but no Unicode font specified",
                sample
            ),
            suggestion: "Consider using --default-font 'Noto Sans' or specifying fallback fonts for better rendering".to_string(),
        }
    }

    pub fn large_document(char_count: usize) -> Self {
        Self {
            kind: WarningKind::LargeDocument,
            message: format!("Large document detected ({} characters)", char_count),
            suggestion: "Processing may take a moment. Consider breaking into smaller documents if generation is slow".to_string(),
        }
    }

    pub fn syntax_warning(issue: &str) -> Self {
        Self {
            kind: WarningKind::SyntaxWarning,
            message: format!("Potential syntax issue: {}", issue),
            suggestion: "PDF will be generated, but check the output for formatting issues"
                .to_string(),
        }
    }
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Warning: {}", self.message)?;
        write!(f, "\n   Suggestion: {}", self.suggestion)
    }
}

/// Validates markdown content and configuration, returning warnings.
///
/// `style_fallback_fonts` is the resolved `[defaults].fallback_fonts`
/// list from the styling config (empty when no TOML config or no
/// fallbacks set). When non-empty, the Unicode-without-font warning is
/// suppressed — fallbacks cover the codepoints the primary doesn't.
pub fn validate_conversion(
    markdown: &str,
    font_config: Option<&FontConfig>,
    style_fallback_fonts: &[String],
    output_path: Option<&str>,
) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    if markdown.len() > 100_000 {
        warnings.push(ValidationWarning::large_document(markdown.len()));
    }

    if let Some(unicode_chars) = detect_unicode_chars(markdown)
        && !has_unicode_font(font_config, style_fallback_fonts)
    {
        warnings.push(ValidationWarning::unicode_without_font(unicode_chars));
    }

    if let Some(path) = output_path
        && let Some(parent) = Path::new(path).parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        warnings.push(ValidationWarning {
            kind: WarningKind::SyntaxWarning,
            message: format!("Output directory does not exist: {}", parent.display()),
            suggestion: format!("Create directory with: mkdir -p {}", parent.display()),
        });
    }

    warnings.extend(check_syntax_issues(markdown));
    warnings.extend(check_image_references(markdown));

    warnings
}

/// Detects if markdown contains non-ASCII Unicode characters
fn detect_unicode_chars(markdown: &str) -> Option<Vec<char>> {
    let unicode_chars: Vec<char> = markdown
        .chars()
        .filter(|c| !c.is_ascii() && !c.is_whitespace())
        .take(10)
        .collect();

    if unicode_chars.is_empty() {
        None
    } else {
        Some(unicode_chars)
    }
}

/// Checks if font config has Unicode-capable fonts.
///
/// Any external `default_font` (specified by name OR by explicit file
/// source) takes the renderer's Identity-H Unicode emit path regardless
/// of the font's name. Configured fallback fonts (either on
/// `FontConfig` or via `[defaults].fallback_fonts` in TOML) also count:
/// uncovered codepoints route to them via the renderer's split path,
/// so the document is not stuck on the Win-Ansi-only built-in path.
///
/// When nothing user-facing is set, the renderer probes a per-OS list
/// of likely-installed system Unicode fonts via [`default_body_source`].
/// If that probe succeeds, the warning is suppressed too — the
/// document will render through the external Unicode path, not the
/// ASCII-only built-in.
fn has_unicode_font(font_config: Option<&FontConfig>, style_fallback_fonts: &[String]) -> bool {
    if !style_fallback_fonts.is_empty() {
        return true;
    }
    if let Some(config) = font_config
        && (config.default_font.is_some()
            || config.default_font_source.is_some()
            || !config.fallback_fonts.is_empty()
            || !config.fallback_font_sources.is_empty())
    {
        return true;
    }
    default_body_source().is_some()
}

/// Checks for common markdown syntax issues
fn check_syntax_issues(markdown: &str) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    let code_fence_count = markdown.matches("```").count();
    if !code_fence_count.is_multiple_of(2) {
        warnings.push(ValidationWarning::syntax_warning(
            "Unclosed code block detected (odd number of ``` markers)",
        ));
    }

    let total_backticks = markdown.matches('`').count();
    let double_backticks = markdown.matches("``").count() * 2;
    let triple_backticks = markdown.matches("```").count() * 3;
    let inline_code_count = total_backticks
        .saturating_sub(double_backticks)
        .saturating_sub(triple_backticks);
    if !inline_code_count.is_multiple_of(2) {
        warnings.push(ValidationWarning::syntax_warning(
            "Possible unclosed inline code (odd number of ` markers)",
        ));
    }

    // Check for unclosed links. Footnote brackets are neutralized
    // first: `[^id]` references/definitions and Pandoc inline
    // footnotes `^[body]` are valid syntax, and the lexer even
    // accepts a stray `^[` (degrading it to literal text), so none of
    // them is "broken link syntax".
    let scan = neutralize_footnote_brackets(markdown);
    let open_links = scan.matches('[').count();
    let close_links = scan.matches(']').count();
    if open_links != close_links {
        warnings.push(ValidationWarning::syntax_warning(
            "Unmatched square brackets detected (possible broken link syntax)",
        ));
    }

    warnings
}

/// Blanks out the brackets that belong to footnote constructs so the
/// crude `[` vs `]` tally in [`check_syntax_issues`] only sees real
/// link brackets. Mirrors the lexer's own acceptance rules:
///
/// - `[^label]` — a footnote reference / definition (label is
///   `[A-Za-z0-9_-]+`). Both brackets are cleared.
/// - `^[body]` — a Pandoc inline footnote. The opener is always
///   cleared (the lexer treats even an unterminated `^[` as literal
///   text, so it must not read as an unmatched bracket); the matching
///   close, if present, is cleared too. Brackets *inside* the body
///   are left alone — they balance among themselves.
///
/// Anything that doesn't match these shapes is untouched, so a real
/// `[unclosed link` still trips the warning.
fn neutralize_footnote_brackets(md: &str) -> String {
    let chars: Vec<char> = md.chars().collect();
    let mut out = chars.clone();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' && chars.get(i + 1) == Some(&'^') {
            let mut j = i + 2;
            while j < chars.len()
                && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '-')
            {
                j += 1;
            }
            if j > i + 2 && chars.get(j) == Some(&']') {
                out[i] = ' ';
                out[j] = ' ';
                i = j + 1;
                continue;
            }
        }
        if chars[i] == '^' && chars.get(i + 1) == Some(&'[') {
            out[i + 1] = ' ';
            let mut depth = 1usize;
            let mut j = i + 2;
            while j < chars.len() {
                match chars[j] {
                    '\\' if j + 1 < chars.len() => {
                        j += 2;
                        continue;
                    }
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            out[j] = ' ';
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            i += 2;
            continue;
        }
        i += 1;
    }
    out.into_iter().collect()
}

/// Checks for image references and validates paths exist
fn check_image_references(markdown: &str) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    let mut chars = markdown.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' && chars.peek() == Some(&'[') {
            // Found potential image
            // Skip to the path part
            while let Some(ch) = chars.next() {
                if ch == ']' && chars.peek() == Some(&'(') {
                    chars.next(); // consume '('
                    let mut path = String::new();
                    for ch in chars.by_ref() {
                        if ch == ')' {
                            break;
                        }
                        path.push(ch);
                    }
                    // Check if it's a local file path (not URL)
                    if !path.starts_with("http://")
                        && !path.starts_with("https://")
                        && !path.is_empty()
                        && !Path::new(&path).exists()
                    {
                        warnings.push(ValidationWarning::missing_image(&path));
                    }
                    break;
                }
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_unicode() {
        let text = "Hello world";
        assert!(detect_unicode_chars(text).is_none());

        let text = "Hello ăâîșț";
        assert!(detect_unicode_chars(text).is_some());

        let text = "Привет мир";
        let chars = detect_unicode_chars(text).unwrap();
        assert!(!chars.is_empty());
    }

    #[test]
    fn test_syntax_validation() {
        let text = "```rust\ncode\n```\nMore content";
        let warnings = check_syntax_issues(text);
        assert!(warnings.is_empty());

        let text = "```rust\ncode\n";
        let warnings = check_syntax_issues(text);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].kind, WarningKind::SyntaxWarning);
    }

    #[test]
    fn test_unclosed_link_detection() {
        let text = "[link](url) [another](url)";
        let warnings = check_syntax_issues(text);
        assert!(warnings.is_empty());

        let text = "[unclosed link";
        let warnings = check_syntax_issues(text);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn footnote_syntax_does_not_trip_bracket_warning() {
        // Inline footnotes, refs/defs, nested brackets in a body, and
        // the intentional degradation cases (`^[` unterminated, `^[]`)
        // are all valid — none should warn.
        let text = "A ref[^a] and inline^[a note with [1] inside].\n\
                    Degrade: an unterminated ^[never closed and ^[] too.\n\
                    [^a]: def";
        let warnings = check_syntax_issues(text);
        assert!(
            warnings.is_empty(),
            "footnote syntax falsely flagged: {warnings:?}"
        );
    }

    #[test]
    fn real_unmatched_bracket_still_warns_alongside_footnotes() {
        // A genuine broken link must still be caught even when
        // footnote brackets are present and balanced.
        let text = "Valid^[note] but [this link is unclosed";
        let warnings = check_syntax_issues(text);
        assert!(
            !warnings.is_empty(),
            "a real unmatched `[` should still warn"
        );
    }

    #[test]
    fn test_large_document_warning() {
        let large_text = "a".repeat(200_000);
        let warnings = validate_conversion(&large_text, None, &[], None);
        assert!(
            warnings
                .iter()
                .any(|w| w.kind == WarningKind::LargeDocument)
        );
    }

    #[test]
    fn external_font_suppresses_unicode_warning() {
        // Any named external default font qualifies — the renderer's
        // Identity-H path handles Unicode regardless of font family.
        let cfg = FontConfig {
            default_font: Some("Georgia".to_string()),
            default_font_source: None,
            code_font: None,
            code_font_source: None,
            fallback_fonts: Vec::new(),
            fallback_font_sources: Vec::new(),
            enable_subsetting: true,
        };
        let warnings = validate_conversion("Hello café", Some(&cfg), &[], None);
        assert!(
            warnings
                .iter()
                .all(|w| w.kind != WarningKind::UnicodeWithoutFont),
            "external font should suppress the Unicode warning"
        );
    }

    #[test]
    fn missing_font_config_warning_tracks_system_unicode_probe() {
        // With no FontConfig the renderer probes the host for a system
        // Unicode body font. The warning must fire iff that probe fails
        // — typically only on minimal Linux containers without DejaVu /
        // Liberation / Noto installed. macOS and Windows defaults make
        // it succeed in practice.
        let warnings = validate_conversion("Hello café", None, &[], None);
        let has_warning = warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnicodeWithoutFont);
        let probe_found = default_body_source().is_some();
        assert_eq!(
            has_warning, !probe_found,
            "warning state must mirror probe outcome (probe found a font: {probe_found})"
        );
    }

    #[test]
    fn auto_detected_body_font_suppresses_unicode_warning() {
        // Direct check that the auto-detect path is wired in: when the
        // probe succeeds, the warning is suppressed even without any
        // FontConfig. Skipped where the probe returns None.
        if default_body_source().is_none() {
            eprintln!("skip: no system Unicode font available on this host");
            return;
        }
        let warnings = validate_conversion("Hello café", None, &[], None);
        assert!(
            warnings
                .iter()
                .all(|w| w.kind != WarningKind::UnicodeWithoutFont),
            "auto-detected system body font should suppress the warning"
        );
    }

    #[test]
    fn style_fallback_fonts_suppress_unicode_warning() {
        // `[defaults].fallback_fonts = ["..."]` from the TOML config
        // is a valid Unicode strategy: uncovered codepoints route to
        // the configured fallbacks. No warning expected.
        let style_fallbacks = vec!["Noto Sans CJK SC".to_string()];
        let warnings = validate_conversion("Hello 日本語", None, &style_fallbacks, None);
        assert!(
            warnings
                .iter()
                .all(|w| w.kind != WarningKind::UnicodeWithoutFont),
            "configured fallback_fonts should suppress the Unicode warning"
        );
    }

    #[test]
    fn font_config_fallback_fonts_suppress_unicode_warning() {
        // Same property must hold when the fallback is set on the
        // programmatic `FontConfig` rather than the TOML config.
        let cfg = FontConfig::new().with_fallback_fonts(["Noto Sans CJK SC"]);
        let warnings = validate_conversion("Hello 日本語", Some(&cfg), &[], None);
        assert!(
            warnings
                .iter()
                .all(|w| w.kind != WarningKind::UnicodeWithoutFont),
            "FontConfig.fallback_fonts should suppress the Unicode warning"
        );
    }
}
