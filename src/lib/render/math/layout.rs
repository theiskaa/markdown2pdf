//! TeXbook Appendix G layout: atom tree → positioned glyphs + rules.
//!
//! A [`Frag`] is a laid-out fragment in points, baseline at `y = 0`,
//! `+y` up, `x` growing right. Styles (Display/Text/Script/
//! ScriptScript, each with a cramped form) drive size and the
//! OpenType MATH constants exactly as TeX does.

use super::font::{MathFont, MathTextFont, Stretch};
use super::parse::Node;
use super::symbols::Class;
use std::cell::RefCell;
use std::collections::HashSet;

/// Which face a placed glyph's `gid` belongs to. Almost everything is
/// [`GlyphFont::Math`] (STIX Two Math); `\text{…}` characters the math
/// font lacks are outlined from the body / fallback text chain and
/// carry the index of the face that covered them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphFont {
    Math,
    /// Index into the text fallback chain passed to `typeset`.
    Text(u8),
}

/// Most text fallback faces a chain may hold — bounded by the `u8`
/// index inside [`GlyphFont::Text`]. Both the chain builder
/// (`ensure_math`) and the per-char lookup (`text_fallback`) cap on
/// this same constant.
pub const MAX_TEXT_FONTS: usize = u8::MAX as usize + 1;

#[derive(Debug, Clone, Copy)]
pub struct PlacedGlyph {
    pub gid: u16,
    pub font: GlyphFont,
    pub x: f32,
    /// Baseline offset, +up.
    pub y: f32,
    pub size: f32,
}

/// A filled rectangle (fraction bar, radical vinculum, overline).
#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub x: f32,
    /// Top edge, baseline-relative, +up.
    pub y_top: f32,
    pub w: f32,
    pub thickness: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Frag {
    pub w: f32,
    pub asc: f32,
    pub desc: f32,
    pub glyphs: Vec<PlacedGlyph>,
    pub rules: Vec<Rule>,
    /// Trailing italic correction (pt) — used when attaching scripts.
    pub italic: f32,
}

impl Frag {
    fn empty() -> Self {
        Frag::default()
    }

    fn shift(mut self, dx: f32, dy: f32) -> Self {
        for g in &mut self.glyphs {
            g.x += dx;
            g.y += dy;
        }
        for r in &mut self.rules {
            r.x += dx;
            r.y_top += dy;
        }
        self
    }

    fn absorb(&mut self, other: Frag, at_x: f32) {
        for mut g in other.glyphs {
            g.x += at_x;
            self.glyphs.push(g);
        }
        for mut r in other.rules {
            r.x += at_x;
            self.rules.push(r);
        }
        self.asc = self.asc.max(other.asc);
        self.desc = self.desc.max(other.desc);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Display,
    DisplayCramped,
    Text,
    TextCramped,
    Script,
    ScriptCramped,
    ScriptScript,
    ScriptScriptCramped,
}

use Style::*;

impl Style {
    fn cramped(self) -> Style {
        match self {
            Display => DisplayCramped,
            Text => TextCramped,
            Script => ScriptCramped,
            ScriptScript => ScriptScriptCramped,
            c => c,
        }
    }
    fn is_cramped(self) -> bool {
        matches!(
            self,
            DisplayCramped | TextCramped | ScriptCramped | ScriptScriptCramped
        )
    }
    fn is_display(self) -> bool {
        matches!(self, Display | DisplayCramped)
    }
    /// Style for superscripts of a nucleus in `self`.
    fn sup(self) -> Style {
        match self {
            Display | Text => Script,
            DisplayCramped | TextCramped => ScriptCramped,
            Script | ScriptScript => ScriptScript,
            ScriptCramped | ScriptScriptCramped => ScriptScriptCramped,
        }
    }
    /// Subscripts are always cramped.
    fn sub(self) -> Style {
        self.sup().cramped()
    }
    fn num(self) -> Style {
        match self {
            Display => Text,
            DisplayCramped => TextCramped,
            Text => Script,
            TextCramped => ScriptCramped,
            s => s.sup(),
        }
    }
    fn den(self) -> Style {
        self.num().cramped()
    }
}

pub struct Ctx<'f> {
    pub font: &'f MathFont,
    /// Body-then-fallback text faces consulted for characters the
    /// math font lacks (`\text{…}` and bare symbols).
    pub text_fonts: &'f [MathTextFont<'f>],
    pub base_pt: f32,
    /// Characters already warned about (covered by no font). Owned by
    /// the caller and shared across every typeset of a render so each
    /// distinct char warns once per document, not once per formula.
    warned: &'f RefCell<HashSet<char>>,
}

impl<'f> Ctx<'f> {
    pub fn new(
        font: &'f MathFont,
        text_fonts: &'f [MathTextFont<'f>],
        warned: &'f RefCell<HashSet<char>>,
        base_pt: f32,
    ) -> Self {
        Ctx {
            font,
            text_fonts,
            base_pt,
            warned,
        }
    }

    fn size(&self, st: Style) -> f32 {
        match st {
            Display | DisplayCramped | Text | TextCramped => self.base_pt,
            Script | ScriptCramped => self.base_pt * self.font.c.script_percent,
            ScriptScript | ScriptScriptCramped => {
                self.base_pt * self.font.c.script_script_percent
            }
        }
    }

    /// Lay out a whole list with inter-atom spacing.
    pub fn list(&self, nodes: &[Node], st: Style) -> Frag {
        // Pre-compute per-atom class with TeX's Bin → Ord fixups.
        let classes = reclassify(nodes);
        let mut out = Frag::empty();
        let mut x = 0.0f32;
        let mut prev: Option<Class> = None;
        for (i, n) in nodes.iter().enumerate() {
            let (f, cls) = self.node(n, st, classes[i]);
            if let Some(p) = prev {
                x += self.spacing(p, cls, st);
            }
            // `node()` already laid the fragment out — reuse its
            // metrics. (Re-deriving the width by calling `node()` a
            // second time made nested layout O(2^depth).)
            let w = f.w;
            let asc = f.asc;
            let desc = f.desc;
            out.italic = f.italic;
            out.absorb(f, x);
            x += w;
            out.asc = out.asc.max(asc);
            out.desc = out.desc.max(desc);
            prev = Some(cls);
        }
        out.w = x;
        out
    }

    fn spacing(&self, l: Class, r: Class, st: Style) -> f32 {
        let mu = spacing_mu(l, r);
        let allow_med_thick = !matches!(
            st,
            Script | ScriptCramped | ScriptScript | ScriptScriptCramped
        );
        let n = match mu {
            0 => 0.0,
            1 => 3.0,
            2 if allow_med_thick => 4.0,
            3 if allow_med_thick => 5.0,
            _ => 0.0,
        };
        // 1 mu = 1/18 em; em = current size.
        n / 18.0 * self.size(st)
    }

    fn node(&self, n: &Node, st: Style, cls: Class) -> (Frag, Class) {
        match n {
            Node::Space(em) => {
                let mut f = Frag::empty();
                f.w = em * self.size(st);
                (f, Class::Ord)
            }
            Node::Symbol { ch, class } => (self.glyph_frag(*ch, st), reclass_one(*class, cls)),
            Node::Text(t) => (self.text_frag(t, st), Class::Ord),
            Node::OpName { text, limits } => {
                let f = self.text_frag(text, st);
                // An operator name is an Op atom; `limits` consumed by
                // an enclosing Scripts node.
                let _ = limits;
                (f, Class::Op)
            }
            Node::BigOp { ch, .. } => (self.bigop_frag(*ch, st), Class::Op),
            Node::Group(inner) => (self.list(inner, st), Class::Ord),
            Node::Frac { num, den, bar } => (self.frac(num, den, *bar, st), Class::Inner),
            Node::Sqrt { index, body } => (self.sqrt(index.as_deref(), body, st), Class::Ord),
            Node::Scripts { base, sup, sub } => {
                (self.scripts(base, sup.as_deref(), sub.as_deref(), st), cls)
            }
            Node::Delimited { left, right, body } => {
                (self.delimited(*left, *right, body, st), Class::Inner)
            }
            Node::SizedDelim { ch, class, level } => {
                (self.sized_delim(*ch, *level, st), *class)
            }
            Node::Accent {
                mark,
                stretchy,
                body,
            } => (self.accent(*mark, *stretchy, body, st), Class::Ord),
            Node::OverUnder {
                body,
                over,
                under,
                rule,
            } => (self.over_under(body, *over, *under, *rule, st), Class::Ord),
            Node::Array {
                rows,
                left,
                right,
                align_left,
            } => (self.array(rows, *left, *right, *align_left, st), Class::Inner),
        }
    }

    /// First face in the text fallback chain that covers `ch` with a
    /// *drawable* glyph, with its glyph id. Whitespace-like chars
    /// accept any covering face: an empty outline with a real advance
    /// IS their correct rendering (NBSP & friends, Braille blank).
    /// Otherwise a face whose cmap maps `ch` to an ink-less glyph is
    /// skipped so a later face — or the visible notdef box — handles
    /// it instead of a silent blank: the emit side drops empty
    /// outlines (`math_glyph_xobject` bails on `segs.is_empty()`), so
    /// this probe is the exact "will it draw" predicate.
    fn text_fallback(&self, ch: char) -> Option<(u8, u16)> {
        let blank_ok = ch.is_whitespace() || ch == '\u{2800}';
        self.text_fonts
            .iter()
            .take(MAX_TEXT_FONTS)
            .enumerate()
            .find_map(|(i, tf)| {
                let g = tf.glyph_id(ch)?;
                (blank_ok || !tf.outline(g).is_empty()).then_some((i as u8, g))
            })
    }

    /// Append a hollow notdef box at `x` for a character no font
    /// covers, warn once per distinct char, and return the advance.
    /// A visible box beats a silent blank: the reader can see that
    /// something was there (issue #115).
    fn push_notdef(&self, f: &mut Frag, ch: char, x: f32, size: f32) -> f32 {
        if self.warned.borrow_mut().insert(ch) {
            log::warn!(
                "math: no font covers {:?} (U+{:04X}); rendering a placeholder box",
                ch,
                ch as u32
            );
        }
        let w = 0.5 * size;
        let h = 0.62 * size;
        let t = 0.045 * size;
        // Hollow rectangle built from four filled rules.
        f.rules.push(Rule { x, y_top: h, w, thickness: t });
        f.rules.push(Rule { x, y_top: t, w, thickness: t });
        f.rules.push(Rule { x, y_top: h, w: t, thickness: h });
        f.rules.push(Rule {
            x: x + w - t,
            y_top: h,
            w: t,
            thickness: h,
        });
        f.asc = f.asc.max(h);
        0.6 * size
    }

    /// Place one character at `x`, resolving math font → text
    /// fallback → notdef box, and return the advance consumed.
    /// Default-ignorable characters (ZWNJ/ZWJ/ZWSP, variation
    /// selectors, bidi marks, …) are dropped with zero advance —
    /// invisible is their correct rendering. The single shared
    /// placement path for `\text{…}` runs and bare fallback symbols.
    fn place_char(&self, f: &mut Frag, ch: char, x: f32, size: f32) -> f32 {
        if is_default_ignorable(ch) {
            return 0.0;
        }
        if let Some(g) = self.font.glyph_id(ch) {
            let m = self.font.glyph(g);
            f.glyphs.push(PlacedGlyph {
                gid: g,
                font: GlyphFont::Math,
                x,
                y: 0.0,
                size,
            });
            f.asc = f.asc.max(self.font.scale(m.height(), size));
            f.desc = f.desc.max(self.font.scale(m.depth(), size));
            self.font.scale(m.advance, size)
        } else if let Some((i, g)) = self.text_fallback(ch) {
            // `\text{…}` is body text; characters outside the math
            // font's coverage (CJK, Arabic, Devanagari, …) come from
            // the document's body / fallback faces.
            let tf = &self.text_fonts[i as usize];
            let m = tf.glyph(g);
            f.glyphs.push(PlacedGlyph {
                gid: g,
                font: GlyphFont::Text(i),
                x,
                y: 0.0,
                size,
            });
            f.asc = f.asc.max(tf.scale(m.height(), size));
            f.desc = f.desc.max(tf.scale(m.depth(), size));
            tf.scale(m.advance, size)
        } else {
            self.push_notdef(f, ch, x, size)
        }
    }

    fn glyph_frag(&self, ch: char, st: Style) -> Frag {
        let size = self.size(st);
        let Some(gid) = self.font.glyph_id(ch) else {
            // Not in the math font: try the body / fallback text
            // faces, else a visible placeholder — never a silent
            // blank.
            let mut f = Frag::empty();
            f.w = self.place_char(&mut f, ch, 0.0, size);
            return f;
        };
        let g = self.font.glyph(gid);
        let s = |v: f32| self.font.scale(v, size);
        Frag {
            w: s(g.advance),
            asc: s(g.height()),
            desc: s(g.depth()),
            glyphs: vec![PlacedGlyph {
                gid,
                font: GlyphFont::Math,
                x: 0.0,
                y: 0.0,
                size,
            }],
            rules: vec![],
            italic: s(g.italic),
        }
    }

    fn text_frag(&self, t: &str, st: Style) -> Frag {
        let size = self.size(st);
        let mut f = Frag::empty();
        let mut x = 0.0;
        // Reorder to visual order first (RTL runs reversed); the
        // default-ignorable skip happens later inside `place_char` so
        // LRM/RLM can steer the reordering before being dropped.
        let mut chars: Vec<char> = t.chars().collect();
        visual_order(&mut chars);
        for ch in chars {
            if ch == ' ' {
                x += 0.28 * size;
                continue;
            }
            x += self.place_char(&mut f, ch, x, size);
        }
        f.w = x;
        f
    }

    /// A large operator, vertically centred on the math axis. In
    /// display style it grows to at least `display_operator_min_height`.
    fn bigop_frag(&self, ch: char, st: Style) -> Frag {
        let size = self.size(st);
        let Some(base) = self.font.glyph_id(ch) else {
            return Frag::empty();
        };
        let mut gid = base;
        if st.is_display() {
            let target = self.font.c.display_operator_min_height;
            if let Stretch::Single(v) = self.font.stretch_vertical(base, target) {
                gid = v;
            }
        }
        let g = self.font.glyph(gid);
        let axis = self.font.scale(self.font.c.axis_height, size);
        let s = |v: f32| self.font.scale(v, size);
        let h = s(g.height());
        let d = s(g.depth());
        // Centre the glyph's vertical mid-point on the axis.
        let mid = (h - d) / 2.0;
        let dy = axis - mid;
        Frag {
            w: s(g.advance),
            asc: h + dy,
            desc: d - dy,
            glyphs: vec![PlacedGlyph {
                gid,
                font: GlyphFont::Math,
                x: 0.0,
                y: dy,
                size,
            }],
            rules: vec![],
            italic: s(g.italic),
        }
    }

    fn frac(&self, num: &[Node], den: &[Node], bar: bool, st: Style) -> Frag {
        let size = self.size(st);
        let nf = self.list(num, st.num());
        let df = self.list(den, st.den());
        let disp = st.is_display();
        let c = &self.font.c;
        let s = |v: f32| self.font.scale(v, size);
        let rule = if bar { s(c.fraction_rule_thickness) } else { 0.0 };
        let axis = s(c.axis_height);
        let (nu0, de0, gn0, gd0) = match (bar, disp) {
            (true, true) => (
                c.fraction_num_display_shift_up,
                c.fraction_denom_display_shift_down,
                c.fraction_num_display_gap_min,
                c.fraction_denom_display_gap_min,
            ),
            (true, false) => (
                c.fraction_num_shift_up,
                c.fraction_denom_shift_down,
                c.fraction_num_gap_min,
                c.fraction_denom_gap_min,
            ),
            (false, true) => (
                c.stack_top_display_shift_up,
                c.stack_bottom_display_shift_down,
                c.stack_display_gap_min,
                c.stack_display_gap_min,
            ),
            (false, false) => (
                c.stack_top_shift_up,
                c.stack_bottom_shift_down,
                c.stack_gap_min,
                c.stack_gap_min,
            ),
        };
        let mut nu = s(nu0);
        let mut de = s(de0);
        let gn = s(gn0);
        let gd = s(gd0);
        let bar_top = axis + rule / 2.0;
        let bar_bot = axis - rule / 2.0;
        if bar {
            if nu - nf.desc < bar_top + gn {
                nu = bar_top + gn + nf.desc;
            }
            if df.asc - de > bar_bot - gd {
                de = df.asc - bar_bot + gd;
            }
        } else {
            let clearance = (nu - nf.desc) - (df.asc - de);
            if clearance < gn {
                let extra = (gn - clearance) / 2.0;
                nu += extra;
                de += extra;
            }
        }
        let width = nf.w.max(df.w);
        let pad = 0.12 * size;
        let mut out = Frag::empty();
        out.absorb(nf.clone().shift(0.0, nu), pad + (width - nf.w) / 2.0);
        out.absorb(df.clone().shift(0.0, -de), pad + (width - df.w) / 2.0);
        if bar {
            out.rules.push(Rule {
                x: pad - 0.03 * size,
                y_top: bar_top,
                w: width + 0.06 * size,
                thickness: rule,
            });
        }
        out.w = width + 2.0 * pad;
        out.asc = (nu + nf.asc).max(bar_top);
        out.desc = (de + df.desc).max(-bar_bot);
        out
    }

    fn sqrt(&self, index: Option<&[Node]>, body: &[Node], st: Style) -> Frag {
        let size = self.size(st);
        let bf = self.list(body, st.cramped());
        let c = &self.font.c;
        let s = |v: f32| self.font.scale(v, size);
        let rule = s(c.radical_rule_thickness);
        let gap = if st.is_display() {
            s(c.radical_display_vertical_gap)
        } else {
            s(c.radical_vertical_gap)
        };
        let extra = s(c.radical_extra_ascender);
        let need_pt = bf.asc + bf.desc + gap + rule;
        let target_units = need_pt / size * self.font.upem;
        let base = self.font.glyph_id('\u{221A}').unwrap_or(0);
        let mut f = Frag::empty();
        let radical_w;
        let vinculum_y;
        match self.font.stretch_vertical(base, target_units) {
            Stretch::Single(g) => {
                let m = self.font.glyph(g);
                let gh = s(m.height());
                let gd = s(m.depth());
                radical_w = s(m.advance);
                // Surd bottom aligned to the body's deepest point.
                let y = -bf.desc - gd;
                f.glyphs.push(PlacedGlyph {
                    gid: g,
                    font: GlyphFont::Math,
                    x: 0.0,
                    y,
                    size,
                });
                vinculum_y = (y + gh).max(bf.asc + gap);
            }
            Stretch::Assembly { parts, overlap } => {
                radical_w = self.assemble_vertical(
                    &mut f, &parts, overlap, need_pt, -bf.desc, 0.0, size,
                );
                vinculum_y = bf.asc + gap;
            }
        }
        let body_x = radical_w;
        f.absorb(bf.clone(), body_x);
        f.rules.push(Rule {
            x: body_x - 0.02 * size,
            y_top: vinculum_y + rule,
            w: bf.w + 0.06 * size,
            thickness: rule,
        });
        let mut total_w = body_x + bf.w + 0.08 * size;
        let mut left = 0.0;
        if let Some(idx) = index {
            let idf = self.list(idx, Style::ScriptScript);
            let kb = s(c.radical_kern_before_degree);
            let ka = s(c.radical_kern_after_degree);
            let raise =
                (c.radical_degree_bottom_raise_percent * (vinculum_y + rule)).max(0.0);
            f = f.shift(kb + idf.w + ka, 0.0);
            f.absorb(idf.clone().shift(0.0, raise), kb);
            left = kb + idf.w + ka;
            total_w += left;
        }
        f.w = total_w;
        let _ = left;
        f.asc = (vinculum_y + rule + extra).max(bf.asc);
        f.desc = bf.desc;
        f
    }

    fn scripts(
        &self,
        base: &Node,
        sup: Option<&[Node]>,
        sub: Option<&[Node]>,
        st: Style,
    ) -> Frag {
        // Big operators / operator names with limits stack their
        // scripts above and below in display style.
        let limits = match base {
            Node::BigOp { limits, .. } => *limits && st.is_display(),
            Node::OpName { limits, .. } => *limits && st.is_display(),
            _ => false,
        };
        let (bf, _) = self.node(base, st, Class::Ord);
        let size = self.size(st);
        if limits {
            return self.limits(bf, sup, sub, st, size);
        }
        let c = &self.font.c;
        let mut f = bf.clone();
        let mut x = bf.w;
        let italic = bf.italic;
        let mut sup_h = 0.0;
        let mut sub_d = 0.0;

        // Lay sub/superscripts out first, then resolve their shifts —
        // when both are present TeXbook rule 18 couples them so they
        // can't collide (this is the step the OpenType MATH
        // `subSuperscriptGapMin` constant exists for).
        let supf = sup.map(|s| self.list(s, st.sup()));
        let subf = sub.map(|s| self.list(s, st.sub()));

        let mut sup_shift = supf.as_ref().map(|sf| {
            let base = self.font.scale(
                if st.is_cramped() {
                    c.superscript_shift_up_cramped
                } else {
                    c.superscript_shift_up
                },
                size,
            );
            base.max(bf.asc - self.font.scale(c.superscript_baseline_drop_max, size))
                .max(sf.desc + self.font.scale(c.superscript_bottom_min, size))
        });
        let mut sub_shift = subf.as_ref().map(|sf| {
            self.font
                .scale(c.subscript_shift_down, size)
                .max(bf.desc + self.font.scale(c.subscript_baseline_drop_min, size))
                .max(sf.asc - self.font.scale(c.subscript_top_max, size))
        });

        if let (Some(supf), Some(subf), Some(u), Some(v)) =
            (&supf, &subf, sup_shift.as_mut(), sub_shift.as_mut())
        {
            let gap_min = self.font.scale(c.sub_superscript_gap_min, size);
            let gap = (*u - supf.desc) - (subf.asc - *v);
            if gap < gap_min {
                *v += gap_min - gap;
                // Don't let the superscript hang too low: lift the
                // whole pair (preserving the gap) until the sup bottom
                // reaches superscriptBottomMaxWithSubscript.
                let max_bottom =
                    self.font.scale(c.superscript_bottom_max_with_subscript, size);
                let bottom = *u - supf.desc;
                if bottom < max_bottom {
                    let lift = max_bottom - bottom;
                    *u += lift;
                    *v -= lift;
                }
            }
        }

        if let (Some(sf), Some(shift)) = (&supf, sup_shift) {
            f.absorb(sf.clone().shift(0.0, shift), x + italic);
            sup_h = shift + sf.asc;
            x = x.max(x + italic + sf.w);
        }
        if let (Some(sf), Some(shift)) = (&subf, sub_shift) {
            f.absorb(sf.clone().shift(0.0, -shift), bf.w);
            sub_d = shift + sf.desc;
            x = x.max(bf.w + sf.w);
        }
        f.w = x + self.font.scale(c.space_after_script, size);
        f.asc = bf.asc.max(sup_h);
        f.desc = bf.desc.max(sub_d);
        f
    }

    fn limits(
        &self,
        bf: Frag,
        sup: Option<&[Node]>,
        sub: Option<&[Node]>,
        st: Style,
        size: f32,
    ) -> Frag {
        let c = &self.font.c;
        let w_base = bf.w;
        let mut over_h = 0.0;
        let mut under_d = 0.0;
        let supf = sup.map(|s| self.list(s, st.sup()));
        let subf = sub.map(|s| self.list(s, st.sub()));
        let max_w = w_base
            .max(supf.as_ref().map(|f| f.w).unwrap_or(0.0))
            .max(subf.as_ref().map(|f| f.w).unwrap_or(0.0));
        let mut f = Frag::empty();
        f.absorb(bf.clone(), (max_w - w_base) / 2.0);
        if let Some(sf) = supf {
            let gap = self.font.scale(c.upper_limit_gap_min, size);
            let rise = self.font.scale(c.upper_limit_baseline_rise_min, size);
            let dy = bf.asc + gap.max(rise) + sf.desc;
            let w = sf.w;
            f.absorb(sf.shift(0.0, dy), (max_w - w) / 2.0);
            over_h = dy + self.list(sup.unwrap(), st.sup()).asc;
        }
        if let Some(sf) = subf {
            let gap = self.font.scale(c.lower_limit_gap_min, size);
            let drop = self.font.scale(c.lower_limit_baseline_drop_min, size);
            let dy = bf.desc + gap.max(drop) + sf.asc;
            let w = sf.w;
            f.absorb(sf.shift(0.0, -dy), (max_w - w) / 2.0);
            under_d = dy + self.list(sub.unwrap(), st.sub()).desc;
        }
        f.w = max_w;
        f.asc = bf.asc.max(over_h);
        f.desc = bf.desc.max(under_d);
        f
    }

    fn delimited(
        &self,
        left: Option<char>,
        right: Option<char>,
        body: &[Node],
        st: Style,
    ) -> Frag {
        let size = self.size(st);
        let inner = self.list(body, st);
        let axis = self.font.scale(self.font.c.axis_height, size);
        // Delimiter must span twice the larger half-extent from the
        // axis (TeXbook delimiter rule), with a sensible floor.
        let delta = (inner.asc - axis).max(inner.desc + axis);
        let target_pt = (2.0 * delta).max(0.9 * size);
        let target = target_pt / size * self.font.upem;
        let mut f = Frag::empty();
        let mut x = 0.0;
        if let Some(lc) = left {
            x += self.place_delim(&mut f, lc, target, axis, x, size);
        }
        f.absorb(inner.clone(), x);
        x += inner.w;
        if let Some(rc) = right {
            x += self.place_delim(&mut f, rc, target, axis, x, size);
        }
        f.w = x;
        f.asc = f.asc.max(inner.asc);
        f.desc = f.desc.max(inner.desc);
        f
    }

    fn sized_delim(&self, ch: char, level: u8, st: Style) -> Frag {
        let size = self.size(st);
        let factor = 1.0 + 0.5 * level as f32; // 1.5 .. 3.0
        let target = factor * self.font.upem;
        let axis = self.font.scale(self.font.c.axis_height, size);
        let mut f = Frag::empty();
        let w = self.place_delim(&mut f, ch, target, axis, 0.0, size);
        f.w = w;
        f
    }

    /// Place a (possibly grown) delimiter glyph; returns its advance.
    fn place_delim(
        &self,
        f: &mut Frag,
        ch: char,
        target_units: f32,
        axis: f32,
        x: f32,
        size: f32,
    ) -> f32 {
        let Some(base) = self.font.glyph_id(ch) else {
            return 0.0;
        };
        match self.font.stretch_vertical(base, target_units) {
            Stretch::Single(g) => {
                let m = self.font.glyph(g);
                let h = self.font.scale(m.height(), size);
                let d = self.font.scale(m.depth(), size);
                let mid = (h - d) / 2.0;
                let dy = axis - mid;
                f.glyphs.push(PlacedGlyph {
                    gid: g,
                    font: GlyphFont::Math,
                    x,
                    y: dy,
                    size,
                });
                f.asc = f.asc.max(h + dy);
                f.desc = f.desc.max(d - dy);
                self.font.scale(m.advance, size)
            }
            Stretch::Assembly { parts, overlap } => {
                let height = target_units / self.font.upem * size;
                let w = self.assemble_vertical(
                    f,
                    &parts,
                    overlap,
                    height,
                    axis - height / 2.0,
                    x,
                    size,
                );
                f.asc = f.asc.max(axis + height / 2.0);
                f.desc = f.desc.max(height / 2.0 - axis);
                w
            }
        }
    }

    /// Stack assembly `parts` from `bottom_y` upward to span `height`
    /// pt; returns the column advance width.
    fn assemble_vertical(
        &self,
        f: &mut Frag,
        parts: &[super::font::AssemblyPart],
        overlap: f32,
        height: f32,
        bottom_y: f32,
        x: f32,
        size: f32,
    ) -> f32 {
        let ov = self.font.scale(overlap, size);
        // Non-extender fixed length.
        let fixed: f32 = parts
            .iter()
            .filter(|p| !p.extender)
            .map(|p| self.font.scale(p.full_advance, size))
            .sum();
        let n_ext = parts.iter().filter(|p| p.extender).count().max(1) as f32;
        let ext_adv: f32 = parts
            .iter()
            .find(|p| p.extender)
            .map(|p| self.font.scale(p.full_advance, size))
            .unwrap_or(0.0);
        // How many times to repeat extenders so the stack reaches height.
        let need = (height - fixed + ov * (parts.len() as f32)).max(0.0);
        let reps = if ext_adv > ov {
            ((need / (ext_adv - ov)).ceil() as usize / n_ext as usize).max(1)
        } else {
            1
        };
        let mut y = bottom_y;
        let mut adv_w = 0.0f32;
        // MATH spec lists assembly parts bottom-to-top, so iterate in
        // natural order — reversing places asymmetric paren corners
        // upside-down (top glyph at bottom, bottom glyph at top),
        // which made tall `(` / `)` look like `\` / `/`.
        for p in parts.iter() {
            let count = if p.extender { reps } else { 1 };
            for _ in 0..count {
                let m = self.font.glyph(p.gid);
                adv_w = adv_w.max(self.font.scale(m.advance, size));
                f.glyphs.push(PlacedGlyph {
                    gid: p.gid,
                    font: GlyphFont::Math,
                    x,
                    y: y - self.font.scale(m.y_min, size),
                    size,
                });
                y += self.font.scale(p.full_advance, size) - ov;
            }
        }
        adv_w
    }

    fn accent(&self, mark: char, stretchy: bool, body: &[Node], st: Style) -> Frag {
        let size = self.size(st);
        let bf = self.list(body, st.cramped());
        let Some(mut ag) = self.font.glyph_id(mark) else {
            return bf;
        };
        // A stretchy accent (`\widehat`, `\widetilde`, `\vec`) grows a
        // wide horizontal variant to span the whole base; a plain
        // accent is a single glyph centred on the attachment point.
        if stretchy {
            let target = bf.w / size * self.font.upem;
            ag = self.font.widen(ag, target);
        }
        let am = self.font.glyph(ag);
        let acc_w = self.font.scale(am.advance.max(am.x_max - am.x_min), size);
        let acc_h = self.font.scale(am.height(), size);
        let centre = if stretchy {
            bf.w / 2.0
        } else if let Some(Node::Symbol { ch, .. }) = body.first() {
            self.font
                .glyph_id(*ch)
                .map(|g| self.font.scale(self.font.top_accent(g), size))
                .unwrap_or(bf.w / 2.0)
        } else {
            bf.w / 2.0
        };
        let mut f = bf.clone();
        let base_top = bf.asc;
        let clearance = (base_top
            - self.font.scale(self.font.c.accent_base_height, size))
        .max(0.0);
        let acc_y = base_top - clearance + self.font.scale(am.depth(), size);
        f.glyphs.push(PlacedGlyph {
            gid: ag,
            font: GlyphFont::Math,
            x: centre - acc_w / 2.0,
            y: acc_y,
            size,
        });
        f.asc = f.asc.max(acc_y + acc_h);
        f.w = bf.w;
        f
    }

    fn over_under(
        &self,
        body: &[Node],
        over: Option<char>,
        under: Option<char>,
        rule: bool,
        st: Style,
    ) -> Frag {
        let size = self.size(st);
        let bf = self.list(body, st);
        let mut f = bf.clone();
        let c = &self.font.c;
        if over.is_some() {
            if rule {
                let gap = self.font.scale(c.overbar_vertical_gap, size);
                let th = self.font.scale(c.overbar_rule_thickness, size);
                let y = bf.asc + gap;
                f.rules.push(Rule {
                    x: 0.0,
                    y_top: y + th,
                    w: bf.w,
                    thickness: th,
                });
                f.asc = y + th + self.font.scale(c.overbar_extra_ascender, size);
            } else if let Some(ch) = over {
                let g = self.font.glyph_id(ch).unwrap_or(0);
                let m = self.font.glyph(g);
                let y = bf.asc + 0.12 * size;
                f.glyphs.push(PlacedGlyph {
                    gid: g,
                    font: GlyphFont::Math,
                    x: 0.0,
                    y,
                    size,
                });
                f.asc = y + self.font.scale(m.height(), size);
            }
        }
        if under.is_some() {
            if rule {
                let gap = self.font.scale(c.underbar_vertical_gap, size);
                let th = self.font.scale(c.underbar_rule_thickness, size);
                let y = -bf.desc - gap;
                f.rules.push(Rule {
                    x: 0.0,
                    y_top: y,
                    w: bf.w,
                    thickness: th,
                });
                f.desc = bf.desc + gap + th
                    + self.font.scale(c.underbar_extra_descender, size);
            } else if let Some(ch) = under {
                let g = self.font.glyph_id(ch).unwrap_or(0);
                let m = self.font.glyph(g);
                let y = -bf.desc - 0.12 * size - self.font.scale(m.height(), size);
                f.glyphs.push(PlacedGlyph {
                    gid: g,
                    font: GlyphFont::Math,
                    x: 0.0,
                    y,
                    size,
                });
                f.desc = bf.desc + 0.12 * size + self.font.scale(m.height(), size);
            }
        }
        f.w = bf.w;
        f
    }

    fn array(
        &self,
        rows: &[Vec<Vec<Node>>],
        left: Option<char>,
        right: Option<char>,
        align_left: bool,
        st: Style,
    ) -> Frag {
        let size = self.size(st);
        let cell_st = if st.is_display() { Style::Text } else { st };
        let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut col_w = vec![0.0f32; ncols];
        let mut laid: Vec<Vec<Frag>> = Vec::new();
        for r in rows {
            let mut lr = Vec::new();
            for (ci, cell) in r.iter().enumerate() {
                let cf = self.list(cell, cell_st);
                if ci < ncols {
                    col_w[ci] = col_w[ci].max(cf.w);
                }
                lr.push(cf);
            }
            laid.push(lr);
        }
        let row_gap = 0.35 * size;
        let col_gap = 0.9 * size;
        let row_h: Vec<(f32, f32)> = laid
            .iter()
            .map(|r| {
                (
                    r.iter().fold(0.0f32, |m, f| m.max(f.asc)),
                    r.iter().fold(0.0f32, |m, f| m.max(f.desc)),
                )
            })
            .collect();
        let total_h: f32 = row_h.iter().map(|(a, d)| a + d).sum::<f32>()
            + row_gap * (laid.len().saturating_sub(1)) as f32;
        let axis = self.font.scale(self.font.c.axis_height, size);
        let mut y = total_h / 2.0 + axis;
        let mut body = Frag::empty();
        for (ri, r) in laid.iter().enumerate() {
            let (ra, rd) = row_h[ri];
            y -= ra;
            let mut x = 0.0;
            for (ci, cf) in r.iter().enumerate() {
                let cw = col_w.get(ci).copied().unwrap_or(cf.w);
                let off = if align_left { 0.0 } else { (cw - cf.w) / 2.0 };
                body.absorb(cf.clone().shift(0.0, y), x + off);
                x += cw + col_gap;
            }
            y -= rd + row_gap;
        }
        let content_w = if ncols == 0 {
            0.0
        } else {
            col_w.iter().sum::<f32>() + col_gap * (ncols - 1) as f32
        };
        body.w = content_w;
        body.asc = total_h / 2.0 + axis;
        body.desc = total_h / 2.0 - axis;
        if left.is_none() && right.is_none() {
            return body;
        }
        // Wrap in delimiters sized to the array.
        let mut f = Frag::empty();
        let mut x = 0.0;
        let tgt = (body.asc + body.desc) / size * self.font.upem;
        if let Some(lc) = left {
            x += self.place_delim(&mut f, lc, tgt, axis, x, size);
        }
        f.absorb(body.clone(), x);
        x += body.w;
        if let Some(rc) = right {
            x += self.place_delim(&mut f, rc, tgt, axis, x, size);
        }
        f.w = x;
        f.asc = f.asc.max(body.asc);
        f.desc = f.desc.max(body.desc);
        f
    }

}

/// Unicode Default_Ignorable_Code_Point (the common subset): soft
/// hyphen, joiners/non-joiners, zero-width spaces, bidi controls,
/// variation selectors, fillers. These format characters are meant to
/// be invisible; drawing a notdef box for them (because no font maps
/// them) would corrupt e.g. Persian text where ZWNJ is standard
/// orthography. They are dropped at placement time — after
/// [`visual_order`], so LRM/RLM can still steer bidi levels.
fn is_default_ignorable(ch: char) -> bool {
    matches!(ch as u32,
        0x00AD                  // soft hyphen
        | 0x034F                // combining grapheme joiner
        | 0x061C                // Arabic letter mark
        | 0x115F..=0x1160       // Hangul fillers
        | 0x17B4..=0x17B5       // Khmer inherent vowels
        | 0x180B..=0x180E       // Mongolian variation selectors + MVS
        | 0x200B..=0x200F       // ZWSP, ZWNJ, ZWJ, LRM, RLM
        | 0x202A..=0x202E       // bidi embedding controls
        | 0x2060..=0x206F       // word joiner, invisible operators, deprecated controls
        | 0x3164                // Hangul filler
        | 0xFE00..=0xFE0F       // variation selectors
        | 0xFEFF                // zero-width no-break space / BOM
        | 0xFFA0                // halfwidth Hangul filler
        | 0x1BCA0..=0x1BCA3     // shorthand format controls
        | 0x1D173..=0x1D17A     // musical format controls
        | 0xE0000..=0xE0FFF     // tags + variation selector supplement
    )
}

/// Simplified bidi character class for [`visual_order`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum BClass {
    /// Strong right-to-left (Hebrew, Arabic, Syriac, Thaana, NKo, …).
    R,
    /// Digits and numeric separators — travel with an RTL run but
    /// keep internal left-to-right order.
    Num,
    /// Strong left-to-right (everything else with a direction).
    L,
    /// Neutral: whitespace, punctuation, symbols.
    N,
}

fn bclass(c: char) -> BClass {
    match c as u32 {
        // Digits first: U+0660–0669 sit inside the Arabic block and
        // must not classify as R. U+066B/066C are the Arabic decimal
        // and thousands separators.
        0x30..=0x39 | 0x0660..=0x0669 | 0x06F0..=0x06F9 | 0x066B..=0x066C => BClass::Num,
        // LRM / RLM: strong marks that steer neutral resolution, then
        // vanish at placement as default-ignorables.
        0x200E => BClass::L,
        0x200F => BClass::R,
        // Hebrew … Arabic Extended-A (one contiguous span), Hebrew /
        // Arabic presentation forms.
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF => BClass::R,
        _ if c.is_whitespace() => BClass::N,
        _ => {
            if c.is_alphanumeric() {
                BClass::L
            } else {
                BClass::N
            }
        }
    }
}

/// Reorder `chars` from logical to visual order: UAX #9 restricted to
/// implicit levels 0 (LTR base) / 1 (RTL) / 2 (numbers inside RTL),
/// with bracket mirroring for reversed runs. This fixes RTL scripts
/// rendering mirrored; it does NOT shape — Arabic renders in isolated
/// letterforms (parity with the body-text emit path). Other
/// limitations: base direction is always LTR, no explicit
/// embedding/isolate controls, no bracket-pair resolution, and bare
/// RTL symbols outside `\text{…}` stay in logical order.
fn visual_order(chars: &mut [char]) {
    let mut cls: Vec<BClass> = chars.iter().map(|&c| bclass(c)).collect();
    if !cls.contains(&BClass::R) {
        return;
    }
    let n = chars.len();
    // W4-lite: a single separator between two digits joins the number
    // so `12.5` survives the level-2 re-reversal intact.
    for i in 1..n.saturating_sub(1) {
        if matches!(chars[i], '.' | ',' | ':' | '/')
            && cls[i - 1] == BClass::Num
            && cls[i + 1] == BClass::Num
        {
            cls[i] = BClass::Num;
        }
    }
    // Implicit levels for strong chars and numbers.
    let mut lvl = vec![0u8; n];
    let mut prev_strong_r = false;
    for i in 0..n {
        match cls[i] {
            BClass::R => {
                lvl[i] = 1;
                prev_strong_r = true;
            }
            BClass::L => {
                lvl[i] = 0;
                prev_strong_r = false;
            }
            BClass::Num => lvl[i] = if prev_strong_r { 2 } else { 0 },
            BClass::N => {}
        }
    }
    // N1-lite: a neutral run flanked by RTL context (level >= 1) on
    // BOTH sides takes the RTL level; boundary neutrals stay at base.
    let mut i = 0;
    while i < n {
        if cls[i] == BClass::N {
            let j = (i..n).find(|&k| cls[k] != BClass::N).unwrap_or(n);
            if i > 0 && lvl[i - 1] >= 1 && j < n && lvl[j] >= 1 {
                for l in lvl.iter_mut().take(j).skip(i) {
                    *l = 1;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    // L4-lite: mirror bracket glyphs that ended up in reversed runs.
    for i in 0..n {
        if lvl[i] >= 1 {
            chars[i] = match chars[i] {
                '(' => ')',
                ')' => '(',
                '[' => ']',
                ']' => '[',
                '{' => '}',
                '}' => '{',
                '<' => '>',
                '>' => '<',
                '«' => '»',
                '»' => '«',
                c => c,
            };
        }
    }
    // L2: reverse level >= 1 runs, then level == 2 runs — the double
    // reversal keeps digits LTR while travelling with the RTL run.
    reverse_runs(chars, &mut lvl, 1);
    reverse_runs(chars, &mut lvl, 2);
}

/// Reverse every maximal run of `lvl[i] >= min`. The level slice is
/// permuted in lockstep with the chars — the level-2 pass reads the
/// levels at their post-reversal positions.
fn reverse_runs(chars: &mut [char], lvl: &mut [u8], min: u8) {
    let mut i = 0;
    while i < chars.len() {
        if lvl[i] >= min {
            let j = (i..chars.len()).find(|&k| lvl[k] < min).unwrap_or(chars.len());
            chars[i..j].reverse();
            lvl[i..j].reverse();
            i = j;
        } else {
            i += 1;
        }
    }
}

/// TeX inter-atom spacing class table → mu code (0 none, 1 thin,
/// 2 medium, 3 thick). Medium/thick are dropped in script styles by
/// the caller.
fn spacing_mu(l: Class, r: Class) -> u8 {
    use Class::*;
    match (l, r) {
        (Ord, Op) => 1,
        (Ord, Bin) => 2,
        (Ord, Rel) => 3,
        (Ord, Inner) => 1,
        (Op, Ord) => 1,
        (Op, Op) => 1,
        (Op, Rel) => 3,
        (Op, Inner) => 1,
        (Bin, Ord) | (Bin, Op) | (Bin, Open) | (Bin, Inner) => 2,
        (Rel, Ord) | (Rel, Op) | (Rel, Open) | (Rel, Inner) => 3,
        (Close, Op) => 1,
        (Close, Bin) => 2,
        (Close, Rel) => 3,
        (Close, Inner) => 1,
        (Inner, Ord) => 1,
        (Inner, Op) => 1,
        (Inner, Bin) => 2,
        (Inner, Rel) => 3,
        (Inner, Open) => 1,
        (Inner, Close) => 1,
        (Inner, Punct) => 1,
        (Inner, Inner) => 1,
        (Punct, _) => 1,
        _ => 0,
    }
}

/// Per-atom class after TeX's Bin fix-ups (a Bin with no valid left
/// operand, or next to Bin/Op/Rel/Open/Punct, becomes Ord).
fn reclassify(nodes: &[Node]) -> Vec<Class> {
    let mut cls: Vec<Class> = nodes.iter().map(node_class).collect();
    for i in 0..cls.len() {
        if cls[i] == Class::Bin {
            let prev = if i == 0 { None } else { Some(cls[i - 1]) };
            let bad_prev = matches!(
                prev,
                None | Some(Class::Bin)
                    | Some(Class::Op)
                    | Some(Class::Rel)
                    | Some(Class::Open)
                    | Some(Class::Punct)
            );
            let next_bad = matches!(
                cls.get(i + 1),
                Some(Class::Rel) | Some(Class::Close) | Some(Class::Punct) | None
            );
            if bad_prev || next_bad {
                cls[i] = Class::Ord;
            }
        }
    }
    cls
}

fn reclass_one(c: Class, _ctx: Class) -> Class {
    c
}

fn node_class(n: &Node) -> Class {
    match n {
        Node::Symbol { class, .. } => *class,
        Node::SizedDelim { class, .. } => *class,
        Node::BigOp { .. } | Node::OpName { .. } => Class::Op,
        Node::Frac { .. } | Node::Delimited { .. } | Node::Array { .. } => Class::Inner,
        Node::Scripts { base, .. } => node_class(base),
        Node::Space(_) => Class::Ord,
        _ => Class::Ord,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::math::font::MATH_FONT_BYTES;
    use crate::render::math::parse::parse;

    fn lay(src: &str, display: bool) -> Frag {
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let ctx = Ctx::new(&font, &[], &warned, 11.0);
        ctx.list(&parse(src), if display { Display } else { Text })
    }

    #[test]
    fn simple_expression_has_size() {
        let f = lay("a + b", false);
        assert!(f.w > 0.0 && f.asc > 0.0);
        assert!(f.glyphs.len() >= 3);
    }

    #[test]
    fn fraction_emits_a_rule_and_is_tall() {
        let f = lay("\\frac{1}{2}", true);
        assert_eq!(f.rules.len(), 1, "fraction needs a bar");
        let plain = lay("1", true);
        assert!(f.asc + f.desc > plain.asc + plain.desc);
    }

    #[test]
    fn sqrt_emits_rule_and_radical_glyph() {
        let f = lay("\\sqrt{x}", false);
        assert!(!f.rules.is_empty(), "radical vinculum");
        assert!(f.glyphs.len() >= 2, "radical sign + body");
    }

    #[test]
    fn display_integral_grows() {
        let small = lay("\\int", false);
        let big = lay("\\int", true);
        let sh = small.asc + small.desc;
        let bh = big.asc + big.desc;
        assert!(bh > sh, "display ∫ must be taller ({sh} -> {bh})");
    }

    #[test]
    fn scripts_raise_and_lower() {
        let f = lay("x^2_n", false);
        let ys: Vec<f32> = f.glyphs.iter().map(|g| g.y).collect();
        assert!(ys.iter().any(|&y| y > 0.5), "superscript raised");
        assert!(ys.iter().any(|&y| y < -0.5), "subscript lowered");
    }

    #[test]
    fn no_panic_on_torture() {
        for s in [
            "\\frac{\\frac{a}{b}}{\\frac{c}{d}}",
            "\\sqrt[3]{\\frac{x^2}{y_1}}",
            "\\left(\\sum_{i=1}^{n} i\\right)",
            "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
            "x^{y^{z^{w}}}",
            "",
            "{}",
        ] {
            let _ = lay(s, true);
            let _ = lay(s, false);
        }
    }

    /// Regression for issue #115: a `\text{…}` character no font
    /// covers must still occupy width and draw a visible notdef box
    /// instead of silently vanishing.
    #[test]
    fn text_missing_everywhere_renders_placeholder_not_blank() {
        let f = lay("\\text{あ}", false);
        assert!(f.w > 0.0, "placeholder must advance");
        assert!(!f.rules.is_empty(), "notdef box must be drawn");
        assert!(f.glyphs.is_empty(), "no font in the chain covers あ");
        assert!(f.asc > 0.0);
    }

    /// Bare symbols outside `\text{}` take the same fallback path.
    #[test]
    fn bare_symbol_missing_everywhere_renders_placeholder() {
        let f = lay("あ", false);
        assert!(f.w > 0.0);
        assert!(!f.rules.is_empty());
    }

    /// Latin `\text{}` must keep coming from the math font even when
    /// a text fallback chain is present (zero visual regression).
    #[test]
    fn text_latin_stays_in_math_font() {
        let f = lay("\\text{Hello}", false);
        assert_eq!(f.glyphs.len(), 5);
        assert!(f.glyphs.iter().all(|g| g.font == GlyphFont::Math));
    }

    /// End-to-end fallback hit: find a system face covering a char
    /// STIX Two Math lacks, and check `\text{…}` routes it there.
    /// Skips cleanly on hosts with no such font (bare CI containers).
    #[test]
    fn text_falls_back_to_text_font() {
        let candidates: &[(&str, char)] = &[
            ("Arial Unicode MS", 'あ'),
            ("Arial Unicode", 'あ'),
            ("AppleGothic", 'あ'),
            ("Noto Sans CJK JP", 'あ'),
            ("Noto Sans CJK", 'あ'),
        ];
        let mut found = None;
        for &(name, probe) in candidates {
            let Some(path) = crate::fonts::find_system_font(name) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if MathTextFont::from_bytes(&bytes)
                .map(|tf| tf.glyph_id(probe).is_some())
                .unwrap_or(false)
            {
                found = Some((bytes, probe));
                break;
            }
        }
        let Some((bytes, probe)) = found else {
            eprintln!("skipping: no CJK-capable system font found");
            return;
        };
        let chain = vec![MathTextFont::from_bytes(&bytes).unwrap()];
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let ctx = Ctx::new(&font, &chain, &warned, 11.0);
        let f = ctx.list(&parse(&format!("\\text{{x{probe}}}")), Text);
        assert_eq!(f.glyphs.len(), 2);
        assert_eq!(f.glyphs[0].font, GlyphFont::Math, "x stays in STIX");
        assert_eq!(
            f.glyphs[1].font,
            GlyphFont::Text(0),
            "{probe} must come from the fallback face"
        );
        assert!(f.rules.is_empty(), "covered char must not draw a notdef box");
        assert!(f.w > 0.0 && f.w.is_finite() && f.asc > 0.0);
        assert!(f.glyphs[1].x > f.glyphs[0].x, "fallback glyph advances");
    }

    fn vo(s: &str) -> String {
        let mut v: Vec<char> = s.chars().collect();
        visual_order(&mut v);
        v.into_iter().collect()
    }

    #[test]
    fn visual_order_reverses_rtl_runs() {
        // Pure-RTL word: fully reversed.
        assert_eq!(vo("مرحبا"), "ابحرم");
        // Mixed: boundary space stays put, only the RTL run reverses.
        assert_eq!(vo("abc مرحبا"), "abc ابحرم");
        // Two RTL words + internal space reverse as ONE run (visual
        // order of "A B" in Arabic is B-then-A).
        assert_eq!(vo("مرحبا بالعالم"), "ملاعلاب ابحرم");
        // LTR-only input is untouched (fast path).
        assert_eq!(vo("hello world"), "hello world");
    }

    #[test]
    fn visual_order_keeps_digits_ltr_inside_rtl() {
        // Digits travel with the RTL run but read left-to-right.
        assert_eq!(vo("مرحبا 123"), "123 ابحرم");
        // Decimal separator joins the number (W4-lite) and the level
        // slice reversal stays in lockstep — `12.5` must not scatter.
        assert_eq!(vo("ا 12.5 م"), "م 12.5 ا");
    }

    #[test]
    fn visual_order_mirrors_internal_brackets_only() {
        // Boundary parens are neutral at base level: not reversed,
        // not mirrored.
        assert_eq!(vo("(مرحبا)"), "(ابحرم)");
        // Parens inside a reversed run swap so they still face their
        // content.
        assert_eq!(vo("اب(جد)هو"), "وه(دج)با");
    }

    /// ZWNJ (and friends) are format characters: invisible, zero
    /// advance, no notdef box — `\text{a‌b}` must lay out exactly
    /// like `\text{ab}`.
    #[test]
    fn default_ignorables_are_invisible() {
        let with = lay("\\text{a\u{200C}b}", false);
        let without = lay("\\text{ab}", false);
        assert_eq!(with.glyphs.len(), 2);
        assert!(with.rules.is_empty(), "no notdef box for ZWNJ");
        assert!((with.w - without.w).abs() < 1e-6);
        // Bidi marks steer ordering but also vanish.
        let rlm = lay("\\text{a\u{200F}b}", false);
        assert_eq!(rlm.glyphs.len(), 2);
        assert!(rlm.rules.is_empty());
    }

    /// RTL `\text{}` places glyphs in visual order end-to-end (STIX
    /// covers no RTL script, so this needs a system face with Arabic;
    /// skips cleanly on hosts without one).
    #[test]
    fn rtl_text_places_glyphs_in_visual_order() {
        let candidates = ["Arial Unicode MS", "Arial Unicode", "Geneva", "AppleGothic"];
        let mut found = None;
        for name in candidates {
            let Some(path) = crate::fonts::find_system_font(name) else {
                continue;
            };
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if MathTextFont::from_bytes(&bytes)
                .map(|tf| tf.glyph_id('\u{0627}').is_some() && tf.glyph_id('\u{0628}').is_some())
                .unwrap_or(false)
            {
                found = Some(bytes);
                break;
            }
        }
        let Some(bytes) = found else {
            eprintln!("skipping: no Arabic-capable system font found");
            return;
        };
        let chain = vec![MathTextFont::from_bytes(&bytes).unwrap()];
        let alef = chain[0].glyph_id('\u{0627}').unwrap();
        let beh = chain[0].glyph_id('\u{0628}').unwrap();
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let ctx = Ctx::new(&font, &chain, &warned, 11.0);
        // Logical اب ; visually ب must land left of ا.
        let f = ctx.list(&parse("\\text{\u{0627}\u{0628}}"), Text);
        assert_eq!(f.glyphs.len(), 2);
        assert_eq!(f.glyphs[0].gid, beh, "leftmost glyph is the logically-second letter");
        assert_eq!(f.glyphs[1].gid, alef);
        assert!(f.glyphs[0].x < f.glyphs[1].x);
    }

    /// Whitespace-like chars (NBSP) accept a covering face even with
    /// an empty outline — advance-without-ink IS their rendering. A
    /// char the face doesn't cover at all still returns None.
    #[test]
    fn text_fallback_accepts_blank_whitespace_glyphs() {
        let chain = vec![MathTextFont::from_bytes(MATH_FONT_BYTES).unwrap()];
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let ctx = Ctx::new(&font, &chain, &warned, 11.0);
        if chain[0].glyph_id('\u{00A0}').is_some() {
            assert!(ctx.text_fallback('\u{00A0}').is_some(), "NBSP accepted");
        }
        assert!(ctx.text_fallback('あ').is_none(), "uncovered char rejected");
    }

    /// The warned set is caller-owned: two typeset calls sharing one
    /// set warn once per distinct char across both.
    #[test]
    fn missing_glyph_warns_once_per_shared_set() {
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let _ = crate::render::math::typeset(&font, &[], &warned, "\\text{あ}", false, 11.0);
        assert_eq!(warned.borrow().len(), 1);
        assert!(warned.borrow().contains(&'あ'));
        let _ = crate::render::math::typeset(&font, &[], &warned, "\\text{ああ}", true, 11.0);
        assert_eq!(warned.borrow().len(), 1, "no re-warn across typeset calls");
    }

    const CORPUS: &[&str] = &[
        "a + b - c",
        "x^2 + y^2 = z^2",
        "\\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}",
        "\\sqrt[3]{\\frac{x^2+1}{y-2}}",
        "\\int_{0}^{\\infty} e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}",
        "\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}",
        "\\prod_{i=1}^{n} i = n!",
        "e^{i\\pi} + 1 = 0",
        "x^{y^{z^{w}}}",
        "a_{i_{j_{k}}}",
        "\\lim_{x \\to 0} \\frac{\\sin x}{x} = 1",
        "\\left( \\frac{1}{1-x^2} \\right)^{n}",
        "\\left\\{ \\frac{a}{b} \\mid b \\neq 0 \\right\\}",
        "\\hat{x}\\;\\bar{y}\\;\\vec{v}\\;\\tilde{n}\\;\\dot{q}",
        "\\nabla \\times \\mathbf{F} = \\mu_0 \\mathbf{J}",
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        "|x| = \\begin{cases} x & x \\ge 0 \\\\ -x & x < 0 \\end{cases}",
        "\\overline{z} = a - bi",
        "\\binom{n}{k} = \\frac{n!}{k!(n-k)!}",
        "\\alpha\\beta\\gamma \\Gamma\\Delta\\Omega",
        "",
        "{}",
        "x",
        "\\unknownmacro \\frac{}{} \\sqrt[]{}",
        "\\text{Hello こんにちは مرحبا}",
        "\\operatorname{ソート}(x) + \\text{नमस्ते}",
    ];

    /// Pathologically deep nesting — size bottoms out at scriptscript,
    /// so metrics must stay finite with no zero-glyph degeneracy.
    fn deep_inputs() -> Vec<String> {
        vec![
            "\\frac{a}{".repeat(40) + "b" + &"}".repeat(40),
            "x".to_string() + &"^{x".repeat(60) + &"}".repeat(60),
            "\\sqrt{".repeat(40) + "x" + &"}".repeat(40),
            "\\left(".repeat(30) + "z" + &"\\right)".repeat(30),
        ]
    }

    /// Every fragment in both styles must have finite, non-negative
    /// metrics and finite glyph / rule coordinates. This is the load-
    /// bearing regression net: a degenerate glyph or a divide-by-zero
    /// anywhere in layout surfaces here as a NaN/∞.
    #[test]
    fn metrics_and_coordinates_are_always_finite() {
        let font = MathFont::new().unwrap();
        let mut inputs: Vec<String> = CORPUS.iter().map(|s| s.to_string()).collect();
        inputs.extend(deep_inputs());
        for src in &inputs {
            for display in [true, false] {
                let warned = RefCell::new(HashSet::new());
        let ctx = Ctx::new(&font, &[], &warned, 11.0);
                let f = ctx.list(&parse(src), if display { Display } else { Text });
                let tag = format!("{src:?} display={display}");
                assert!(
                    f.w.is_finite() && f.asc.is_finite() && f.desc.is_finite(),
                    "non-finite metrics for {tag}: w={} asc={} desc={}",
                    f.w,
                    f.asc,
                    f.desc
                );
                assert!(f.w >= 0.0 && f.asc >= 0.0 && f.desc >= 0.0, "negative for {tag}");
                for g in &f.glyphs {
                    assert!(
                        g.x.is_finite() && g.y.is_finite() && g.size.is_finite(),
                        "non-finite glyph in {tag}"
                    );
                    assert!(g.size > 0.0, "non-positive glyph size in {tag}");
                }
                for r in &f.rules {
                    assert!(
                        r.x.is_finite()
                            && r.y_top.is_finite()
                            && r.w.is_finite()
                            && r.thickness.is_finite(),
                        "non-finite rule in {tag}"
                    );
                    assert!(r.w >= 0.0 && r.thickness >= 0.0, "negative rule in {tag}");
                }
            }
        }
    }

    /// Same input ⇒ byte-for-byte same layout (positions, sizes,
    /// rules). Guards refactors against silent geometry drift.
    #[test]
    fn layout_is_deterministic() {
        for &src in CORPUS {
            let a = lay(src, true);
            let b = lay(src, true);
            assert_eq!(a.glyphs.len(), b.glyphs.len(), "{src}");
            for (g1, g2) in a.glyphs.iter().zip(&b.glyphs) {
                assert_eq!(
                    (g1.gid, g1.x.to_bits(), g1.y.to_bits(), g1.size.to_bits()),
                    (g2.gid, g2.x.to_bits(), g2.y.to_bits(), g2.size.to_bits()),
                    "non-deterministic glyph for {src}"
                );
            }
            assert_eq!(a.rules.len(), b.rules.len(), "{src}");
        }
    }

    #[test]
    fn style_sizes_are_monotonic() {
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let c = Ctx::new(&font, &[], &warned, 12.0);
        let d = c.size(Display);
        let t = c.size(Text);
        let s = c.size(Script);
        let ss = c.size(ScriptScript);
        assert_eq!(d, t, "display and text are the same size");
        assert!(s < t, "script ({s}) < text ({t})");
        assert!(ss < s, "scriptscript ({ss}) < script ({s})");
        assert!(ss > 0.0);
    }

    #[test]
    fn spacing_table_and_bin_reclassification() {
        use super::super::symbols::Class::*;
        // The canonical TeX inter-atom table (mu codes).
        assert_eq!(spacing_mu(Ord, Op), 1);
        assert_eq!(spacing_mu(Ord, Bin), 2);
        assert_eq!(spacing_mu(Ord, Rel), 3);
        assert_eq!(spacing_mu(Rel, Ord), 3);
        assert_eq!(spacing_mu(Bin, Ord), 2);
        assert_eq!(spacing_mu(Open, Ord), 0);
        assert_eq!(spacing_mu(Ord, Close), 0);
        assert_eq!(spacing_mu(Punct, Ord), 1);

        // A leading binary operator is re-typed Ord (no left operand);
        // a medial one stays Bin.
        let leading = reclassify(&parse("-x"));
        assert_eq!(leading[0], Ord, "leading - must reclassify to Ord");
        let medial = reclassify(&parse("a-b"));
        assert_eq!(medial, vec![Ord, Bin, Ord]);
    }

    #[test]
    fn script_styles_suppress_medium_thick_spacing() {
        use super::super::symbols::Class::*;
        let font = MathFont::new().unwrap();
        let warned = RefCell::new(HashSet::new());
        let c = Ctx::new(&font, &[], &warned, 11.0);
        // Bin spacing (medium) is real in text style, gone in script.
        assert!(c.spacing(Ord, Bin, Text) > 0.0);
        assert_eq!(c.spacing(Ord, Bin, Script), 0.0);
        assert_eq!(c.spacing(Ord, Rel, ScriptScript), 0.0);
        // Thin space survives in every style.
        assert!(c.spacing(Ord, Op, Script) > 0.0);
    }

    #[test]
    fn binary_and_relation_spacing_widens_a_run() {
        let plain = lay("ab", false).w;
        let bin = lay("a+b", false).w;
        let rel = lay("a=b", false).w;
        assert!(bin > plain, "+ must add binary spacing ({bin} vs {plain})");
        assert!(rel > plain, "= must add relation spacing ({rel} vs {plain})");
    }

    #[test]
    fn fraction_geometry_is_sane() {
        let f = lay("\\frac{abc}{d}", true);
        assert_eq!(f.rules.len(), 1);
        let rule = f.rules[0];
        // Numerator sits above the bar, denominator below it.
        assert!(
            f.glyphs.iter().any(|g| g.y > rule.y_top),
            "numerator above the rule"
        );
        assert!(
            f.glyphs.iter().any(|g| g.y < rule.y_top - rule.thickness),
            "denominator below the rule"
        );
        // Box is at least as wide as the wider part + the rule.
        assert!(f.w >= rule.w);
        assert!(f.asc > 0.0 && f.desc > 0.0);
    }

    #[test]
    fn sqrt_shifts_body_past_the_radical() {
        let f = lay("\\sqrt{x}", false);
        assert!(!f.rules.is_empty(), "vinculum rule");
        // Some glyph (the radicand) starts right of the surd's left.
        let min_x = f.glyphs.iter().map(|g| g.x).fold(f32::MAX, f32::min);
        assert!(
            f.glyphs.iter().any(|g| g.x > min_x + 0.5),
            "radicand must be inset past the radical sign"
        );
    }

    #[test]
    fn big_operator_limits_stack_in_display_only() {
        let d = lay("\\sum_{i=1}^{n} i", true);
        let t = lay("\\sum_{i=1}^{n} i", false);
        // Display stacks the limits → taller; text sets them to the
        // side → wider and shorter.
        assert!(
            d.asc + d.desc > t.asc + t.desc,
            "display limits should be taller (d={} t={})",
            d.asc + d.desc,
            t.asc + t.desc
        );
        assert!(t.w > d.w, "text-style scripts should be wider");
    }

    #[test]
    fn delimiters_grow_with_their_content() {
        let small = lay("\\left( x \\right)", true);
        let tall = lay(
            "\\left( \\frac{\\frac{a}{b}}{\\frac{c}{d}} \\right)",
            true,
        );
        assert!(
            tall.asc + tall.desc > 2.0 * (small.asc + small.desc),
            "fences must grow to a tall body ({} vs {})",
            tall.asc + tall.desc,
            small.asc + small.desc
        );
    }

    #[test]
    fn accent_sits_above_its_base() {
        let f = lay("\\hat{x}", false);
        assert!(f.glyphs.len() >= 2, "base + accent");
        let top = f.glyphs.iter().map(|g| g.y).fold(f32::MIN, f32::max);
        let base_like = f.glyphs.iter().filter(|g| g.y < top).count();
        assert!(base_like >= 1, "accent must be the highest glyph");
        assert!(f.asc > 0.0);
    }

    #[test]
    fn simultaneous_super_and_subscript_never_collide() {
        // Tall scripts on the same nucleus: the superscript cluster
        // must stay strictly above the subscript cluster (TeXbook
        // rule 18 / subSuperscriptGapMin). Before the coupled-shift
        // fix these overlapped for fraction scripts.
        for src in [
            "x_{\\frac{a}{b}}^{\\frac{c}{d}}",
            "X_{\\frac{1}{2}}^{\\frac{3}{4}}",
            "\\sigma_{ij}^{kl}",
            "A_{n+1}^{2}",
        ] {
            for display in [false, true] {
                let f = lay(src, display);
                let lo_sup = f
                    .glyphs
                    .iter()
                    .filter(|g| g.y > 1.0)
                    .map(|g| g.y)
                    .fold(f32::MAX, f32::min);
                let hi_sub = f
                    .glyphs
                    .iter()
                    .filter(|g| g.y < -1.0)
                    .map(|g| g.y)
                    .fold(f32::MIN, f32::max);
                if lo_sup.is_finite() && hi_sub.is_finite() {
                    assert!(
                        lo_sup > hi_sub,
                        "{src} (display={display}): superscript bottom \
                         {lo_sup} not above subscript top {hi_sub}"
                    );
                }
            }
        }
    }

    #[test]
    fn fraction_rule_spans_the_wider_part() {
        let f = lay("\\frac{a+b+c+d}{x}", true);
        let rule = f.rules[0];
        let widest = f
            .glyphs
            .iter()
            .map(|g| g.x)
            .fold(0.0_f32, f32::max);
        // The bar must reach at least to the rightmost glyph it spans.
        assert!(
            rule.x + rule.w >= widest,
            "fraction rule (x={}, w={}) doesn't cover content to x={widest}",
            rule.x,
            rule.w
        );
        assert!(rule.x <= 1.0, "rule should start near the left edge");
    }

    #[test]
    fn radical_vinculum_covers_the_radicand() {
        let f = lay("\\sqrt{a+b+c}", true);
        let rule = f.rules[0];
        let max_x = f.glyphs.iter().map(|g| g.x).fold(0.0_f32, f32::max);
        assert!(
            rule.x + rule.w >= max_x,
            "vinculum (x={}, w={}) must extend over the radicand to x={max_x}",
            rule.x,
            rule.w
        );
    }

    #[test]
    fn integral_keeps_scripts_to_the_side_even_in_display() {
        // \int defaults to nolimits: scripts stay sup/sub, not stacked,
        // unlike \sum. So display \int_0^1 is wider-than-tall relative
        // to display \sum_0^1.
        let i = lay("\\int_{0}^{1}", true);
        let s = lay("\\sum_{0}^{1}", true);
        assert!(
            i.w >= s.w,
            "∫ scripts to the side should be at least as wide as ∑'s ({} vs {})",
            i.w,
            s.w
        );
        assert!(
            (s.asc + s.desc) > (i.asc + i.desc),
            "stacked ∑ limits should be taller than ∫'s side scripts"
        );
    }

    #[test]
    fn accent_is_horizontally_within_the_base_span() {
        let f = lay("\\hat{M}", false);
        let base_right = f.w;
        let accent = f
            .glyphs
            .iter()
            .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
            .unwrap();
        assert!(
            accent.x >= -base_right && accent.x <= base_right * 1.5,
            "accent x={} should sit over the base (w={base_right})",
            accent.x
        );
    }

    #[test]
    fn limits_override_flips_script_placement() {
        // \int\limits stacks (taller) like \sum; \sum\nolimits sets to
        // the side (wider) like \int.
        let int_side = lay("\\int_{0}^{1}", true);
        let int_lim = lay("\\int\\limits_{0}^{1}", true);
        assert!(
            int_lim.asc + int_lim.desc > int_side.asc + int_side.desc,
            "\\int\\limits must stack (taller): {} vs {}",
            int_lim.asc + int_lim.desc,
            int_side.asc + int_side.desc
        );
        let sum_stack = lay("\\sum_{0}^{1}", true);
        let sum_nolim = lay("\\sum\\nolimits_{0}^{1}", true);
        assert!(
            sum_nolim.w > sum_stack.w,
            "\\sum\\nolimits must set scripts to the side (wider): {} vs {}",
            sum_nolim.w,
            sum_stack.w
        );
    }

    #[test]
    fn stretchy_accent_widens_with_the_base() {
        // \widehat over a wide base must select a wider accent glyph
        // (different gid) than over a single letter; a plain \hat must
        // not stretch.
        let topmost = |f: &Frag| {
            f.glyphs
                .iter()
                .max_by(|a, b| a.y.partial_cmp(&b.y).unwrap())
                .unwrap()
                .gid
        };
        let narrow = topmost(&lay("\\widehat{x}", false));
        let wide = topmost(&lay("\\widehat{xxxxxxxx}", false));
        assert_ne!(
            narrow, wide,
            "\\widehat must grow a wider variant over a wide base"
        );
        let hat_n = topmost(&lay("\\hat{x}", false));
        let hat_w = topmost(&lay("\\hat{xxxxxxxx}", false));
        assert_eq!(hat_n, hat_w, "\\hat is not stretchy");
    }

    #[test]
    fn cramped_style_lowers_scripts_under_a_radical() {
        // A superscript inside \sqrt{...} is laid out cramped, so it
        // sits no higher than the same superscript in free flow.
        let free = lay("x^{2}", false);
        let crmp = lay("\\sqrt{x^{2}}", false);
        let max_y = |f: &Frag| f.glyphs.iter().map(|g| g.y).fold(f32::MIN, f32::max);
        assert!(
            max_y(&crmp) <= max_y(&free) + 0.01,
            "cramped superscript ({}) should not exceed free ({})",
            max_y(&crmp),
            max_y(&free)
        );
    }

    #[test]
    fn matrix_lays_out_a_grid_with_fences() {
        let f = lay("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", true);
        // 4 entries + 2 parens ⇒ at least 6 glyphs.
        assert!(f.glyphs.len() >= 6, "got {}", f.glyphs.len());
        let xs: Vec<i32> = f.glyphs.iter().map(|g| (g.x * 4.0) as i32).collect();
        let ys: Vec<i32> = f.glyphs.iter().map(|g| (g.y * 4.0) as i32).collect();
        let distinct = |v: &[i32]| {
            let mut u = v.to_vec();
            u.sort_unstable();
            u.dedup();
            u.len()
        };
        assert!(distinct(&xs) >= 2, "matrix needs ≥2 columns");
        assert!(distinct(&ys) >= 2, "matrix needs ≥2 rows");
    }
}
