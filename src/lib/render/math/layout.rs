//! TeXbook Appendix G layout: atom tree → positioned glyphs + rules.
//!
//! A [`Frag`] is a laid-out fragment in points, baseline at `y = 0`,
//! `+y` up, `x` growing right. Styles (Display/Text/Script/
//! ScriptScript, each with a cramped form) drive size and the
//! OpenType MATH constants exactly as TeX does.

use super::font::{MathFont, Stretch};
use super::parse::Node;
use super::symbols::Class;

#[derive(Debug, Clone, Copy)]
pub struct PlacedGlyph {
    pub gid: u16,
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
    pub base_pt: f32,
}

impl<'f> Ctx<'f> {
    pub fn new(font: &'f MathFont, base_pt: f32) -> Self {
        Ctx { font, base_pt }
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
            let asc = f.asc;
            let desc = f.desc;
            let italic = f.italic;
            out.absorb(f, x);
            // advance includes the fragment's own width.
            let w = self.frag_w(nodes, i, st, classes[i]);
            x += w;
            out.asc = out.asc.max(asc);
            out.desc = out.desc.max(desc);
            out.italic = italic;
            let _ = (asc, desc);
            prev = Some(cls);
        }
        out.w = x;
        out
    }

    fn frag_w(&self, nodes: &[Node], i: usize, st: Style, cls: Class) -> f32 {
        let (f, _) = self.node(&nodes[i], st, cls);
        f.w
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

    fn glyph_frag(&self, ch: char, st: Style) -> Frag {
        let size = self.size(st);
        let Some(gid) = self.font.glyph_id(ch) else {
            // Missing glyph: leave a blank the width of a digit.
            let mut f = Frag::empty();
            f.w = 0.5 * size;
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
        for ch in t.chars() {
            if ch == ' ' {
                x += 0.28 * size;
                continue;
            }
            let g = match self.font.glyph_id(ch) {
                Some(g) => g,
                None => continue,
            };
            let m = self.font.glyph(g);
            f.glyphs.push(PlacedGlyph {
                gid: g,
                x,
                y: 0.0,
                size,
            });
            f.asc = f.asc.max(self.font.scale(m.height(), size));
            f.desc = f.desc.max(self.font.scale(m.depth(), size));
            x += self.font.scale(m.advance, size);
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
        if let Some(s) = sup {
            let sf = self.list(s, st.sup());
            let mut shift = self.font.scale(
                if st.is_cramped() {
                    c.superscript_shift_up_cramped
                } else {
                    c.superscript_shift_up
                },
                size,
            );
            let min_bottom = self.font.scale(c.superscript_bottom_min, size);
            shift = shift
                .max(bf.asc - self.font.scale(c.superscript_baseline_drop_max, size))
                .max(sf.desc + min_bottom);
            f.absorb(sf.clone().shift(0.0, shift), x + italic);
            sup_h = shift + sf.asc;
            x = x.max(x + italic + sf.w);
        }
        if let Some(s) = sub {
            let sf = self.list(s, st.sub());
            let mut shift = self.font.scale(c.subscript_shift_down, size);
            let top_max = self.font.scale(c.subscript_top_max, size);
            shift = shift
                .max(bf.desc + self.font.scale(c.subscript_baseline_drop_min, size))
                .max(sf.asc - top_max);
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
        for p in parts.iter().rev() {
            let count = if p.extender { reps } else { 1 };
            for _ in 0..count {
                let m = self.font.glyph(p.gid);
                adv_w = adv_w.max(self.font.scale(m.advance, size));
                f.glyphs.push(PlacedGlyph {
                    gid: p.gid,
                    x,
                    y: y - self.font.scale(m.y_min, size),
                    size,
                });
                y += self.font.scale(p.full_advance, size) - ov;
            }
        }
        adv_w
    }

    fn accent(&self, mark: char, _stretchy: bool, body: &[Node], st: Style) -> Frag {
        let size = self.size(st);
        let bf = self.list(body, st.cramped());
        let Some(ag) = self.font.glyph_id(mark) else {
            return bf;
        };
        let am = self.font.glyph(ag);
        let acc_w = self.font.scale(am.advance.max(am.x_max - am.x_min), size);
        let acc_h = self.font.scale(am.height(), size);
        // Centre the accent over the base's accent-attachment point.
        let base_skew = if let Some(Node::Symbol { ch, .. }) = body.first() {
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
            x: base_skew - acc_w / 2.0,
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
    use crate::render::math::parse::parse;

    fn lay(src: &str, display: bool) -> Frag {
        let font = MathFont::new().unwrap();
        let ctx = Ctx::new(&font, 11.0);
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
}
