//! Font metrics cache for the renderer.
//!
//! We use printpdf 0.9's PDF built-ins (Helvetica, Times, Courier and
//! their variants) for actual PDF embedding — no font file I/O needed
//! at render time. For *measurement* (line wrapping, page layout) we
//! parse the same embedded TTF bytes with `ttf-parser` directly so we
//! get real glyph advance widths.
//!
//! Enabling printpdf's `text_layout` feature would give us the same
//! metrics via [`printpdf::ParsedFont`], but that feature drags in
//! the entire azul-layout stack. Going through `ttf-parser` keeps the
//! dep tree slim — and `ttf-parser` is already a direct dependency.

use printpdf::BuiltinFont;
use ttf_parser::Face;

use super::ir::RunFlags;

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
        let mut sum: u64 = 0;
        let mut count: u64 = 0;

        for (i, slot) in widths.iter_mut().enumerate() {
            let ch = char::from_u32(i as u32).unwrap_or('\0');
            let w = face
                .glyph_index(ch)
                .and_then(|gid| face.glyph_hor_advance(gid))
                .unwrap_or(0);
            *slot = w;
            if w > 0 {
                sum += w as u64;
                count += 1;
            }
        }

        let fallback_width = if count > 0 {
            (sum / count) as u16
        } else {
            // Helvetica-ish default if no glyphs are mappable.
            500
        };

        VariantMetrics {
            units_per_em,
            unscaled_widths: widths,
            fallback_width,
        }
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

/// Cache of [`VariantMetrics`] for every variant the renderer can pick.
///
/// Built once per render call. The 8 variants combined are a few KiB.
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
