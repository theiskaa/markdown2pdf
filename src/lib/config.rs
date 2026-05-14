//! Configuration loading: TOML on disk / embedded string → `ResolvedStyle`.
//!
//! Thin orchestration layer over the `styling/` module: pick a source,
//! parse with serde, merge user fields over the named theme preset,
//! lower to `ResolvedStyle`. Errors surface through
//! [`styling::ResolveError`].

use crate::styling::{DocumentConfig, ResolveError, ResolvedStyle, merge::resolve};
use std::fs;
use std::path::Path;

/// Where the styling configuration comes from.
#[derive(Debug, Clone)]
pub enum ConfigSource<'a> {
    /// Use the bundled `default` theme preset, no user overrides.
    Default,
    /// Load and parse `path` as a user config file (see
    /// `docs/config.toml` in the repo for the full schema reference).
    File(&'a str),
    /// Treat `s` as the body of a TOML config (no I/O).
    Embedded(&'a str),
}

/// Load the styling configuration and resolve it to a concrete
/// `ResolvedStyle`. Surfaces a typed error on parse / I/O / unknown
/// theme / cyclic inheritance. Use [`load_config_from_source`] when
/// you want a silent fallback to the default theme.
///
/// `theme_override` lets the CLI's `--theme` flag win over the
/// `theme = "..."` field inside the user's config file.
pub fn load_config_strict(
    source: ConfigSource,
    theme_override: Option<&str>,
) -> Result<ResolvedStyle, ResolveError> {
    let (toml_text, file_for_errors) = match source {
        ConfigSource::Default => return resolve(DocumentConfig::default(), theme_override),
        ConfigSource::File(path) => {
            let p = Path::new(path).to_path_buf();
            let text = fs::read_to_string(&p).map_err(|source| ResolveError::Io {
                path: p.clone(),
                source,
            })?;
            (text, Some(p))
        }
        ConfigSource::Embedded(s) => (s.to_string(), None),
    };

    let user: DocumentConfig = toml::from_str(&toml_text).map_err(|source| {
        let suggestion = crate::styling::error::unknown_field_suggestion(source.message());
        ResolveError::BadToml {
            source,
            file: file_for_errors,
            suggestion,
        }
    })?;

    resolve(user, theme_override)
}

/// Soft-fail version of [`load_config_strict`]. On any error logs a
/// warning and returns the bundled default preset. Preserves the
/// historic behavior of `parse_into_file` / `parse_into_bytes` so
/// existing callers don't need to handle a new error variant.
pub fn load_config_from_source(source: ConfigSource) -> ResolvedStyle {
    match load_config_strict(source, None) {
        Ok(style) => style,
        Err(e) => {
            log::warn!(
                "could not load config; falling back to built-in default theme: {}",
                e
            );
            ResolvedStyle::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_source_loads_built_in_theme() {
        let style = load_config_from_source(ConfigSource::Default);
        assert_eq!(style.paragraph.font_size_pt, 8.0);
        assert_eq!(style.headings[0].font_size_pt, 14.0);
    }

    #[test]
    fn nonexistent_file_falls_back_to_default() {
        let style = load_config_from_source(ConfigSource::File("nonexistent.toml"));
        assert_eq!(style.paragraph.font_size_pt, 8.0);
    }

    #[test]
    fn embedded_config_overrides_paragraph_font_size() {
        let style = load_config_from_source(ConfigSource::Embedded(
            r#"
            [paragraph]
            font_size_pt = 11.0
            "#,
        ));
        assert_eq!(style.paragraph.font_size_pt, 11.0);
    }

    #[test]
    fn theme_preset_override() {
        let style = load_config_strict(ConfigSource::Default, Some("github")).unwrap();
        assert_eq!(style.paragraph.font_size_pt, 10.0);
    }

    #[test]
    fn unknown_theme_returns_typed_error() {
        let err = load_config_strict(ConfigSource::Default, Some("doesnotexist"));
        match err {
            Err(ResolveError::UnknownTheme { name, .. }) => assert_eq!(name, "doesnotexist"),
            other => panic!("expected UnknownTheme, got {:?}", other),
        }
    }

    #[test]
    fn invalid_toml_returns_typed_error() {
        let err = load_config_strict(ConfigSource::Embedded("not valid toml {{{"), None);
        match err {
            Err(ResolveError::BadToml { .. }) => {}
            other => panic!("expected BadToml, got {:?}", other),
        }
    }

    #[test]
    fn unknown_field_returns_typed_error() {
        let err = load_config_strict(
            ConfigSource::Embedded(
                r##"
                [paragraph]
                texcolor = "#000000"
                "##,
            ),
            None,
        );
        assert!(matches!(err, Err(ResolveError::BadToml { .. })));
    }
}
