//! User-facing TOML schema (the annotated reference lives at
//! `docs/config.toml` in the repo).
//!
//! Every field is `Option<T>` so "the user didn't write this" is
//! distinguishable from "the user wrote zero / empty". The merge step
//! in `super::merge` collapses preset + user into a concrete
//! `super::resolved::ResolvedStyle` that the renderer consumes.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct DocumentConfig {
    /// Named theme preset to inherit from. Resolved before any field
    /// in this document overrides it. Default: `"default"`.
    pub theme: Option<String>,
    /// Theme presets use this to chain off another preset (e.g.
    /// `github.toml` has `inherits = "default"`).
    pub inherits: Option<String>,
    pub page: Option<PageConfig>,
    pub defaults: Option<BlockConfig>,
    pub headings: Option<HeadingsConfig>,
    pub paragraph: Option<BlockConfig>,
    pub code_block: Option<BlockConfig>,
    pub code_inline: Option<InlineConfig>,
    pub blockquote: Option<BlockConfig>,
    /// Per-kind callout / admonition styling. The top-level
    /// `[admonition]` block holds shared shape fields (padding,
    /// margins, font defaults). The nested `[admonition.note]`,
    /// `[admonition.info]`, `[admonition.tip]`, `[admonition.warning]`,
    /// `[admonition.danger]`, and `[admonition.generic]` blocks layer
    /// per-kind colour and label overrides on top.
    pub admonition: Option<AdmonitionConfig>,
    pub list: Option<ListsConfig>,
    pub table: Option<TableConfig>,
    pub image: Option<ImageConfig>,
    pub link: Option<InlineConfig>,
    /// Inline highlight (`==text==`). Only `background_color` is
    /// load-bearing today; the rest of `InlineConfig` is accepted for
    /// symmetry with `link`/`code_inline`.
    pub mark: Option<InlineConfig>,
    pub horizontal_rule: Option<RuleConfig>,
    /// LaTeX math (`$…$` / `$$…$$`). Display blocks honour `align`,
    /// `scale`, `color`, and block margins; inline math always flows
    /// with its surrounding text at the body size.
    pub math: Option<MathConfig>,
    pub metadata: Option<MetadataConfig>,
    pub header: Option<PageFurnitureConfig>,
    pub footer: Option<PageFurnitureConfig>,
    pub title_page: Option<TitlePageConfig>,
    pub toc: Option<TocConfig>,
    /// Operator-only policy on what the document is allowed to pull in
    /// while rendering. See [`SecurityConfig`].
    pub security: Option<SecurityConfig>,
}

/// Operator-controlled limits on what a document is allowed to pull in
/// while rendering. These exist for callers who render *untrusted*
/// markdown: a document can name any local path in an image reference,
/// and without a root to confine it to, the renderer will read it. The
/// document itself can never set these — frontmatter is metadata-only,
/// so this block comes solely from the operator's config.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct SecurityConfig {
    /// Directory that local image paths are resolved against and confined
    /// to. Relative paths resolve inside it; any path that escapes it
    /// after symlink resolution is refused. `None` (the default) keeps
    /// the historical behavior: relative paths resolve against the
    /// process working directory and absolute paths are read as given.
    pub image_root: Option<String>,
    /// Whether a document may reference an absolute local path.
    /// Defaults to `true` for backward compatibility.
    pub allow_absolute_image_paths: Option<bool>,
    /// Whether a document may reference a remote (`http`/`https`) image.
    /// Defaults to `true`. Independent of the `fetch` feature — with the
    /// feature off, remote images already fail.
    pub allow_remote_images: Option<bool>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct PageConfig {
    pub size: Option<PageSize>,
    pub orientation: Option<Orientation>,
    pub margins: Option<Sides<f32>>,
    pub columns: Option<u8>,
    pub column_gap_mm: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct HeadingsConfig {
    pub h1: Option<BlockConfig>,
    pub h2: Option<BlockConfig>,
    pub h3: Option<BlockConfig>,
    pub h4: Option<BlockConfig>,
    pub h5: Option<BlockConfig>,
    pub h6: Option<BlockConfig>,
}

/// The workhorse style block. Applies to any flowable block: paragraph,
/// heading, code block, blockquote, list, table cell, etc.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct BlockConfig {
    pub font_family: Option<String>,
    pub font_size_pt: Option<f32>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyleVariant>,
    pub text_color: Option<Color>,
    pub background_color: Option<Color>,
    /// Multiplier of `font_size_pt`. `1.4` means the line advance is
    /// `font_size * 1.4`.
    pub line_height: Option<f32>,
    pub text_align: Option<TextAlignment>,
    pub border: Option<BorderConfig>,
    pub padding: Option<Sides<f32>>,
    pub margin_before_pt: Option<f32>,
    pub margin_after_pt: Option<f32>,
    pub indent_pt: Option<f32>,
    pub letter_spacing_pt: Option<f32>,
    pub strikethrough: Option<bool>,
    pub underline: Option<bool>,
    /// When true, every lowercase letter in this block's text is
    /// rendered uppercase at ~78% font size (faux small caps). Real
    /// OpenType `smcp` substitution depends on the loaded font and is
    /// a follow-up.
    pub small_caps: Option<bool>,
    /// Ordered list of fallback font names. Codepoints not covered by
    /// the primary body / code font are looked up in each fallback in
    /// turn; the first font that has a glyph wins. Only the value on
    /// the document `[defaults]` block is read by the renderer — the
    /// field is accepted syntactically on per-block tables but ignored.
    pub fallback_fonts: Option<Vec<String>>,
}

/// Subset of `BlockConfig` for true inline runs (`code_inline`,
/// `link`): block-level fields like `padding`/`border`/`text_align`/
/// `line_height`/`margin_*` don't make sense for inline.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct InlineConfig {
    pub font_family: Option<String>,
    pub font_size_pt: Option<f32>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyleVariant>,
    pub text_color: Option<Color>,
    pub background_color: Option<Color>,
    pub padding: Option<Sides<f32>>,
    pub strikethrough: Option<bool>,
    pub underline: Option<bool>,
}

/// Per-kind admonition styling. The top-level [admonition] block
/// flattens a [`BlockConfig`] so shared shape fields (padding, margins,
/// font defaults) can be set in one place; the per-kind sub-blocks
/// layer colour and label overrides on top.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AdmonitionConfig {
    #[serde(flatten)]
    pub defaults: BlockConfig,
    pub note: Option<AdmonitionKindConfig>,
    pub info: Option<AdmonitionKindConfig>,
    pub tip: Option<AdmonitionKindConfig>,
    pub warning: Option<AdmonitionKindConfig>,
    pub danger: Option<AdmonitionKindConfig>,
    /// Catch-all for unknown kinds (`!!! bug "…"`). Inherits from
    /// [admonition] defaults; the renderer surfaces the author's raw
    /// label name as the header when no per-kind label override is set.
    pub generic: Option<AdmonitionKindConfig>,
}

/// Per-kind admonition overrides. Shape comes from the parent
/// [admonition] defaults; this block layers colour (`accent_color`
/// drives the icon + left border) and an optional explicit label
/// override on top.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct AdmonitionKindConfig {
    #[serde(flatten)]
    pub block: BlockConfig,
    pub accent_color: Option<Color>,
    pub label: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ListsConfig {
    pub ordered: Option<ListStyleConfig>,
    pub unordered: Option<ListStyleConfig>,
    pub task: Option<ListStyleConfig>,
    /// Shared between all list flavors unless overridden per-flavor.
    pub common: Option<ListStyleConfig>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ListStyleConfig {
    #[serde(flatten)]
    pub block: BlockConfig,
    /// For `unordered`: bullet glyph (`•`, `▪`, `–`, `→`).
    /// For `ordered`: numeric format hint (`"1."`, `"1)"`).
    /// For `task`: usually left unset; `[x]`/`[ ]` are emitted by the renderer.
    pub bullet: Option<String>,
    pub indent_per_level_pt: Option<f32>,
    /// Spacing between items in a tight (CommonMark default) list.
    pub item_spacing_tight_pt: Option<f32>,
    /// Spacing between items in a loose list (blank line between items).
    pub item_spacing_loose_pt: Option<f32>,
    /// Horizontal gap between the bullet/number and the item text.
    pub bullet_gap_pt: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TableConfig {
    pub header: Option<BlockConfig>,
    pub cell: Option<BlockConfig>,
    pub border: Option<BorderConfig>,
    pub alternating_row_background: Option<Color>,
    pub cell_padding: Option<Sides<f32>>,
    pub row_gap_pt: Option<f32>,
    pub margin_before_pt: Option<f32>,
    pub margin_after_pt: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ImageConfig {
    pub max_width_pct: Option<f32>,
    pub align: Option<ImageAlign>,
    pub caption: Option<BlockConfig>,
    pub margin_before_pt: Option<f32>,
    pub margin_after_pt: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RuleConfig {
    pub color: Option<Color>,
    pub thickness_pt: Option<f32>,
    pub style: Option<BorderStyle>,
    /// Width as percent of the content column. 100 = full width.
    pub width_pct: Option<f32>,
    pub margin_before_pt: Option<f32>,
    pub margin_after_pt: Option<f32>,
}

/// Styling for typeset math. `align` / `margin_*` apply to display
/// (`$$…$$`) blocks; `scale` multiplies the body font size for
/// display math (inline `$…$` always tracks the surrounding text
/// size); `color` is the math ink (defaults to the paragraph color).
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MathConfig {
    pub align: Option<TextAlignment>,
    pub scale: Option<f32>,
    pub color: Option<Color>,
    pub margin_before_pt: Option<f32>,
    pub margin_after_pt: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MetadataConfig {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub creator: Option<String>,
    /// BCP-47 / ISO-639 language tag (`"en"`, `"en-US"`, `"de"`).
    /// Emitted as the PDF Catalog `/Lang` entry for screen readers.
    /// Omitted entirely when unset.
    pub language: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct PageFurnitureConfig {
    pub left: Option<String>,
    pub center: Option<String>,
    pub right: Option<String>,
    pub style: Option<BlockConfig>,
    pub show_on_first_page: Option<bool>,
    /// Distance in points between the body's content edge and this
    /// piece of furniture's baseline. For `[header]` it's the gap
    /// above the body's first line; for `[footer]` it's the gap
    /// below the body's last line. Larger value = more breathing
    /// room. Default ≈ 14pt.
    pub gap_pt: Option<f32>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TitlePageConfig {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub date: Option<String>,
    pub cover_image_path: Option<String>,
    pub style: Option<BlockConfig>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct TocConfig {
    pub enabled: Option<bool>,
    pub title: Option<String>,
    pub max_depth: Option<u8>,
    pub style: Option<BlockConfig>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextAlignment {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    Portrait,
    Landscape,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImageAlign {
    Left,
    Center,
    Right,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FontStyleVariant {
    Normal,
    Italic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontWeight {
    Normal,
    Bold,
    /// CSS-style numeric weight (100..=900). Maps to bold ≥ 600 in the
    /// renderer today; richer mapping arrives once the
    /// per-weight font variant work.
    Numeric(u16),
}

impl Serialize for FontWeight {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            FontWeight::Normal => s.serialize_str("normal"),
            FontWeight::Bold => s.serialize_str("bold"),
            FontWeight::Numeric(n) => s.serialize_u16(*n),
        }
    }
}

impl<'de> Deserialize<'de> for FontWeight {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = FontWeight;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("`normal` | `bold` | integer 100..=900")
            }
            fn visit_str<E: Error>(self, s: &str) -> Result<FontWeight, E> {
                match s {
                    "normal" => Ok(FontWeight::Normal),
                    "bold" => Ok(FontWeight::Bold),
                    other => Err(E::custom(format!("unknown font weight `{}`", other))),
                }
            }
            fn visit_i64<E: Error>(self, n: i64) -> Result<FontWeight, E> {
                if (100..=900).contains(&n) {
                    Ok(FontWeight::Numeric(n as u16))
                } else {
                    Err(E::custom(format!("font weight {} not in 100..=900", n)))
                }
            }
            fn visit_u64<E: Error>(self, n: u64) -> Result<FontWeight, E> {
                self.visit_i64(n as i64)
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BorderStyle {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Deserialize, Serialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct BorderConfig {
    pub all: Option<BorderSide>,
    pub top: Option<BorderSide>,
    pub right: Option<BorderSide>,
    pub bottom: Option<BorderSide>,
    pub left: Option<BorderSide>,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct BorderSide {
    pub width_pt: f32,
    pub color: Color,
    pub style: BorderStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PageSize {
    A4,
    Letter,
    Legal,
    A3,
    A5,
    Custom { width_mm: f32, height_mm: f32 },
}

impl Serialize for PageSize {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            PageSize::A4 => s.serialize_str("A4"),
            PageSize::Letter => s.serialize_str("Letter"),
            PageSize::Legal => s.serialize_str("Legal"),
            PageSize::A3 => s.serialize_str("A3"),
            PageSize::A5 => s.serialize_str("A5"),
            PageSize::Custom { width_mm, height_mm } => {
                let mut m = s.serialize_map(Some(2))?;
                m.serialize_entry("width_mm", width_mm)?;
                m.serialize_entry("height_mm", height_mm)?;
                m.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for PageSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, MapAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = PageSize;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("`A4`/`Letter`/`Legal`/`A3`/`A5` or { width_mm, height_mm }")
            }
            fn visit_str<E: Error>(self, s: &str) -> Result<PageSize, E> {
                match s.to_ascii_lowercase().as_str() {
                    "a4" => Ok(PageSize::A4),
                    "letter" => Ok(PageSize::Letter),
                    "legal" => Ok(PageSize::Legal),
                    "a3" => Ok(PageSize::A3),
                    "a5" => Ok(PageSize::A5),
                    other => Err(E::custom(format!("unknown page size `{}`", other))),
                }
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<PageSize, M::Error> {
                let mut w: Option<f32> = None;
                let mut h: Option<f32> = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "width_mm" => w = Some(m.next_value()?),
                        "height_mm" => h = Some(m.next_value()?),
                        other => {
                            return Err(M::Error::custom(format!(
                                "unknown page.size field `{}` (expected width_mm/height_mm)",
                                other
                            )));
                        }
                    }
                }
                match (w, h) {
                    (Some(width_mm), Some(height_mm)) => {
                        Ok(PageSize::Custom { width_mm, height_mm })
                    }
                    _ => Err(M::Error::custom(
                        "custom page size requires both width_mm and height_mm",
                    )),
                }
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b))
    }
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Color, D::Error> {
        use serde::de::{Error, MapAccess, SeqAccess, Visitor};
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Color;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("`#RRGGBB` / `#RGB` / { r, g, b } / [r, g, b]")
            }
            fn visit_str<E: Error>(self, s: &str) -> Result<Color, E> {
                let s = s.trim();
                let hex = s.strip_prefix('#').ok_or_else(|| {
                    E::custom(format!("color string must start with #, got `{}`", s))
                })?;
                let (r, g, b) = match hex.len() {
                    3 => {
                        let parse = |c: char| -> Result<u8, E> {
                            u8::from_str_radix(&c.to_string(), 16)
                                .map(|v| v * 17)
                                .map_err(|e| E::custom(e.to_string()))
                        };
                        let mut it = hex.chars();
                        let r = parse(it.next().unwrap())?;
                        let g = parse(it.next().unwrap())?;
                        let b = parse(it.next().unwrap())?;
                        (r, g, b)
                    }
                    6 => {
                        let parse = |s: &str| -> Result<u8, E> {
                            u8::from_str_radix(s, 16).map_err(|e| E::custom(e.to_string()))
                        };
                        (parse(&hex[0..2])?, parse(&hex[2..4])?, parse(&hex[4..6])?)
                    }
                    _ => {
                        return Err(E::custom(format!(
                            "color hex must be 3 or 6 chars, got `{}`",
                            hex
                        )));
                    }
                };
                Ok(Color { r, g, b })
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Color, M::Error> {
                let mut r: Option<u8> = None;
                let mut g: Option<u8> = None;
                let mut b: Option<u8> = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "r" => r = Some(m.next_value()?),
                        "g" => g = Some(m.next_value()?),
                        "b" => b = Some(m.next_value()?),
                        other => {
                            return Err(M::Error::custom(format!(
                                "unknown color field `{}` (expected r/g/b)",
                                other
                            )));
                        }
                    }
                }
                Ok(Color {
                    r: r.ok_or_else(|| M::Error::missing_field("r"))?,
                    g: g.ok_or_else(|| M::Error::missing_field("g"))?,
                    b: b.ok_or_else(|| M::Error::missing_field("b"))?,
                })
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut s: S) -> Result<Color, S::Error> {
                let r: u8 = s
                    .next_element()?
                    .ok_or_else(|| S::Error::custom("color array missing red"))?;
                let g: u8 = s
                    .next_element()?
                    .ok_or_else(|| S::Error::custom("color array missing green"))?;
                let b: u8 = s
                    .next_element()?
                    .ok_or_else(|| S::Error::custom("color array missing blue"))?;
                Ok(Color { r, g, b })
            }
        }
        d.deserialize_any(V)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sides<T: Copy> {
    pub top: T,
    pub right: T,
    pub bottom: T,
    pub left: T,
}

impl<T: Copy + Serialize> Serialize for Sides<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(4))?;
        m.serialize_entry("top", &self.top)?;
        m.serialize_entry("right", &self.right)?;
        m.serialize_entry("bottom", &self.bottom)?;
        m.serialize_entry("left", &self.left)?;
        m.end()
    }
}

impl<T: Copy> Sides<T> {
    pub const fn uniform(v: T) -> Self {
        Self {
            top: v,
            right: v,
            bottom: v,
            left: v,
        }
    }
}

impl<'de, T> Deserialize<'de> for Sides<T>
where
    T: Deserialize<'de> + Copy,
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Sides<T>, D::Error> {
        use serde::de::{Error, MapAccess, SeqAccess, Visitor};
        use std::marker::PhantomData;

        struct V<T>(PhantomData<T>);
        impl<'de, T> Visitor<'de> for V<T>
        where
            T: Deserialize<'de> + Copy,
        {
            type Value = Sides<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("scalar (all sides), [v,h], [t,r,b,l], or { top,right,bottom,left }")
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut s: S) -> Result<Sides<T>, S::Error> {
                let first: T = s
                    .next_element()?
                    .ok_or_else(|| S::Error::custom("empty side array"))?;
                let second: Option<T> = s.next_element()?;
                let third: Option<T> = s.next_element()?;
                let fourth: Option<T> = s.next_element()?;
                match (second, third, fourth) {
                    (None, _, _) => Ok(Sides::uniform(first)),
                    (Some(h), None, _) => Ok(Sides {
                        top: first,
                        bottom: first,
                        right: h,
                        left: h,
                    }),
                    (Some(r), Some(b), Some(l)) => Ok(Sides {
                        top: first,
                        right: r,
                        bottom: b,
                        left: l,
                    }),
                    _ => Err(S::Error::custom(
                        "side array must have 1, 2, or 4 elements",
                    )),
                }
            }
            fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Sides<T>, M::Error> {
                let mut top: Option<T> = None;
                let mut right: Option<T> = None;
                let mut bottom: Option<T> = None;
                let mut left: Option<T> = None;
                while let Some(k) = m.next_key::<String>()? {
                    match k.as_str() {
                        "top" => top = Some(m.next_value()?),
                        "right" => right = Some(m.next_value()?),
                        "bottom" => bottom = Some(m.next_value()?),
                        "left" => left = Some(m.next_value()?),
                        other => {
                            return Err(M::Error::custom(format!(
                                "unknown side field `{}` (expected top/right/bottom/left)",
                                other
                            )));
                        }
                    }
                }
                Ok(Sides {
                    top: top.ok_or_else(|| M::Error::missing_field("top"))?,
                    right: right.ok_or_else(|| M::Error::missing_field("right"))?,
                    bottom: bottom.ok_or_else(|| M::Error::missing_field("bottom"))?,
                    left: left.ok_or_else(|| M::Error::missing_field("left"))?,
                })
            }
            fn visit_i64<E: Error>(self, n: i64) -> Result<Sides<T>, E> {
                // Build a one-element seq using the value as a number.
                // toml passes integers and floats through visit_i64/f64;
                // we forward to T's own deserializer via a tiny shim.
                let v: T = T::deserialize(serde::de::value::I64Deserializer::new(n))
                    .map_err(|e: E| e)?;
                Ok(Sides::uniform(v))
            }
            fn visit_f64<E: Error>(self, n: f64) -> Result<Sides<T>, E> {
                let v: T = T::deserialize(serde::de::value::F64Deserializer::new(n))
                    .map_err(|e: E| e)?;
                Ok(Sides::uniform(v))
            }
            fn visit_u64<E: Error>(self, n: u64) -> Result<Sides<T>, E> {
                self.visit_i64(n as i64)
            }
        }
        d.deserialize_any(V::<T>(std::marker::PhantomData))
    }
}
