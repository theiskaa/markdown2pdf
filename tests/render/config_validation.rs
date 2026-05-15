//! Config-input robustness (W7g). Two contracts:
//!
//! 1. `load_config_strict` must surface malformed input as a typed
//!    `ResolveError::BadToml` — never panic.
//! 2. `parse_into_bytes` (the soft-fail path) must always produce a
//!    valid PDF: bad config degrades to the default theme, hostile
//!    numerics are clamped so the renderer never hangs or crashes.

use markdown2pdf::config::{
    ConfigSource, load_config_strict, load_config_strict_with_overrides,
};
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

/// Inline styles (`code_inline`, `link`) lower through a different
/// path than block styles and historically skipped the anti-hang
/// font-size clamp — a zero/negative inline size reached the wrap
/// loop and never made progress.
mod inline_font_size_clamp {
    use super::*;

    #[test]
    fn zero_inline_code_size_does_not_hang() {
        render_is_valid(
            "Some `inline code` mixed into a wrapping paragraph of text.",
            "[code_inline]\nfont_size_pt = 0.0\n",
        );
    }

    #[test]
    fn negative_inline_link_size_does_not_hang() {
        render_is_valid(
            "A [link](https://example.com) sitting inside body text.",
            "[link]\nfont_size_pt = -5.0\n",
        );
    }

    #[test]
    fn inline_size_resolves_to_positive() {
        let style = load_config_strict(
            ConfigSource::Embedded("[code_inline]\nfont_size_pt = -5.0\n"),
            None,
        )
        .expect("config should resolve (clamp, not error)");
        assert!(
            style.code_inline.font_size_pt > 0.0,
            "negative inline font size must clamp positive, got {}",
            style.code_inline.font_size_pt
        );
    }
}

/// A custom page size is taken from config verbatim into all page
/// math. 0 / negative collapses the content box; NaN/inf make every
/// page-break comparison false (the break never fires). All must
/// degrade gracefully to a renderable page.
mod custom_page_size {
    use super::*;

    #[test]
    fn zero_custom_page_does_not_hang() {
        render_is_valid(
            &multi_page_markdown(5),
            "[page]\nsize = { width_mm = 0.0, height_mm = 0.0 }\n",
        );
    }

    #[test]
    fn negative_custom_page_does_not_hang() {
        render_is_valid(
            &multi_page_markdown(5),
            "[page]\nsize = { width_mm = -100.0, height_mm = -200.0 }\n",
        );
    }

    #[test]
    fn non_finite_custom_page_does_not_hang() {
        // 1e39 overflows an f32 to +inf; inf is non-finite and must
        // fall back rather than poison every page-break comparison.
        render_is_valid(
            &multi_page_markdown(5),
            "[page]\nsize = { width_mm = 1e39, height_mm = 1e39 }\n",
        );
    }

    #[test]
    fn absurdly_large_custom_page_does_not_crash() {
        render_is_valid(
            &multi_page_markdown(3),
            "[page]\nsize = { width_mm = 100000.0, height_mm = 100000.0 }\n",
        );
    }
}

/// `ResolveError::BadToml` must locate the failing key in the source
/// text. The byte-offset → line/column lift previously had no access
/// to the source and always reported "line 0".
mod config_error_location {
    use super::*;

    #[test]
    fn bad_toml_reports_real_line_not_zero() {
        let cfg = "[paragraph]\nfont_size_pt = 9.0\ntexcolor = \"#000000\"\n";
        let err = load_config_strict(ConfigSource::Embedded(cfg), None)
            .expect_err("unknown field must surface as a typed error");
        let msg = err.to_string();
        assert!(
            !msg.contains("at line 0,"),
            "config error still reports the dead 'line 0' offset: {msg}"
        );
        assert!(
            msg.contains("at line 3,"),
            "expected the unknown field (line 3) to be located: {msg}"
        );
    }
}

/// The lower-bound clamp handles non-positive/non-finite, but an
/// enormous *finite* value (e.g. `1e30`, or `1e39` which parses to
/// f32 inf) still overflowed page math, and non-finite letter
/// spacing poisoned every layout comparison. Negative letter spacing
/// is legitimate and must survive.
mod scalar_extremes {
    use super::*;

    #[test]
    fn huge_font_size_clamps_to_finite_range() {
        let style = load_config_strict(
            ConfigSource::Embedded("[paragraph]\nfont_size_pt = 1e30\n"),
            None,
        )
        .expect("config should resolve (clamp, not error)");
        let s = style.paragraph.font_size_pt;
        assert!(
            s.is_finite() && s > 0.0 && s <= 1000.0,
            "huge font size must clamp into a renderable range, got {s}"
        );
    }

    #[test]
    fn huge_line_height_clamps_to_finite_range() {
        let style = load_config_strict(
            ConfigSource::Embedded("[paragraph]\nline_height = 1e30\n"),
            None,
        )
        .expect("config should resolve");
        let lh = style.paragraph.line_height;
        assert!(
            lh.is_finite() && lh > 0.0 && lh <= 100.0,
            "huge line height must clamp into a renderable range, got {lh}"
        );
    }

    #[test]
    fn non_finite_letter_spacing_is_neutralised() {
        // 1e39 overflows an f32 to +inf; inf tracking would poison
        // every x-advance comparison.
        let style = load_config_strict(
            ConfigSource::Embedded("[paragraph]\nletter_spacing_pt = 1e39\n"),
            None,
        )
        .expect("config should resolve");
        assert_eq!(
            style.paragraph.letter_spacing_pt, 0.0,
            "non-finite letter spacing must be neutralised to 0"
        );
    }

    #[test]
    fn negative_letter_spacing_is_preserved() {
        // Negative tracking is a legitimate typographic choice — it
        // must NOT be clamped away.
        let style = load_config_strict(
            ConfigSource::Embedded("[paragraph]\nletter_spacing_pt = -1.5\n"),
            None,
        )
        .expect("config should resolve");
        assert_eq!(style.paragraph.letter_spacing_pt, -1.5);
    }

    #[test]
    fn huge_scalars_still_render_valid_pdf() {
        render_is_valid(
            "A paragraph that must wrap across the column width here.",
            "[paragraph]\nfont_size_pt = 1e30\nline_height = 1e30\nletter_spacing_pt = 1e39\n",
        );
    }
}

/// Margins larger than the page. Horizontally the content box would
/// go negative (wrap hang) — `begin_block` floors it at 10 pt.
/// Vertically the usable band goes negative; the page break is
/// non-recursive so output stays bounded and valid (one page per
/// block — the faithful result of an absurd-but-finite config, not
/// a defect). Contract: no panic / hang, valid PDF.
mod margins_vs_page {
    use super::*;

    #[test]
    fn margins_exceed_page_width_no_panic() {
        render_is_valid(
            &multi_page_markdown(10),
            "[page]\nmargins = [50, 2000, 50, 2000]\n",
        );
    }

    #[test]
    fn margins_exceed_page_height_no_panic() {
        render_is_valid(
            &multi_page_markdown(10),
            "[page]\nmargins = [2000, 50, 2000, 50]\n",
        );
    }

    #[test]
    fn uniform_margins_larger_than_page_no_panic() {
        render_is_valid(&multi_page_markdown(10), "[page]\nmargins = 1000.0\n");
    }
}

/// An indivisible unit (a single line / heading) taller than the
/// usable page cannot be paginated. Documented limitation: it
/// overflows the page (not auto-scaled) and the renderer continues —
/// never a hang, crash, or malformed PDF.
mod oversized_indivisible_units {
    use super::*;

    #[test]
    fn heading_taller_than_page_no_panic() {
        render_is_valid(
            "# A heading far taller than one page\n",
            "[headings.h1]\nfont_size_pt = 900.0\n",
        );
    }

    #[test]
    fn paragraph_line_taller_than_page_no_panic() {
        render_is_valid(
            "Body text rendered enormously.\n",
            "[paragraph]\nfont_size_pt = 900.0\n",
        );
    }

    #[test]
    fn content_after_oversized_unit_still_renders() {
        render_is_valid(
            "Intro paragraph.\n\n# Huge\n\nParagraph after the oversized heading.\n",
            "[headings.h1]\nfont_size_pt = 900.0\n",
        );
    }
}

/// Un-lowered geometry fields (rule thickness/width, image
/// max-width %, list indent, column gap, toc depth, border width)
/// flow to the renderer un-clamped. They feed drawing geometry, not
/// the wrap loop, and downstream `.max(..)` guards neutralise
/// extremes — so hostile values degrade to a valid (if ugly) PDF
/// rather than hanging or crashing.
mod geometry_field_extremes {
    use super::*;

    #[test]
    fn horizontal_rule_extremes() {
        render_is_valid("---\n", "[horizontal_rule]\nthickness_pt = 1e39\nwidth_pct = 1e39\n");
        render_is_valid("---\n", "[horizontal_rule]\nthickness_pt = -50.0\n");
    }

    #[test]
    fn image_max_width_pct_extremes() {
        render_is_valid("![x](missing.png)\n", "[image]\nmax_width_pct = 1e39\n");
        render_is_valid("![x](missing.png)\n", "[image]\nmax_width_pct = -100.0\n");
    }

    #[test]
    fn list_indent_and_column_gap_extremes() {
        render_is_valid("- a\n  - b\n", "[list]\nindent_per_level_pt = 1e39\n");
        render_is_valid(
            &multi_page_markdown(5),
            "[page]\ncolumn_gap_mm = 1e39\ncolumns = 3\n",
        );
    }

    #[test]
    fn toc_depth_and_border_width_extremes() {
        render_is_valid(
            "# H1\n\n## H2\n\nbody\n",
            "[toc]\nenabled = true\nmax_depth = 999999999\n",
        );
        render_is_valid("> quote\n", "[blockquote.border.left]\nwidth_pt = 1e39\n");
    }
}

/// `Sides` (margins) accepts a scalar, `[v]`, `[v,h]`, `[t,r,b,l]`,
/// or `{top,right,bottom,left}`. Every malformed shape must be a
/// typed `BadToml`, never a panic or a silent default.
mod sides_deserialize_boundaries {
    use super::*;

    #[test]
    fn three_element_array_is_typed_error() {
        must_be_bad_toml("[page]\nmargins = [1, 2, 3]\n");
    }

    #[test]
    fn empty_array_is_typed_error() {
        must_be_bad_toml("[page]\nmargins = []\n");
    }

    #[test]
    fn partial_map_is_typed_error() {
        must_be_bad_toml("[page]\nmargins = { top = 1 }\n");
    }

    #[test]
    fn mixed_type_array_is_typed_error() {
        must_be_bad_toml("[page]\nmargins = [1, \"x\"]\n");
    }
}

/// An unknown key must produce a "did you mean" hint when a close
/// candidate exists — including top-level keys and keys inside the
/// flattened list config.
mod typo_suggestions {
    use super::*;

    fn hint_for(cfg: &str) -> String {
        match load_config_strict(ConfigSource::Embedded(cfg), None) {
            Err(e) => e.to_string(),
            Ok(_) => panic!("expected an error for {cfg:?}"),
        }
    }

    #[test]
    fn top_level_typo_suggests_closest() {
        let msg = hint_for("[pagee]\nsize = \"A4\"\n");
        assert!(
            msg.contains("did you mean `page`?"),
            "no suggestion in: {msg}"
        );
    }

    #[test]
    fn nested_color_typo_suggests_closest() {
        let msg = hint_for("[paragraph]\ntext_colour = \"#000000\"\n");
        assert!(
            msg.contains("did you mean `text_color`?"),
            "no suggestion in: {msg}"
        );
    }

    #[test]
    fn typo_inside_flattened_list_is_still_typed_error() {
        // `[list]` flattens; a bad key there must still be BadToml.
        must_be_bad_toml("[list]\nstarrt = 1\n");
    }
}

/// The CLI `-V key=value` heuristic renders the value as TOML; a
/// mistyped value (number/bool where a string/enum is expected)
/// must surface as the same typed `BadToml`, never a panic.
mod cli_override_mistyping {
    use super::*;

    fn override_is_bad_toml(frag: &str) {
        let r = load_config_strict_with_overrides(ConfigSource::Default, None, Some(frag));
        assert!(
            matches!(r, Err(ResolveError::BadToml { .. })),
            "expected BadToml for override {frag:?}, got {r:?}"
        );
    }

    #[test]
    fn title_as_integer_is_typed_error() {
        override_is_bad_toml("metadata.title = 2024");
    }

    #[test]
    fn author_as_bool_is_typed_error() {
        override_is_bad_toml("metadata.author = true");
    }

    #[test]
    fn page_size_as_number_is_typed_error() {
        override_is_bad_toml("page.size = 4");
    }

    #[test]
    fn unbalanced_inline_table_is_typed_error() {
        override_is_bad_toml("page.margins = {a = 1");
    }
}

/// Broad hostile-config matrix: each must either resolve (graceful
/// default) or be a typed error — never a panic.
mod config_hardening_matrix {
    use super::*;

    #[test]
    fn empty_config_resolves_to_default() {
        let style = load_config_strict(ConfigSource::Embedded(""), None)
            .expect("empty config should resolve to the default theme");
        assert!(style.paragraph.font_size_pt > 0.0);
    }

    #[test]
    fn wrong_typed_fields_are_typed_errors() {
        must_be_bad_toml("[paragraph]\nfont_size_pt = \"big\"\n");
        must_be_bad_toml("[page]\nsize = 12\n");
    }

    #[test]
    fn emoji_and_unicode_font_family_resolves() {
        let style = load_config_strict(
            ConfigSource::Embedded("[paragraph]\nfont_family = \"😀 Sans 日本\"\n"),
            None,
        )
        .expect("unicode font family should resolve (resolved later by the font layer)");
        assert!(style.paragraph.font_size_pt > 0.0);
    }

    #[test]
    fn hostile_config_still_renders_valid_pdf() {
        render_is_valid(
            "# Title\n\nBody.\n",
            "[paragraph]\nfont_family = \"😀\"\nfont_size_pt = 1e30\n",
        );
    }
}
