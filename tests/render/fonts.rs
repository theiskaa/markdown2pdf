//! End-to-end regression tests for the renderer's font handling.
//!
//! These tests assert on bytes emitted into the rendered PDF. The
//! post-process ships compressed object + cross-reference streams, so
//! `extract_named_numbers` first routes through `scan` to expand the
//! PDF back to plain individual objects before the byte-level scan.

use markdown2pdf::config::ConfigSource;
use markdown2pdf::fonts::{FontConfig, FontSource};
use markdown2pdf::parse_into_bytes;

use super::common::{any_system_font, scan};

/// Read every `/Ascent <number>` value emitted in the PDF.
///
/// printpdf serializes FontDescriptor entries as `/Ascent 916\n` (one
/// per embedded font).
fn ascents(bytes: &[u8]) -> Vec<i32> {
    extract_named_numbers(bytes, b"/Ascent ")
}

fn descents(bytes: &[u8]) -> Vec<i32> {
    extract_named_numbers(bytes, b"/Descent ")
}

fn extract_named_numbers(bytes: &[u8], key: &[u8]) -> Vec<i32> {
    // The post-process packs FontDescriptors into a compressed object
    // stream; expand back to individual classic objects so the
    // `/Ascent <n>` byte scan still sees them.
    let bytes = &scan(bytes);
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(rel) = bytes[pos..].windows(key.len()).position(|w| w == key) {
        let start = pos + rel + key.len();
        let mut end = start;
        while end < bytes.len() && (bytes[end] == b'-' || bytes[end].is_ascii_digit()) {
            end += 1;
        }
        if end > start
            && let Ok(s) = std::str::from_utf8(&bytes[start..end])
            && let Ok(n) = s.parse::<i32>()
        {
            out.push(n);
        }
        pos = end.max(start + 1);
    }
    out
}

#[test]
fn ascent_and_descent_are_normalized_to_1000_em() {
    // External Unicode body font triggers the external-font path
    // where the ascent/descent normalization actually matters. A
    // 8pt Georgia paragraph used to emit /Ascent 1878 /Descent -449
    // (raw 2048-UPEM units) which made viewer selection rects
    // ~2× too tall.
    let Some(font) = any_system_font() else {
        eprintln!("skip: no system font available to exercise external-font path");
        return;
    };
    let md = format!("Body text in {} for the ascent test.", font);
    let cfg = FontConfig::new().with_default_font(&font);
    let bytes = parse_into_bytes(md, ConfigSource::Default, Some(&cfg))
        .expect("render must succeed when an external font is present");

    let ascent_values = ascents(&bytes);
    let descent_values = descents(&bytes);
    assert!(
        !ascent_values.is_empty(),
        "expected at least one /Ascent entry in the rendered PDF"
    );

    // Every emitted ascent must fall inside the /1000-em window. Most
    // real fonts ascend to ~700–950; a sentinel cap at 1200 catches
    // regressions to raw font units (Georgia would emit 1878).
    for a in &ascent_values {
        assert!(
            (200..=1200).contains(a),
            "ascent {} outside /1000-em range — \
             FontDescriptor regressed to raw font units",
            a
        );
    }
    for d in &descent_values {
        assert!(
            (-500..=0).contains(d),
            "descent {} outside /1000-em range — \
             FontDescriptor regressed to raw font units",
            d
        );
    }
}

#[test]
fn inline_code_does_not_fall_back_to_builtin_courier_when_external_body_is_loaded() {
    // The regression: mixing built-in Courier (Win-Ansi, 600/1000-em
    // space) with external Georgia (Identity-H, ~280/1000-em space)
    // created a visible gap at every font transition. With an external
    // body font configured, the renderer should auto-pick a system
    // monospace so both paths stay external Unicode.
    let Some(font) = any_system_font() else {
        eprintln!("skip: no system font available to exercise external-font path");
        return;
    };
    let md = "Body text with `inline code` between words.".to_string();
    let cfg = FontConfig::new().with_default_font(&font);
    let bytes = parse_into_bytes(md, ConfigSource::Default, Some(&cfg))
        .expect("render must succeed when an external font is present");

    // Built-in Courier paths emit `(literal text) Tj`; external paths
    // emit `<hex glyph ids> Tj`. Finding the parenthesized form for
    // our inline code would prove the fallback didn't fire.
    let needle = b"(inline code) Tj";
    assert!(
        !bytes.windows(needle.len()).any(|w| w == needle),
        "inline code regressed to the built-in Courier path \
         (literal Tj string present); external monospace fallback \
         is not firing"
    );

    // Likewise, no FontDescriptor for the built-in Courier BaseFont
    // when a system monospace is found.
    let courier_basefont = b"/BaseFont/Courier";
    let has_builtin = bytes
        .windows(courier_basefont.len())
        .any(|w| w == courier_basefont);
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        assert!(
            !has_builtin,
            "expected the external monospace fallback to replace built-in Courier"
        );
    }
}

#[test]
fn renderer_works_without_any_external_font_config() {
    // Baseline: with no FontConfig, everything goes through the
    // built-in Helvetica/Courier path.
    let md = "Plain paragraph with `inline code` and **bold**.".to_string();
    let bytes = parse_into_bytes(md, ConfigSource::Default, None).expect("render");
    assert!(bytes.starts_with(b"%PDF-"), "rendered output is not a PDF");
}

#[test]
fn ascent_and_descent_normalize_for_a_second_unrelated_font() {
    // The normalization formula is font-agnostic; this test exercises
    // a second font with a different UPEM to make sure no Georgia-
    // specific assumptions snuck in.
    let md = "Body text in Times New Roman.".to_string();
    let cfg = FontConfig::new().with_default_font("Times New Roman");
    let Ok(bytes) = parse_into_bytes(md, ConfigSource::Default, Some(&cfg)) else {
        // System doesn't have the font — opportunistic test.
        return;
    };
    let values = ascents(&bytes);
    if values.is_empty() {
        return; // fell back to built-in; nothing to assert
    }
    for a in &values {
        assert!(
            (200..=1200).contains(a),
            "ascent {} outside /1000-em range for Times New Roman",
            a
        );
    }
}

// T18 — a user-supplied font that can't be used must degrade to the
// built-in font, never panic. (The bundled subset's `Face::parse`
// `.expect` in font.rs is a build invariant — bundled bytes are
// always valid — and is not reachable from user input.)

#[test]
fn garbage_font_bytes_fall_back_to_builtin() {
    let cfg = FontConfig::new().with_default_font_source(FontSource::bytes(&[0, 1, 2, 3, 4, 5]));
    let bytes = parse_into_bytes(
        "# Title\n\nBody text here.".to_string(),
        ConfigSource::Default,
        Some(&cfg),
    )
    .expect("garbage font must fall back, not error");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn empty_font_bytes_fall_back_to_builtin() {
    let cfg = FontConfig::new().with_default_font_source(FontSource::bytes(&[]));
    let bytes = parse_into_bytes("Body text.".to_string(), ConfigSource::Default, Some(&cfg))
        .expect("empty font must fall back, not error");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn nonexistent_font_path_falls_back_to_builtin() {
    let cfg = FontConfig::new().with_default_font("/no/such/font-file.ttf");
    let bytes = parse_into_bytes("Body text.".to_string(), ConfigSource::Default, Some(&cfg))
        .expect("missing font path must fall back, not error");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn non_ascii_with_builtin_font_does_not_panic() {
    // Built-in Helvetica can't cover CJK/emoji/RTL; the win1252
    // fallback must keep it a valid PDF rather than panic or empty.
    let md = "# 日本語 タイトル\n\nemoji 😀 Ω, مرحبا بالعالم.".to_string();
    let bytes = parse_into_bytes(md, ConfigSource::Default, None).expect("render must not error");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn unknown_fallback_font_renders_and_degrades_gracefully() {
    // A configured fallback that can't be located must not error or
    // panic — it simply doesn't load, and uncovered codepoints stay
    // uncovered (the pre-fallback behavior).
    let md = "# Hello 日本語\n\nMixed text.".to_string();
    let cfg_toml = r#"
        [defaults]
        fallback_fonts = ["This_Font_Definitely_Does_Not_Exist_12345"]
    "#;
    let bytes = parse_into_bytes(md, ConfigSource::Embedded(cfg_toml), None)
        .expect("missing fallback must not error");
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn unicode_text_without_font_config_takes_auto_detected_external_path() {
    // #111: without any FontConfig, the renderer used to fall through
    // to built-in Type 1 Helvetica — WinAnsi-only, so `café — naïve`
    // and curly quotes silently turned into `?` replacement chars.
    // The default-body-source probe now keeps rendering on the
    // external Identity-H Unicode path on any host that has at least
    // one candidate font installed.
    if markdown2pdf::fonts::default_body_source().is_none() {
        eprintln!("skip: host has no candidate system Unicode font");
        return;
    }
    let md = "Hello café — naïve “quoted” word.".to_string();
    let bytes = parse_into_bytes(md, ConfigSource::Default, None).expect("render must succeed");
    assert!(bytes.starts_with(b"%PDF-"));

    // External Identity-H embedding writes one `/Ascent` per loaded
    // FontDescriptor. The built-in Type 1 path doesn't, so any
    // `/Ascent` at all proves the external path was taken.
    let asc = ascents(&bytes);
    assert!(
        !asc.is_empty(),
        "expected an embedded external body font when default_body_source resolves"
    );
}

#[test]
fn unresolved_builtin_alias_falls_through_to_auto_detect() {
    // Default themes spell `font_family = \"Helvetica\"`, and macOS
    // ships Helvetica only inside `Helvetica.ttc` — the loader skips
    // .ttc collections, so the configured name resolves to nothing
    // and we used to land on built-in Type 1 Helvetica. The
    // auto-detect fallback now retries with the per-OS Unicode
    // candidate list so Unicode rendering still works.
    if markdown2pdf::fonts::default_body_source().is_none() {
        eprintln!("skip: host has no candidate system Unicode font");
        return;
    }
    let md = "Body with café and — dashes.".to_string();
    let cfg = FontConfig::new().with_default_font("Helvetica");
    let bytes =
        parse_into_bytes(md, ConfigSource::Default, Some(&cfg)).expect("render must succeed");
    assert!(bytes.starts_with(b"%PDF-"));
    let asc = ascents(&bytes);
    assert!(
        !asc.is_empty(),
        "Helvetica that can't resolve should fall through to auto-detected external font"
    );
}

#[test]
fn fallback_font_loads_when_system_font_available() {
    // When a *real* system font is configured as the fallback, the
    // renderer must load it and embed it alongside the primary. With
    // no FontConfig the primary is the built-in Helvetica path, which
    // emits no `/Ascent` entry — any `/Ascent` value in the PDF must
    // come from an embedded external font (the fallback).
    let Some(name) = any_system_font() else {
        eprintln!("skipping: no system font available");
        return;
    };
    let md = "Body text with Greek Ω and Latin Aé.".to_string();
    let cfg_toml = format!("[defaults]\nfallback_fonts = [\"{}\"]\n", name);
    let bytes =
        parse_into_bytes(md, ConfigSource::Embedded(&cfg_toml), None).expect("render must succeed");
    assert!(bytes.starts_with(b"%PDF-"));
    let asc = ascents(&bytes);
    assert!(
        !asc.is_empty(),
        "expected at least one embedded font (the fallback) with an `/Ascent` entry, got none"
    );
}

/// Real, deterministic font files without depending on host-installed fonts.
struct WeightFixtures(std::path::PathBuf);

impl WeightFixtures {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "mdp-weights-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn write(&self, name: &str, font: printpdf::BuiltinFont) -> std::path::PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, font.get_subset_font().bytes).unwrap();
        path
    }
}
impl Drop for WeightFixtures {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// PostScript name of the embedded face that draws the first glyph.
fn first_emitted_face(bytes: &[u8]) -> String {
    let mut doc = lopdf::Document::load_mem(bytes).unwrap();
    doc.decompress();
    let page = *doc.get_pages().values().next().unwrap();
    let fonts = doc.get_page_fonts(page).unwrap();
    let content = lopdf::content::Content::decode(&doc.get_page_content(page)).unwrap();
    let mut current = None;
    for op in content.operations {
        if op.operator == "Tf" {
            current = Some(op.operands[0].as_name().unwrap().to_vec());
        }
        if op.operator == "Tj" {
            let font = fonts[&current.unwrap()];
            let descendant = doc
                .dereference(&font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0])
                .unwrap()
                .1
                .as_dict()
                .unwrap();
            let descriptor = doc
                .dereference(descendant.get(b"FontDescriptor").unwrap())
                .unwrap()
                .1
                .as_dict()
                .unwrap();
            let stream = doc
                .dereference(descriptor.get(b"FontFile2").unwrap())
                .unwrap()
                .1
                .as_stream()
                .unwrap();
            return postscript_name(&stream.content);
        }
    }
    panic!("no emitted glyph");
}

fn postscript_name(font_bytes: &[u8]) -> String {
    let face = ttf_parser::Face::parse(font_bytes, 0).unwrap();
    face.names()
        .into_iter()
        .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
        .find_map(|n| n.to_string())
        .expect("fixture face has a PostScript name")
}

fn expected_face(font: printpdf::BuiltinFont) -> String {
    postscript_name(&font.get_subset_font().bytes)
}

#[test]
fn configured_weights_select_real_sibling_faces_in_pdf() {
    use printpdf::BuiltinFont as B;
    let fixtures = WeightFixtures::new();
    let anchor = fixtures.write("Foo-Regular.ttf", B::Helvetica);
    let cases = [
        ("thin", 100, "Foo-Thin.ttf", B::Courier),
        ("extra-light", 200, "Foo_Extra_Light.OTF", B::TimesRoman),
        ("light", 300, "Foo Light.ttf", B::TimesItalic),
        ("normal", 400, "Foo-Regular.ttf", B::Helvetica),
        ("medium", 500, "FooMedium.ttf", B::TimesBold),
        ("semibold", 600, "Foo-SemiBold.ttf", B::TimesBoldItalic),
        ("bold", 700, "Foo-Bold.ttf", B::HelveticaBold),
        ("extra-bold", 800, "Foo-ExtraBold.ttf", B::HelveticaOblique),
        ("black", 900, "Foo-Black.ttf", B::HelveticaBoldOblique),
    ];
    for &(_, _, name, font) in &cases {
        fixtures.write(name, font);
    }
    let cfg = FontConfig::new()
        .with_default_font_source(FontSource::file(&anchor))
        .with_code_font_source(FontSource::file(&anchor));
    for (name, numeric, _, font) in cases {
        for value in [format!("\"{name}\""), numeric.to_string()] {
            for (section, md) in [
                ("paragraph", "A"),
                ("code_block", "```\nA\n```"),
                ("code_inline", "`A`"),
            ] {
                let toml = format!("[{section}]\nfont_weight = {value}");
                let bytes =
                    parse_into_bytes(md.to_string(), ConfigSource::Embedded(&toml), Some(&cfg))
                        .unwrap();
                assert_eq!(
                    first_emitted_face(&bytes),
                    expected_face(font),
                    "{section} weight {value}"
                );
            }
        }
    }
}

#[test]
fn weighted_italic_and_markdown_bold_select_matching_faces() {
    use printpdf::BuiltinFont as B;
    let fixtures = WeightFixtures::new();
    let anchor = fixtures.write("Foo-Regular.ttf", B::Helvetica);
    fixtures.write("Foo-Light.ttf", B::TimesRoman);
    fixtures.write("foo_light_italic.TTF", B::TimesItalic);
    fixtures.write("Foo-Bold.ttf", B::HelveticaBold);
    fixtures.write("Foo-BoldOblique.ttf", B::HelveticaBoldOblique);
    let cfg = FontConfig::new().with_default_font_source(FontSource::file(anchor));
    for (md, style, font) in [
        ("*A*", "font_weight = 300", B::TimesItalic),
        (
            "A",
            "font_weight = 300\nfont_style = \"italic\"",
            B::TimesItalic,
        ),
        ("**A**", "font_weight = 300", B::HelveticaBold),
        ("***A***", "font_weight = 300", B::HelveticaBoldOblique),
        // Missing medium matches regular; missing black matches bold.
        ("A", "font_weight = 500", B::Helvetica),
        ("A", "font_weight = 900", B::HelveticaBold),
    ] {
        let toml = format!("[paragraph]\n{style}");
        let bytes =
            parse_into_bytes(md.to_string(), ConfigSource::Embedded(&toml), Some(&cfg)).unwrap();
        assert_eq!(
            first_emitted_face(&bytes),
            expected_face(font),
            "{md}: {style}"
        );
    }
}

fn render_with(md: &str, toml: &str, cfg: &FontConfig) -> Vec<u8> {
    parse_into_bytes(md.to_string(), ConfigSource::Embedded(toml), Some(cfg)).unwrap()
}

#[test]
fn bold_faces_are_found_for_family_names_ending_in_weight_words() {
    use printpdf::BuiltinFont as B;
    let fixtures = WeightFixtures::new();
    let anchor = fixtures.write("Serif Roman.ttf", B::TimesRoman);
    fixtures.write("Serif Roman Bold.ttf", B::TimesBold);
    fixtures.write("Serif Roman Italic.ttf", B::TimesItalic);
    let cfg = FontConfig::new().with_default_font_source(FontSource::file(anchor));
    for (md, font) in [
        ("A", B::TimesRoman),
        ("# A", B::TimesBold),
        ("**A**", B::TimesBold),
        ("*A*", B::TimesItalic),
    ] {
        assert_eq!(
            first_emitted_face(&render_with(md, "", &cfg)),
            expected_face(font),
            "{md}"
        );
    }
}

#[test]
fn bold_never_resolves_lighter_than_a_heavy_configured_face() {
    use printpdf::BuiltinFont as B;
    let fixtures = WeightFixtures::new();
    fixtures.write("Sans.ttf", B::Helvetica);
    fixtures.write("Sans Bold.ttf", B::HelveticaBold);
    let anchor = fixtures.write("Sans Black.ttf", B::TimesBold);
    let cfg = FontConfig::new().with_default_font_source(FontSource::file(anchor));
    for md in ["A", "**A**", "# A"] {
        assert_eq!(
            first_emitted_face(&render_with(md, "", &cfg)),
            expected_face(B::TimesBold),
            "{md}"
        );
    }
}

#[test]
fn inline_code_inherits_block_weight_unless_configured() {
    use printpdf::BuiltinFont as B;
    let fixtures = WeightFixtures::new();
    let body = fixtures.write("Body.ttf", B::Helvetica);
    fixtures.write("Body Bold.ttf", B::HelveticaBold);
    let mono = fixtures.write("Mono.ttf", B::Courier);
    fixtures.write("Mono Light.ttf", B::CourierOblique);
    fixtures.write("Mono Bold.ttf", B::CourierBold);
    let cfg = FontConfig::new()
        .with_default_font_source(FontSource::file(body))
        .with_code_font_source(FontSource::file(mono));
    for (md, toml, font) in [
        ("`A`", "", B::Courier),
        ("# `A`", "", B::CourierBold),
        ("**`A`**", "", B::CourierBold),
        ("`A`", "[paragraph]\nfont_weight = \"bold\"", B::CourierBold),
        (
            "# `A`",
            "[code_inline]\nfont_weight = \"light\"",
            B::CourierOblique,
        ),
    ] {
        assert_eq!(
            first_emitted_face(&render_with(md, toml, &cfg)),
            expected_face(font),
            "{md}: {toml}"
        );
    }
}
