//! Coverage for the class of bug where a config field resolves into
//! `ResolvedStyle` but the renderer silently ignores it. Each test
//! pairs a positive assertion ("this field actually moves the output")
//! with a no-op guard ("with the field unset the render is unchanged")
//! and, where the behavior is deliberate ("code blocks inside a
//! blockquote do NOT inherit"), a negative test that pins it down.
//!
//! All tests live behind the public `parse_into_bytes` API. Two
//! helpers make the byte-level assertions reliable:
//! - `normalize_pdf` strips the few non-deterministic bits the PDF
//!   writer injects (`/ID`, `/CreationDate`, `/ModDate`, random font
//!   subset prefixes / object names) so two valid identical renders
//!   compare equal even though their raw bytes wouldn't.
//! - `rg_op` formats an RGB triple the way printpdf does (the
//!   shortest-roundtrip Display form for an `f32`), so a search for a
//!   given fill-color op matches what's actually in the stream.

use super::common::*;
use markdown2pdf::config::ConfigSource;
use markdown2pdf::fonts::FontConfig;
use markdown2pdf::parse_into_bytes;

/// `Courier New` is the macOS / Windows family that ships as separate
/// per-weight `.ttf` files, so the sibling-discovery path can locate
/// bold + italic faces. Linux CI typically lacks it — returning `None`
/// lets individual tests skip cleanly rather than spuriously fail.
fn external_mono_family() -> Option<&'static str> {
    if markdown2pdf::fonts::find_system_font("Courier New").is_some() {
        Some("Courier New")
    } else {
        None
    }
}

/// Strip the bits of a rendered PDF that legitimately vary across
/// otherwise-identical renders: the `/ID` byte string, `/CreationDate`,
/// `/ModDate`, font-subset prefixes (printpdf assigns a 32-char
/// alphabetic ID per embedded subset, distinct per run), and the
/// random `H...` font names that printpdf hands to its built-in font
/// dictionaries. Two semantically equivalent renders compare equal
/// after normalization.
fn normalize_pdf(bytes: &[u8]) -> Vec<u8> {
    let mut s = String::from_utf8_lossy(&scan(bytes)).into_owned();
    // /ID[(...)(...)]
    s = strip_between(&s, "/ID[", "]");
    s = strip_after_marker(&s, "/CreationDate(", ')');
    s = strip_after_marker(&s, "/ModDate(", ')');
    // printpdf's 32-char A–J subset prefixes used as font names.
    // Replace any run of `[A-J]{32}` (their charset) with a fixed
    // token so two renders that picked different prefixes still
    // compare equal.
    let bytes = s.into_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 32 <= bytes.len() && bytes[i..i + 32].iter().all(|b| (b'A'..=b'J').contains(b)) {
            out.extend_from_slice(b"<FONTID>");
            i += 32;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn strip_between(s: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        out.push_str(open);
        out.push_str("<NORMALIZED>");
        rest = &rest[start + open.len()..];
        if let Some(end) = rest.find(close) {
            out.push_str(&rest[end..end + close.len()]);
            rest = &rest[end + close.len()..];
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

fn strip_after_marker(s: &str, marker: &str, end_char: char) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find(marker) {
        out.push_str(&rest[..start]);
        out.push_str(marker);
        out.push_str("<NORMALIZED>");
        rest = &rest[start + marker.len()..];
        if let Some(end) = rest.find(end_char) {
            rest = &rest[end..];
            if let Some(c) = rest.chars().next() {
                out.push(c);
                rest = &rest[c.len_utf8()..];
            }
        } else {
            break;
        }
    }
    out.push_str(rest);
    out
}

/// printpdf serializes SetFillColor as `R G B rg` with each channel
/// in shortest-roundtrip Display form (`{}` on `f32`). Match that.
fn rg_op(r: u8, g: u8, b: u8) -> String {
    format!(
        "{} {} {} rg",
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0
    )
}

#[test]
fn code_block_bold_loads_bold_mono_face_when_external_code_font_present() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    let md = "```\nfn main() {}\n```";
    let regular = parse_into_bytes(
        md.to_string(),
        ConfigSource::Embedded(""),
        Some(&FontConfig::new().with_code_font(mono)),
    )
    .expect("render regular");
    let bold = parse_into_bytes(
        md.to_string(),
        ConfigSource::Embedded("[code_block]\nfont_weight = \"bold\""),
        Some(&FontConfig::new().with_code_font(mono)),
    )
    .expect("render bold");
    // The bold face is a separate font subset, so the bold PDF must
    // embed strictly more font bytes than the regular one. A meaningful
    // margin (>=1 KB) rules out timestamp / ID noise. The pre-fix
    // renderer flagged only `mono_regular` in VariantUsage so
    // external_code never loaded its bold sibling, and the sizes
    // matched.
    assert!(
        bold.len() > regular.len() + 1024,
        "bold code block did not embed a bold mono face \
         (regular {} bytes, bold {} bytes — expected a >=1 KB margin)",
        regular.len(),
        bold.len()
    );
}

#[test]
fn code_block_italic_loads_italic_mono_face_when_external_code_font_present() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // Long enough block that a second-face glyph subset is big
    // enough to clearly exceed any timestamp / ID noise margin.
    let md = "```\n\
fn main() { let abcdefghijklmnopqrstuvwxyz = 0; }\n\
let extra_text_for_a_bigger_subset_table = true;\n\
```";
    let regular = parse_into_bytes(
        md.to_string(),
        ConfigSource::Embedded(""),
        Some(&FontConfig::new().with_code_font(mono)),
    )
    .expect("render regular");
    let italic = parse_into_bytes(
        md.to_string(),
        ConfigSource::Embedded("[code_block]\nfont_style = \"italic\""),
        Some(&FontConfig::new().with_code_font(mono)),
    )
    .expect("render italic");
    assert!(
        italic.len() > regular.len() + 1024,
        "italic code block did not embed an italic mono face \
         (regular {} bytes, italic {} bytes)",
        regular.len(),
        italic.len()
    );
}

#[test]
fn code_block_no_weight_override_is_normalize_identical() {
    // The variant-usage augmentation reads [code_block]'s weight; with
    // nothing set, it must not perturb the baseline render even though
    // the augmentation runs unconditionally.
    let a = render("```\nx\n```", "");
    let b = render("```\nx\n```", "");
    assert_eq!(
        normalize_pdf(&a),
        normalize_pdf(&b),
        "two identical renders diverged after metadata normalization"
    );
}

#[test]
fn code_block_bold_with_builtin_courier_uses_courier_bold_handle() {
    // Without an external code font, [code_block].font_weight=bold
    // must still route through builtin CourierBold; pre-existing
    // behavior, regression-guarded here.
    let bytes = render("```\nx\n```", "[code_block]\nfont_weight = \"bold\"");
    let has_bold_courier =
        contains_text(&bytes, "Courier-Bold") || contains_text(&bytes, "CourierBold");
    assert!(
        has_bold_courier,
        "bold code block did not pick a builtin Courier-Bold variant"
    );
}

#[test]
fn list_inside_blockquote_inherits_blockquote_text_color() {
    let md = "> Quote line.\n>\n> - first item\n> - second item\n";
    let cfg = r##"
        [blockquote]
        text_color = "#1A6ED8"
    "##;
    let bytes = render(md, cfg);
    let needle = rg_op(0x1A, 0x6E, 0xD8);
    let count = count_substr(&scan(&bytes), needle.as_bytes());
    // Body paragraph + bullet + item text for each of 2 items.
    // Conservatively assert >= 3 (paragraph + 2 item baselines).
    assert!(
        count >= 3,
        "expected the blockquote text color {:?} on the body paragraph \
         AND the nested list (>= 3 fills); got {}",
        needle,
        count
    );
}

#[test]
fn ordered_list_inside_blockquote_inherits_text_color() {
    let md = "> A quote.\n>\n> 1. one\n> 2. two\n";
    let cfg = r##"
        [blockquote]
        text_color = "#22AA66"
    "##;
    let bytes = render(md, cfg);
    let needle = rg_op(0x22, 0xAA, 0x66);
    assert!(
        count_substr(&scan(&bytes), needle.as_bytes()) >= 3,
        "ordered list inside blockquote did not inherit the container color"
    );
}

#[test]
fn list_inside_admonition_inherits_text_color() {
    let md = "!!! note\n    Body sentence.\n\n    - alpha\n    - beta\n";
    let cfg = r##"
        [admonition.note]
        text_color = "#AA3300"
    "##;
    let bytes = render(md, cfg);
    let needle = rg_op(0xAA, 0x33, 0x00);
    assert!(
        count_substr(&scan(&bytes), needle.as_bytes()) >= 3,
        "list inside admonition did not inherit the container color"
    );
}

#[test]
fn code_block_inside_blockquote_keeps_its_own_text_color() {
    let md = "> quoted body\n>\n> ```\n> x = 1\n> ```\n";
    let cfg = r##"
        [blockquote]
        text_color = "#FF0000"

        [code_block]
        text_color = "#00AA00"
    "##;
    let bytes = render(md, cfg);
    let scanned = scan(&bytes);
    let bq = rg_op(0xFF, 0x00, 0x00);
    let cb = rg_op(0x00, 0xAA, 0x00);
    assert!(
        count_substr(&scanned, bq.as_bytes()) >= 1,
        "blockquote body should still paint its own color {:?}",
        bq
    );
    assert!(
        count_substr(&scanned, cb.as_bytes()) >= 1,
        "code block inside blockquote must keep its own configured \
         color {:?} (inheritance is for lists only, by design)",
        cb
    );
}

#[test]
fn top_level_list_default_render_is_normalize_identical_across_runs() {
    let md = "- one\n- two\n- three\n";
    let a = render(md, "");
    let b = render(md, "");
    assert_eq!(
        normalize_pdf(&a),
        normalize_pdf(&b),
        "deterministic re-render of a plain list diverged"
    );
}

#[test]
fn nested_list_inside_blockquote_inherits_too() {
    // List-inside-list-inside-blockquote: the inherit path runs for
    // every nesting level because text_style_override is still set
    // across the recursive render_block call.
    let md = "\
> Quote.\n\
>\n\
> - outer\n\
>   - inner\n\
>   - inner two\n\
> - outer two\n";
    let cfg = r##"
        [blockquote]
        text_color = "#3344CC"
    "##;
    let bytes = render(md, cfg);
    let needle = rg_op(0x33, 0x44, 0xCC);
    // Body paragraph + 4 list item lines minimum (each item paints
    // the inherited color at least once).
    let count = count_substr(&scan(&bytes), needle.as_bytes());
    assert!(
        count >= 5,
        "nested list inside blockquote: only {} inherited-color fills found",
        count
    );
}

#[test]
fn task_list_inside_blockquote_inherits_text_color() {
    let md = "> Quote.\n>\n> - [ ] todo\n> - [x] done\n";
    let cfg = r##"
        [blockquote]
        text_color = "#5566AA"
    "##;
    let bytes = render(md, cfg);
    let needle = rg_op(0x55, 0x66, 0xAA);
    // Body + 2 task labels = >= 3.
    assert!(
        count_substr(&scan(&bytes), needle.as_bytes()) >= 3,
        "task list inside blockquote did not inherit the container color"
    );
}

#[test]
fn code_inline_font_family_equal_to_code_block_is_a_no_op() {
    // Default theme spells both as "Courier"; with no override this
    // must stay identical (modulo PDF metadata) to a render with both
    // fields explicitly set to "Courier" (no second font loaded).
    let a = render("plain `code` line", "");
    let b = render(
        "plain `code` line",
        r##"
        [code_block]
        font_family = "Courier"
        [code_inline]
        font_family = "Courier"
    "##,
    );
    assert_eq!(
        normalize_pdf(&a),
        normalize_pdf(&b),
        "matching font_family on both [code_block] and [code_inline] \
         should be a no-op vs the default-theme baseline"
    );
}

#[test]
fn code_inline_font_family_distinct_from_code_block_loads_a_second_family() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    let baseline = render("plain `code` line\n\n```\nblock\n```\n", "");
    let with_inline_font = render(
        "plain `code` line\n\n```\nblock\n```\n",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    assert!(
        with_inline_font.len() > baseline.len() + 4 * 1024,
        "distinct code_inline.font_family did not embed a second font \
         (baseline {} vs override {} — expected >=4 KB growth)",
        baseline.len(),
        with_inline_font.len()
    );
}

#[test]
fn code_inline_font_family_routes_through_a_distinct_font_handle() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // With an external code font for code blocks AND a different
    // [code_inline].font_family, the page content stream must
    // reference >= 2 distinct external font handles: one for the
    // inline `code` span, a different one for the fenced block.
    // Before the fix, both inline and block used the single
    // external_code family.
    let Some(other) = any_system_font() else {
        eprintln!("skip: no second system font available");
        return;
    };
    // Different external families for code-block (via --code-font)
    // and inline-code (via [code_inline].font_family). They can't
    // collapse onto the same handle.
    let bytes = parse_into_bytes(
        "plain `code` here\n\n```\nblock body\n```\n".to_string(),
        ConfigSource::Embedded(&format!("[code_inline]\nfont_family = \"{other}\"")),
        Some(&FontConfig::new().with_code_font(mono)),
    )
    .expect("render");
    let scanned = scan(&bytes);
    let s = String::from_utf8_lossy(&scanned);
    let mut handles = std::collections::HashSet::new();
    for line in s.lines() {
        let line = line.trim();
        if let Some(stripped) = line.strip_prefix('/')
            && line.ends_with(" Tf")
        {
            let name: String = stripped
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            if name.len() == 32 && name.chars().all(|c| ('A'..='J').contains(&c)) {
                handles.insert(name);
            }
        }
    }
    assert!(
        handles.len() >= 2,
        "expected >= 2 distinct external font handles (code-block \
         + inline-code); got {} ({:?})",
        handles.len(),
        handles
    );
}

#[test]
fn kbd_html_inline_routes_through_code_inline_font_family() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // `<kbd>` is lowered with `with_inline_code()`, so it picks up
    // `[code_inline].font_family` exactly like a backticked span.
    let baseline = render("Press <kbd>Enter</kbd>.", "");
    let routed = render(
        "Press <kbd>Enter</kbd>.",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    assert!(
        routed.len() > baseline.len() + 4 * 1024,
        "<kbd> did not pick up the distinct code_inline.font_family"
    );
}

#[test]
fn code_inline_font_family_unset_does_not_load_a_third_family() {
    // With nothing configured the inline-code family must stay empty.
    // Two identical renders should differ only in PDF metadata.
    let a = render("`x`", "");
    let b = render("`x`", "");
    assert_eq!(
        normalize_pdf(&a),
        normalize_pdf(&b),
        "two identical renders diverged after metadata normalization"
    );
}

#[test]
fn bold_inline_code_picks_a_bold_face_when_distinct_family_is_configured() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    let regular = render(
        "Lead `code` trail.",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    let bold = render(
        "Lead **`code`** trail.",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    // Bold inline-code embeds a second face of the inline-code family.
    assert!(
        bold.len() > regular.len() + 1024,
        "bold inline code did not embed a bold face of the inline-code family \
         (regular {} bytes, bold {} bytes)",
        regular.len(),
        bold.len()
    );
}

#[test]
fn code_inline_padding_unset_is_normalize_identical_to_baseline() {
    // The wrap-pipeline change always runs; with padding=0 it must
    // be a strict no-op (no TJ offset emitted, no width change).
    let a = render("Lead `code` trail.", "");
    let b = render("Lead `code` trail.", "");
    assert_eq!(
        normalize_pdf(&a),
        normalize_pdf(&b),
        "two identical renders diverged after metadata normalization"
    );
}

#[test]
fn code_inline_horizontal_padding_emits_two_tj_offsets_per_span() {
    // Padding emits as `[-N.NNNN] TJ` (PDF's text-position-adjust)
    // — one before the inline-code text, one after.
    let baseline = render("Lead `code` trail.", "");
    let padded = render(
        "Lead `code` trail.",
        r##"
        [code_inline]
        padding = 5.0
    "##,
    );
    let baseline_tj = count_substr(&scan(&baseline), b" TJ");
    let padded_tj = count_substr(&scan(&padded), b" TJ");
    assert_eq!(
        padded_tj.saturating_sub(baseline_tj),
        2,
        "expected exactly 2 boundary TJ offsets for one inline-code \
         span; baseline TJ {}, padded TJ {}",
        baseline_tj,
        padded_tj
    );
}

#[test]
fn code_inline_padding_only_at_span_boundaries_not_per_word() {
    // A multi-word inline-code span like `foo bar baz` must get
    // ONE pad pair (boundary), not one per word. With 3 words split
    // on whitespace, a naive per-word pad would emit 6 offsets;
    // boundary-only emits 2.
    let baseline = render("Lead `foo bar baz` trail.", "");
    let padded = render(
        "Lead `foo bar baz` trail.",
        r##"
        [code_inline]
        padding = 4.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 2,
        "expected 2 boundary offsets for one multi-word inline-code \
         span; got {} (per-word would be 6+)",
        extra
    );
}

#[test]
fn two_separate_inline_code_spans_each_get_their_own_boundary_pair() {
    let baseline = render("first `aaa` mid `bbb` last", "");
    let padded = render(
        "first `aaa` mid `bbb` last",
        r##"
        [code_inline]
        padding = 3.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 4,
        "expected 4 offsets total (2 spans × 2 boundaries); got {}",
        extra
    );
}

#[test]
fn code_inline_padding_does_not_leak_into_mark_highlight() {
    // [mark]'s background isn't padded — only [code_inline]'s is.
    // A doc with a `==highlight==` and NO inline code must not emit
    // boundary TJ ops just because code_inline.padding is set.
    let baseline = render("Lead ==hi== trail.", "");
    let with_pad = render(
        "Lead ==hi== trail.",
        r##"
        [code_inline]
        padding = 5.0
    "##,
    );
    let baseline_tj = count_substr(&scan(&baseline), b" TJ");
    let pad_tj = count_substr(&scan(&with_pad), b" TJ");
    assert_eq!(
        baseline_tj, pad_tj,
        "code_inline.padding leaked into mark/highlight emission \
         (baseline TJ {}, padded {})",
        baseline_tj, pad_tj
    );
}

#[test]
fn code_inline_padding_does_not_emit_tj_inside_a_code_block() {
    // Inside a fenced code block `in_code_block` is true → the
    // inline-code-padding guard skips. A code block must not gain
    // boundary offsets from this setting.
    let baseline = render("```\nfn f() {}\n```\n", "");
    let with_pad = render(
        "```\nfn f() {}\n```\n",
        r##"
        [code_inline]
        padding = 5.0
    "##,
    );
    let baseline_tj = count_substr(&scan(&baseline), b" TJ");
    let pad_tj = count_substr(&scan(&with_pad), b" TJ");
    assert_eq!(
        baseline_tj, pad_tj,
        "code_inline.padding leaked into a code block"
    );
}

#[test]
fn code_inline_vertical_padding_grows_background_box_height() {
    // With a background color set, the inline-code box is painted
    // via `draw_filled_rect`. The fill polygon's y-coords change when
    // padding.top/bottom are nonzero; bytes must diverge.
    let no_v_pad = render(
        "Lead `x` trail.",
        r##"
        [code_inline]
        background_color = "#FFEE00"
    "##,
    );
    let with_v_pad = render(
        "Lead `x` trail.",
        r##"
        [code_inline]
        background_color = "#FFEE00"
        padding = { top = 2.0, right = 0.0, bottom = 2.0, left = 0.0 }
    "##,
    );
    assert_ne!(
        normalize_pdf(&no_v_pad),
        normalize_pdf(&with_v_pad),
        "vertical inline-code padding did not change the rendered box"
    );
}

#[test]
fn code_inline_padding_at_paragraph_start_still_emits_left_offset() {
    // Edge: inline code at the very start of the paragraph. The
    // span is still bounded — non-code on its right — so it gets
    // both boundary offsets.
    let baseline = render("`code` trail.", "");
    let padded = render(
        "`code` trail.",
        r##"
        [code_inline]
        padding = 4.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 2,
        "expected 2 boundary offsets at-start; got {}",
        extra
    );
}

#[test]
fn code_inline_padding_at_paragraph_end_still_emits_right_offset() {
    let baseline = render("Lead `code`", "");
    let padded = render(
        "Lead `code`",
        r##"
        [code_inline]
        padding = 4.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 2,
        "expected 2 boundary offsets at-end; got {}",
        extra
    );
}

#[test]
fn code_inline_padding_in_justified_paragraph_emits_both_tj_offsets() {
    // Justified alignment also emits a TJ-ish op via Tw (word
    // spacing), so the count comparison must rely on the *delta*
    // between baseline (justify, no padding) and padded (justify +
    // padding) for the same alignment. The +2 boundary offsets must
    // still appear cleanly even though the line is justified.
    let baseline = render(
        "Some words `code` more words to force justification fill across the line.",
        "[paragraph]\ntext_align = \"justify\"",
    );
    let padded = render(
        "Some words `code` more words to force justification fill across the line.",
        r##"
        [paragraph]
        text_align = "justify"

        [code_inline]
        padding = 5.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 2,
        "justified paragraph + padded inline code should still emit \
         exactly 2 boundary offsets; got {}",
        extra
    );
}

#[test]
fn code_inline_padding_in_center_aligned_paragraph_works() {
    // Center alignment uses an absolute Td per line. Padding still
    // must emit cleanly and not crash the render.
    let bytes = render(
        "Lead `code` trail.",
        r##"
        [paragraph]
        text_align = "center"

        [code_inline]
        padding = 5.0
    "##,
    );
    assert!(pdf_well_formed(&bytes));
    let tj_count = count_substr(&scan(&bytes), b" TJ");
    assert!(
        tj_count >= 2,
        "center-aligned padded inline code emitted {} TJ ops; expected >=2",
        tj_count
    );
}

#[test]
fn inline_code_in_heading_routes_through_code_inline_font_family() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // Headings also flow through write_wrapped_runs; an inline code
    // span inside a heading must pick up [code_inline].font_family.
    let baseline = render("# Heading with `code` in it", "");
    let routed = render(
        "# Heading with `code` in it",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    assert!(
        routed.len() > baseline.len() + 4 * 1024,
        "inline code in heading didn't pick up code_inline.font_family"
    );
}

#[test]
fn inline_code_in_table_cell_picks_up_padding() {
    let baseline = render("| h1 | h2 |\n| -- | -- |\n| a  | `x` |\n", "");
    let padded = render(
        "| h1 | h2 |\n| -- | -- |\n| a  | `x` |\n",
        r##"
        [code_inline]
        padding = 3.0
    "##,
    );
    // Padding effect — at minimum the bytes must differ (the
    // boundary TJs appear in the cell's content stream).
    assert!(
        padded.len() != baseline.len() || padded != baseline,
        "inline code inside a table cell did not pick up padding"
    );
}

#[test]
fn bold_italic_inline_code_still_routes_through_inline_code_font_family() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // `***\`code\`***` → bold + italic + monospace + inline_code flags.
    // The inline-code family must serve it (falling through to its
    // bold-italic sibling if loaded, else bold/italic/regular).
    let regular = render(
        "plain `code` here",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    let bold_italic = render(
        "plain ***`code`*** here",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    // At minimum: the bold-italic variant load adds bytes (could be
    // bold-italic face, or italic + bold fallbacks separately).
    assert!(
        bold_italic.len() > regular.len() + 512,
        "bold-italic inline code didn't pick up any additional face \
         (regular {} bytes, bold-italic {} bytes)",
        regular.len(),
        bold_italic.len()
    );
}

#[test]
fn inline_code_inside_blockquote_still_keeps_code_inline_font_family() {
    let Some(mono) = external_mono_family() else {
        eprintln!("skip: no external monospace family installed");
        return;
    };
    // Inside a blockquote the body text inherits container style,
    // but inline-code routing should stay on the inline-code path
    // (mono + inline_code flags, routed to external_code_inline).
    let baseline = render("> Quote with `code` inside.\n", "");
    let routed = render(
        "> Quote with `code` inside.\n",
        &format!(
            r##"
            [code_inline]
            font_family = "{mono}"
        "##
        ),
    );
    assert!(
        routed.len() > baseline.len() + 4 * 1024,
        "inline code inside blockquote didn't pick up code_inline.font_family"
    );
}

#[test]
fn code_inline_padding_works_inside_blockquote() {
    let baseline = render("> Quote with `code` inside.\n", "");
    let padded = render(
        "> Quote with `code` inside.\n",
        r##"
        [code_inline]
        padding = 5.0
    "##,
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 2,
        "inline code inside blockquote should emit 2 boundary offsets; got {}",
        extra
    );
}

#[test]
fn empty_inline_code_does_not_panic() {
    // Edge: backticks around literally nothing. Often elided by the
    // lexer but worth a panic-free guard.
    let bytes = render("Lead `` trail.", "[code_inline]\npadding = 5.0");
    assert!(pdf_well_formed(&bytes));
}

#[test]
fn inline_code_padding_with_letter_spacing_does_not_crash() {
    // letter_spacing emits SetCharacterSpacing; combined with the
    // TJ offset for padding it's two state changes per span. Just a
    // smoke test that nothing collides.
    let bytes = render(
        "Lead `code` trail.",
        r##"
        [paragraph]
        letter_spacing_pt = 0.4

        [code_inline]
        padding = 5.0
    "##,
    );
    assert!(pdf_well_formed(&bytes));
    let tj = count_substr(&scan(&bytes), b" TJ");
    assert!(
        tj >= 2,
        "expected at least 2 TJ offsets even with letter spacing; got {}",
        tj
    );
}

#[test]
fn many_consecutive_inline_code_spans_each_get_their_own_boundary_pair() {
    // Stress: 5 spans → 10 boundary offsets.
    let baseline = render("a `1` b `2` c `3` d `4` e `5` f", "");
    let padded = render(
        "a `1` b `2` c `3` d `4` e `5` f",
        "[code_inline]\npadding = 3.0",
    );
    let extra = count_substr(&scan(&padded), b" TJ") - count_substr(&scan(&baseline), b" TJ");
    assert_eq!(
        extra, 10,
        "expected 10 boundary offsets (5 spans × 2); got {}",
        extra
    );
}

#[test]
fn nested_list_in_blockquote_color_does_not_paint_outside_callout() {
    // Negative: text outside the blockquote stays the default text
    // color, not the blockquote color.
    let bytes = render(
        "Plain paragraph here.\n\n> - quoted item one\n> - quoted item two\n\nAnother plain paragraph.\n",
        r##"
        [blockquote]
        text_color = "#1234FF"
    "##,
    );
    let inherit = rg_op(0x12, 0x34, 0xFF);
    let default_black = rg_op(0, 0, 0);
    let scanned = scan(&bytes);
    assert!(
        count_substr(&scanned, inherit.as_bytes()) >= 2,
        "blockquote color should fire for both list items"
    );
    // Both plain paragraphs paint with the default text color.
    assert!(
        count_substr(&scanned, default_black.as_bytes()) >= 2,
        "plain paragraphs outside the blockquote must NOT inherit"
    );
}

#[test]
fn code_inline_padding_increases_pdf_size_meaningfully() {
    // Sanity: extra TJ ops + extra spacing data must visibly grow
    // the content stream. Tiny but >0.
    let baseline = render("Lead `code` trail.", "");
    let padded = render(
        "Lead `code` trail.",
        r##"
        [code_inline]
        padding = 5.0
    "##,
    );
    assert!(
        padded.len() > baseline.len(),
        "padded render is not larger than baseline ({} vs {})",
        padded.len(),
        baseline.len()
    );
}
