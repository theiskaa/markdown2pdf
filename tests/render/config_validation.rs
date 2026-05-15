//! Config-input robustness (W7g). Two contracts:
//!
//! 1. `load_config_strict` must surface malformed input as a typed
//!    `ResolveError::BadToml` — never panic.
//! 2. `parse_into_bytes` (the soft-fail path) must always produce a
//!    valid PDF: bad config degrades to the default theme, hostile
//!    numerics are clamped so the renderer never hangs or crashes.

use markdown2pdf::config::{ConfigSource, load_config_strict};
use markdown2pdf::styling::ResolveError;

use super::common::*;

/// `load_config_strict` on `cfg` must be `Err(BadToml)` (typed, no
/// panic). Wrapped in catch_unwind so a regression that panics shows
/// up as a failed assertion.
fn must_be_bad_toml(cfg: &str) {
    let cfg = cfg.to_string();
    let result = std::panic::catch_unwind(move || {
        load_config_strict(ConfigSource::Embedded(&cfg), None)
    });
    let result = result.expect("config parse panicked instead of erroring");
    assert!(
        matches!(result, Err(ResolveError::BadToml { .. })),
        "expected ResolveError::BadToml, got {:?}",
        result
    );
}

/// The soft-fail render path: any config (even garbage) must produce
/// a structurally valid PDF, never panic, never hang.
fn render_is_valid(md: &str, cfg: &str) {
    let md = md.to_string();
    let cfg = cfg.to_string();
    let bytes = std::panic::catch_unwind(move || render(&md, &cfg))
        .expect("render panicked on adversarial config");
    assert!(
        pdf_well_formed(&bytes),
        "render produced a malformed PDF for adversarial config"
    );
}

mod color_errors_are_typed {
    use super::*;

    #[test]
    fn missing_hash_prefix() {
        must_be_bad_toml("[paragraph]\ntext_color = \"FF0000\"\n");
    }

    #[test]
    fn bad_hex_digit() {
        must_be_bad_toml("[paragraph]\ntext_color = \"#GG0000\"\n");
    }

    #[test]
    fn wrong_hex_length_four() {
        must_be_bad_toml("[paragraph]\ntext_color = \"#FF00\"\n");
    }

    #[test]
    fn wrong_hex_length_eight() {
        must_be_bad_toml("[paragraph]\ntext_color = \"#FF00FF00\"\n");
    }

    #[test]
    fn empty_color_string() {
        must_be_bad_toml("[paragraph]\ntext_color = \"\"\n");
    }

    #[test]
    fn named_color_not_supported() {
        // We only accept hex / struct / array. A CSS name is a clean
        // error, not a panic.
        must_be_bad_toml("[paragraph]\ntext_color = \"red\"\n");
    }

    #[test]
    fn rgb_function_not_supported() {
        must_be_bad_toml("[paragraph]\ntext_color = \"rgb(255,0,0)\"\n");
    }

    #[test]
    fn struct_unknown_field() {
        must_be_bad_toml(
            "[paragraph]\ntext_color = { r = 1, g = 2, b = 3, a = 4 }\n",
        );
    }

    #[test]
    fn struct_missing_field() {
        must_be_bad_toml("[paragraph]\ntext_color = { r = 1, g = 2 }\n");
    }

    #[test]
    fn struct_value_over_255() {
        must_be_bad_toml(
            "[paragraph]\ntext_color = { r = 300, g = 0, b = 0 }\n",
        );
    }

    #[test]
    fn struct_negative_value() {
        must_be_bad_toml(
            "[paragraph]\ntext_color = { r = -1, g = 0, b = 0 }\n",
        );
    }

    #[test]
    fn array_too_short() {
        must_be_bad_toml("[paragraph]\ntext_color = [255, 0]\n");
    }

    #[test]
    fn array_value_over_255() {
        must_be_bad_toml("[paragraph]\ntext_color = [999, 0, 0]\n");
    }

    #[test]
    fn background_color_same_rules() {
        must_be_bad_toml("[blockquote]\nbackground_color = \"not-a-color\"\n");
    }
}

mod bad_color_soft_fails_to_default {
    use super::*;

    #[test]
    fn invalid_color_still_renders_valid_pdf() {
        render_is_valid("Body text.", "[paragraph]\ntext_color = \"#ZZZ\"\n");
    }

    #[test]
    fn garbage_toml_still_renders() {
        render_is_valid("Body text.", "this is not valid toml {{{{");
    }

    #[test]
    fn unknown_field_still_renders() {
        render_is_valid(
            "Body text.",
            "[paragraph]\nnonexistent_field = 42\n",
        );
    }
}

mod numeric_clamping {
    use super::*;

    #[test]
    fn negative_font_size_does_not_hang_or_crash() {
        // Zero/negative font size → glyph advances 0 → naive wrap
        // would never progress. Clamp guards against the hang. Test
        // completing at all proves no infinite loop.
        render_is_valid(
            "A paragraph with several words that must wrap to lines.",
            "[paragraph]\nfont_size_pt = -5.0\n",
        );
    }

    #[test]
    fn zero_font_size_does_not_hang() {
        render_is_valid(
            "Word word word word word word word word word word.",
            "[paragraph]\nfont_size_pt = 0.0\n",
        );
    }

    #[test]
    fn huge_font_size_does_not_crash() {
        // 5000pt text overflows every page but must not panic.
        render_is_valid(
            "Huge text here.",
            "[paragraph]\nfont_size_pt = 5000.0\n",
        );
    }

    #[test]
    fn negative_line_height_does_not_hang() {
        render_is_valid(
            "Several lines of body text that wrap across the column width here.",
            "[paragraph]\nline_height = -2.0\n",
        );
    }

    #[test]
    fn zero_line_height_does_not_hang() {
        render_is_valid(
            "Several lines of body text that wrap across the column width here.",
            "[paragraph]\nline_height = 0.0\n",
        );
    }

    #[test]
    fn negative_margins_do_not_break_pagination() {
        render_is_valid(
            &multi_page_markdown(10),
            "[paragraph]\nmargin_before_pt = -50.0\nmargin_after_pt = -50.0\n",
        );
    }

    #[test]
    fn negative_padding_does_not_crash() {
        render_is_valid(
            "Padded block content.",
            "[blockquote]\npadding = -20.0\n",
        );
    }

    #[test]
    fn negative_indent_does_not_crash() {
        render_is_valid("Indented paragraph.", "[paragraph]\nindent_pt = -100.0\n");
    }

    #[test]
    fn clamped_font_size_resolves_to_positive() {
        // Verify the clamp actually fires at resolve time, not just
        // that render survives.
        let style =
            load_config_strict(ConfigSource::Embedded("[paragraph]\nfont_size_pt = -10.0\n"), None)
                .expect("config should resolve (clamp, not error)");
        assert!(
            style.paragraph.font_size_pt > 0.0,
            "negative font size must clamp to a positive value, got {}",
            style.paragraph.font_size_pt
        );
    }

    #[test]
    fn clamped_line_height_resolves_to_positive() {
        let style =
            load_config_strict(ConfigSource::Embedded("[paragraph]\nline_height = -1.0\n"), None)
                .expect("config should resolve");
        assert!(
            style.paragraph.line_height > 0.0,
            "negative line height must clamp positive, got {}",
            style.paragraph.line_height
        );
    }

    #[test]
    fn clamped_padding_resolves_nonnegative() {
        let style =
            load_config_strict(ConfigSource::Embedded("[blockquote]\npadding = -30.0\n"), None)
                .expect("config should resolve");
        let p = &style.blockquote.padding;
        assert!(
            p.top >= 0.0 && p.right >= 0.0 && p.bottom >= 0.0 && p.left >= 0.0,
            "negative padding must clamp to >= 0, got {:?}",
            (p.top, p.right, p.bottom, p.left)
        );
    }

    #[test]
    fn valid_numerics_pass_through_unchanged() {
        // Sanity: clamping must not alter legitimate values.
        let style = load_config_strict(
            ConfigSource::Embedded(
                "[paragraph]\nfont_size_pt = 11.0\nline_height = 1.5\nmargin_before_pt = 6.0\n",
            ),
            None,
        )
        .expect("config should resolve");
        assert_eq!(style.paragraph.font_size_pt, 11.0);
        assert_eq!(style.paragraph.line_height, 1.5);
        assert_eq!(style.paragraph.margin_before_pt, 6.0);
    }
}
