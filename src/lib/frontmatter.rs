//! Detect and parse YAML or TOML front-matter at the top of a markdown
//! document.
//!
//! Supported delimiters:
//! - `---` (YAML, the convention used by Jekyll, Hugo, MkDocs, Zola
//!   when YAML is enabled, etc.)
//! - `+++` (TOML, Hugo's other supported format)
//!
//! The YAML side is a hand-rolled minimal parser — sufficient for the
//! flat key/value bag that frontmatter typically carries. It accepts:
//!
//! - `key: value`
//! - `key: "value"` / `key: 'value'`
//! - `key: [a, b, c]` (flow sequence)
//! - `key:\n  - a\n  - b` (block sequence)
//! - `#` comments and blank lines
//!
//! Nested mappings, multi-line scalars (`|`, `>`), anchors, and merge
//! keys are not supported. If the parser can't make sense of a line
//! it skips it; the goal is to extract well-known metadata keys
//! (title/author/etc.), not to be a conforming YAML implementation.
//!
//! The TOML side delegates to the `toml` crate.

use crate::styling::ResolvedMetadata;

/// Parsed frontmatter values, ready to be merged onto a resolved
/// style's metadata.
#[derive(Debug, Default, Clone)]
pub struct Frontmatter {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub keywords: Vec<String>,
}

impl Frontmatter {
    /// Layer the frontmatter on top of an existing metadata struct.
    /// Frontmatter wins for any field it specifies; absent fields
    /// leave the existing value untouched.
    pub fn apply(self, metadata: &mut ResolvedMetadata) {
        if let Some(v) = self.title {
            metadata.title = Some(v);
        }
        if let Some(v) = self.author {
            metadata.author = Some(v);
        }
        if let Some(v) = self.subject {
            metadata.subject = Some(v);
        }
        if let Some(v) = self.creator {
            metadata.creator = Some(v);
        }
        if !self.keywords.is_empty() {
            metadata.keywords = self.keywords;
        }
    }
}

/// Look for a frontmatter block at the start of `input`. On success
/// returns the parsed frontmatter and the byte offset where the
/// markdown body starts. On no-match returns `None` and the caller
/// passes `input` through unchanged.
pub fn extract(input: &str) -> Option<(Frontmatter, usize)> {
    let bytes = input.as_bytes();
    if bytes.starts_with(b"---\n") || bytes.starts_with(b"---\r\n") {
        let after_open = if bytes.starts_with(b"---\r\n") { 5 } else { 4 };
        find_close(input, after_open, "---").map(|(end, body_start)| {
            let body = &input[after_open..end];
            (parse_yaml(body), body_start)
        })
    } else if bytes.starts_with(b"+++\n") || bytes.starts_with(b"+++\r\n") {
        let after_open = if bytes.starts_with(b"+++\r\n") { 5 } else { 4 };
        find_close(input, after_open, "+++").map(|(end, body_start)| {
            let body = &input[after_open..end];
            (parse_toml(body), body_start)
        })
    } else {
        None
    }
}

/// Scan forward from `start` looking for a line that is exactly
/// `delim` (`---` or `+++`). Returns `(close_line_start, body_start)`
/// where `close_line_start` ends the frontmatter body and
/// `body_start` is where the markdown content resumes.
fn find_close(input: &str, start: usize, delim: &str) -> Option<(usize, usize)> {
    let mut pos = start;
    while pos < input.len() {
        let rest = &input[pos..];
        let line_end = rest.find('\n').map(|i| pos + i).unwrap_or(input.len());
        let line = input[pos..line_end].trim_end_matches('\r');
        if line == delim {
            let body_start = (line_end + 1).min(input.len());
            return Some((pos, body_start));
        }
        if line_end == input.len() {
            break;
        }
        pos = line_end + 1;
    }
    None
}

fn parse_yaml(body: &str) -> Frontmatter {
    let mut out = Frontmatter::default();
    let mut lines = body.lines().peekable();
    while let Some(raw) = lines.next() {
        let line = strip_comment(raw);
        if line.trim().is_empty() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent > 0 {
            continue;
        }
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = rest.trim();

        if value.is_empty() {
            // Block sequence on following lines: `keywords:\n  - a\n  - b`.
            let mut items = Vec::new();
            while let Some(next) = lines.peek() {
                let stripped = strip_comment(next);
                if stripped.trim().is_empty() {
                    lines.next();
                    continue;
                }
                let leading = stripped.len() - stripped.trim_start().len();
                if leading == 0 {
                    break;
                }
                let item = stripped.trim_start();
                if let Some(it) = item.strip_prefix("- ") {
                    items.push(unquote(it.trim()).to_string());
                    lines.next();
                } else {
                    break;
                }
            }
            if !items.is_empty() {
                assign(&mut out, key, YamlValue::List(items));
            }
        } else if let Some(inner) = value.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let items: Vec<String> = inner
                .split(',')
                .map(|s| unquote(s.trim()).to_string())
                .filter(|s| !s.is_empty())
                .collect();
            assign(&mut out, key, YamlValue::List(items));
        } else {
            assign(&mut out, key, YamlValue::Scalar(unquote(value).to_string()));
        }
    }
    out
}

fn parse_toml(body: &str) -> Frontmatter {
    #[derive(serde::Deserialize, Default)]
    #[serde(default)]
    struct Raw {
        title: Option<String>,
        author: Option<String>,
        subject: Option<String>,
        creator: Option<String>,
        keywords: Option<Vec<String>>,
    }
    let raw: Raw = toml::from_str(body).unwrap_or_default();
    Frontmatter {
        title: raw.title,
        author: raw.author,
        subject: raw.subject,
        creator: raw.creator,
        keywords: raw.keywords.unwrap_or_default(),
    }
}

enum YamlValue {
    Scalar(String),
    List(Vec<String>),
}

fn assign(fm: &mut Frontmatter, key: &str, value: YamlValue) {
    match (key.to_ascii_lowercase().as_str(), value) {
        ("title", YamlValue::Scalar(s)) => fm.title = Some(s),
        ("author" | "authors", YamlValue::Scalar(s)) => fm.author = Some(s),
        ("author" | "authors", YamlValue::List(v)) => fm.author = Some(v.join(", ")),
        ("subject" | "description", YamlValue::Scalar(s)) => fm.subject = Some(s),
        ("creator", YamlValue::Scalar(s)) => fm.creator = Some(s),
        ("keywords" | "tags", YamlValue::List(v)) => fm.keywords = v,
        ("keywords" | "tags", YamlValue::Scalar(s)) => {
            fm.keywords = s.split(',').map(|s| s.trim().to_string()).collect();
        }
        _ => {}
    }
}

fn strip_comment(line: &str) -> &str {
    if let Some((before, _)) = line.split_once('#') {
        before
    } else {
        line
    }
}

fn unquote(s: &str) -> &str {
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter_returns_none() {
        assert!(extract("# Hello world").is_none());
        assert!(extract("").is_none());
    }

    #[test]
    fn yaml_basic_keys() {
        let src = "---\ntitle: My Document\nauthor: Jane Doe\n---\n# Body";
        let (fm, off) = extract(src).expect("frontmatter parsed");
        assert_eq!(fm.title.as_deref(), Some("My Document"));
        assert_eq!(fm.author.as_deref(), Some("Jane Doe"));
        assert_eq!(&src[off..], "# Body");
    }

    #[test]
    fn yaml_quoted_values() {
        let src = "---\ntitle: \"With: colon\"\nsubject: 'quoted'\n---\nbody";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.title.as_deref(), Some("With: colon"));
        assert_eq!(fm.subject.as_deref(), Some("quoted"));
    }

    #[test]
    fn yaml_flow_list_keywords() {
        let src = "---\nkeywords: [rust, pdf, markdown]\n---\nbody";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.keywords, vec!["rust", "pdf", "markdown"]);
    }

    #[test]
    fn yaml_block_list_keywords() {
        let src = "---\nkeywords:\n  - rust\n  - pdf\n  - markdown\n---\nbody";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.keywords, vec!["rust", "pdf", "markdown"]);
    }

    #[test]
    fn yaml_comma_separated_keywords_string() {
        let src = "---\nkeywords: rust, pdf, markdown\n---\nbody";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.keywords, vec!["rust", "pdf", "markdown"]);
    }

    #[test]
    fn yaml_comments_and_blank_lines_ignored() {
        let src = "---\n# this is a comment\n\ntitle: Hi  # trailing\n---\nbody";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.title.as_deref(), Some("Hi"));
    }

    #[test]
    fn yaml_alias_keys() {
        let src = "---\nauthors: Alice\ntags: [one, two]\ndescription: A subject\n---\n";
        let (fm, _) = extract(src).unwrap();
        assert_eq!(fm.author.as_deref(), Some("Alice"));
        assert_eq!(fm.keywords, vec!["one", "two"]);
        assert_eq!(fm.subject.as_deref(), Some("A subject"));
    }

    #[test]
    fn toml_basic_keys() {
        let src = "+++\ntitle = \"My Document\"\nauthor = \"Jane Doe\"\nkeywords = [\"rust\", \"pdf\"]\n+++\nbody";
        let (fm, off) = extract(src).expect("frontmatter parsed");
        assert_eq!(fm.title.as_deref(), Some("My Document"));
        assert_eq!(fm.author.as_deref(), Some("Jane Doe"));
        assert_eq!(fm.keywords, vec!["rust", "pdf"]);
        assert_eq!(&src[off..], "body");
    }

    #[test]
    fn missing_close_returns_none() {
        let src = "---\ntitle: Foo\n\nstill in frontmatter";
        assert!(extract(src).is_none());
    }

    #[test]
    fn unrelated_triple_dash_in_body_not_consumed() {
        // The frontmatter must be at the very start; a `---` later is
        // a thematic break, not a frontmatter delimiter.
        let src = "Body\n\n---\n\nMore body.";
        assert!(extract(src).is_none());
    }

    #[test]
    fn apply_merges_onto_existing_metadata() {
        let mut meta = ResolvedMetadata {
            title: Some("Old".to_string()),
            author: None,
            subject: None,
            creator: Some("CLI".to_string()),
            keywords: vec!["existing".to_string()],
            language: None,
        };
        let fm = Frontmatter {
            title: Some("New".to_string()),
            author: Some("Alice".to_string()),
            subject: None,
            creator: None,
            keywords: vec!["fresh".to_string()],
        };
        fm.apply(&mut meta);
        assert_eq!(meta.title.as_deref(), Some("New"));
        assert_eq!(meta.author.as_deref(), Some("Alice"));
        assert_eq!(meta.creator.as_deref(), Some("CLI"));
        assert_eq!(meta.keywords, vec!["fresh"]);
    }

    #[test]
    fn crlf_line_endings_supported() {
        let src = "---\r\ntitle: Foo\r\n---\r\nbody";
        let (fm, _) = extract(src).expect("frontmatter parsed");
        assert_eq!(fm.title.as_deref(), Some("Foo"));
    }
}
