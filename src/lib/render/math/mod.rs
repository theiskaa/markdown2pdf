//! In-tree TeX math typesetting.
//!
//! `$…$` / `$$…$$` content is parsed ([`parse`]) into an atom tree,
//! laid out per the TeXbook Appendix G algorithm ([`layout`]) using
//! the OpenType MATH metrics of STIX Two Math ([`font`]), then drawn
//! by the renderer as filled glyph *outlines* + rule rectangles — no
//! font is embedded and nothing is selectable, so an equation behaves
//! like a figure in every PDF viewer.
//!

pub mod font;
pub mod layout;
pub mod parse;
pub mod symbols;

use self::font::MathFont;
use self::layout::{Ctx, Frag, Style};

/// Parse + lay out `content`. Returns `None` only for whitespace-only
/// input; malformed TeX still produces a (best-effort) fragment.
pub fn typeset(font: &MathFont, content: &str, display: bool, base_pt: f32) -> Option<Frag> {
    if content.trim().is_empty() {
        return None;
    }
    let nodes = parse::parse(content);
    let ctx = Ctx::new(font, base_pt);
    let st = if display { Style::Display } else { Style::Text };
    let frag = ctx.list(&nodes, st);
    if frag.glyphs.is_empty() && frag.rules.is_empty() {
        return None;
    }
    Some(frag)
}
