//! Configuration-loading error type with line/column + typo
//! suggestion formatting.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ResolveError {
    /// TOML failed to parse (syntax error, type mismatch, unknown
    /// field). Wraps `toml::de::Error` for span info.
    BadToml {
        source: toml::de::Error,
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
                file,
                suggestion,
            } => {
                let where_ = file
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "<config>".to_string());
                if let Some(span) = source.span() {
                    let (line, col) = line_col_for(source, span.start);
                    write!(f, "error in {} at line {}, column {}: {}", where_, line, col, source.message())?;
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
                write!(
                    f,
                    "theme inheritance cycle: {}",
                    chain.join(" -> ")
                )
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
                write!(f, "could not read config file {}: {}", path.display(), source)
            }
        }
    }
}

impl std::error::Error for ResolveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ResolveError::BadToml { source, .. } => Some(source),
            ResolveError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// `toml::de::Error::span()` returns the byte range; we lift that to a
/// line / column pair by scanning the original source string. The
/// source isn't always available, so this is a best-effort helper —
/// when we can't find it, we fall back to printing the raw span.
fn line_col_for(source: &toml::de::Error, byte_offset: usize) -> (usize, usize) {
    if let Some(input) = source_input(source) {
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
    } else {
        (0, byte_offset)
    }
}

/// `toml::de::Error` keeps the source text internally for its own
/// pretty-printed message; we don't have public access to it. We work
/// around that by re-parsing nothing here — the wrapper that built the
/// error already knows the input and can supply it via a side channel
/// if desired. For now this returns None and the line/col fall back to
/// raw byte offset.
fn source_input(_err: &toml::de::Error) -> Option<&str> {
    None
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
        if d <= cutoff && best.map_or(true, |(_, bd)| d < bd) {
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
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("text_color", "texcolor"), 2);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn closest_match_under_cutoff() {
        let opts = ["text_color", "background_color", "font_size_pt"];
        assert_eq!(closest_match("texcolor", opts, 3), Some("text_color"));
        assert_eq!(closest_match("backround_color", opts, 3), Some("background_color"));
        assert_eq!(closest_match("totally_unrelated", opts, 3), None);
    }
}
