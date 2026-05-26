//! Font metrics cache for the renderer.
//!
//! Two paths live here side by side:
//!
//! - **Built-in path**: printpdf 0.9's PDF Type 1 built-ins
//!   (Helvetica / Times / Courier and their bold / italic variants).
//!   Zero file I/O. ASCII-only because lopdf 0.39's WinAnsiEncoding
//!   handling falls through to a raw UTF-8 byte passthrough — see
//!   `to_win1252` in `layout.rs` for the gory detail. Used as the
//!   fallback whenever no external font is configured.
//!
//! - **External path**: a real TTF/OTF parsed via `ttf-parser` (for
//!   glyph metrics + cmap walking) *and* via `printpdf::ParsedFont`
//!   (for PDF embedding). Full Unicode rendering, glyph-ID encoded
//!   text. Selected when [`FontConfig`](crate::fonts::FontConfig)
//!   resolves to a [`FontSource::File`](crate::fonts::FontSource) or
//!   [`FontSource::System`](crate::fonts::FontSource) that we can
//!   locate.
//!
//! For measurement (line wrapping) both paths produce point-width
//! values via [`VariantMetrics::measure`]. For emission, the layout
//! engine asks [`FontSet::handle_for`] which font handle and which
//! transliteration policy to use.

use std::collections::BTreeMap;
use std::path::PathBuf;

use printpdf::{BuiltinFont, FontId, PdfDocument, PdfFontHandle};
use ttf_parser::Face;

use super::ir::{RunFlags, VariantUsage};
use crate::fonts::{FontConfig, FontSource, default_body_source, find_system_font};

/// The set of built-in PDF fonts the renderer can fall back to when
/// no external Unicode font is loaded. Body / emphasis runs map to a
/// Helvetica variant; monospace runs map to Courier.
#[derive(Debug, Clone, Copy)]
pub enum FontVariant {
    HelveticaRegular,
    HelveticaBold,
    HelveticaItalic,
    HelveticaBoldItalic,
    CourierRegular,
    CourierBold,
    CourierItalic,
    CourierBoldItalic,
}

impl FontVariant {
    /// Pick the variant that matches a run's [`RunFlags`].
    pub fn for_flags(flags: RunFlags) -> Self {
        match (flags.monospace, flags.bold, flags.italic) {
            (true, true, true) => FontVariant::CourierBoldItalic,
            (true, true, false) => FontVariant::CourierBold,
            (true, false, true) => FontVariant::CourierItalic,
            (true, false, false) => FontVariant::CourierRegular,
            (false, true, true) => FontVariant::HelveticaBoldItalic,
            (false, true, false) => FontVariant::HelveticaBold,
            (false, false, true) => FontVariant::HelveticaItalic,
            (false, false, false) => FontVariant::HelveticaRegular,
        }
    }

    /// printpdf's [`BuiltinFont`] handle for emission.
    pub fn builtin(self) -> BuiltinFont {
        match self {
            FontVariant::HelveticaRegular => BuiltinFont::Helvetica,
            FontVariant::HelveticaBold => BuiltinFont::HelveticaBold,
            FontVariant::HelveticaItalic => BuiltinFont::HelveticaOblique,
            FontVariant::HelveticaBoldItalic => BuiltinFont::HelveticaBoldOblique,
            FontVariant::CourierRegular => BuiltinFont::Courier,
            FontVariant::CourierBold => BuiltinFont::CourierBold,
            FontVariant::CourierItalic => BuiltinFont::CourierOblique,
            FontVariant::CourierBoldItalic => BuiltinFont::CourierBoldOblique,
        }
    }
}

/// Per-variant glyph-width data extracted from the embedded TTF.
///
/// `unscaled_widths[c as usize]` is the advance width in font units
/// (typically 1/1000 em) for characters U+0000..=U+00FF. Anything
/// outside ASCII/Latin-1 falls back to the average width — the
/// built-in subsets only cover Win-1252 anyway, so unsupported
/// characters won't render correctly in the PDF either way.
pub struct VariantMetrics {
    pub units_per_em: u16,
    pub unscaled_widths: Box<[u16; 256]>,
    pub fallback_width: u16,
}

impl VariantMetrics {
    fn load(variant: FontVariant) -> Self {
        let subset = variant.builtin().get_subset_font();
        // The built-in subsets are uncompressed TTF bytes ready for parsing.
        let face = Face::parse(&subset.bytes, 0).expect("built-in subset font must parse");

        let units_per_em = face.units_per_em();
        let mut widths = Box::new([0u16; 256]);

        for (i, slot) in widths.iter_mut().enumerate() {
            let ch = char::from_u32(i as u32).unwrap_or('\0');
            *slot = face
                .glyph_index(ch)
                .and_then(|gid| face.glyph_hor_advance(gid))
                .unwrap_or(0);
        }

        // The embedded Helvetica subset is missing the glyph for
        // U+0020 (space) — `face.glyph_index(' ')` returns None. PDF
        // viewers render built-in fonts using the Adobe Type 1
        // metrics (Helvetica space = 278/1000 em), so our wrapping
        // needs the same value or every space causes the measured
        // x-position to drift relative to the actual rendered
        // position. Backfill from a hardcoded AFM table, scaled to
        // the subset's units_per_em.
        backfill_afm_widths(variant, units_per_em, &mut widths);

        // Compute the average advance for missing-codepoint fallback.
        let (sum, count) = widths.iter().fold((0u64, 0u64), |(s, c), w| {
            if *w > 0 {
                (s + *w as u64, c + 1)
            } else {
                (s, c)
            }
        });
        let fallback_width = if count > 0 {
            (sum / count) as u16
        } else {
            500
        };

        VariantMetrics {
            units_per_em,
            unscaled_widths: widths,
            fallback_width,
        }
    }

    /// Advance width of `text` at `font_size_pt`, in points.
    #[allow(dead_code)]
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// Advance width of `text` at `font_size_pt`, in points.
    ///
    /// For each input char, sums the advances of the chars that the
    /// built-in emit path will actually write (see
    /// [`for_each_builtin_emit_char`]). Measuring the source codepoints
    /// directly would charge a `fallback_width` for things like `•`
    /// while the emit path writes `*` — the resulting drift between
    /// measured and rendered x positions misplaces underlines, link
    /// rects, and wrap break points on lines containing transliterated
    /// characters.
    pub fn measure(&self, text: &str, font_size_pt: f32) -> f32 {
        let mut unscaled: u64 = 0;
        for c in text.chars() {
            for_each_builtin_emit_char(c, |emitted| {
                let idx = emitted as u32 as usize;
                let w = if idx < 256 {
                    self.unscaled_widths[idx]
                } else {
                    0
                };
                let w = if w == 0 { self.fallback_width } else { w };
                unscaled += w as u64;
            });
        }
        unscaled as f32 * font_size_pt / self.units_per_em as f32
    }
}

/// Cache of [`VariantMetrics`] for every built-in variant the
/// renderer can pick. Built once per render call. The 8 variants
/// combined are a few KiB.
pub struct FontMetricsCache {
    helvetica_regular: VariantMetrics,
    helvetica_bold: VariantMetrics,
    helvetica_italic: VariantMetrics,
    helvetica_bold_italic: VariantMetrics,
    courier_regular: VariantMetrics,
    courier_bold: VariantMetrics,
    courier_italic: VariantMetrics,
    courier_bold_italic: VariantMetrics,
}

impl FontMetricsCache {
    pub fn new() -> Self {
        Self {
            helvetica_regular: VariantMetrics::load(FontVariant::HelveticaRegular),
            helvetica_bold: VariantMetrics::load(FontVariant::HelveticaBold),
            helvetica_italic: VariantMetrics::load(FontVariant::HelveticaItalic),
            helvetica_bold_italic: VariantMetrics::load(FontVariant::HelveticaBoldItalic),
            courier_regular: VariantMetrics::load(FontVariant::CourierRegular),
            courier_bold: VariantMetrics::load(FontVariant::CourierBold),
            courier_italic: VariantMetrics::load(FontVariant::CourierItalic),
            courier_bold_italic: VariantMetrics::load(FontVariant::CourierBoldItalic),
        }
    }

    pub fn for_variant(&self, v: FontVariant) -> &VariantMetrics {
        match v {
            FontVariant::HelveticaRegular => &self.helvetica_regular,
            FontVariant::HelveticaBold => &self.helvetica_bold,
            FontVariant::HelveticaItalic => &self.helvetica_italic,
            FontVariant::HelveticaBoldItalic => &self.helvetica_bold_italic,
            FontVariant::CourierRegular => &self.courier_regular,
            FontVariant::CourierBold => &self.courier_bold,
            FontVariant::CourierItalic => &self.courier_italic,
            FontVariant::CourierBoldItalic => &self.courier_bold_italic,
        }
    }
}

/// One user-supplied external font, registered with the PDF document.
///
/// Holds the printpdf [`FontId`] for emission plus a glyph-width
/// table for measurement. The font is the same one whether the run
/// has bold / italic flags or not — synthetic weights aren't faked
/// today, so bold / italic flags only affect text *color* (when
/// linked) but not the visual weight when an external font is used.
/// Per-weight external variants are a phase-8 follow-up.
pub struct ExternalFont {
    pub font_id: FontId,
    units_per_em: u16,
    /// codepoint -> glyph advance width in `units_per_em`.
    advance_by_codepoint: BTreeMap<u32, u16>,
    /// Average glyph width, used as fallback for unmapped codepoints.
    fallback_advance: u16,
}

impl ExternalFont {
    /// Measure the advance of `text` at `font_size_pt`.
    pub fn measure(&self, text: &str, font_size_pt: f32) -> f32 {
        let mut unscaled: u64 = 0;
        for c in text.chars() {
            let w = self
                .advance_by_codepoint
                .get(&(c as u32))
                .copied()
                .unwrap_or(self.fallback_advance);
            unscaled += w as u64;
        }
        unscaled as f32 * font_size_pt / self.units_per_em as f32
    }

    /// `true` if this font has a real glyph for `c` (i.e. the
    /// codepoint was in the keep-set when the font was loaded *and*
    /// the un-subsetted face exposed a non-`.notdef` glyph index for
    /// it). Used to pick which font in a fallback chain emits a
    /// given codepoint.
    pub fn covers(&self, c: char) -> bool {
        self.advance_by_codepoint.contains_key(&(c as u32))
    }
}

/// The complete font set for one render call: built-ins always
/// available, plus optional external default-body and code fonts.
///
/// Each external family has up to four weight slots; `regular` is
/// the anchor (loaded from whatever path the user pointed at), and
/// the others are discovered by searching sibling files in the
/// same directory (`Georgia.ttf` -> `Georgia Bold.ttf`,
/// `Georgia Italic.ttf`, `Georgia Bold Italic.ttf`). Missing slots
/// fall back to `regular` at resolve time.
pub struct FontSet {
    pub builtin: FontMetricsCache,
    pub external_body: ExternalFamily,
    pub external_code: ExternalFamily,
    /// Inline-code / `<kbd>` family, separate from `external_code`
    /// (which serves fenced code blocks). Loaded only when
    /// `[code_inline].font_family` is configured; otherwise inline-code
    /// runs fall through to `external_code`, then to builtin Courier.
    pub external_code_inline: ExternalFamily,
    /// Ordered fallback fonts consulted when the primary body / code
    /// font does not cover a codepoint. Regular weight only — fallbacks
    /// are loaded once per family and reused for every flag combination.
    pub fallbacks: Vec<ExternalFont>,
}

/// Up to four weight slots for an external font family.
#[derive(Default)]
pub struct ExternalFamily {
    pub regular: Option<ExternalFont>,
    pub bold: Option<ExternalFont>,
    pub italic: Option<ExternalFont>,
    pub bold_italic: Option<ExternalFont>,
}

impl ExternalFamily {
    /// Best match for the given flags. Falls back through
    /// bold_italic -> bold -> italic -> regular as variants are
    /// missing.
    pub fn pick(&self, flags: RunFlags) -> Option<&ExternalFont> {
        match (flags.bold, flags.italic) {
            (true, true) => self
                .bold_italic
                .as_ref()
                .or(self.bold.as_ref())
                .or(self.italic.as_ref())
                .or(self.regular.as_ref()),
            (true, false) => self.bold.as_ref().or(self.regular.as_ref()),
            (false, true) => self.italic.as_ref().or(self.regular.as_ref()),
            (false, false) => self.regular.as_ref(),
        }
    }

    /// Any external slot is filled — used to decide whether to take
    /// the external path at all.
    pub fn is_loaded(&self) -> bool {
        self.regular.is_some()
            || self.bold.is_some()
            || self.italic.is_some()
            || self.bold_italic.is_some()
    }
}

/// How a variant resolves at emit time.
pub enum FontResolution<'a> {
    /// Use a built-in PDF font. Text must pass through
    /// `to_win1252` transliteration before reaching `Op::ShowText`.
    Builtin {
        handle: PdfFontHandle,
        metrics: &'a VariantMetrics,
    },
    /// Use a user-supplied external TTF. `Op::ShowText` accepts
    /// full Unicode; printpdf does codepoint -> glyph lookup
    /// using the registered [`ParsedFont`].
    External {
        handle: PdfFontHandle,
        font: &'a ExternalFont,
    },
}

/// One contiguous run of text destined for a single PDF font handle.
/// Produced by [`FontSet::split_for_emit`]: the layout engine emits
/// one `SetFont` + `ShowText` pair per chunk.
#[derive(Debug, Clone)]
pub struct EmitChunk {
    pub handle: PdfFontHandle,
    /// `true` iff `text` must pass through `to_win1252` before reaching
    /// `Op::ShowText`. Only set for built-in chunks.
    pub needs_transliteration: bool,
    pub text: String,
    /// Advance width of `text` at the size requested by the caller of
    /// `split_for_emit`. Precomputed so the call site doesn't have to
    /// re-walk the codepoints.
    pub width_pt: f32,
}

/// Per-codepoint choice of which font slot emits it. Used internally
/// by `split_for_emit` to group consecutive same-choice codepoints
/// into chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontPick {
    Primary,
    Fallback(usize),
}

impl FontSet {
    /// Build the font set for a render call.
    ///
    /// `used_codepoints` should be every distinct character that
    /// appears in the document. `usage` tells us which weight
    /// variants are actually referenced so we don't embed
    /// bold/italic/bold-italic faces that the document never asks
    /// for. Regular is always loaded; the optional weights are
    /// loaded only when `usage` flags them.
    ///
    /// `extra_fallbacks` is the list of fallback font sources
    /// configured at the document level (`[defaults].fallback_fonts`
    /// in TOML) combined with any sources/names on `FontConfig`. Each
    /// is loaded in order, regular weight only; consumed by
    /// [`FontSet::split_for_emit`] when the primary lacks a glyph.
    pub fn load(
        font_config: Option<&FontConfig>,
        used_codepoints: &[char],
        usage: VariantUsage,
        doc: &mut PdfDocument,
    ) -> Self {
        let builtin = FontMetricsCache::new();
        let body_variants = BodyVariantNeed {
            bold: usage.body_bold || usage.body_bold_italic,
            italic: usage.body_italic || usage.body_bold_italic,
            bold_italic: usage.body_bold_italic,
        };
        // Inline-code variants count toward the regular code family
        // too: when `[code_inline].font_family` isn't configured,
        // inline-code runs fall back to `external_code` and still need
        // its bold / italic faces.
        let code_variants = BodyVariantNeed {
            bold: usage.mono_bold
                || usage.mono_bold_italic
                || usage.inline_code_bold
                || usage.inline_code_bold_italic,
            italic: usage.mono_italic
                || usage.mono_bold_italic
                || usage.inline_code_italic
                || usage.inline_code_bold_italic,
            bold_italic: usage.mono_bold_italic || usage.inline_code_bold_italic,
        };
        // Try the user-picked body font first. If that resolves
        // (System name finds a .ttf/.otf, an explicit File path exists,
        // or raw Bytes parse), use it. Otherwise probe a per-OS list
        // of likely-installed system Unicode fonts. This covers two
        // cases that would otherwise fall through to the built-in
        // Type 1 Helvetica — whose WinAnsi-only encoder turns every
        // non-ASCII codepoint into `?`:
        //   1. Nothing is configured at all (programmatic caller
        //      passed `None`, default theme's `font_family` not
        //      propagated).
        //   2. The configured name is a built-in alias like
        //      `Helvetica` whose only on-disk copy on macOS lives in
        //      a `.ttc` collection the loader skips.
        //
        // An explicit `FontSource::Builtin(...)` opt-out skips the
        // auto-detect — callers (notably the test render helper) use
        // it to assert on the deterministic WinAnsi text emission of
        // the built-in path, which the Identity-H external path
        // doesn't produce.
        let user_src = font_config.and_then(default_source);
        let opted_into_builtin = matches!(&user_src, Some(FontSource::Builtin(_)));
        let external_body = load_external_family(user_src, used_codepoints, body_variants, doc)
            .or_else(|| {
                if opted_into_builtin {
                    return None;
                }
                load_external_family(
                    default_body_source(),
                    used_codepoints,
                    body_variants,
                    doc,
                )
            })
            .unwrap_or_default();
        // If the user picked an external body font but didn't specify
        // a code font, try a sensible system monospace fallback. Mixing
        // an external Unicode body font with the built-in Type 1 Courier
        // for inline code makes the inline-code space glyph ~2× wider
        // than the surrounding body spaces (Courier 600/1000 em vs e.g.
        // Georgia ~280/1000 em), which shows up as a visible gap and a
        // jumpy baseline at every font transition.
        let user_code_src = font_config.and_then(code_source);
        let code_src = match user_code_src {
            Some(src) => Some(src),
            None if external_body.is_loaded() => default_monospace_source(),
            None => None,
        };
        let external_code = load_external_family(code_src, used_codepoints, code_variants, doc)
            .unwrap_or_default();
        let fallbacks = load_fallbacks(font_config, used_codepoints, doc);
        Self {
            builtin,
            external_body,
            external_code,
            external_code_inline: ExternalFamily::default(),
            fallbacks,
        }
    }

    /// Build the font set with an additional list of fallback sources
    /// (resolved from `[defaults].fallback_fonts` in the styling
    /// config) and an optional dedicated inline-code font family
    /// (`[code_inline].font_family`). Fallback names are appended
    /// *after* anything declared on `FontConfig` so programmatic
    /// config wins on order. When `code_inline_name` is `None` the
    /// inline-code family stays empty and inline-code runs fall
    /// through to the regular code family.
    pub fn load_with_style_fallbacks(
        font_config: Option<&FontConfig>,
        style_fallback_names: &[String],
        code_inline_name: Option<&str>,
        used_codepoints: &[char],
        usage: VariantUsage,
        doc: &mut PdfDocument,
    ) -> Self {
        let mut set = Self::load(font_config, used_codepoints, usage, doc);
        if let Some(name) = code_inline_name {
            let inline_variants = BodyVariantNeed {
                bold: usage.inline_code_bold || usage.inline_code_bold_italic,
                italic: usage.inline_code_italic || usage.inline_code_bold_italic,
                bold_italic: usage.inline_code_bold_italic,
            };
            set.external_code_inline = load_external_family(
                Some(name_to_external_source(name)),
                used_codepoints,
                inline_variants,
                doc,
            )
            .unwrap_or_default();
        }
        for name in style_fallback_names {
            let src = name_to_external_source(name);
            let Some((_, bytes)) = resolve_regular(src) else {
                continue;
            };
            if let Some(font) = parse_and_register(bytes, "fallback", used_codepoints, doc) {
                set.fallbacks.push(font);
            }
        }
        set
    }

    /// Resolve a [`RunFlags`] to a concrete font choice — the
    /// *primary* font for that flag combination. Fallback selection
    /// happens per-codepoint inside [`FontSet::split_for_emit`].
    pub fn resolve(&self, flags: RunFlags) -> FontResolution<'_> {
        if flags.inline_code {
            if let Some(ext) = self.external_code_inline.pick(flags) {
                return FontResolution::External {
                    handle: PdfFontHandle::External(ext.font_id.clone()),
                    font: ext,
                };
            }
        }
        if flags.monospace {
            if let Some(ext) = self.external_code.pick(flags) {
                return FontResolution::External {
                    handle: PdfFontHandle::External(ext.font_id.clone()),
                    font: ext,
                };
            }
        } else if let Some(ext) = self.external_body.pick(flags) {
            return FontResolution::External {
                handle: PdfFontHandle::External(ext.font_id.clone()),
                font: ext,
            };
        }
        let variant = FontVariant::for_flags(flags);
        FontResolution::Builtin {
            handle: PdfFontHandle::Builtin(variant.builtin()),
            metrics: self.builtin.for_variant(variant),
        }
    }

    /// Total advance width of `text` at `size_pt`. Walks fallback
    /// coverage so a mixed-script run measures correctly even when
    /// different codepoints render in different fonts.
    pub fn measure(&self, flags: RunFlags, text: &str, size_pt: f32) -> f32 {
        if self.fallbacks.is_empty() {
            return match self.resolve(flags) {
                FontResolution::Builtin { metrics, .. } => metrics.measure(text, size_pt),
                FontResolution::External { font, .. } => font.measure(text, size_pt),
            };
        }
        self.split_for_emit(flags, text, size_pt)
            .iter()
            .map(|c| c.width_pt)
            .sum()
    }

    /// `true` if the *primary* font for `flags` is a built-in and
    /// emitted text has to pass through `to_win1252`. Note: even when
    /// this returns `true`, individual codepoints may still emit via
    /// an external fallback — only the primary's policy is reported.
    pub fn needs_transliteration(&self, flags: RunFlags) -> bool {
        matches!(self.resolve(flags), FontResolution::Builtin { .. })
    }

    /// Split `text` into the smallest sequence of single-font chunks
    /// that the layout engine can emit one-after-another. Each chunk's
    /// codepoints are all covered by the same font slot: the primary
    /// for the run's flags, or one of the loaded fallbacks (the first
    /// one whose face has a glyph for the codepoint).
    ///
    /// When the primary is a built-in (ASCII-only via WinAnsi
    /// transliteration), every non-ASCII codepoint tries the fallback
    /// chain. When the primary is an external Unicode font,
    /// codepoints absent from its subset try the fallback chain.
    /// Codepoints covered nowhere are emitted on the primary (where
    /// the renderer will show `.notdef` boxes or transliterate to
    /// `?`).
    ///
    /// Width is precomputed against `size_pt` so callers that need
    /// both a width sum (for wrapping) and a per-chunk emission don't
    /// pay the per-glyph scan twice.
    pub fn split_for_emit(
        &self,
        flags: RunFlags,
        text: &str,
        size_pt: f32,
    ) -> Vec<EmitChunk> {
        if text.is_empty() {
            return Vec::new();
        }
        let primary = self.resolve(flags);
        // Fast path: no fallbacks configured. Everything emits via the
        // primary as a single chunk. Identical behavior to the
        // pre-fallback code path.
        if self.fallbacks.is_empty() {
            return vec![chunk_from_resolution(&primary, text.to_string(), size_pt)];
        }
        let mut chunks: Vec<EmitChunk> = Vec::new();
        let mut buf = String::new();
        let mut current: Option<FontPick> = None;
        for c in text.chars() {
            let pick = if primary_covers(&primary, c) {
                FontPick::Primary
            } else if let Some(idx) = self.fallbacks.iter().position(|f| f.covers(c)) {
                FontPick::Fallback(idx)
            } else {
                // No font in the chain covers `c`. Emit via the primary
                // — that path will either render `.notdef` (external) or
                // transliterate to `?` (built-in). Either degradation is
                // visible and non-panicking.
                FontPick::Primary
            };
            match current {
                Some(cur) if cur == pick => buf.push(c),
                Some(cur) => {
                    chunks.push(self.build_chunk(cur, std::mem::take(&mut buf), &primary, size_pt));
                    buf.push(c);
                    current = Some(pick);
                }
                None => {
                    buf.push(c);
                    current = Some(pick);
                }
            }
        }
        if let Some(cur) = current {
            chunks.push(self.build_chunk(cur, buf, &primary, size_pt));
        }
        chunks
    }

    fn build_chunk(
        &self,
        pick: FontPick,
        text: String,
        primary: &FontResolution<'_>,
        size_pt: f32,
    ) -> EmitChunk {
        match pick {
            FontPick::Primary => chunk_from_resolution(primary, text, size_pt),
            FontPick::Fallback(idx) => {
                let font = &self.fallbacks[idx];
                let width_pt = font.measure(&text, size_pt);
                EmitChunk {
                    handle: PdfFontHandle::External(font.font_id.clone()),
                    needs_transliteration: false,
                    text,
                    width_pt,
                }
            }
        }
    }
}

/// `true` iff the *primary* font (already resolved for the run flags)
/// has a glyph for `c`. For built-ins this is "ASCII or directly
/// representable in WinAnsi"; for external faces it's a subset
/// membership check.
fn primary_covers(primary: &FontResolution<'_>, c: char) -> bool {
    match primary {
        FontResolution::External { font, .. } => font.covers(c),
        FontResolution::Builtin { .. } => (c as u32) < 0x80,
    }
}

fn chunk_from_resolution(
    primary: &FontResolution<'_>,
    text: String,
    size_pt: f32,
) -> EmitChunk {
    match primary {
        FontResolution::Builtin { handle, metrics } => {
            let width_pt = metrics.measure(&text, size_pt);
            EmitChunk {
                handle: handle.clone(),
                needs_transliteration: true,
                text,
                width_pt,
            }
        }
        FontResolution::External { handle, font } => {
            let width_pt = font.measure(&text, size_pt);
            EmitChunk {
                handle: handle.clone(),
                needs_transliteration: false,
                text,
                width_pt,
            }
        }
    }
}

/// Load every fallback font declared on `FontConfig`, in order. Each
/// is parsed as a regular-weight family (no bold / italic discovery —
/// fallbacks reuse the regular glyphs for every flag combination).
/// Failures are logged and skipped, never propagated; a missing
/// fallback simply means uncovered codepoints stay uncovered.
fn load_fallbacks(
    font_config: Option<&FontConfig>,
    used_codepoints: &[char],
    doc: &mut PdfDocument,
) -> Vec<ExternalFont> {
    let mut out = Vec::new();
    let Some(cfg) = font_config else {
        return out;
    };
    let mut sources: Vec<FontSource> = Vec::new();
    sources.extend(cfg.fallback_font_sources.iter().cloned());
    sources.extend(cfg.fallback_fonts.iter().map(|n| name_to_external_source(n)));
    for src in sources {
        let Some((_, bytes)) = resolve_regular(src) else {
            continue;
        };
        if let Some(font) = parse_and_register(bytes, "fallback", used_codepoints, doc) {
            out.push(font);
        }
    }
    out
}

fn default_source(c: &FontConfig) -> Option<FontSource> {
    if let Some(src) = c.default_font_source.clone() {
        return Some(src);
    }
    c.default_font.as_deref().map(name_to_external_source)
}

fn code_source(c: &FontConfig) -> Option<FontSource> {
    if let Some(src) = c.code_font_source.clone() {
        return Some(src);
    }
    c.code_font.as_deref().map(name_to_external_source)
}

/// Walk a per-OS list of likely-installed system monospace fonts and
/// return the first one we can locate. Used when an external body font
/// is configured but the user didn't specify a code font — keeps both
/// paths on the same external Unicode renderer so inline code shares a
/// baseline with surrounding body text.
fn default_monospace_source() -> Option<FontSource> {
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] = &["Menlo", "Monaco", "Courier New"];
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &["Consolas", "Cascadia Code", "Courier New", "Lucida Console"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CANDIDATES: &[&str] = &[
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
        "Ubuntu Mono",
    ];
    for name in CANDIDATES {
        if find_system_font(name).is_some() {
            return Some(FontSource::System((*name).to_string()));
        }
    }
    None
}

/// Resolve a user-supplied font name to a source the external
/// loader can consume.
///
/// `crate::fonts::resolve_font_source` maps friendly names like
/// "Arial" -> `Builtin("Helvetica")` because at the type-1 level
/// they're interchangeable — but the built-in path is ASCII-only,
/// so honoring that alias defeats Unicode rendering. Here we bias
/// toward a real system font lookup: path-like names go straight
/// to `File`, everything else goes to `System`. Falling back to a
/// built-in still happens, but only when the system lookup fails.
fn name_to_external_source(name: &str) -> FontSource {
    if name.contains('/')
        || name.contains('\\')
        || name.ends_with(".ttf")
        || name.ends_with(".otf")
    {
        return FontSource::File(name.into());
    }
    FontSource::System(name.to_string())
}

/// Resolve a `FontSource` to a regular-weight path (if any) and the
/// font bytes. The path is what we use for sibling-variant discovery.
fn resolve_regular(source: FontSource) -> Option<(Option<PathBuf>, Vec<u8>)> {
    match source {
        FontSource::Builtin(_) => None,
        FontSource::Bytes(b) => Some((None, b.to_vec())),
        FontSource::File(path) => {
            let bytes = read_font_file(&path)?;
            Some((Some(path), bytes))
        }
        FontSource::System(name) => {
            let path = find_system_font(&name).or_else(|| {
                log::warn!("could not locate system font {:?}", name);
                None
            })?;
            let bytes = read_font_file(&path)?;
            Some((Some(path), bytes))
        }
    }
}

/// Which weight variants the family loader should bother searching
/// for and embedding. Regular is always loaded if the family loads
/// at all; the optional weights are gated by document usage so we
/// don't embed (typically ~25 KB per variant after subsetting) for
/// weights the document never references.
#[derive(Debug, Clone, Copy, Default)]
pub struct BodyVariantNeed {
    pub bold: bool,
    pub italic: bool,
    pub bold_italic: bool,
}

/// Load the regular weight plus the discoverable variants that the
/// document actually uses. Returns `None` only if even the regular
/// weight failed to load.
fn load_external_family(
    source: Option<FontSource>,
    used_codepoints: &[char],
    need: BodyVariantNeed,
    doc: &mut PdfDocument,
) -> Option<ExternalFamily> {
    let source = source?;
    let (anchor_path, regular_bytes) = resolve_regular(source)?;
    let regular = parse_and_register(regular_bytes, "regular", used_codepoints, doc)?;

    let mut family = ExternalFamily {
        regular: Some(regular),
        ..ExternalFamily::default()
    };

    if let Some(path) = anchor_path {
        let candidates: &[(VariantKind, &[&str], bool)] = &[
            (VariantKind::Bold, &["Bold"], need.bold),
            (VariantKind::Italic, &["Italic", "Oblique"], need.italic),
            (
                VariantKind::BoldItalic,
                &["Bold Italic", "BoldItalic", "Bold-Italic", "BoldOblique"],
                need.bold_italic,
            ),
        ];
        for (kind, names, wanted) in candidates {
            if !wanted {
                continue;
            }
            if let Some(variant_path) = find_variant_path(&path, names) {
                if let Some(bytes) = read_font_file(&variant_path) {
                    if let Some(parsed) =
                        parse_and_register(bytes, kind.label(), used_codepoints, doc)
                    {
                        match kind {
                            VariantKind::Bold => family.bold = Some(parsed),
                            VariantKind::Italic => family.italic = Some(parsed),
                            VariantKind::BoldItalic => family.bold_italic = Some(parsed),
                        }
                    }
                }
            }
        }
    }

    if family.is_loaded() { Some(family) } else { None }
}

#[derive(Clone, Copy)]
enum VariantKind {
    Bold,
    Italic,
    BoldItalic,
}

impl VariantKind {
    fn label(self) -> &'static str {
        match self {
            VariantKind::Bold => "bold",
            VariantKind::Italic => "italic",
            VariantKind::BoldItalic => "bold-italic",
        }
    }
}

/// Given the regular-weight font's path, return a sibling file
/// matching one of the variant name patterns
/// (`Foo Bold.ttf`, `Foo-Bold.ttf`, `FooBold.ttf`, plus `.otf`).
fn find_variant_path(anchor: &std::path::Path, variant_names: &[&str]) -> Option<PathBuf> {
    let parent = anchor.parent()?;
    let stem = anchor.file_stem()?.to_string_lossy().to_string();
    for variant in variant_names {
        for sep in [" ", "-", ""] {
            for ext in ["ttf", "otf"] {
                let candidate = parent.join(format!("{}{}{}.{}", stem, sep, variant, ext));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

/// Invoke `f` once per char that the built-in (Helvetica/Courier)
/// emit path will actually write for `c`. ASCII passes through; a
/// curated set of Win-1252 punctuation transliterates to ASCII (often
/// expanding one source char into several, e.g. `—` → `--`); anything
/// else becomes `?`. The shared source of truth for both measurement
/// (`VariantMetrics::measure`) and emission (`to_win1252` in layout) —
/// drift between the two misplaces underlines, link rects, and line
/// breaks on lines containing transliterated characters.
pub fn for_each_builtin_emit_char(c: char, mut f: impl FnMut(char)) {
    match c as u32 {
        0x00..=0x7F => f(c),
        0x2014 => {
            f('-');
            f('-');
        }
        0x2013 => f('-'),
        0x2022 => f('*'),
        0x2018 | 0x2019 => f('\''),
        0x201C | 0x201D => f('"'),
        0x2026 => {
            f('.');
            f('.');
            f('.');
        }
        0x00A0 => f(' '),
        0x00A9 => {
            f('(');
            f('c');
            f(')');
        }
        0x00AE => {
            f('(');
            f('R');
            f(')');
        }
        0x2122 => {
            f('(');
            f('T');
            f('M');
            f(')');
        }
        _ => f('?'),
    }
}

/// Code points the renderer might synthesize at layout time even if
/// they never appear in the source document — bullet glyphs,
/// task-list brackets, ordered-list digits, etc. Including them in
/// the subset prevents `.notdef` boxes from showing where the layout
/// pass inserts them.
const RENDERER_INJECTED_CHARS: &[char] = &[
    '\u{2022}', // bullet •
    '[', ']', 'x', ' ', '.',
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    '(', ')', ':', '-',
];

fn parse_and_register(
    bytes: Vec<u8>,
    label: &str,
    used_codepoints: &[char],
    doc: &mut PdfDocument,
) -> Option<ExternalFont> {
    let face = match Face::parse(&bytes, 0) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("could not parse {} font face: {}", label, e);
            return None;
        }
    };
    let units_per_em = face.units_per_em();
    // Union of document codepoints + renderer-injected glyphs.
    // Deliberately *not* the whole BMP — keeping the keep-set small
    // is what makes the subset small.
    let mut codepoints: Vec<char> = used_codepoints.to_vec();
    codepoints.extend_from_slice(RENDERER_INJECTED_CHARS);
    codepoints.sort();
    codepoints.dedup();
    let used_codepoints = &codepoints[..];
    // Collect the original (pre-subset) glyph IDs we need along with
    // their advance widths in font units.
    let mut codepoint_to_orig_gid: BTreeMap<u32, u16> = BTreeMap::new();
    let mut orig_gid_advance: BTreeMap<u16, u16> = BTreeMap::new();
    for &ch in used_codepoints {
        let cp = ch as u32;
        if let Some(gid) = face.glyph_index(ch) {
            if let Some(w) = face.glyph_hor_advance(gid) {
                codepoint_to_orig_gid.insert(cp, gid.0);
                orig_gid_advance.insert(gid.0, w);
            }
        }
    }

    // Subset the font down to just those glyphs. `.notdef` (gid 0)
    // is included implicitly by the remapper, plus whatever the
    // subsetter pulls in for composite glyph dependencies and
    // required tables. If subsetting fails for any reason (CFF2
    // font, malformed font, etc.) we degrade gracefully to the full
    // font with original GIDs.
    let orig_gids: Vec<u16> = orig_gid_advance.keys().copied().collect();
    let remapper = subsetter::GlyphRemapper::new_from_glyphs_sorted(&orig_gids);
    let (subset_bytes, gid_remap): (Vec<u8>, Box<dyn Fn(u16) -> u16>) =
        match subsetter::subset(&bytes, 0, &remapper) {
            Ok(b) => (
                b,
                Box::new(move |old| remapper.get(old).unwrap_or(0)),
            ),
            Err(e) => {
                log::warn!(
                    "could not subset {} font: {:?}; embedding full font instead",
                    label,
                    e
                );
                (bytes.clone(), Box::new(|old| old))
            }
        };

    // Rebuild codepoint -> glyph and glyph -> width maps using the
    // *new* (post-subset) GIDs. printpdf looks up codepoints in
    // `codepoint_to_glyph` and emits those GIDs as the PDF byte
    // stream — they have to match the subset font's glyph table.
    let mut advance_by_codepoint: BTreeMap<u32, u16> = BTreeMap::new();
    let mut codepoint_to_glyph: BTreeMap<u32, u16> = BTreeMap::new();
    let mut glyph_widths: BTreeMap<u16, u16> = BTreeMap::new();
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for (cp, orig_gid) in &codepoint_to_orig_gid {
        let new_gid = gid_remap(*orig_gid);
        let w = orig_gid_advance.get(orig_gid).copied().unwrap_or(0);
        codepoint_to_glyph.insert(*cp, new_gid);
        glyph_widths.insert(new_gid, w);
        advance_by_codepoint.insert(*cp, w);
        if w > 0 {
            sum += w as u64;
            count += 1;
        }
    }
    let fallback_advance = if count > 0 {
        (sum / count) as u16
    } else {
        units_per_em / 2
    };

    // PDF spec (PDF 32000-1:2008 §9.8) requires /Ascent and /Descent
    // in glyph space normalized to 1000 units per em. printpdf 0.9
    // writes the FontMetrics values straight into the FontDescriptor,
    // so we have to pre-normalize here. Passing raw font units (1878
    // for Georgia at UPEM=2048) makes viewers compute selection
    // bounding boxes ~2× too tall, causing adjacent-line selection
    // rectangles to overlap.
    let ascent = normalize_to_1000_em(face.ascender(), units_per_em);
    let descent = normalize_to_1000_em(face.descender(), units_per_em);

    let parsed = printpdf::ParsedFont::with_glyph_data(
        subset_bytes,
        0,
        None,
        codepoint_to_glyph,
        glyph_widths,
        units_per_em,
        printpdf::FontMetrics { ascent, descent },
    );
    let font_id = doc.add_font(&parsed);

    Some(ExternalFont {
        font_id,
        units_per_em,
        advance_by_codepoint,
        fallback_advance,
    })
}

/// Rescale a metric expressed in font units into PDF's `/1000-em`
/// glyph space. Font-agnostic: works for any `units_per_em` from
/// 1 to 65535. The guard against zero avoids divide-by-zero on
/// pathologically malformed fonts.
fn normalize_to_1000_em(value: i16, units_per_em: u16) -> i16 {
    let upem = i32::from(units_per_em).max(1);
    let scaled = i32::from(value) * 1000 / upem;
    scaled.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

fn read_font_file(path: &std::path::Path) -> Option<Vec<u8>> {
    std::fs::read(path)
        .map_err(|e| log::warn!("could not read font {:?}: {}", path, e))
        .ok()
}

/// Fill in widths for code points that the embedded subset doesn't
/// provide. The values match Adobe's Standard 14 Type 1 metrics
/// (AFM) for the built-in PDF fonts, expressed per-1000-em and
/// scaled to the subset's actual `units_per_em` here.
///
/// Today only U+0020 (space) is missing across all variants. The
/// table is open to extension if other code points turn up missing
/// for the Times/Courier families.
fn backfill_afm_widths(variant: FontVariant, units_per_em: u16, widths: &mut [u16; 256]) {
    let space_per_1000 = match variant {
        FontVariant::HelveticaRegular
        | FontVariant::HelveticaBold
        | FontVariant::HelveticaItalic
        | FontVariant::HelveticaBoldItalic => 278u32,
        FontVariant::CourierRegular
        | FontVariant::CourierBold
        | FontVariant::CourierItalic
        | FontVariant::CourierBoldItalic => 600u32,
    };
    if widths[b' ' as usize] == 0 {
        let scaled = space_per_1000 * units_per_em as u32 / 1000;
        widths[b' ' as usize] = scaled.min(u16::MAX as u32) as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures_helvetica_width_monotonic_in_text_length() {
        let cache = FontMetricsCache::new();
        let m = cache.for_variant(FontVariant::HelveticaRegular);
        let a = m.measure("a", 12.0);
        let aaa = m.measure("aaa", 12.0);
        assert!(aaa > a * 2.5);
        // Sanity: 'a' at 12pt is roughly 6 points wide in Helvetica.
        assert!(a > 1.0 && a < 12.0, "unexpected width: {}", a);
    }

    #[test]
    fn courier_is_monospace() {
        let cache = FontMetricsCache::new();
        let m = cache.for_variant(FontVariant::CourierRegular);
        let i = m.measure("i", 12.0);
        let w = m.measure("w", 12.0);
        assert!((i - w).abs() < 0.5, "courier should be monospace");
    }

    #[test]
    fn normalize_to_1000_em_is_font_agnostic() {
        // Georgia: UPEM 2048, ascender 1878 → 916.
        assert_eq!(normalize_to_1000_em(1878, 2048), 916);
        assert_eq!(normalize_to_1000_em(-449, 2048), -219);
        // Many CFF / OTF fonts have UPEM 1000 — identity transform.
        assert_eq!(normalize_to_1000_em(800, 1000), 800);
        assert_eq!(normalize_to_1000_em(-200, 1000), -200);
        // Common TTF UPEM 1024.
        assert_eq!(normalize_to_1000_em(819, 1024), 799);
        // Apple-style high-precision UPEM 4096.
        assert_eq!(normalize_to_1000_em(3000, 4096), 732);
        // Zero UPEM is malformed; guard prevents divide-by-zero.
        assert_eq!(normalize_to_1000_em(1000, 0), i16::MAX);
        // i16-overflow inputs saturate rather than wrap.
        assert_eq!(normalize_to_1000_em(i16::MAX, 1), i16::MAX);
        assert_eq!(normalize_to_1000_em(i16::MIN, 1), i16::MIN);
    }

    #[test]
    fn default_monospace_source_resolves_on_supported_oses() {
        // We don't assert *which* font wins — the per-OS candidate
        // list is what guarantees Menlo/Monaco/Consolas/DejaVu Mono
        // is found. The contract is just: at least one is locatable
        // on a typical developer machine.
        let src = default_monospace_source();
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            assert!(
                src.is_some(),
                "expected a system monospace fallback on macOS/Windows"
            );
        }
        // On Linux containers without any monospace font installed,
        // None is the correct answer (graceful degradation to the
        // built-in Courier path).
    }

    #[test]
    fn split_with_no_fallbacks_returns_single_chunk() {
        // No font_config + no fallbacks means everything routes through
        // a single primary (either the auto-detected external body font
        // on hosts that have one, or built-in Helvetica when the probe
        // returns None). Either way the input must collapse to a single
        // chunk; only the transliteration flag differs by path (built-in
        // sets it true so `to_win1252` runs; external leaves it false).
        let mut doc = PdfDocument::new("test");
        let set = FontSet::load(None, &[], VariantUsage::default(), &mut doc);
        let chunks = set.split_for_emit(RunFlags::default(), "Hello", 12.0);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, "Hello");
        let on_external_path = set.external_body.is_loaded();
        assert_eq!(chunks[0].needs_transliteration, !on_external_path);
        assert!(chunks[0].width_pt > 0.0);
    }

    #[test]
    fn split_empty_text_returns_empty() {
        let mut doc = PdfDocument::new("test");
        let set = FontSet::load(None, &[], VariantUsage::default(), &mut doc);
        let chunks = set.split_for_emit(RunFlags::default(), "", 12.0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn measure_equals_sum_of_per_chunk_widths() {
        // The wrapping path calls `measure(flags, text, size)` and the
        // emit path iterates `split_for_emit(flags, text, size)`. The
        // sum of per-chunk widths must equal `measure` exactly, or
        // wrap will under-/over-estimate where the glyphs actually
        // land. This test pins the invariant for the no-fallback fast
        // path (the only one we can construct without an external
        // font file in unit tests).
        let mut doc = PdfDocument::new("test");
        let set = FontSet::load(None, &[], VariantUsage::default(), &mut doc);
        let cases = ["", "Hello", "Hello world", "ABCDE 12345 !?.,"];
        for text in cases {
            let direct = set.measure(RunFlags::default(), text, 10.0);
            let summed: f32 = set
                .split_for_emit(RunFlags::default(), text, 10.0)
                .iter()
                .map(|c| c.width_pt)
                .sum();
            assert!(
                (direct - summed).abs() < 1e-3,
                "measure({:?}) {} != sum-of-chunks {}",
                text,
                direct,
                summed
            );
        }
    }

    #[test]
    fn builtin_measure_matches_transliterated_emit() {
        // `to_win1252` rewrites a curated set of Win-1252 punctuation
        // to ASCII before emission (`•` → `*`, `—` → `--`, `…` → `...`,
        // `(c)`, `(R)`, `(TM)`, etc.). `measure` must price each source
        // char at the advance the emit path will actually write, or the
        // layout cursor drifts away from the PDF text matrix and
        // misplaces underlines / link rects.
        let cache = FontMetricsCache::new();
        let m = cache.for_variant(FontVariant::HelveticaRegular);
        let pairs = [
            ("\u{2022}", "*"),
            ("\u{2014}", "--"),
            ("\u{2013}", "-"),
            ("\u{2026}", "..."),
            ("\u{2018}", "'"),
            ("\u{2019}", "'"),
            ("\u{201C}", "\""),
            ("\u{201D}", "\""),
            ("\u{00A0}", " "),
            ("\u{00A9}", "(c)"),
            ("\u{00AE}", "(R)"),
            ("\u{2122}", "(TM)"),
            ("a \u{2022} b \u{2014} c \u{2026}", "a * b -- c ..."),
        ];
        for (src, emitted) in pairs {
            let measured = m.measure(src, 12.0);
            let actual = m.measure(emitted, 12.0);
            assert!(
                (measured - actual).abs() < 1e-3,
                "measure({:?}) = {} but emit writes {:?} with width {}",
                src,
                measured,
                emitted,
                actual,
            );
        }
        // Codepoints that aren't in the curated map should price as
        // `?`, which is what `to_win1252` will actually emit for them.
        let unknown = m.measure("\u{4E2D}", 12.0);
        let q = m.measure("?", 12.0);
        assert!(
            (unknown - q).abs() < 1e-3,
            "uncurated codepoint should price as '?': {} vs {}",
            unknown,
            q,
        );
    }

    #[test]
    fn missing_fallback_source_does_not_panic() {
        // A configured fallback that can't be located on disk should
        // simply not load — the renderer must still emit text through
        // the primary font (degraded for uncovered codepoints, never
        // a panic). Verifies graceful no-op on bad config.
        let cfg = FontConfig {
            default_font: None,
            code_font: None,
            default_font_source: None,
            code_font_source: None,
            fallback_fonts: vec!["This_Font_Definitely_Does_Not_Exist_12345".to_string()],
            fallback_font_sources: Vec::new(),
            enable_subsetting: true,
        };
        let mut doc = PdfDocument::new("test");
        let set = FontSet::load(Some(&cfg), &['日' as char], VariantUsage::default(), &mut doc);
        assert!(set.fallbacks.is_empty());
        // Uncovered codepoint must not panic — it routes through the
        // primary's degraded path. With the auto-detected body font
        // on macOS/Windows that's a `.notdef` glyph (external path,
        // no transliteration); on a host without a probe candidate
        // it's the built-in path that transliterates to `?`.
        let chunks = set.split_for_emit(RunFlags::default(), "日", 12.0);
        assert_eq!(chunks.len(), 1);
        let on_external_path = set.external_body.is_loaded();
        assert_eq!(chunks[0].needs_transliteration, !on_external_path);
    }

    #[test]
    fn for_flags_routes_correctly() {
        assert!(matches!(
            FontVariant::for_flags(RunFlags::default()),
            FontVariant::HelveticaRegular
        ));
        assert!(matches!(
            FontVariant::for_flags(RunFlags::default().with_bold()),
            FontVariant::HelveticaBold
        ));
        assert!(matches!(
            FontVariant::for_flags(RunFlags::default().with_monospace().with_bold()),
            FontVariant::CourierBold
        ));
    }
}
