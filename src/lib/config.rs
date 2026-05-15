//! Configuration loading: TOML on disk / embedded string → `ResolvedStyle`.
//!
//! Thin orchestration layer over the `styling/` module: pick a source,
//! parse with serde, merge user fields over the named theme preset,
//! lower to `ResolvedStyle`. Errors surface through
//! [`styling::ResolveError`].

use crate::styling::{
    DocumentConfig, ResolveError, ResolvedStyle, merge::resolve_with_overrides,
};
use std::fs;
use std::path::Path;

/// Where the styling configuration comes from.
#[derive(Debug, Clone)]
pub enum ConfigSource<'a> {
    /// Use the bundled `default` theme preset, no user overrides.
    Default,
    /// Use a bundled theme preset by name (`"default"`, `"github"`,
    /// `"academic"`, `"minimal"`, `"compact"`, `"modern"`). Unknown
    /// names surface as [`ResolveError::UnknownTheme`] from
    /// [`load_config_strict`]; [`load_config_from_source`] silently
    /// falls back to the bundled default.
    Theme(&'a str),
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
    load_config_strict_with_overrides(source, theme_override, None)
}

/// Like [`load_config_strict`], but applies `overrides_toml` (a TOML
/// fragment, typically built from CLI flags) as the highest-priority
/// layer — winning over the config file *and* `--theme`. The fragment
/// is parsed through the **same** schema + typo-suggestion error path
/// as a real config file, so an unknown key surfaces as
/// [`ResolveError::BadToml`] with a hint. `None` is equivalent to
/// [`load_config_strict`].
pub fn load_config_strict_with_overrides(
    source: ConfigSource,
    theme_override: Option<&str>,
    overrides_toml: Option<&str>,
) -> Result<ResolvedStyle, ResolveError> {
    // Parse the override fragment once, reusing the config-file error
    // mapping (unknown key → BadToml + suggestion).
    let overrides: Option<DocumentConfig> = match overrides_toml {
        Some(text) if !text.trim().is_empty() => {
            let parsed = toml::from_str(text).map_err(|source| {
                let suggestion =
                    crate::styling::error::unknown_field_suggestion(source.message());
                ResolveError::BadToml {
                    source,
                    file: None,
                    suggestion,
                }
            })?;
            Some(parsed)
        }
        _ => None,
    };

    let (toml_text, file_for_errors) = match source {
        ConfigSource::Default => {
            return resolve_with_overrides(
                DocumentConfig::default(),
                theme_override,
                overrides,
            );
        }
        ConfigSource::Theme(name) => {
            // CLI `--theme` semantics: caller-supplied theme_override
            // still wins so a user can layer overrides on top.
            let theme = theme_override.or(Some(name));
            return resolve_with_overrides(DocumentConfig::default(), theme, overrides);
        }
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

    resolve_with_overrides(user, theme_override, overrides)
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
    fn theme_source_picks_named_preset() {
        let style = load_config_strict(ConfigSource::Theme("github"), None).unwrap();
        assert_eq!(style.paragraph.font_size_pt, 10.0);
    }

    #[test]
    fn theme_source_unknown_returns_typed_error() {
        let err = load_config_strict(ConfigSource::Theme("doesnotexist"), None);
        match err {
            Err(ResolveError::UnknownTheme { name, .. }) => assert_eq!(name, "doesnotexist"),
            other => panic!("expected UnknownTheme, got {:?}", other),
        }
    }

    #[test]
    fn theme_source_falls_back_via_load_from_source() {
        // Soft-fail helper masks the typed error; an unknown theme
        // should drop us to the bundled default.
        let style = load_config_from_source(ConfigSource::Theme("doesnotexist"));
        assert_eq!(style.paragraph.font_size_pt, 8.0);
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

    // --- CLI override layer (issue #67) -----------------------------

    #[test]
    fn overrides_none_equals_no_overrides() {
        let a = load_config_strict(ConfigSource::Default, None).unwrap();
        let b =
            load_config_strict_with_overrides(ConfigSource::Default, None, None).unwrap();
        assert_eq!(a.paragraph.font_size_pt, b.paragraph.font_size_pt);
        assert_eq!(a.headings[0].font_size_pt, b.headings[0].font_size_pt);
    }

    #[test]
    fn override_beats_embedded_config_file() {
        // Config file sets paragraph 9pt; the override sets the same
        // per-block key to 11pt and must win.
        let style = load_config_strict_with_overrides(
            ConfigSource::Embedded("[paragraph]\nfont_size_pt = 9.0\n"),
            None,
            Some("paragraph.font_size_pt = 11.0"),
        )
        .unwrap();
        assert_eq!(style.paragraph.font_size_pt, 11.0);
    }

    #[test]
    fn override_beats_theme_preset() {
        // github theme sets paragraph 10pt; override forces 13pt.
        let style = load_config_strict_with_overrides(
            ConfigSource::Theme("github"),
            None,
            Some("defaults.font_size_pt = 13.0"),
        )
        .unwrap();
        assert_eq!(style.paragraph.font_size_pt, 13.0);
    }

    #[test]
    fn override_dotted_heading_key() {
        let style = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some("headings.h1.font_size_pt = 28.0"),
        )
        .unwrap();
        assert_eq!(style.headings[0].font_size_pt, 28.0);
    }

    #[test]
    fn override_color_value() {
        let style = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some("blockquote.text_color = \"#888888\""),
        )
        .unwrap();
        assert_eq!(
            style.blockquote.text_color,
            crate::styling::Color::rgb(0x88, 0x88, 0x88)
        );
    }

    #[test]
    fn override_uniform_margins_scalar() {
        let style = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some("page.margins = 25.0"),
        )
        .unwrap();
        assert_eq!(style.page.margins_mm.top, 25.0);
        assert_eq!(style.page.margins_mm.left, 25.0);
    }

    #[test]
    fn override_metadata_and_footer() {
        let style = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some(
                "metadata.title = \"My Report\"\n\
                 footer.center = \"{page} / {total_pages}\"",
            ),
        )
        .unwrap();
        assert_eq!(style.metadata.title.as_deref(), Some("My Report"));
        assert_eq!(
            style.footer.as_ref().and_then(|f| f.center.clone()),
            Some("{page} / {total_pages}".to_string())
        );
    }

    #[test]
    fn invalid_override_key_is_typed_error_not_panic() {
        let err = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some("bogus.key = 1"),
        );
        assert!(matches!(err, Err(ResolveError::BadToml { .. })));
    }

    #[test]
    fn empty_override_fragment_is_noop() {
        let a = load_config_strict(ConfigSource::Default, None).unwrap();
        let b = load_config_strict_with_overrides(
            ConfigSource::Default,
            None,
            Some("   \n  "),
        )
        .unwrap();
        assert_eq!(a.paragraph.font_size_pt, b.paragraph.font_size_pt);
    }

    #[test]
    fn override_layers_on_top_of_theme_override_arg() {
        // theme_override (the --theme flag) selects github; the
        // overlay still wins for the field it sets.
        let style = load_config_strict_with_overrides(
            ConfigSource::Default,
            Some("github"),
            Some("defaults.font_size_pt = 7.5"),
        )
        .unwrap();
        assert_eq!(style.paragraph.font_size_pt, 7.5);
    }
}
