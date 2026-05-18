//! The math font: STIX Two Math (SIL OFL), parsed via `ttf-parser`.
//!
//! Everything here works in *font design units* on the original
//! (un-subset) font. The layout engine converts to points via
//! [`MathFont::scale`]. Glyphs are addressed by their original glyph
//! id; the PDF emit step remaps them through the subsetter.

use ttf_parser::{Face, GlyphId};

/// STIX Two Math, embedded once. ~820 KB; only pulled into the PDF
/// when a document actually contains math (see `emit`).
pub static MATH_FONT_BYTES: &[u8] =
    include_bytes!("../../../../assets/fonts/STIXTwoMath.otf");

/// A glyph's design-space metrics (font units).
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    /// Horizontal advance.
    pub advance: f32,
    /// Italic correction (extra space to add after the glyph when it
    /// is followed by an upright element, e.g. before a superscript).
    pub italic: f32,
    /// Tight bounding box, baseline-relative.
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

impl Glyph {
    pub fn height(&self) -> f32 {
        self.y_max.max(0.0)
    }
    pub fn depth(&self) -> f32 {
        (-self.y_min).max(0.0)
    }
}

/// One piece of an extensible-glyph assembly (e.g. a tall brace built
/// from top / middle-extender / bottom segments).
#[derive(Debug, Clone, Copy)]
pub struct AssemblyPart {
    pub gid: u16,
    pub full_advance: f32,
    pub extender: bool,
}

/// How a stretchy glyph (delimiter, radical, brace) was realised.
#[derive(Debug, Clone)]
pub enum Stretch {
    /// A single (possibly larger) variant glyph.
    Single(u16),
    /// A vertical stack assembled from `parts` (top → bottom order),
    /// repeating extenders as needed. `overlap` is the minimum
    /// connector overlap between adjacent parts.
    Assembly { parts: Vec<AssemblyPart>, overlap: f32 },
}

/// The subset of the OpenType MATH constant table the engine uses,
/// captured as plain font-unit values so no table borrow outlives
/// construction.
#[derive(Debug, Clone, Copy, Default)]
pub struct MathConstants {
    pub script_percent: f32,
    pub script_script_percent: f32,
    pub axis_height: f32,
    pub accent_base_height: f32,
    pub display_operator_min_height: f32,

    pub subscript_shift_down: f32,
    pub subscript_top_max: f32,
    pub subscript_baseline_drop_min: f32,
    pub superscript_shift_up: f32,
    pub superscript_shift_up_cramped: f32,
    pub superscript_bottom_min: f32,
    pub superscript_baseline_drop_max: f32,
    pub sub_superscript_gap_min: f32,
    pub superscript_bottom_max_with_subscript: f32,
    pub space_after_script: f32,

    pub upper_limit_gap_min: f32,
    pub upper_limit_baseline_rise_min: f32,
    pub lower_limit_gap_min: f32,
    pub lower_limit_baseline_drop_min: f32,

    pub stack_top_shift_up: f32,
    pub stack_top_display_shift_up: f32,
    pub stack_bottom_shift_down: f32,
    pub stack_bottom_display_shift_down: f32,
    pub stack_gap_min: f32,
    pub stack_display_gap_min: f32,

    pub fraction_num_shift_up: f32,
    pub fraction_num_display_shift_up: f32,
    pub fraction_denom_shift_down: f32,
    pub fraction_denom_display_shift_down: f32,
    pub fraction_num_gap_min: f32,
    pub fraction_num_display_gap_min: f32,
    pub fraction_rule_thickness: f32,
    pub fraction_denom_gap_min: f32,
    pub fraction_denom_display_gap_min: f32,

    pub overbar_vertical_gap: f32,
    pub overbar_rule_thickness: f32,
    pub overbar_extra_ascender: f32,
    pub underbar_vertical_gap: f32,
    pub underbar_rule_thickness: f32,
    pub underbar_extra_descender: f32,

    pub radical_vertical_gap: f32,
    pub radical_display_vertical_gap: f32,
    pub radical_rule_thickness: f32,
    pub radical_extra_ascender: f32,
    pub radical_kern_before_degree: f32,
    pub radical_kern_after_degree: f32,
    pub radical_degree_bottom_raise_percent: f32,
}

pub struct MathFont {
    face: Face<'static>,
    pub upem: f32,
    pub c: MathConstants,
}

impl MathFont {
    pub fn new() -> Option<MathFont> {
        let face = Face::parse(MATH_FONT_BYTES, 0).ok()?;
        let math = face.tables().math?;
        let k = math.constants?;
        let v = |m: ttf_parser::math::MathValue| m.value as f32;
        let c = MathConstants {
            script_percent: k.script_percent_scale_down() as f32 / 100.0,
            script_script_percent: k.script_script_percent_scale_down() as f32 / 100.0,
            axis_height: v(k.axis_height()),
            accent_base_height: v(k.accent_base_height()),
            display_operator_min_height: k.display_operator_min_height() as f32,
            subscript_shift_down: v(k.subscript_shift_down()),
            subscript_top_max: v(k.subscript_top_max()),
            subscript_baseline_drop_min: v(k.subscript_baseline_drop_min()),
            superscript_shift_up: v(k.superscript_shift_up()),
            superscript_shift_up_cramped: v(k.superscript_shift_up_cramped()),
            superscript_bottom_min: v(k.superscript_bottom_min()),
            superscript_baseline_drop_max: v(k.superscript_baseline_drop_max()),
            sub_superscript_gap_min: v(k.sub_superscript_gap_min()),
            superscript_bottom_max_with_subscript: v(
                k.superscript_bottom_max_with_subscript(),
            ),
            space_after_script: v(k.space_after_script()),
            upper_limit_gap_min: v(k.upper_limit_gap_min()),
            upper_limit_baseline_rise_min: v(k.upper_limit_baseline_rise_min()),
            lower_limit_gap_min: v(k.lower_limit_gap_min()),
            lower_limit_baseline_drop_min: v(k.lower_limit_baseline_drop_min()),
            stack_top_shift_up: v(k.stack_top_shift_up()),
            stack_top_display_shift_up: v(k.stack_top_display_style_shift_up()),
            stack_bottom_shift_down: v(k.stack_bottom_shift_down()),
            stack_bottom_display_shift_down: v(k.stack_bottom_display_style_shift_down()),
            stack_gap_min: v(k.stack_gap_min()),
            stack_display_gap_min: v(k.stack_display_style_gap_min()),
            fraction_num_shift_up: v(k.fraction_numerator_shift_up()),
            fraction_num_display_shift_up: v(k.fraction_numerator_display_style_shift_up()),
            fraction_denom_shift_down: v(k.fraction_denominator_shift_down()),
            fraction_denom_display_shift_down: v(
                k.fraction_denominator_display_style_shift_down(),
            ),
            fraction_num_gap_min: v(k.fraction_numerator_gap_min()),
            fraction_num_display_gap_min: v(k.fraction_num_display_style_gap_min()),
            fraction_rule_thickness: v(k.fraction_rule_thickness()),
            fraction_denom_gap_min: v(k.fraction_denominator_gap_min()),
            fraction_denom_display_gap_min: v(k.fraction_denom_display_style_gap_min()),
            overbar_vertical_gap: v(k.overbar_vertical_gap()),
            overbar_rule_thickness: v(k.overbar_rule_thickness()),
            overbar_extra_ascender: v(k.overbar_extra_ascender()),
            underbar_vertical_gap: v(k.underbar_vertical_gap()),
            underbar_rule_thickness: v(k.underbar_rule_thickness()),
            underbar_extra_descender: v(k.underbar_extra_descender()),
            radical_vertical_gap: v(k.radical_vertical_gap()),
            radical_display_vertical_gap: v(k.radical_display_style_vertical_gap()),
            radical_rule_thickness: v(k.radical_rule_thickness()),
            radical_extra_ascender: v(k.radical_extra_ascender()),
            radical_kern_before_degree: v(k.radical_kern_before_degree()),
            radical_kern_after_degree: v(k.radical_kern_after_degree()),
            radical_degree_bottom_raise_percent: k.radical_degree_bottom_raise_percent()
                as f32
                / 100.0,
        };
        Some(MathFont {
            upem: face.units_per_em() as f32,
            face,
            c,
        })
    }

    /// Font units → points at `size_pt`.
    pub fn scale(&self, units: f32, size_pt: f32) -> f32 {
        units * size_pt / self.upem
    }

    pub fn glyph_id(&self, ch: char) -> Option<u16> {
        self.face.glyph_index(ch).map(|g| g.0)
    }

    pub fn glyph(&self, gid: u16) -> Glyph {
        let g = GlyphId(gid);
        let advance = self.face.glyph_hor_advance(g).unwrap_or(0) as f32;
        let bb = self.face.glyph_bounding_box(g);
        let (x_min, y_min, x_max, y_max) = match bb {
            Some(r) => (
                r.x_min as f32,
                r.y_min as f32,
                r.x_max as f32,
                r.y_max as f32,
            ),
            None => (0.0, 0.0, advance, 0.0),
        };
        Glyph {
            advance,
            italic: self.italic_correction(gid),
            x_min,
            y_min,
            x_max,
            y_max,
        }
    }

    pub fn italic_correction(&self, gid: u16) -> f32 {
        self.face
            .tables()
            .math
            .and_then(|m| m.glyph_info)
            .and_then(|gi| gi.italic_corrections)
            .and_then(|ic| ic.get(GlyphId(gid)))
            .map(|m| m.value as f32)
            .unwrap_or(0.0)
    }

    /// Horizontal position (font units) at which a math accent should
    /// be centered over `gid`; falls back to the glyph's mid-advance.
    pub fn top_accent(&self, gid: u16) -> f32 {
        self.face
            .tables()
            .math
            .and_then(|m| m.glyph_info)
            .and_then(|gi| gi.top_accent_attachments)
            .and_then(|ta| ta.get(GlyphId(gid)))
            .map(|m| m.value as f32)
            .unwrap_or_else(|| self.glyph(gid).advance / 2.0)
    }

    /// Glyph outline as flattened contours in font units (y up,
    /// origin at the glyph's baseline). Béziers are subdivided into
    /// line segments so the renderer can fill them with the proven
    /// polygon path — math is drawn as vector graphics, never as
    /// selectable text, so it behaves like a figure in every viewer.
    pub fn outline(&self, gid: u16) -> Vec<Vec<(f32, f32)>> {
        let mut b = Outliner {
            contours: Vec::new(),
            cur: Vec::new(),
            last: (0.0, 0.0),
        };
        self.face.outline_glyph(GlyphId(gid), &mut b);
        b.flush();
        b.contours
    }

    /// Choose a vertical realisation of `base` at least `target` font
    /// units tall (height + depth). Tries prepared variants, then an
    /// assembly, else returns the base glyph unchanged.
    pub fn stretch_vertical(&self, base: u16, target: f32) -> Stretch {
        let Some(variants) = self.face.tables().math.and_then(|m| m.variants) else {
            return Stretch::Single(base);
        };
        if let Some(con) = variants.vertical_constructions.get(GlyphId(base)) {
            for var in con.variants {
                if var.advance_measurement as f32 >= target {
                    return Stretch::Single(var.variant_glyph.0);
                }
            }
            if let Some(asm) = con.assembly {
                let parts: Vec<AssemblyPart> = asm
                    .parts
                    .into_iter()
                    .map(|p| AssemblyPart {
                        gid: p.glyph_id.0,
                        full_advance: p.full_advance as f32,
                        extender: p.part_flags.extender(),
                    })
                    .collect();
                if !parts.is_empty() {
                    return Stretch::Assembly {
                        parts,
                        overlap: variants.min_connector_overlap as f32,
                    };
                }
            }
            // Largest available variant if nothing reached `target`.
            if let Some(last) = con.variants.last() {
                return Stretch::Single(last.variant_glyph.0);
            }
        }
        Stretch::Single(base)
    }

    /// Smallest horizontal variant of `base` at least `target` font
    /// units wide (for stretchy accents like `\widehat`, `\overline`
    /// arrows). Returns `base` unchanged when the font has no wider
    /// variant.
    pub fn widen(&self, base: u16, target: f32) -> u16 {
        let Some(variants) = self.face.tables().math.and_then(|m| m.variants) else {
            return base;
        };
        let Some(con) = variants.horizontal_constructions.get(GlyphId(base)) else {
            return base;
        };
        for var in con.variants {
            if var.advance_measurement as f32 >= target {
                return var.variant_glyph.0;
            }
        }
        con.variants.last().map(|v| v.variant_glyph.0).unwrap_or(base)
    }
}

/// Flattens a glyph outline into polylines. Quadratic / cubic
/// Béziers are subdivided at a fixed step — at print resolution the
/// segments are visually indistinguishable from true curves.
struct Outliner {
    contours: Vec<Vec<(f32, f32)>>,
    cur: Vec<(f32, f32)>,
    last: (f32, f32),
}

impl Outliner {
    fn flush(&mut self) {
        if self.cur.len() >= 2 {
            self.contours.push(std::mem::take(&mut self.cur));
        } else {
            self.cur.clear();
        }
    }
}

const BEZIER_STEPS: usize = 12;

impl ttf_parser::OutlineBuilder for Outliner {
    fn move_to(&mut self, x: f32, y: f32) {
        self.flush();
        self.last = (x, y);
        self.cur.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.last = (x, y);
        self.cur.push((x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x0, y0) = self.last;
        for i in 1..=BEZIER_STEPS {
            let t = i as f32 / BEZIER_STEPS as f32;
            let u = 1.0 - t;
            let px = u * u * x0 + 2.0 * u * t * x1 + t * t * x;
            let py = u * u * y0 + 2.0 * u * t * y1 + t * t * y;
            self.cur.push((px, py));
        }
        self.last = (x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x0, y0) = self.last;
        for i in 1..=BEZIER_STEPS {
            let t = i as f32 / BEZIER_STEPS as f32;
            let u = 1.0 - t;
            let px = u * u * u * x0
                + 3.0 * u * u * t * x1
                + 3.0 * u * t * t * x2
                + t * t * t * x;
            let py = u * u * u * y0
                + 3.0 * u * u * t * y1
                + 3.0 * u * t * t * y2
                + t * t * t * y;
            self.cur.push((px, py));
        }
        self.last = (x, y);
    }
    fn close(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bundled_font_and_constants() {
        let f = MathFont::new().expect("STIX Two Math must parse");
        assert_eq!(f.upem, 1000.0);
        // Sanity: STIX axis height is ~258 units; rule thickness > 0.
        assert!(f.c.axis_height > 100.0 && f.c.axis_height < 400.0);
        assert!(f.c.fraction_rule_thickness > 0.0);
        assert!(f.c.script_percent > 0.5 && f.c.script_percent < 1.0);
    }

    #[test]
    fn integral_has_a_taller_vertical_variant() {
        let f = MathFont::new().unwrap();
        let int = f.glyph_id('\u{222B}').expect("∫ in cmap");
        let g = f.glyph(int);
        // Ask for something far taller than the base glyph.
        let target = (g.height() + g.depth()) * 3.0;
        match f.stretch_vertical(int, target) {
            Stretch::Single(v) => assert_ne!(v, int, "expected a larger variant"),
            Stretch::Assembly { parts, .. } => assert!(!parts.is_empty()),
        }
    }
}
