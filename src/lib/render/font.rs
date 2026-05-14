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

use super::ir::RunFlags;
use crate::fonts::{FontConfig, FontSource, find_system_font};

/// The family of built-in fonts the renderer uses for everything.
///
/// Phase 1: regular paragraphs use Helvetica; code blocks use Courier;
/// emphasis/strong picks the corresponding Helvetica variant.
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
    pub fn measure(&self, text: &str, font_size_pt: f32) -> f32 {
        let mut unscaled: u64 = 0;
        for c in text.chars() {
            let w = if (c as u32) < 256 {
                self.unscaled_widths[c as usize]
            } else {
                0
            };
            let w = if w == 0 { self.fallback_width } else { w };
            unscaled += w as u64;
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

impl FontSet {
    /// Build the font set for a render call.
    ///
    /// `used_codepoints` should be every distinct character that
    /// appears in the document — we walk the font's cmap once per
    /// codepoint to populate the glyph-index table. Missing
    /// codepoints fall back to glyph 0 (typically `.notdef`, a small
    /// box) which is the standard PDF behavior.
    pub fn load(
        font_config: Option<&FontConfig>,
        used_codepoints: &[char],
        doc: &mut PdfDocument,
    ) -> Self {
        let builtin = FontMetricsCache::new();
        let external_body = font_config
            .and_then(|c| load_external_family(default_source(c), used_codepoints, doc))
            .unwrap_or_default();
        let external_code = font_config
            .and_then(|c| load_external_family(code_source(c), used_codepoints, doc))
            .unwrap_or_default();
        Self {
            builtin,
            external_body,
            external_code,
        }
    }

    /// Resolve a [`RunFlags`] to a concrete font choice.
    pub fn resolve(&self, flags: RunFlags) -> FontResolution<'_> {
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

    pub fn measure(&self, flags: RunFlags, text: &str, size_pt: f32) -> f32 {
        match self.resolve(flags) {
            FontResolution::Builtin { metrics, .. } => metrics.measure(text, size_pt),
            FontResolution::External { font, .. } => font.measure(text, size_pt),
        }
    }

    pub fn handle(&self, flags: RunFlags) -> PdfFontHandle {
        match self.resolve(flags) {
            FontResolution::Builtin { handle, .. } | FontResolution::External { handle, .. } => {
                handle
            }
        }
    }

    /// `true` if text emitted via this variant has to pass through
    /// the `to_win1252` ASCII transliterator (because the built-in
    /// font path can't survive non-ASCII bytes).
    pub fn needs_transliteration(&self, flags: RunFlags) -> bool {
        matches!(self.resolve(flags), FontResolution::Builtin { .. })
    }
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

/// Load the regular weight plus any discoverable bold/italic/bold-italic
/// variants from sibling files. Returns `None` only if even the
/// regular weight failed to load — partial loads (regular + just
/// bold, say) still produce a viable family.
fn load_external_family(
    source: Option<FontSource>,
    used_codepoints: &[char],
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
        for (kind, names) in [
            (VariantKind::Bold, &["Bold"][..]),
            (VariantKind::Italic, &["Italic", "Oblique"][..]),
            (
                VariantKind::BoldItalic,
                &["Bold Italic", "BoldItalic", "Bold-Italic", "BoldOblique"][..],
            ),
        ] {
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
    // Pre-populate from the document's codepoints first (cheap,
    // covers everything the user typed), then sweep the BMP so the
    // renderer's synthesized characters — bullet glyphs, task-list
    // brackets, etc. — also resolve to real glyphs. Codepoints
    // already present from the document path are skipped on the
    // second pass.
    let mut codepoints: Vec<char> = used_codepoints.to_vec();
    for cp in 0x0020u32..=0xFFFDu32 {
        if let Some(c) = char::from_u32(cp) {
            codepoints.push(c);
        }
    }
    codepoints.sort();
    codepoints.dedup();
    let used_codepoints = &codepoints[..];
    let mut advance_by_codepoint: BTreeMap<u32, u16> = BTreeMap::new();
    let mut codepoint_to_glyph: BTreeMap<u32, u16> = BTreeMap::new();
    let mut glyph_widths: BTreeMap<u16, u16> = BTreeMap::new();
    let mut sum: u64 = 0;
    let mut count: u64 = 0;
    for &ch in used_codepoints {
        let cp = ch as u32;
        if let Some(gid) = face.glyph_index(ch) {
            if let Some(w) = face.glyph_hor_advance(gid) {
                advance_by_codepoint.insert(cp, w);
                codepoint_to_glyph.insert(cp, gid.0);
                glyph_widths.insert(gid.0, w);
                sum += w as u64;
                count += 1;
            }
        }
    }
    let fallback_advance = if count > 0 {
        (sum / count) as u16
    } else {
        units_per_em / 2
    };

    // Pull ascent/descent from the font so PDF metadata is at least
    // not nonsense.
    let ascent = face.ascender();
    let descent = face.descender();

    // Hand printpdf the same data so its TextItem::Text emission
    // can do codepoint -> glyph lookups without the text_layout
    // feature.
    let parsed = printpdf::ParsedFont::with_glyph_data(
        bytes,
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
