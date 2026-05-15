//! Validation and warning system for markdown2pdf
//!
//! This module provides pre-flight checks that warn users about potential issues
//! without blocking PDF generation.

use crate::fonts::FontConfig;
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
            suggestion: "PDF will be generated, but check the output for formatting issues".to_string(),
        }
    }
}

impl std::fmt::Display for ValidationWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "⚠️  {}", self.message)?;
        write!(f, "\n   💡 {}", self.suggestion)
    }
}

/// Validates markdown content and configuration, returning warnings
pub fn validate_conversion(
    markdown: &str,
    font_config: Option<&FontConfig>,
    output_path: Option<&str>,
) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    // Check document size
    if markdown.len() > 100_000 {
        warnings.push(ValidationWarning::large_document(markdown.len()));
    }

    // Check for Unicode characters without appropriate font
    if let Some(unicode_chars) = detect_unicode_chars(markdown) {
        if !has_unicode_font(font_config) {
            warnings.push(ValidationWarning::unicode_without_font(unicode_chars));
        }
    }

    // Check output path
    if let Some(path) = output_path {
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                warnings.push(ValidationWarning {
                    kind: WarningKind::SyntaxWarning,
                    message: format!("Output directory does not exist: {}", parent.display()),
                    suggestion: format!("Create directory with: mkdir -p {}", parent.display()),
                });
            }
        }
    }

    // Check for common markdown syntax issues
    warnings.extend(check_syntax_issues(markdown));

    // Check for image references
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
/// of the font's name, so we only flag the built-in-fonts case — when
/// nothing at all is specified, the renderer falls back to printpdf's
/// Helvetica/Courier (WinAnsi-encoded, no Unicode).
fn has_unicode_font(font_config: Option<&FontConfig>) -> bool {
    let Some(config) = font_config else {
        return false;
    };
    config.default_font.is_some() || config.default_font_source.is_some()
}

/// Checks for common markdown syntax issues
fn check_syntax_issues(markdown: &str) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    // Check for unclosed code blocks
    let code_fence_count = markdown.matches("```").count();
    if code_fence_count % 2 != 0 {
        warnings.push(ValidationWarning::syntax_warning(
            "Unclosed code block detected (odd number of ``` markers)",
        ));
    }

    // Check for unclosed inline code
    let total_backticks = markdown.matches('`').count();
    let double_backticks = markdown.matches("``").count() * 2;
    let triple_backticks = markdown.matches("```").count() * 3;
    let inline_code_count = total_backticks.saturating_sub(double_backticks).saturating_sub(triple_backticks);
    if inline_code_count % 2 != 0 {
        warnings.push(ValidationWarning::syntax_warning(
            "Possible unclosed inline code (odd number of ` markers)",
        ));
    }

    // Check for unclosed links
    let open_links = markdown.matches('[').count();
    let close_links = markdown.matches(']').count();
    if open_links != close_links {
        warnings.push(ValidationWarning::syntax_warning(
            "Unmatched square brackets detected (possible broken link syntax)",
        ));
    }

    warnings
}

/// Checks for image references and validates paths exist
fn check_image_references(markdown: &str) -> Vec<ValidationWarning> {
    let mut warnings = Vec::new();

    // Simple regex-like pattern matching for ![alt](path)
    let mut chars = markdown.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '!' {
            if chars.peek() == Some(&'[') {
                // Found potential image
                // Skip to the path part
                while let Some(ch) = chars.next() {
                    if ch == ']' {
                        if chars.peek() == Some(&'(') {
                            chars.next(); // consume '('
                            let mut path = String::new();
                            while let Some(ch) = chars.next() {
                                if ch == ')' {
                                    break;
                                }
                                path.push(ch);
                            }
                            // Check if it's a local file path (not URL)
                            if !path.starts_with("http://")
                                && !path.starts_with("https://")
                                && !path.is_empty()
                            {
                                if !Path::new(&path).exists() {
                                    warnings.push(ValidationWarning::missing_image(&path));
                                }
                            }
                            break;
                        }
                    }
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
    fn test_large_document_warning() {
        let large_text = "a".repeat(200_000);
        let warnings = validate_conversion(&large_text, None, None);
        assert!(warnings
            .iter()
            .any(|w| w.kind == WarningKind::LargeDocument));
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
            enable_subsetting: true,
        };
        let warnings = validate_conversion("Hello café", Some(&cfg), None);
        assert!(
            warnings
                .iter()
                .all(|w| w.kind != WarningKind::UnicodeWithoutFont),
            "external font should suppress the Unicode warning"
        );
    }

    #[test]
    fn missing_font_config_still_warns_about_unicode() {
        // No font specified → renderer falls back to built-in
        // WinAnsi-encoded Helvetica/Courier; warning still fires.
        let warnings = validate_conversion("Hello café", None, None);
        assert!(warnings
            .iter()
            .any(|w| w.kind == WarningKind::UnicodeWithoutFont));
    }
}
