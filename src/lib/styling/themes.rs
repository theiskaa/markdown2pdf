//! Embedded theme preset registry.
//!
//! Six presets ship bundled. `default` is the exhaustive fallback;
//! every other preset uses `inherits = "default"` and only overrides
//! what makes it distinctive. Adding a theme is one `include_str!` +
//! one row in `PRESETS`.

use super::error::{ResolveError, closest_match};
use super::schema::DocumentConfig;

const DEFAULT_TOML: &str = include_str!("themes/default.toml");
const GITHUB_TOML: &str = include_str!("themes/github.toml");
const ACADEMIC_TOML: &str = include_str!("themes/academic.toml");
const MINIMAL_TOML: &str = include_str!("themes/minimal.toml");
const COMPACT_TOML: &str = include_str!("themes/compact.toml");
const MODERN_TOML: &str = include_str!("themes/modern.toml");

const PRESETS: &[(&str, &str)] = &[
    ("default", DEFAULT_TOML),
    ("github", GITHUB_TOML),
    ("academic", ACADEMIC_TOML),
    ("minimal", MINIMAL_TOML),
    ("compact", COMPACT_TOML),
    ("modern", MODERN_TOML),
];

/// Resolve a theme name to a parsed `DocumentConfig`. Walks
/// `inherits = "..."` chains recursively; rejects cycles.
pub fn load_theme_preset(name: &str) -> Result<DocumentConfig, ResolveError> {
    let mut chain: Vec<String> = Vec::new();
    load_with_chain(name, &mut chain)
}

fn load_with_chain(name: &str, chain: &mut Vec<String>) -> Result<DocumentConfig, ResolveError> {
    if chain.iter().any(|n| n == name) {
        chain.push(name.to_string());
        return Err(ResolveError::InheritsCycle(chain.clone()));
    }
    chain.push(name.to_string());

    let raw = preset_source(name).ok_or_else(|| ResolveError::UnknownTheme {
        name: name.to_string(),
        suggestion: closest_match(name, PRESETS.iter().map(|(n, _)| *n), 3)
            .map(|s| s.to_string()),
    })?;

    let cfg: DocumentConfig = toml::from_str(raw).map_err(|e| ResolveError::BadToml {
        source: Box::new(e),
        input: raw.to_string(),
        file: None,
        suggestion: Some(format!("bundled theme `{}` failed to parse — this is a bug, please file an issue", name)),
    })?;

    if let Some(parent_name) = cfg.inherits.as_deref() {
        let parent = load_with_chain(parent_name, chain)?;
        let merged = super::merge::merge_documents(parent, cfg);
        Ok(merged)
    } else {
        Ok(cfg)
    }
}

fn preset_source(name: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(n, _)| *n == name).map(|(_, s)| *s)
}

/// All bundled theme names. Used for error messages and the CLI's
/// `--theme` arg help text.
pub fn available_theme_names() -> &'static [&'static str] {
    static NAMES: &[&str] = &["default", "github", "academic", "minimal", "compact", "modern"];
    NAMES
}
