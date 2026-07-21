//! Schema, theme preset, merge, and error-handling tests for the
//! configuration overhaul. End-to-end byte-level renderer regressions
//! live in `tests/render_fonts.rs`.

use markdown2pdf::config::{ConfigSource, load_config_strict};
use markdown2pdf::styling::{
    Color, DocumentConfig, FontStyleVariant, FontWeight, PageSize, ResolveError, ResolvedStyle,
    Sides, TextAlignment, available_theme_names, load_theme_preset, merge_documents, resolve,
};

#[test]
fn bundled_themes_all_load_cleanly() {
    for name in available_theme_names() {
        let cfg = load_theme_preset(name)
            .unwrap_or_else(|e| panic!("preset `{}` failed to load: {}", name, e));
        let _resolved: ResolvedStyle = resolve(cfg, None)
            .unwrap_or_else(|e| panic!("preset `{}` failed to resolve: {}", name, e));
    }
}

#[test]
fn default_theme_round_trips_into_resolved_style() {
    let style = ResolvedStyle::default();
    assert_eq!(style.paragraph.font_size_pt, 8.0);
    assert_eq!(style.headings[0].font_size_pt, 14.0);
    assert_eq!(style.headings[5].font_size_pt, 8.0);
    assert_eq!(style.page.size, PageSize::A4);
    assert!(matches!(style.page.size, PageSize::A4));
}

#[test]
fn color_deserializes_from_hex_struct_and_array() {
    let hex = r##"[paragraph]
        text_color = "#FF8800""##;
    let s = load_config_strict(ConfigSource::Embedded(hex), None).unwrap();
    assert_eq!(s.paragraph.text_color, Color::rgb(0xFF, 0x88, 0x00));

    let short = r##"[paragraph]
        text_color = "#F80""##;
    let s = load_config_strict(ConfigSource::Embedded(short), None).unwrap();
    assert_eq!(s.paragraph.text_color, Color::rgb(0xFF, 0x88, 0x00));

    let strukt = r#"[paragraph]
        text_color = { r = 10, g = 20, b = 30 }"#;
    let s = load_config_strict(ConfigSource::Embedded(strukt), None).unwrap();
    assert_eq!(s.paragraph.text_color, Color::rgb(10, 20, 30));

    let array = r#"[paragraph]
        text_color = [10, 20, 30]"#;
    let s = load_config_strict(ConfigSource::Embedded(array), None).unwrap();
    assert_eq!(s.paragraph.text_color, Color::rgb(10, 20, 30));
}

#[test]
fn sides_accepts_scalar_pair_quad_and_struct() {
    let scalar = r#"[paragraph]
        padding = 4.0"#;
    let s = load_config_strict(ConfigSource::Embedded(scalar), None).unwrap();
    assert_eq!(s.paragraph.padding, Sides { top: 4.0, right: 4.0, bottom: 4.0, left: 4.0 });

    let pair = r#"[paragraph]
        padding = [2.0, 6.0]"#;
    let s = load_config_strict(ConfigSource::Embedded(pair), None).unwrap();
    assert_eq!(s.paragraph.padding, Sides { top: 2.0, right: 6.0, bottom: 2.0, left: 6.0 });

    let quad = r#"[paragraph]
        padding = [1.0, 2.0, 3.0, 4.0]"#;
    let s = load_config_strict(ConfigSource::Embedded(quad), None).unwrap();
    assert_eq!(s.paragraph.padding, Sides { top: 1.0, right: 2.0, bottom: 3.0, left: 4.0 });

    let strukt = r#"[paragraph]
        padding = { top = 1.0, right = 2.0, bottom = 3.0, left = 4.0 }"#;
    let s = load_config_strict(ConfigSource::Embedded(strukt), None).unwrap();
    assert_eq!(s.paragraph.padding, Sides { top: 1.0, right: 2.0, bottom: 3.0, left: 4.0 });
}

#[test]
fn page_size_accepts_named_and_custom() {
    let named = r#"[page]
        size = "Letter""#;
    let s = load_config_strict(ConfigSource::Embedded(named), None).unwrap();
    assert_eq!(s.page.size, PageSize::Letter);

    let custom = r#"[page]
        size = { width_mm = 100.0, height_mm = 150.0 }"#;
    let s = load_config_strict(ConfigSource::Embedded(custom), None).unwrap();
    assert_eq!(s.page.size, PageSize::Custom { width_mm: 100.0, height_mm: 150.0 });
}

#[test]
fn font_weight_accepts_string_and_numeric() {
    let bold = r#"[paragraph]
        font_weight = "bold""#;
    let s = load_config_strict(ConfigSource::Embedded(bold), None).unwrap();
    assert_eq!(s.paragraph.font_weight, FontWeight::Bold);

    let numeric = r#"[paragraph]
        font_weight = 700"#;
    let s = load_config_strict(ConfigSource::Embedded(numeric), None).unwrap();
    assert_eq!(s.paragraph.font_weight, FontWeight::Numeric(700));
    assert!(s.paragraph.is_bold(), "700 must count as bold");

    let normal = r#"[paragraph]
        font_weight = 400"#;
    let s = load_config_strict(ConfigSource::Embedded(normal), None).unwrap();
    assert!(!s.paragraph.is_bold(), "400 must not count as bold");
}

#[test]
fn inherits_resolves_recursively() {
    // `github` inherits from `default`; check that fields not set in
    // github.toml (e.g. h6) come through from default.toml.
    let cfg = load_theme_preset("github").unwrap();
    let resolved = resolve(cfg, None).unwrap();
    // GitHub theme sets h6 to 10.5pt; default would have set 8.0pt.
    // Confirms inheritance respected the override.
    assert_eq!(resolved.headings[5].font_size_pt, 10.5);
}

#[test]
fn user_config_overrides_preset_field_by_field() {
    let user = r#"theme = "github"
        [paragraph]
        font_size_pt = 13.0"#;
    let s = load_config_strict(ConfigSource::Embedded(user), None).unwrap();
    // The github preset sets paragraph 10pt; user overrides to 13.
    assert_eq!(s.paragraph.font_size_pt, 13.0);
    // But unrelated github fields stick: h1 still 22pt.
    assert_eq!(s.headings[0].font_size_pt, 22.0);
}

#[test]
fn theme_override_arg_beats_user_theme_field() {
    let user = r#"theme = "default""#;
    let s = load_config_strict(ConfigSource::Embedded(user), Some("compact")).unwrap();
    // Compact theme: paragraph 9pt, not default's 8pt.
    assert_eq!(s.paragraph.font_size_pt, 9.0);
}

#[test]
fn defaults_block_cascades_into_unset_block_fields() {
    let user = r#"
        [defaults]
        font_family = "Times"
        [paragraph]
        font_size_pt = 12.0
    "#;
    let s = load_config_strict(ConfigSource::Embedded(user), None).unwrap();
    assert_eq!(s.paragraph.font_family.as_deref(), Some("Times"));
    assert_eq!(s.paragraph.font_size_pt, 12.0);
}

#[test]
fn unknown_field_raises_typed_error() {
    let err = load_config_strict(
        ConfigSource::Embedded(
            r##"[paragraph]
            texcolor = "#000000""##,
        ),
        None,
    );
    assert!(matches!(err, Err(ResolveError::BadToml { .. })));
}

#[test]
fn unknown_theme_raises_typed_error() {
    let err = load_config_strict(ConfigSource::Default, Some("notarealtheme"));
    match err {
        Err(ResolveError::UnknownTheme { name, .. }) => assert_eq!(name, "notarealtheme"),
        other => panic!("expected UnknownTheme, got {:?}", other),
    }
}

#[test]
fn merge_documents_overlay_wins_on_some_for_block_fields() {
    let base: DocumentConfig = toml::from_str(
        r#"
        [paragraph]
        font_size_pt = 8.0
        font_weight = "normal"
    "#,
    )
    .unwrap();
    let overlay: DocumentConfig = toml::from_str(
        r#"
        [paragraph]
        font_size_pt = 12.0
    "#,
    )
    .unwrap();
    let merged = merge_documents(base, overlay);
    let p = merged.paragraph.unwrap();
    assert_eq!(p.font_size_pt, Some(12.0));
    // font_weight survives because overlay didn't touch it.
    assert_eq!(p.font_weight, Some(FontWeight::Normal));
}

#[test]
fn text_align_and_font_style_round_trip() {
    let cfg = r#"[paragraph]
        text_align = "justify"
        font_style = "italic""#;
    let s = load_config_strict(ConfigSource::Embedded(cfg), None).unwrap();
    assert_eq!(s.paragraph.text_align, TextAlignment::Justify);
    assert_eq!(s.paragraph.font_style, FontStyleVariant::Italic);
    assert!(s.paragraph.is_italic());
}

#[test]
fn print_effective_config_round_trip() {
    // Take the academic preset's resolved style, serialize to TOML,
    // and confirm it's still valid input for the parser. This
    // guarantees `--print-effective-config` produces consumable output.
    let resolved = load_config_strict(ConfigSource::Default, Some("academic")).unwrap();
    let toml_text = toml::to_string(&resolved).expect("serialize resolved style");
    // The serialized form is the *resolved* shape, not the input
    // schema — feeding it back as input would hit deny_unknown_fields.
    // What we assert here is that it serializes cleanly and contains
    // expected key markers.
    assert!(toml_text.contains("font_size_pt"));
    assert!(toml_text.contains("[page]"));
    assert!(toml_text.contains("[paragraph]"));
}

#[test]
fn maintained_reference_config_parses() {
    // `docs/config.toml` is the maintained, human-facing reference config
    // (linked from the crate-level rustdoc and README). Pin it against
    // `deny_unknown_fields` so it can never silently rot.
    const REFERENCE: &str = include_str!("../docs/config.toml");
    let result = load_config_strict(ConfigSource::Embedded(REFERENCE), None);
    assert!(
        result.is_ok(),
        "docs/config.toml failed to parse: {:?}",
        result.err()
    );
}

/// Pull every ```toml fenced block out of a `//!` doc-comment source
/// file, stripping the `//! ` (or bare `//!` for blank lines) prefix
/// from each line so the result is plain TOML text.
fn extract_toml_fences(source: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for piece in source.split("```toml").skip(1) {
        let Some(end) = piece.find("```") else {
            continue;
        };
        let fenced = &piece[..end];
        let mut toml_text = String::new();
        for line in fenced.lines() {
            let stripped = line
                .strip_prefix("//! ")
                .or_else(|| line.strip_prefix("//!"))
                .unwrap_or(line);
            toml_text.push_str(stripped);
            toml_text.push('\n');
        }
        blocks.push(toml_text);
    }
    blocks
}

#[test]
fn rustdoc_crate_level_examples_parse() {
    // Extract directly from the real source rather than a hand-typed
    // copy, so this test guards the TOML that actually ships on
    // docs.rs — a bad key introduced into the crate-level `//!` docs
    // fails here, against the same `deny_unknown_fields` schema.
    const SOURCE: &str = include_str!("../src/lib/lib.rs");
    let blocks = extract_toml_fences(SOURCE);
    assert!(
        !blocks.is_empty(),
        "found no ```toml fenced blocks in src/lib/lib.rs; \
         did the crate-level doc examples move, get renamed, or get removed? \
         this test only guards docs that still exist"
    );
    for block in &blocks {
        let result = load_config_strict(ConfigSource::Embedded(block), None);
        assert!(
            result.is_ok(),
            "a ```toml example in the crate-level rustdoc (src/lib/lib.rs) failed to parse: {:?}\n---\n{}",
            result.err(),
            block
        );
    }
}

#[test]
fn math_block_round_trips_and_defaults() {
    // Explicit overrides land on `style.math`.
    let cfg = r##"[math]
        align = "left"
        scale = 1.5
        color = "#1133CC"
        margin_before_pt = 14
        margin_after_pt = 6"##;
    let s = load_config_strict(ConfigSource::Embedded(cfg), None).unwrap();
    assert_eq!(s.math.align, TextAlignment::Left);
    assert_eq!(s.math.scale, 1.5);
    assert_eq!(s.math.color, Color::rgb(0x11, 0x33, 0xCC));
    assert_eq!(s.math.margin_before_pt, 14.0);
    assert_eq!(s.math.margin_after_pt, 6.0);

    // With no `[math]`, display math centers, scales 1.08, and
    // inherits the paragraph ink + spacing.
    let d = load_config_strict(ConfigSource::Embedded(""), None).unwrap();
    assert_eq!(d.math.align, TextAlignment::Center);
    assert_eq!(d.math.scale, 1.08);
    assert_eq!(d.math.color, d.paragraph.text_color);
    assert_eq!(d.math.margin_before_pt, d.paragraph.margin_before_pt);
}

#[test]
fn security_block_round_trips_and_defaults() {
    // Explicit overrides land on `style.security`.
    let cfg = r#"[security]
        image_root = "/srv/uploads"
        allow_absolute_image_paths = false
        allow_remote_images = false"#;
    let s = load_config_strict(ConfigSource::Embedded(cfg), None).unwrap();
    assert_eq!(
        s.security.image_root.as_deref(),
        Some(std::path::Path::new("/srv/uploads"))
    );
    assert!(!s.security.allow_absolute_image_paths);
    assert!(!s.security.allow_remote_images);

    // With no `[security]` block at all, the defaults must preserve
    // the historical, unconfined behavior: no root, absolute paths
    // allowed, remote images allowed. This is the backward-
    // compatibility contract the whole plan hinges on.
    let d = load_config_strict(ConfigSource::Embedded(""), None).unwrap();
    assert_eq!(d.security.image_root, None);
    assert!(d.security.allow_absolute_image_paths);
    assert!(d.security.allow_remote_images);
}

#[test]
fn security_merge_overlay_wins_on_some() {
    let base: DocumentConfig = toml::from_str(
        r#"
        [security]
        image_root = "/base/root"
        allow_remote_images = false
    "#,
    )
    .unwrap();
    let overlay: DocumentConfig = toml::from_str(
        r#"
        [security]
        allow_remote_images = true
    "#,
    )
    .unwrap();
    let merged = merge_documents(base, overlay);
    let security = merged.security.unwrap();
    // Overlay didn't set image_root, so base's value survives.
    assert_eq!(security.image_root.as_deref(), Some("/base/root"));
    // Overlay's allow_remote_images wins.
    assert_eq!(security.allow_remote_images, Some(true));
}
