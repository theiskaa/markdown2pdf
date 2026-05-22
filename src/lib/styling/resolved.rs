//! Concrete, fully-resolved style consumed by the renderer.
//!
//! No `Option<T>` here — every field has a value either from a theme
//! preset or a user override. The `super::merge::resolve` function is
//! the one place that promotes a `super::schema::DocumentConfig` into
//! this form.

use serde::Serialize;

pub use super::schema::{
    BorderStyle, Color, FontStyleVariant, FontWeight, ImageAlign, Orientation, PageSize, Sides,
    TextAlignment,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedStyle {
    pub page: ResolvedPage,
    pub headings: [ResolvedBlock; 6],
    pub paragraph: ResolvedBlock,
    pub code_block: ResolvedBlock,
    pub code_inline: ResolvedInline,
    pub blockquote: ResolvedBlock,
    pub admonition: ResolvedAdmonition,
    pub list_ordered: ResolvedList,
    pub list_unordered: ResolvedList,
    pub list_task: ResolvedList,
    pub table: ResolvedTable,
    pub image: ResolvedImage,
    pub link: ResolvedInline,
    pub mark: ResolvedInline,
    pub horizontal_rule: ResolvedRule,
    pub math: ResolvedMath,
    pub metadata: ResolvedMetadata,
    pub header: Option<ResolvedPageFurniture>,
    pub footer: Option<ResolvedPageFurniture>,
    pub title_page: Option<ResolvedTitlePage>,
    pub toc: Option<ResolvedToc>,
    /// Ordered list of fallback font names (resolved from
    /// `[defaults].fallback_fonts`). The renderer consults these in
    /// order when the primary body / code font lacks a glyph for a
    /// codepoint.
    pub fallback_fonts: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedPage {
    pub size: PageSize,
    pub orientation: Orientation,
    pub margins_mm: Sides<f32>,
    pub columns: u8,
    pub column_gap_mm: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedBlock {
    pub font_family: Option<String>,
    pub font_size_pt: f32,
    pub font_weight: FontWeight,
    pub font_style: FontStyleVariant,
    pub text_color: Color,
    pub background_color: Option<Color>,
    pub line_height: f32,
    pub text_align: TextAlignment,
    pub border: ResolvedBorder,
    pub padding: Sides<f32>,
    pub margin_before_pt: f32,
    pub margin_after_pt: f32,
    pub indent_pt: f32,
    pub letter_spacing_pt: f32,
    pub strikethrough: bool,
    pub underline: bool,
    pub small_caps: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedInline {
    pub font_family: Option<String>,
    pub font_size_pt: f32,
    pub font_weight: FontWeight,
    pub font_style: FontStyleVariant,
    pub text_color: Color,
    pub background_color: Option<Color>,
    pub padding: Sides<f32>,
    pub strikethrough: bool,
    pub underline: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedList {
    pub block: ResolvedBlock,
    pub bullet: String,
    pub indent_per_level_pt: f32,
    pub item_spacing_tight_pt: f32,
    pub item_spacing_loose_pt: f32,
    pub bullet_gap_pt: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedTable {
    pub header: ResolvedBlock,
    pub cell: ResolvedBlock,
    pub border: ResolvedBorder,
    pub alternating_row_background: Option<Color>,
    pub cell_padding: Sides<f32>,
    pub row_gap_pt: f32,
    pub margin_before_pt: f32,
    pub margin_after_pt: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedImage {
    pub max_width_pct: f32,
    pub align: ImageAlign,
    pub margin_before_pt: f32,
    pub margin_after_pt: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedRule {
    pub color: Color,
    pub thickness_pt: f32,
    pub style: BorderStyle,
    pub width_pct: f32,
    pub margin_before_pt: f32,
    pub margin_after_pt: f32,
}

/// Resolved math styling. `align` / `margin_*` drive display
/// (`$$…$$`) blocks; `scale` multiplies the body size for display
/// math; `color` is the math ink.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedMath {
    pub align: TextAlignment,
    pub scale: f32,
    pub color: Color,
    pub margin_before_pt: f32,
    pub margin_after_pt: f32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Vec<String>,
    pub creator: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedPageFurniture {
    pub left: Option<String>,
    pub center: Option<String>,
    pub right: Option<String>,
    pub style: ResolvedBlock,
    pub show_on_first_page: bool,
    /// Gap in points from the body's content edge to the furniture's
    /// baseline (above for headers, below for footers).
    pub gap_pt: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedTitlePage {
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub cover_image_path: Option<String>,
    pub style: ResolvedBlock,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedToc {
    pub title: String,
    pub max_depth: u8,
    pub style: ResolvedBlock,
}

/// Resolved admonition styling. The renderer picks the matching
/// per-kind block via [`for_kind`]; unknown canonical kinds fall back
/// to `generic`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedAdmonition {
    pub note: ResolvedAdmonitionKind,
    pub info: ResolvedAdmonitionKind,
    pub tip: ResolvedAdmonitionKind,
    pub warning: ResolvedAdmonitionKind,
    pub danger: ResolvedAdmonitionKind,
    pub generic: ResolvedAdmonitionKind,
}

/// One callout kind's fully-resolved styling. `block` carries the
/// shape (padding, margins, font, background); `accent_color` drives
/// the icon and left-edge border; `label` is the header text the
/// renderer shows when the author didn't write an explicit title.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedAdmonitionKind {
    pub block: ResolvedBlock,
    pub accent_color: Color,
    pub label: String,
}

impl ResolvedAdmonition {
    /// Look up the per-kind block by canonical kind name. Falls back
    /// to `generic` for anything outside the first-class set.
    pub fn for_kind(&self, kind: &str) -> &ResolvedAdmonitionKind {
        match kind {
            "note" => &self.note,
            "info" => &self.info,
            "tip" => &self.tip,
            "warning" => &self.warning,
            "danger" => &self.danger,
            _ => &self.generic,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedBorder {
    pub top: Option<ResolvedBorderSide>,
    pub right: Option<ResolvedBorderSide>,
    pub bottom: Option<ResolvedBorderSide>,
    pub left: Option<ResolvedBorderSide>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ResolvedBorderSide {
    pub width_pt: f32,
    pub color: Color,
    pub style: BorderStyle,
}

impl ResolvedBlock {
    /// True for bold-or-heavier weights (CSS-style 600+ counts).
    pub fn is_bold(&self) -> bool {
        match self.font_weight {
            FontWeight::Bold => true,
            FontWeight::Numeric(n) if n >= 600 => true,
            _ => false,
        }
    }
    pub fn is_italic(&self) -> bool {
        matches!(self.font_style, FontStyleVariant::Italic)
    }
    pub fn text_color_rgb(&self) -> (u8, u8, u8) {
        (self.text_color.r, self.text_color.g, self.text_color.b)
    }
    pub fn background_color_rgb(&self) -> Option<(u8, u8, u8)> {
        self.background_color.map(|c| (c.r, c.g, c.b))
    }
}

impl ResolvedInline {
    pub fn is_bold(&self) -> bool {
        match self.font_weight {
            FontWeight::Bold => true,
            FontWeight::Numeric(n) if n >= 600 => true,
            _ => false,
        }
    }
    pub fn is_italic(&self) -> bool {
        matches!(self.font_style, FontStyleVariant::Italic)
    }
    pub fn text_color_rgb(&self) -> (u8, u8, u8) {
        (self.text_color.r, self.text_color.g, self.text_color.b)
    }
    pub fn background_color_rgb(&self) -> Option<(u8, u8, u8)> {
        self.background_color.map(|c| (c.r, c.g, c.b))
    }
}

impl ResolvedRule {
    pub fn color_rgb(&self) -> (u8, u8, u8) {
        (self.color.r, self.color.g, self.color.b)
    }
}

impl Default for ResolvedStyle {
    /// Synchronously load the bundled `default` theme preset. Panics
    /// only if the bundled `default.toml` itself is malformed — that's
    /// a programmer error in the repo, not something a caller can
    /// recover from at runtime, and `tests/styling_schema.rs` catches
    /// it in CI.
    fn default() -> Self {
        super::merge::resolve(super::schema::DocumentConfig::default(), None)
            .expect("bundled `default` theme preset failed to load — please file an issue")
    }
}

