//! Merge and resolve pipeline: `DocumentConfig` + theme preset →
//! `ResolvedStyle`. The renderer never sees a `DocumentConfig`; the
//! parser never produces a `ResolvedStyle`.
//!
//! Rules:
//! - Overlay wins on `Some`. Every `Option<T>` field on the user's
//!   config replaces the base preset's value when it's `Some`.
//! - `defaults: BlockConfig` cascades: any field unset on a specific
//!   block inherits from `defaults`, which itself inherits from the
//!   preset's `defaults`.
//! - After merging, the preset is required to have left every concrete
//!   field set. A `None` after merge is a programmer error in the
//!   bundled `default.toml`, surfaced as `PresetIncomplete`.

use super::error::ResolveError;
use super::resolved::{
    ResolvedAdmonition, ResolvedAdmonitionKind, ResolvedBlock, ResolvedBorder, ResolvedBorderSide,
    ResolvedImage, ResolvedInline, ResolvedList, ResolvedMath, ResolvedMetadata, ResolvedPage,
    ResolvedPageFurniture, ResolvedRule, ResolvedStyle, ResolvedTable, ResolvedTitlePage,
    ResolvedToc,
};
use super::schema::*;
use super::themes::load_theme_preset;

/// Top-level entry. Pick the theme (CLI `--theme` > user's `theme =`
/// > `"default"`), merge preset + user, lower to `ResolvedStyle`.
pub fn resolve(
    user: DocumentConfig,
    theme_override: Option<&str>,
) -> Result<ResolvedStyle, ResolveError> {
    resolve_with_overrides(user, theme_override, None)
}

/// Like [`resolve`], but applies `overrides` as the highest-priority
/// layer — on top of the theme preset *and* the user config. Used by
/// the CLI so per-parameter flags win over the config file and
/// `--theme`. The cascade is therefore:
/// `preset → user config → overrides`.
pub fn resolve_with_overrides(
    user: DocumentConfig,
    theme_override: Option<&str>,
    overrides: Option<DocumentConfig>,
) -> Result<ResolvedStyle, ResolveError> {
    let theme_name = theme_override
        .map(str::to_string)
        .or_else(|| user.theme.clone())
        .unwrap_or_else(|| "default".to_string());
    let preset = load_theme_preset(&theme_name)?;
    let mut merged = merge_documents(preset, user);
    if let Some(ov) = overrides {
        merged = merge_documents(merged, ov);
    }
    lower(&theme_name, merged)
}

/// Merge two `DocumentConfig` values field by field. Overlay wins on
/// `Some`. Nested config types recurse via their dedicated merge
/// helpers so that overriding `headings.h2.font_size_pt` doesn't wipe
/// out `headings.h2.font_weight`.
pub fn merge_documents(base: DocumentConfig, overlay: DocumentConfig) -> DocumentConfig {
    DocumentConfig {
        theme: overlay.theme.or(base.theme),
        inherits: overlay.inherits.or(base.inherits),
        page: merge_optional(base.page, overlay.page, merge_page),
        defaults: merge_optional(base.defaults, overlay.defaults, merge_block),
        headings: merge_optional(base.headings, overlay.headings, merge_headings),
        paragraph: merge_optional(base.paragraph, overlay.paragraph, merge_block),
        code_block: merge_optional(base.code_block, overlay.code_block, merge_block),
        code_inline: merge_optional(base.code_inline, overlay.code_inline, merge_inline),
        blockquote: merge_optional(base.blockquote, overlay.blockquote, merge_block),
        admonition: merge_optional(base.admonition, overlay.admonition, merge_admonition),
        list: merge_optional(base.list, overlay.list, merge_lists),
        table: merge_optional(base.table, overlay.table, merge_table),
        image: merge_optional(base.image, overlay.image, merge_image),
        link: merge_optional(base.link, overlay.link, merge_inline),
        mark: merge_optional(base.mark, overlay.mark, merge_inline),
        horizontal_rule: merge_optional(base.horizontal_rule, overlay.horizontal_rule, merge_rule),
        math: merge_optional(base.math, overlay.math, merge_math),
        metadata: merge_optional(base.metadata, overlay.metadata, merge_metadata),
        header: merge_optional(base.header, overlay.header, merge_furniture),
        footer: merge_optional(base.footer, overlay.footer, merge_furniture),
        title_page: merge_optional(base.title_page, overlay.title_page, merge_title_page),
        toc: merge_optional(base.toc, overlay.toc, merge_toc),
    }
}

fn merge_optional<T, F: FnOnce(T, T) -> T>(base: Option<T>, overlay: Option<T>, f: F) -> Option<T> {
    match (base, overlay) {
        (None, x) | (x, None) => x,
        (Some(b), Some(o)) => Some(f(b, o)),
    }
}

fn merge_page(base: PageConfig, overlay: PageConfig) -> PageConfig {
    PageConfig {
        size: overlay.size.or(base.size),
        orientation: overlay.orientation.or(base.orientation),
        margins: overlay.margins.or(base.margins),
        columns: overlay.columns.or(base.columns),
        column_gap_mm: overlay.column_gap_mm.or(base.column_gap_mm),
    }
}

fn merge_admonition(base: AdmonitionConfig, overlay: AdmonitionConfig) -> AdmonitionConfig {
    AdmonitionConfig {
        defaults: merge_block(base.defaults, overlay.defaults),
        note: merge_optional(base.note, overlay.note, merge_admonition_kind),
        info: merge_optional(base.info, overlay.info, merge_admonition_kind),
        tip: merge_optional(base.tip, overlay.tip, merge_admonition_kind),
        warning: merge_optional(base.warning, overlay.warning, merge_admonition_kind),
        danger: merge_optional(base.danger, overlay.danger, merge_admonition_kind),
        generic: merge_optional(base.generic, overlay.generic, merge_admonition_kind),
    }
}

fn merge_admonition_kind(
    base: AdmonitionKindConfig,
    overlay: AdmonitionKindConfig,
) -> AdmonitionKindConfig {
    AdmonitionKindConfig {
        block: merge_block(base.block, overlay.block),
        accent_color: overlay.accent_color.or(base.accent_color),
        label: overlay.label.or(base.label),
    }
}

fn merge_block(base: BlockConfig, overlay: BlockConfig) -> BlockConfig {
    BlockConfig {
        font_family: overlay.font_family.or(base.font_family),
        font_size_pt: overlay.font_size_pt.or(base.font_size_pt),
        font_weight: overlay.font_weight.or(base.font_weight),
        font_style: overlay.font_style.or(base.font_style),
        text_color: overlay.text_color.or(base.text_color),
        background_color: overlay.background_color.or(base.background_color),
        line_height: overlay.line_height.or(base.line_height),
        text_align: overlay.text_align.or(base.text_align),
        border: merge_optional(base.border, overlay.border, merge_border),
        padding: overlay.padding.or(base.padding),
        margin_before_pt: overlay.margin_before_pt.or(base.margin_before_pt),
        margin_after_pt: overlay.margin_after_pt.or(base.margin_after_pt),
        indent_pt: overlay.indent_pt.or(base.indent_pt),
        letter_spacing_pt: overlay.letter_spacing_pt.or(base.letter_spacing_pt),
        strikethrough: overlay.strikethrough.or(base.strikethrough),
        underline: overlay.underline.or(base.underline),
        small_caps: overlay.small_caps.or(base.small_caps),
        fallback_fonts: overlay.fallback_fonts.or(base.fallback_fonts),
    }
}

fn merge_inline(base: InlineConfig, overlay: InlineConfig) -> InlineConfig {
    InlineConfig {
        font_family: overlay.font_family.or(base.font_family),
        font_size_pt: overlay.font_size_pt.or(base.font_size_pt),
        font_weight: overlay.font_weight.or(base.font_weight),
        font_style: overlay.font_style.or(base.font_style),
        text_color: overlay.text_color.or(base.text_color),
        background_color: overlay.background_color.or(base.background_color),
        padding: overlay.padding.or(base.padding),
        strikethrough: overlay.strikethrough.or(base.strikethrough),
        underline: overlay.underline.or(base.underline),
    }
}

fn merge_headings(base: HeadingsConfig, overlay: HeadingsConfig) -> HeadingsConfig {
    HeadingsConfig {
        h1: merge_optional(base.h1, overlay.h1, merge_block),
        h2: merge_optional(base.h2, overlay.h2, merge_block),
        h3: merge_optional(base.h3, overlay.h3, merge_block),
        h4: merge_optional(base.h4, overlay.h4, merge_block),
        h5: merge_optional(base.h5, overlay.h5, merge_block),
        h6: merge_optional(base.h6, overlay.h6, merge_block),
    }
}

fn merge_lists(base: ListsConfig, overlay: ListsConfig) -> ListsConfig {
    ListsConfig {
        ordered: merge_optional(base.ordered, overlay.ordered, merge_list_style),
        unordered: merge_optional(base.unordered, overlay.unordered, merge_list_style),
        task: merge_optional(base.task, overlay.task, merge_list_style),
        common: merge_optional(base.common, overlay.common, merge_list_style),
    }
}

fn merge_list_style(base: ListStyleConfig, overlay: ListStyleConfig) -> ListStyleConfig {
    ListStyleConfig {
        block: merge_block(base.block, overlay.block),
        bullet: overlay.bullet.or(base.bullet),
        indent_per_level_pt: overlay.indent_per_level_pt.or(base.indent_per_level_pt),
        item_spacing_tight_pt: overlay.item_spacing_tight_pt.or(base.item_spacing_tight_pt),
        item_spacing_loose_pt: overlay.item_spacing_loose_pt.or(base.item_spacing_loose_pt),
    }
}

fn merge_table(base: TableConfig, overlay: TableConfig) -> TableConfig {
    TableConfig {
        header: merge_optional(base.header, overlay.header, merge_block),
        cell: merge_optional(base.cell, overlay.cell, merge_block),
        border: merge_optional(base.border, overlay.border, merge_border),
        alternating_row_background: overlay.alternating_row_background.or(base.alternating_row_background),
        cell_padding: overlay.cell_padding.or(base.cell_padding),
        row_gap_pt: overlay.row_gap_pt.or(base.row_gap_pt),
        margin_before_pt: overlay.margin_before_pt.or(base.margin_before_pt),
        margin_after_pt: overlay.margin_after_pt.or(base.margin_after_pt),
    }
}

fn merge_image(base: ImageConfig, overlay: ImageConfig) -> ImageConfig {
    ImageConfig {
        max_width_pct: overlay.max_width_pct.or(base.max_width_pct),
        align: overlay.align.or(base.align),
        caption: merge_optional(base.caption, overlay.caption, merge_block),
        margin_before_pt: overlay.margin_before_pt.or(base.margin_before_pt),
        margin_after_pt: overlay.margin_after_pt.or(base.margin_after_pt),
    }
}

fn merge_rule(base: RuleConfig, overlay: RuleConfig) -> RuleConfig {
    RuleConfig {
        color: overlay.color.or(base.color),
        thickness_pt: overlay.thickness_pt.or(base.thickness_pt),
        style: overlay.style.or(base.style),
        width_pct: overlay.width_pct.or(base.width_pct),
        margin_before_pt: overlay.margin_before_pt.or(base.margin_before_pt),
        margin_after_pt: overlay.margin_after_pt.or(base.margin_after_pt),
    }
}

fn merge_math(base: MathConfig, overlay: MathConfig) -> MathConfig {
    MathConfig {
        align: overlay.align.or(base.align),
        scale: overlay.scale.or(base.scale),
        color: overlay.color.or(base.color),
        margin_before_pt: overlay.margin_before_pt.or(base.margin_before_pt),
        margin_after_pt: overlay.margin_after_pt.or(base.margin_after_pt),
    }
}

fn merge_metadata(base: MetadataConfig, overlay: MetadataConfig) -> MetadataConfig {
    MetadataConfig {
        title: overlay.title.or(base.title),
        author: overlay.author.or(base.author),
        subject: overlay.subject.or(base.subject),
        keywords: overlay.keywords.or(base.keywords),
        creator: overlay.creator.or(base.creator),
        language: overlay.language.or(base.language),
    }
}

fn merge_furniture(base: PageFurnitureConfig, overlay: PageFurnitureConfig) -> PageFurnitureConfig {
    PageFurnitureConfig {
        left: overlay.left.or(base.left),
        center: overlay.center.or(base.center),
        right: overlay.right.or(base.right),
        style: merge_optional(base.style, overlay.style, merge_block),
        show_on_first_page: overlay.show_on_first_page.or(base.show_on_first_page),
        gap_pt: overlay.gap_pt.or(base.gap_pt),
    }
}

fn merge_title_page(base: TitlePageConfig, overlay: TitlePageConfig) -> TitlePageConfig {
    TitlePageConfig {
        title: overlay.title.or(base.title),
        subtitle: overlay.subtitle.or(base.subtitle),
        author: overlay.author.or(base.author),
        date: overlay.date.or(base.date),
        cover_image_path: overlay.cover_image_path.or(base.cover_image_path),
        style: merge_optional(base.style, overlay.style, merge_block),
    }
}

fn merge_toc(base: TocConfig, overlay: TocConfig) -> TocConfig {
    TocConfig {
        enabled: overlay.enabled.or(base.enabled),
        title: overlay.title.or(base.title),
        max_depth: overlay.max_depth.or(base.max_depth),
        style: merge_optional(base.style, overlay.style, merge_block),
    }
}

fn merge_border(base: BorderConfig, overlay: BorderConfig) -> BorderConfig {
    BorderConfig {
        all: overlay.all.or(base.all),
        top: overlay.top.or(base.top),
        right: overlay.right.or(base.right),
        bottom: overlay.bottom.or(base.bottom),
        left: overlay.left.or(base.left),
    }
}

fn lower(theme: &str, cfg: DocumentConfig) -> Result<ResolvedStyle, ResolveError> {
    let defaults = cfg.defaults.unwrap_or_default();
    let page_cfg = cfg.page.ok_or_else(|| missing(theme, "page"))?;
    let headings_cfg = cfg.headings.unwrap_or_default();

    let page = ResolvedPage {
        size: page_cfg.size.ok_or_else(|| missing(theme, "page.size"))?,
        orientation: page_cfg
            .orientation
            .ok_or_else(|| missing(theme, "page.orientation"))?,
        margins_mm: page_cfg
            .margins
            .ok_or_else(|| missing(theme, "page.margins"))?,
        columns: page_cfg.columns.unwrap_or(1),
        column_gap_mm: page_cfg.column_gap_mm.unwrap_or(0.0),
    };

    let paragraph = lower_block(theme, "paragraph", &defaults, cfg.paragraph.unwrap_or_default())?;
    let h1 = lower_block(theme, "headings.h1", &defaults, headings_cfg.h1.unwrap_or_default())?;
    let h2 = lower_block(theme, "headings.h2", &defaults, headings_cfg.h2.unwrap_or_default())?;
    let h3 = lower_block(theme, "headings.h3", &defaults, headings_cfg.h3.unwrap_or_default())?;
    let h4 = lower_block(theme, "headings.h4", &defaults, headings_cfg.h4.unwrap_or_default())?;
    let h5 = lower_block(theme, "headings.h5", &defaults, headings_cfg.h5.unwrap_or_default())?;
    let h6 = lower_block(theme, "headings.h6", &defaults, headings_cfg.h6.unwrap_or_default())?;
    let code_block = lower_block(theme, "code_block", &defaults, cfg.code_block.unwrap_or_default())?;
    let code_inline = lower_inline(theme, "code_inline", &defaults, cfg.code_inline.unwrap_or_default())?;
    let blockquote = lower_block(theme, "blockquote", &defaults, cfg.blockquote.unwrap_or_default())?;
    let admonition =
        lower_admonition(theme, &defaults, cfg.admonition.unwrap_or_default())?;
    let link = lower_inline(theme, "link", &defaults, cfg.link.unwrap_or_default())?;
    let mark = lower_inline(theme, "mark", &defaults, cfg.mark.unwrap_or_default())?;

    let list_cfg = cfg.list.unwrap_or_default();
    let list_common = list_cfg.common.unwrap_or_default();
    let list_unordered = lower_list(theme, "list.unordered", &defaults, &list_common, list_cfg.unordered.unwrap_or_default())?;
    let list_ordered = lower_list(theme, "list.ordered", &defaults, &list_common, list_cfg.ordered.unwrap_or_default())?;
    let list_task = lower_list(theme, "list.task", &defaults, &list_common, list_cfg.task.unwrap_or_default())?;

    let table_cfg = cfg.table.unwrap_or_default();
    let table = ResolvedTable {
        header: lower_block(theme, "table.header", &defaults, table_cfg.header.unwrap_or_default())?,
        cell: lower_block(theme, "table.cell", &defaults, table_cfg.cell.unwrap_or_default())?,
        border: lower_border(table_cfg.border.unwrap_or_default()),
        alternating_row_background: table_cfg.alternating_row_background,
        cell_padding: table_cfg
            .cell_padding
            .unwrap_or_else(|| Sides::uniform(0.0)),
        row_gap_pt: table_cfg.row_gap_pt.unwrap_or(0.0),
        margin_before_pt: table_cfg.margin_before_pt.unwrap_or(0.0),
        margin_after_pt: table_cfg.margin_after_pt.unwrap_or(0.0),
    };

    let image_cfg = cfg.image.unwrap_or_default();
    let image = ResolvedImage {
        max_width_pct: image_cfg.max_width_pct.unwrap_or(100.0),
        align: image_cfg.align.unwrap_or(ImageAlign::Center),
        margin_before_pt: image_cfg.margin_before_pt.unwrap_or(0.0),
        margin_after_pt: image_cfg.margin_after_pt.unwrap_or(0.0),
    };

    let rule_cfg = cfg.horizontal_rule.unwrap_or_default();
    let horizontal_rule = ResolvedRule {
        color: rule_cfg.color.unwrap_or(Color::rgb(128, 128, 128)),
        thickness_pt: rule_cfg.thickness_pt.unwrap_or(0.5),
        style: rule_cfg.style.unwrap_or(BorderStyle::Solid),
        width_pct: rule_cfg.width_pct.unwrap_or(100.0),
        margin_before_pt: rule_cfg.margin_before_pt.unwrap_or(0.0),
        margin_after_pt: rule_cfg.margin_after_pt.unwrap_or(0.0),
    };

    let math_cfg = cfg.math.unwrap_or_default();
    let math = ResolvedMath {
        align: math_cfg.align.unwrap_or(TextAlignment::Center),
        scale: math_cfg.scale.unwrap_or(1.08).max(0.05),
        color: math_cfg.color.unwrap_or(paragraph.text_color),
        // Default to the paragraph's block spacing so a display
        // equation sits like a normal paragraph unless overridden.
        margin_before_pt: math_cfg
            .margin_before_pt
            .unwrap_or(paragraph.margin_before_pt),
        margin_after_pt: math_cfg
            .margin_after_pt
            .unwrap_or(paragraph.margin_after_pt),
    };

    let metadata_cfg = cfg.metadata.unwrap_or_default();
    let metadata = ResolvedMetadata {
        title: metadata_cfg.title,
        author: metadata_cfg.author,
        subject: metadata_cfg.subject,
        keywords: metadata_cfg.keywords.unwrap_or_default(),
        creator: metadata_cfg.creator,
        language: metadata_cfg.language,
    };

    let header = lower_furniture(theme, "header", &defaults, cfg.header)?;
    let footer = lower_furniture(theme, "footer", &defaults, cfg.footer)?;
    let title_page = lower_title_page(theme, &defaults, cfg.title_page)?;
    let toc = lower_toc(theme, &defaults, cfg.toc)?;
    let fallback_fonts = defaults.fallback_fonts.clone().unwrap_or_default();

    Ok(ResolvedStyle {
        page,
        headings: [h1, h2, h3, h4, h5, h6],
        paragraph,
        code_block,
        code_inline,
        blockquote,
        admonition,
        list_ordered,
        list_unordered,
        list_task,
        table,
        image,
        link,
        mark,
        horizontal_rule,
        math,
        metadata,
        header,
        footer,
        title_page,
        toc,
        fallback_fonts,
    })
}

/// Clamp a font size to a finite, strictly-positive value. A
/// non-positive or non-finite size makes glyph advances zero or NaN,
/// so the greedy line-wrap loop never makes forward progress and the
/// renderer hangs. Applied to both block and inline lowering so a
/// hostile config can never reach layout with a poisoned size.
fn safe_font_size(pt: f32) -> f32 {
    if pt.is_finite() && pt > 0.0 {
        pt.min(1000.0)
    } else {
        1.0
    }
}

/// Line height is a multiple of the font size. Non-finite or
/// non-positive collapses vertical advance (the page cursor stalls);
/// an enormous value (e.g. `1e30`) overflows the f32 page math into
/// inf/NaN. Clamp into a sane, renderable range.
fn safe_line_height(lh: f32) -> f32 {
    if lh.is_finite() && lh > 0.0 {
        lh.min(100.0)
    } else {
        0.1
    }
}

/// Letter spacing (tracking). Negative tracking is a legitimate
/// typographic choice, so any finite value — including a negative
/// one — is preserved. Only a non-finite value (NaN / inf), which
/// would poison every layout comparison, is neutralised to zero.
fn safe_letter_spacing(pt: f32) -> f32 {
    if pt.is_finite() { pt } else { 0.0 }
}

/// Built-in accent palette used when neither the theme nor the user
/// has supplied a colour for a given kind. Keeps the renderer's
/// output recognisable even on a sparse custom theme.
fn builtin_accent(kind: &str) -> Color {
    match kind {
        "note" => Color::rgb(0x44, 0x8A, 0xFF),
        "info" => Color::rgb(0x00, 0xB8, 0xD4),
        "tip" => Color::rgb(0x00, 0xC8, 0x53),
        "warning" => Color::rgb(0xFF, 0xAB, 0x00),
        "danger" => Color::rgb(0xFF, 0x17, 0x44),
        _ => Color::rgb(0x75, 0x75, 0x75),
    }
}

fn builtin_label(kind: &str) -> String {
    match kind {
        "note" => "NOTE",
        "info" => "INFO",
        "tip" => "TIP",
        "warning" => "WARNING",
        "danger" => "DANGER",
        _ => "NOTE",
    }
    .to_string()
}

fn lower_admonition(
    theme: &str,
    defaults: &BlockConfig,
    raw: AdmonitionConfig,
) -> Result<ResolvedAdmonition, ResolveError> {
    // The [admonition] block carries the shared shape. Per-kind blocks
    // inherit it before falling back to the document [defaults].
    let shared_defaults = merge_block(defaults.clone(), raw.defaults);
    let lower_kind = |kind: &str, raw_kind: Option<AdmonitionKindConfig>| {
        lower_admonition_kind(theme, kind, &shared_defaults, raw_kind.unwrap_or_default())
    };
    Ok(ResolvedAdmonition {
        note: lower_kind("note", raw.note)?,
        info: lower_kind("info", raw.info)?,
        tip: lower_kind("tip", raw.tip)?,
        warning: lower_kind("warning", raw.warning)?,
        danger: lower_kind("danger", raw.danger)?,
        generic: lower_kind("generic", raw.generic)?,
    })
}

fn lower_admonition_kind(
    theme: &str,
    kind: &str,
    shared_defaults: &BlockConfig,
    raw: AdmonitionKindConfig,
) -> Result<ResolvedAdmonitionKind, ResolveError> {
    let where_ = format!("admonition.{}", kind);
    let block = lower_block(theme, &where_, shared_defaults, raw.block)?;
    let accent_color = raw.accent_color.unwrap_or_else(|| builtin_accent(kind));
    let label = raw.label.unwrap_or_else(|| builtin_label(kind));
    Ok(ResolvedAdmonitionKind {
        block,
        accent_color,
        label,
    })
}

fn lower_block(
    theme: &str,
    where_: &str,
    defaults: &BlockConfig,
    raw: BlockConfig,
) -> Result<ResolvedBlock, ResolveError> {
    let merged = merge_block(defaults.clone(), raw);
    let font_size_pt = merged
        .font_size_pt
        .ok_or_else(|| missing(theme, &format!("{}.font_size_pt", where_)))?;
    // Hostile / mistaken config must never crash or hang the renderer.
    // A non-positive or enormous font size / line height makes glyph
    // advances zero or overflows the f32 page math, so the wrap and
    // page-break loops never progress. Clamp into a renderable range
    // (graceful degradation; the output is still a valid, if ugly,
    // PDF). `letter_spacing_pt` keeps negative values (legitimate
    // tracking) but drops non-finite ones.
    let font_size_pt = safe_font_size(font_size_pt);
    let line_height = safe_line_height(merged.line_height.unwrap_or(1.4));
    let clamp_nonneg = |v: f32| if v.is_finite() && v > 0.0 { v } else { 0.0 };
    let pad = merged.padding.unwrap_or_else(|| Sides::uniform(0.0));
    let padding = Sides {
        top: clamp_nonneg(pad.top),
        right: clamp_nonneg(pad.right),
        bottom: clamp_nonneg(pad.bottom),
        left: clamp_nonneg(pad.left),
    };
    Ok(ResolvedBlock {
        font_family: merged.font_family,
        font_size_pt,
        font_weight: merged.font_weight.unwrap_or(FontWeight::Normal),
        font_style: merged.font_style.unwrap_or(FontStyleVariant::Normal),
        text_color: merged.text_color.unwrap_or(Color::rgb(0, 0, 0)),
        background_color: merged.background_color,
        line_height,
        text_align: merged.text_align.unwrap_or(TextAlignment::Left),
        border: lower_border(merged.border.unwrap_or_default()),
        padding,
        margin_before_pt: clamp_nonneg(merged.margin_before_pt.unwrap_or(0.0)),
        margin_after_pt: clamp_nonneg(merged.margin_after_pt.unwrap_or(0.0)),
        indent_pt: clamp_nonneg(merged.indent_pt.unwrap_or(0.0)),
        letter_spacing_pt: safe_letter_spacing(merged.letter_spacing_pt.unwrap_or(0.0)),
        strikethrough: merged.strikethrough.unwrap_or(false),
        underline: merged.underline.unwrap_or(false),
        small_caps: merged.small_caps.unwrap_or(false),
    })
}

fn lower_inline(
    theme: &str,
    where_: &str,
    defaults: &BlockConfig,
    raw: InlineConfig,
) -> Result<ResolvedInline, ResolveError> {
    // Inline merges only the field subset they share with defaults.
    let font_size_pt = raw
        .font_size_pt
        .or(defaults.font_size_pt)
        .ok_or_else(|| missing(theme, &format!("{}.font_size_pt", where_)))?;
    let font_size_pt = safe_font_size(font_size_pt);
    Ok(ResolvedInline {
        font_family: raw.font_family.or_else(|| defaults.font_family.clone()),
        font_size_pt,
        font_weight: raw
            .font_weight
            .or(defaults.font_weight)
            .unwrap_or(FontWeight::Normal),
        font_style: raw
            .font_style
            .or(defaults.font_style)
            .unwrap_or(FontStyleVariant::Normal),
        text_color: raw
            .text_color
            .or(defaults.text_color)
            .unwrap_or(Color::rgb(0, 0, 0)),
        background_color: raw.background_color.or(defaults.background_color),
        padding: raw.padding.unwrap_or_else(|| Sides::uniform(0.0)),
        strikethrough: raw
            .strikethrough
            .or(defaults.strikethrough)
            .unwrap_or(false),
        underline: raw.underline.or(defaults.underline).unwrap_or(false),
    })
}

fn lower_list(
    theme: &str,
    where_: &str,
    defaults: &BlockConfig,
    common: &ListStyleConfig,
    raw: ListStyleConfig,
) -> Result<ResolvedList, ResolveError> {
    // common is the inner-list default that cascades to every flavor.
    let merged_block = merge_block(defaults.clone(), merge_block(common.block.clone(), raw.block));
    let block = lower_block(theme, where_, &BlockConfig::default(), merged_block)?;
    Ok(ResolvedList {
        block,
        bullet: raw
            .bullet
            .or_else(|| common.bullet.clone())
            .unwrap_or_else(|| "•".to_string()),
        indent_per_level_pt: raw
            .indent_per_level_pt
            .or(common.indent_per_level_pt)
            .unwrap_or(17.0),
        item_spacing_tight_pt: raw
            .item_spacing_tight_pt
            .or(common.item_spacing_tight_pt)
            .unwrap_or(0.5),
        item_spacing_loose_pt: raw
            .item_spacing_loose_pt
            .or(common.item_spacing_loose_pt)
            .unwrap_or(2.0),
    })
}

fn lower_border(raw: BorderConfig) -> ResolvedBorder {
    let from_all = raw.all.map(lower_border_side);
    ResolvedBorder {
        top: raw.top.map(lower_border_side).or(from_all),
        right: raw.right.map(lower_border_side).or(from_all),
        bottom: raw.bottom.map(lower_border_side).or(from_all),
        left: raw.left.map(lower_border_side).or(from_all),
    }
}

fn lower_border_side(raw: BorderSide) -> ResolvedBorderSide {
    ResolvedBorderSide {
        width_pt: raw.width_pt,
        color: raw.color,
        style: raw.style,
    }
}

fn lower_furniture(
    theme: &str,
    where_: &str,
    defaults: &BlockConfig,
    raw: Option<PageFurnitureConfig>,
) -> Result<Option<ResolvedPageFurniture>, ResolveError> {
    let Some(raw) = raw else { return Ok(None) };
    let style = lower_block(theme, where_, defaults, raw.style.unwrap_or_default())?;
    Ok(Some(ResolvedPageFurniture {
        left: raw.left,
        center: raw.center,
        right: raw.right,
        style,
        show_on_first_page: raw.show_on_first_page.unwrap_or(true),
        gap_pt: raw.gap_pt.unwrap_or(14.0),
    }))
}

fn lower_title_page(
    theme: &str,
    defaults: &BlockConfig,
    raw: Option<TitlePageConfig>,
) -> Result<Option<ResolvedTitlePage>, ResolveError> {
    let Some(raw) = raw else { return Ok(None) };
    let Some(title) = raw.title else { return Ok(None) };
    let style = lower_block(theme, "title_page", defaults, raw.style.unwrap_or_default())?;
    Ok(Some(ResolvedTitlePage {
        title,
        subtitle: raw.subtitle,
        author: raw.author,
        date: raw.date,
        cover_image_path: raw.cover_image_path,
        style,
    }))
}

fn lower_toc(
    theme: &str,
    defaults: &BlockConfig,
    raw: Option<TocConfig>,
) -> Result<Option<ResolvedToc>, ResolveError> {
    let Some(raw) = raw else { return Ok(None) };
    if !raw.enabled.unwrap_or(false) {
        return Ok(None);
    }
    let style = lower_block(theme, "toc", defaults, raw.style.unwrap_or_default())?;
    Ok(Some(ResolvedToc {
        title: raw.title.unwrap_or_else(|| "Contents".to_string()),
        max_depth: raw.max_depth.unwrap_or(3),
        style,
    }))
}

fn missing(theme: &str, field: &str) -> ResolveError {
    ResolveError::PresetIncomplete {
        theme: theme.to_string(),
        missing_field: field.to_string(),
    }
}
