//! In-tree TeX math typesetting.
//!
//! `$…$` / `$$…$$` content is parsed ([`parse`]) into an atom tree,
//! laid out per the TeXbook Appendix G algorithm ([`layout`]) using
//! the OpenType MATH metrics of STIX Two Math ([`font`]), then drawn
//! by the renderer as filled glyph *outlines* + rule rectangles — no
//! font is embedded and nothing is selectable, so an equation behaves
//! like a figure in every PDF viewer.
//!
//! Characters STIX Two Math lacks (CJK, Arabic, Devanagari, … inside
//! `\text{…}` or as bare symbols) are outlined the same way from the
//! document's body / fallback faces (`text_fonts`) — still vector-only,
//! nothing embedded. A character no face covers renders as a hollow
//! placeholder box with a `log::warn!` rather than vanishing. RTL
//! runs inside `\text{…}` are reordered to visual order (see
//! [`layout::visual_order`]); no joining/shaping is performed —
//! parity with the body-text emit path.
//!

pub mod font;
pub mod layout;
pub mod parse;
pub mod symbols;

use self::font::{MathFont, MathTextFont};
use self::layout::{Ctx, Frag, Style};
use std::cell::RefCell;
use std::collections::HashSet;

/// Parse + lay out `content`. Returns `None` only for whitespace-only
/// input; malformed TeX still produces a (best-effort) fragment.
/// `text_fonts` is the body-then-fallback chain consulted for
/// characters outside the math font's coverage (may be empty).
/// `warned` collects characters no font covers; share one set across
/// a render so each distinct char warns once per document.
pub fn typeset(
    font: &MathFont,
    text_fonts: &[MathTextFont<'_>],
    warned: &RefCell<HashSet<char>>,
    content: &str,
    display: bool,
    base_pt: f32,
) -> Option<Frag> {
    if content.trim().is_empty() {
        return None;
    }
    let nodes = parse::parse(content);
    let ctx = Ctx::new(font, text_fonts, warned, base_pt);
    let st = if display { Style::Display } else { Style::Text };
    let frag = ctx.list(&nodes, st);
    if frag.glyphs.is_empty() && frag.rules.is_empty() {
        return None;
    }
    Some(frag)
}
