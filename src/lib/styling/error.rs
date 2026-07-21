//! Configuration-loading error type with line/column + typo
//! suggestion formatting.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
#[non_exhaustive]
pub enum ResolveError {
    /// TOML failed to parse (syntax error, type mismatch, unknown
    /// field). Wraps `toml::de::Error` for span info. Boxed because
    /// `toml::de::Error` alone is ~88 bytes, which would otherwise
    /// make every `Result<_, ResolveError>` pay for this variant's
    /// size regardless of which variant is actually returned.
    BadToml {
        source: Box<toml::de::Error>,
        /// The original TOML text. `toml::de::Error::span()` gives a
        /// byte offset but not the source, so we keep it here to
        /// resolve that offset to a line/column.
        input: String,
        file: Option<PathBuf>,
        suggestion: Option<String>,
    },
    /// `theme = "xyz"` named a preset that doesn't exist.
    UnknownTheme {
        name: String,
        suggestion: Option<String>,
    },
    /// `inherits = "a"`, where a inherits from b, where b inherits from a.
    InheritsCycle(Vec<String>),
    /// After all merges, a required field is still unset. This is a
    /// programmer error in the bundled theme preset, not a user error.
    PresetIncomplete {
        theme: String,
        missing_field: String,
    },
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ResolveError::BadToml {
                source,
                input,
                file,
                suggestion,
            } => {
                let where_ = file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<config>".to_string());
                if let Some(span) = source.span() {
                    let (line, col) = line_col_in(input, span.start);
                    write!(
                        f,
                        "error in {} at line {}, column {}: {}",
                        where_,
                        line,
                        col,
                        source.message()
                    )?;
                } else {
                    write!(f, "error in {}: {}", where_, source.message())?;
                }
                if let Some(s) = suggestion {
                    write!(f, "\n  hint: {}", s)?;
                }
                Ok(())
            }
            ResolveError::UnknownTheme { name, suggestion } => {
                write!(f, "unknown theme preset `{}`", name)?;
                if let Some(s) = suggestion {
                    write!(f, "\n  did you mean `{}`?", s)?;
                }
                Ok(())
            }
            ResolveError::InheritsCycle(chain) => {
                write!(f, "theme inheritance cycle: {}", chain.join(" -> "))
            }
            ResolveError::PresetIncomplete {
                theme,
                missing_field,
            } => {
                write!(
                    f,
                    "internal: theme preset `{}` is missing required field `{}`",
                    theme, missing_field
                )
            }
            ResolveError::Io { path, source } => {
                write!(
                    f,
                    "could not read config file {}: {}",
                    path.display(),
                    source
                )
            }
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::BadToml { source, .. } => Some(source.as_ref()),
            ResolveError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Resolve a byte offset within the original TOML source to a 1-based
/// line / column pair. `toml::de::Error::span()` gives the offset but
/// not the text; the error carries the source so this can scan it.
fn line_col_in(input: &str, byte_offset: usize) -> (usize, usize) {
    let mut line = 1usize;
    let mut col = 1usize;
    for (i, ch) in input.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// `serde(deny_unknown_fields)` produces messages shaped like:
///   ``unknown field `foo`, expected one of `bar`, `baz`, `qux` ``
/// Extract the unknown field + the candidate list, run a typo-tolerant
/// match against the candidates, and format a "did you mean" hint. None
/// if the message doesn't match the shape (e.g. it's a type-mismatch
/// error rather than an unknown-field error) or no candidate is close
/// enough.
pub(crate) fn unknown_field_suggestion(msg: &str) -> Option<String> {
    let after_prefix = msg.strip_prefix("unknown field `")?;
    let (field, rest) = after_prefix.split_once("`, expected one of ")?;
    if field.is_empty() {
        return None;
    }
    // Rest is `cand1`, `cand2`, ... — strip surrounding backticks.
    let candidates: Vec<&str> = rest
        .split(", ")
        .map(|c| c.trim().trim_matches('`'))
        .filter(|c| !c.is_empty())
        .collect();
    closest_match(field, candidates.iter().copied(), 3).map(|m| format!("did you mean `{}`?", m))
}

/// Hand-rolled Levenshtein for typo suggestions on unknown fields and
/// unknown theme names. Limited to small inputs (TOML keys), so the
/// O(n*m) cost is irrelevant. Returns the closest candidate whose
/// edit distance is at most `cutoff`, or None.
pub(super) fn closest_match<'a, I: IntoIterator<Item = &'a str>>(
    target: &str,
    candidates: I,
    cutoff: usize,
) -> Option<&'a str> {
    let target_lower = target.to_ascii_lowercase();
    let mut best: Option<(&str, usize)> = None;
    for cand in candidates {
        let d = levenshtein(&target_lower, &cand.to_ascii_lowercase());
        if d <= cutoff && best.is_none_or(|(_, bd)| d < bd) {
            best = Some((cand, d));
        }
    }
    best.map(|(c, _)| c)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr: Vec<usize> = vec![0; b.len() + 1];
    for (i, ac) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, bc) in b.iter().enumerate() {
            let cost = if ac == bc { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_error_stays_small() {
        assert!(std::mem::size_of::<ResolveError>() <= 96);
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("text_color", "texcolor"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn unknown_field_suggestion_extracts_match() {
        let msg = "unknown field `text_colr`, expected one of `text_color`, `background_color`, `font_size_pt`";
        assert_eq!(
            unknown_field_suggestion(msg),
            Some("did you mean `text_color`?".to_string())
        );
    }

    #[test]
    fn unknown_field_suggestion_returns_none_when_far_off() {
        let msg = "unknown field `xyzzy`, expected one of `text_color`, `background_color`";
        assert_eq!(unknown_field_suggestion(msg), None);
    }

    #[test]
    fn unknown_field_suggestion_returns_none_for_non_matching_shape() {
        // Type-mismatch errors don't contain "expected one of"; we
        // can't synthesize a suggestion for them.
        let msg = "invalid type: integer `12`, expected a string";
        assert_eq!(unknown_field_suggestion(msg), None);
    }

    #[test]
    fn closest_match_under_cutoff() {
        let opts = ["text_color", "background_color", "font_size_pt"];
        assert_eq!(closest_match("texcolor", opts, 3), Some("text_color"));
        assert_eq!(
            closest_match("backround_color", opts, 3),
            Some("background_color")
        );
        assert_eq!(closest_match("totally_unrelated", opts, 3), None);
    }
}
