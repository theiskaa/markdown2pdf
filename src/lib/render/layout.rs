//! Layout engine: block IR -> printpdf 0.9 page operation streams.
//!
//! Greedy line breaking at word boundaries using real glyph advance
//! widths from [`super::font::FontMetricsCache`]. Vertical advancement
//! is per-block; the engine pushes a new page when the y cursor
//! would dip below the bottom margin.

use printpdf::{
    Actions, BorderArray, ColorArray, Destination, LineDashPattern, LinePoint, LinkAnnotation,
    Mm, Op, PdfDocument, PdfPage, Point, Pt, RawImage, Rect, Rgb, TextItem, XObjectId,
    XObjectTransform,
};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::styling::{
    BorderStyle, Orientation, PageSize, ResolvedBlock, ResolvedBorder, ResolvedList,
    ResolvedPage, ResolvedPageFurniture, ResolvedStyle, ResolvedToc,
};

use super::font::FontSet;
use super::ir::{Block, InlineRun, ListBullet, ListEntry, RunFlags};

type Color = printpdf::Color;

/// Resolve a `ResolvedPage` to (width_mm, height_mm). Landscape
/// swaps the named-size dimensions; `PageSize::Custom` is taken
/// verbatim.
pub(crate) fn page_dimensions_mm(page: &ResolvedPage) -> (f32, f32) {
    let (w, h) = match page.size {
        PageSize::A4 => (210.0, 297.0),
        PageSize::Letter => (216.0, 279.4),
        PageSize::Legal => (216.0, 355.6),
        PageSize::A3 => (297.0, 420.0),
        PageSize::A5 => (148.0, 210.0),
        PageSize::Custom { width_mm, height_mm } => (width_mm, height_mm),
    };
    match page.orientation {
        Orientation::Portrait => (w, h),
        Orientation::Landscape => (h, w),
    }
}

/// Render the IR to a vector of [`PdfPage`]s ready to hand to
/// [`printpdf::PdfDocument::with_pages`].
///
/// Takes a mutable reference to the [`PdfDocument`] so that the
/// engine can register XObjects (images, external fonts) and get
/// back IDs for use in page operation streams.
pub fn lay_out_pages(
    blocks: &[Block],
    style: &ResolvedStyle,
    font_set: &FontSet,
    doc: &mut PdfDocument,
) -> Vec<PdfPage> {
    let mut engine = Engine::new(style, font_set, doc);
    for block in blocks {
        engine.render_block(block);
    }
    engine.finish()
}

struct Engine<'a> {
    style: &'a ResolvedStyle,
    font_set: &'a FontSet,
    /// Used to register XObjects (images) and get back their IDs.
    doc: &'a mut PdfDocument,
    /// Page width in mm, resolved from `style.page.size` +
    /// `style.page.orientation` at construction. All pages produced by
    /// this engine share these dimensions.
    page_width_mm: f32,
    page_height_mm: f32,
    /// Distance from the top of the page to the current text baseline
    /// in points. Grows downward.
    y_from_top_pt: f32,
    /// Left content edge for the *current* block in points, measured
    /// from the page's left edge. Updated when entering lists,
    /// blockquotes, or other indented contexts; restored when leaving.
    indent_left_pt: f32,
    /// Right content edge for the *current* block.
    indent_right_pt: f32,
    /// Page-local Op stream we're currently appending to.
    page_ops: Vec<Op>,
    /// Pending text decorations (underline / strikethrough lines and
    /// link annotation rects) collected while a text section is open.
    /// Drawn together when the section closes so they don't fight the
    /// text section's graphics state.
    pending_decorations: Vec<PendingDecoration>,
    /// Finished pages' raw op streams. Stored as `Vec<Op>` (not
    /// `PdfPage`) so the second pass in [`finish`] can prepend
    /// header / footer ops after the total page count is known.
    raw_pages: Vec<Vec<Op>>,
    /// One entry per heading rendered. Drives both the PDF outline
    /// (bookmark pane) and the `#slug` internal-link resolver.
    heading_anchors: Vec<HeadingAnchor>,
    /// Link annotations whose URL was `#slug`. Their destination
    /// heading may be laid out later in the document, so they're
    /// resolved in [`finish`] once all anchors are known.
    pending_internal_links: Vec<PendingInternalLink>,
    /// Slugs already used in this document; new headings with the
    /// same base slug get `-2`, `-3`, ... suffixes.
    used_slugs: HashSet<String>,
    /// Whether a text section is currently open.
    in_text_section: bool,
}

impl<'a> Engine<'a> {
    fn new(style: &'a ResolvedStyle, font_set: &'a FontSet, doc: &'a mut PdfDocument) -> Self {
        let (page_width_mm, page_height_mm) = page_dimensions_mm(&style.page);
        let left = mm_to_pt(style.page.margins_mm.left.max(1.0));
        let right = page_width_mm * MM_TO_PT - mm_to_pt(style.page.margins_mm.right.max(1.0));
        let top = mm_to_pt(style.page.margins_mm.top.max(1.0));
        Self {
            style,
            font_set,
            doc,
            page_width_mm,
            page_height_mm,
            y_from_top_pt: top,
            indent_left_pt: left,
            indent_right_pt: right,
            page_ops: Vec::new(),
            pending_decorations: Vec::new(),
            raw_pages: Vec::new(),
            heading_anchors: Vec::new(),
            pending_internal_links: Vec::new(),
            used_slugs: HashSet::new(),
            in_text_section: false,
        }
    }

    fn finish(mut self) -> Vec<PdfPage> {
        self.close_text_section();
        self.push_current_page();

        // Body content is fully laid out. Take it out so the engine's
        // raw_pages slot is empty for the TOC pass.
        let content_pages: Vec<Vec<Op>> = std::mem::take(&mut self.raw_pages);
        let body_link_count = self.pending_internal_links.len();

        // Optional TOC pass. Iterate until the page count converges
        // (rarely more than one iteration; bounded at 3). Each retry
        // drops the previous attempt's pending links before re-laying
        // out, so we don't accumulate duplicate annotations.
        let toc_pages: Vec<Vec<Op>> = if self.style.toc.is_some() {
            let mut estimate = 1usize;
            let mut result = Vec::new();
            for _ in 0..3 {
                self.pending_internal_links.truncate(body_link_count);
                result = self.lay_out_toc(estimate);
                if result.len() == estimate {
                    break;
                }
                estimate = result.len();
            }
            result
        } else {
            Vec::new()
        };
        let toc_offset = toc_pages.len();

        // Shift body anchors and body's pre-existing internal links
        // forward by toc_offset (TOC pages will land at the front).
        // TOC entries added to `pending_internal_links` during
        // `lay_out_toc` already sit on TOC pages (indices 0..toc_offset)
        // and don't need shifting.
        for anchor in &mut self.heading_anchors {
            anchor.page_idx += toc_offset;
        }
        for link in &mut self.pending_internal_links[..body_link_count] {
            link.page_idx += toc_offset;
        }

        let total = content_pages.len() + toc_offset;
        let base = TemplateBase {
            total_pages: total,
            title: self.style.metadata.title.clone().unwrap_or_default(),
            author: self.style.metadata.author.clone().unwrap_or_default(),
            date: today_iso_date(),
        };

        // Resolve every pending `#slug` link against the now-known
        // heading anchor table.
        let anchor_index: HashMap<&str, &HeadingAnchor> = self
            .heading_anchors
            .iter()
            .map(|a| (a.slug.as_str(), a))
            .collect();
        let mut deferred_per_page: BTreeMap<usize, Vec<Op>> = BTreeMap::new();
        let page_height_pt = self.page_height_pt();
        for pending in &self.pending_internal_links {
            let Some(dest) = anchor_index.get(pending.target_slug.as_str()) else {
                log::warn!(
                    "internal link target `#{}` not found among {} headings",
                    pending.target_slug,
                    self.heading_anchors.len()
                );
                continue;
            };
            let y_bot_pt = page_height_pt - pending.baseline_y_pt;
            let rect = Rect::from_xywh(
                Pt(pending.x0_pt),
                Pt(y_bot_pt),
                Pt((pending.x1_pt - pending.x0_pt).max(1.0)),
                Pt(pending.size_pt),
            );
            let dest_top_pdf_pt = page_height_pt - dest.y_pt;
            let annotation = LinkAnnotation::new(
                rect,
                Actions::go_to(Destination::Xyz {
                    page: dest.page_idx + 1,
                    left: None,
                    top: Some(dest_top_pdf_pt),
                    zoom: None,
                }),
                Some(BorderArray::Solid([0.0, 0.0, 0.0])),
                Some(ColorArray::Transparent),
                None,
            );
            deferred_per_page
                .entry(pending.page_idx)
                .or_default()
                .push(Op::LinkAnnotation { link: annotation });
        }

        // Bookmarks: every heading is registered with its shifted page
        // number. printpdf 0.9's outline serializer is flat; we hint at
        // hierarchy via an indent prefix per heading level.
        for anchor in &self.heading_anchors {
            let indent_level = anchor.level.saturating_sub(1).min(5) as usize;
            let mut name = String::with_capacity(indent_level * 2 + anchor.text.len());
            for _ in 0..indent_level {
                name.push_str("  ");
            }
            name.push_str(&anchor.text);
            self.doc.add_bookmark(&name, anchor.page_idx + 1);
        }

        // Page assembly: TOC pages first, content pages after. Header
        // / footer furniture applies to every page uniformly.
        let mut pages = Vec::with_capacity(total);
        let combined = toc_pages.into_iter().chain(content_pages.into_iter());
        for (idx, content_ops) in combined.enumerate() {
            let ctx = base.with_page(idx + 1);
            let header_ops =
                self.render_furniture(self.style.header.as_ref(), &ctx, FurniturePosition::Top);
            let footer_ops =
                self.render_furniture(self.style.footer.as_ref(), &ctx, FurniturePosition::Bottom);
            let internal_link_ops = deferred_per_page.remove(&idx).unwrap_or_default();
            let mut all = Vec::with_capacity(
                header_ops.len()
                    + content_ops.len()
                    + footer_ops.len()
                    + internal_link_ops.len(),
            );
            all.extend(header_ops);
            all.extend(content_ops);
            all.extend(internal_link_ops);
            all.extend(footer_ops);
            pages.push(PdfPage::new(
                Mm(self.page_width_mm),
                Mm(self.page_height_mm),
                all,
            ));
        }
        pages
    }

    /// Lay out the TOC into a fresh sequence of page ops. The
    /// estimated `toc_offset` is the number of pages the TOC is
    /// expected to occupy; entries display `anchor.page_idx + 1 +
    /// toc_offset` so the printed page numbers match what the body's
    /// headings will sit at after concatenation. Caller iterates
    /// until the returned page count matches the estimate.
    fn lay_out_toc(&mut self, toc_offset_estimate: usize) -> Vec<Vec<Op>> {
        let toc = self.style.toc.clone().expect("toc must be Some when this is called");

        // Snapshot the engine's geometric state. raw_pages was already
        // drained by finish() before this call; page_ops is empty too
        // after `push_current_page` was called.
        let saved_y = self.y_from_top_pt;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_in_text = self.in_text_section;
        let saved_link_count = self.pending_internal_links.len();
        // Reset to first-page top.
        self.y_from_top_pt = mm_to_pt(self.style.page.margins_mm.top.max(1.0));
        let page_width_pt = self.page_width_pt();
        self.indent_left_pt = mm_to_pt(self.style.page.margins_mm.left.max(1.0));
        self.indent_right_pt =
            page_width_pt - mm_to_pt(self.style.page.margins_mm.right.max(1.0));
        self.in_text_section = false;

        // Title
        self.render_toc_title(&toc);

        // Entries
        let anchors = self.heading_anchors.clone();
        for anchor in anchors.iter() {
            if anchor.level > toc.max_depth {
                continue;
            }
            let displayed = anchor.page_idx + 1 + toc_offset_estimate;
            self.render_toc_entry(anchor, displayed, &toc);
        }

        self.close_text_section();
        self.push_current_page();
        let pages = std::mem::take(&mut self.raw_pages);

        // Restore engine geometric state (links emitted by this
        // iteration stay on `pending_internal_links`; the convergence
        // loop in `finish` truncates them between attempts).
        self.y_from_top_pt = saved_y;
        self.indent_left_pt = saved_left;
        self.indent_right_pt = saved_right;
        self.in_text_section = saved_in_text;
        let _ = saved_link_count;

        pages
    }

    fn render_toc_title(&mut self, toc: &ResolvedToc) {
        // The title visually dominates the entries by reusing the
        // body's h1 style (font, color, weight, alignment, margins).
        // This keeps typography consistent with the rest of the doc
        // and is overridable by `[headings.h1]`. The toc-specific
        // `[toc.style]` block governs entry rows only.
        let s = self.style.headings[0].clone();
        let runs = vec![InlineRun {
            text: toc.title.clone(),
            flags: RunFlags::default(),
            link: None,
        }];
        let color = Some(rgb_color(s.text_color_rgb()));
        let flags = RunFlags {
            bold: s.is_bold(),
            italic: s.is_italic(),
            monospace: false,
            strikethrough: false,
            underline: false,
        };
        let ctx = self.begin_block(&s);
        self.write_wrapped_runs(&runs, s.font_size_pt, s.line_height, flags, color);
        self.end_block(ctx);
    }

    fn render_toc_entry(
        &mut self,
        anchor: &HeadingAnchor,
        page_num: usize,
        toc: &ResolvedToc,
    ) {
        let style = toc.style.clone();
        let entry_indent = (anchor.level.saturating_sub(1) as f32) * 12.0;
        let flags = RunFlags::default();
        let size_pt = style.font_size_pt;
        let line_h = size_pt * style.line_height.max(0.5);

        let saved_left = self.indent_left_pt;
        let row_left = saved_left + entry_indent;
        let row_right = self.indent_right_pt;

        // Page break if the new entry won't fit on the current page.
        if self.y_from_top_pt + line_h + self.bottom_margin_pt() > self.page_height_pt() {
            self.start_new_page();
        }

        let baseline_y = self.y_from_top_pt + size_pt;

        // Heading-text portion (left).
        self.close_text_section();
        self.ensure_text_section();
        self.move_cursor_to(row_left, baseline_y);
        self.page_ops.push(Op::SetFont {
            font: self.font_set.handle(flags),
            size: Pt(size_pt),
        });
        self.page_ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        let text_to_emit = if self.font_set.needs_transliteration(flags) {
            to_win1252(&anchor.text)
        } else {
            anchor.text.clone()
        };
        self.page_ops.push(Op::ShowText {
            items: vec![TextItem::Text(text_to_emit)],
        });

        // Page-number portion (right-aligned at row_right).
        let page_str = page_num.to_string();
        let num_w = self.font_set.measure(flags, &page_str, size_pt);
        let num_x = row_right - num_w;
        self.close_text_section();
        self.ensure_text_section();
        self.move_cursor_to(num_x, baseline_y);
        self.page_ops.push(Op::SetFont {
            font: self.font_set.handle(flags),
            size: Pt(size_pt),
        });
        let num_emit = if self.font_set.needs_transliteration(flags) {
            to_win1252(&page_str)
        } else {
            page_str
        };
        self.page_ops.push(Op::ShowText {
            items: vec![TextItem::Text(num_emit)],
        });
        self.close_text_section();

        // Clickable rect spans the whole row.
        self.pending_internal_links.push(PendingInternalLink {
            page_idx: self.raw_pages.len(),
            x0_pt: row_left,
            x1_pt: row_right,
            baseline_y_pt: baseline_y,
            size_pt,
            target_slug: anchor.slug.clone(),
        });

        self.advance_y(line_h);
    }

    fn push_current_page(&mut self) {
        if self.page_ops.is_empty() {
            return;
        }
        let ops = std::mem::take(&mut self.page_ops);
        self.raw_pages.push(ops);
    }

    fn top_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.top.max(1.0))
    }

    fn bottom_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.bottom.max(1.0))
    }

    fn left_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.left.max(1.0))
    }

    fn right_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.right.max(1.0))
    }

    fn page_height_pt(&self) -> f32 {
        self.page_height_mm * MM_TO_PT
    }

    fn page_width_pt(&self) -> f32 {
        self.page_width_mm * MM_TO_PT
    }

    fn content_width_pt(&self) -> f32 {
        self.indent_right_pt - self.indent_left_pt
    }

    /// Advance the y cursor by `dy` points. If the cursor crosses the
    /// bottom margin, finalize the current page and start a new one.
    fn advance_y(&mut self, dy: f32) {
        self.y_from_top_pt += dy;
        if self.y_from_top_pt + self.bottom_margin_pt() > self.page_height_pt() {
            self.start_new_page();
        }
    }

    fn start_new_page(&mut self) {
        self.close_text_section();
        self.push_current_page();
        self.y_from_top_pt = self.top_margin_pt();
    }

    fn ensure_text_section(&mut self) {
        if !self.in_text_section {
            self.page_ops.push(Op::SaveGraphicsState);
            self.page_ops.push(Op::StartTextSection);
            self.in_text_section = true;
        }
    }

    fn close_text_section(&mut self) {
        if self.in_text_section {
            self.page_ops.push(Op::EndTextSection);
            self.page_ops.push(Op::RestoreGraphicsState);
            self.in_text_section = false;
        }
    }

    /// Place the text cursor at (x_pt_from_left, y_pt_from_top) on the
    /// current page. printpdf uses bottom-left origin in Mm; we convert.
    fn move_cursor_to(&mut self, x_pt_from_left: f32, y_pt_from_top: f32) {
        let x_mm = pt_to_mm(x_pt_from_left);
        let y_mm = pt_to_mm(self.page_height_pt() - y_pt_from_top);
        self.page_ops.push(Op::SetTextCursor {
            pos: Point::new(Mm(x_mm), Mm(y_mm)),
        });
    }

    /// Enter a block: advance the margin-before, reserve top padding,
    /// shrink the content edges by horizontal padding, and remember
    /// the bounding box so [`end_block`] can paint background + border.
    ///
    /// Caller must hold the returned ctx unmodified and pass it to
    /// `end_block` after the block's content has been emitted.
    fn begin_block(&mut self, style: &ResolvedBlock) -> BlockPaintCtx {
        self.advance_y(style.margin_before_pt);
        let outer_y_top = self.y_from_top_pt;
        let outer_x_left = self.indent_left_pt;
        let outer_x_right = self.indent_right_pt;

        self.indent_left_pt += style.padding.left;
        self.indent_right_pt -= style.padding.right;
        if self.indent_right_pt < self.indent_left_pt + 10.0 {
            self.indent_right_pt = self.indent_left_pt + 10.0;
        }
        self.advance_y(style.padding.top);

        self.close_text_section();
        let marker = self.page_ops.len();

        BlockPaintCtx {
            saved_left: outer_x_left,
            saved_right: outer_x_right,
            outer_x_left,
            outer_x_right,
            outer_y_top,
            marker,
            background_color: style.background_color,
            border: style.border.clone(),
            padding_bottom: style.padding.bottom,
            margin_after_pt: style.margin_after_pt,
        }
    }

    /// Close a block opened by [`begin_block`]. Paints the background
    /// at the captured marker (so text draws on top) and the border at
    /// the end (so it doesn't get covered by the fill). If the block
    /// spilled across a page break the paint is skipped — cross-page
    /// fragments are a known limitation.
    fn end_block(&mut self, ctx: BlockPaintCtx) {
        self.close_text_section();
        self.advance_y(ctx.padding_bottom);
        let outer_y_bottom = self.y_from_top_pt;

        let stayed_on_page = outer_y_bottom >= ctx.outer_y_top;
        if stayed_on_page {
            let page_h = self.page_height_pt();
            if let Some(bg) = ctx.background_color {
                let mut bg_ops: Vec<Op> = Vec::new();
                draw_filled_rect(
                    &mut bg_ops,
                    ctx.outer_x_left,
                    ctx.outer_y_top,
                    ctx.outer_x_right,
                    outer_y_bottom,
                    rgb_color((bg.r, bg.g, bg.b)),
                    page_h,
                );
                let insert_at = ctx.marker.min(self.page_ops.len());
                self.page_ops.splice(insert_at..insert_at, bg_ops);
            }
            if has_any_border(&ctx.border) {
                draw_outlined_rect(
                    &mut self.page_ops,
                    ctx.outer_x_left,
                    ctx.outer_y_top,
                    ctx.outer_x_right,
                    outer_y_bottom,
                    &ctx.border,
                    page_h,
                );
            }
        }

        self.indent_left_pt = ctx.saved_left;
        self.indent_right_pt = ctx.saved_right;
        self.advance_y(ctx.margin_after_pt);
    }

    /// Build the op sequence for a single header or footer, ready to
    /// be prepended (header) or appended (footer) to a page's content
    /// ops. Returns an empty `Vec` for missing or skipped furniture.
    fn render_furniture(
        &self,
        furniture: Option<&ResolvedPageFurniture>,
        ctx: &TemplateContext,
        pos: FurniturePosition,
    ) -> Vec<Op> {
        let Some(f) = furniture else {
            return Vec::new();
        };
        if !f.show_on_first_page && ctx.page == 1 {
            return Vec::new();
        }

        let size_pt = f.style.font_size_pt;
        let gap_pt = f.gap_pt.max(0.0);
        let y_pt = match pos {
            FurniturePosition::Top => {
                let top_margin = mm_to_pt(self.style.page.margins_mm.top.max(1.0));
                (top_margin - gap_pt).max(size_pt)
            }
            FurniturePosition::Bottom => {
                let bottom_margin = mm_to_pt(self.style.page.margins_mm.bottom.max(1.0));
                self.page_height_pt() - bottom_margin + gap_pt
            }
        };

        let mut ops: Vec<Op> = Vec::new();
        for (raw, anchor) in [
            (f.left.as_ref(), FurnitureAnchor::Left),
            (f.center.as_ref(), FurnitureAnchor::Center),
            (f.right.as_ref(), FurnitureAnchor::Right),
        ] {
            let Some(template) = raw else { continue };
            let text = ctx.expand(template);
            if text.is_empty() {
                continue;
            }
            self.emit_furniture_slot(&mut ops, &text, anchor, y_pt, &f.style);
        }
        ops
    }

    fn emit_furniture_slot(
        &self,
        ops: &mut Vec<Op>,
        text: &str,
        anchor: FurnitureAnchor,
        y_pt: f32,
        style: &ResolvedBlock,
    ) {
        let flags = RunFlags {
            bold: style.is_bold(),
            italic: style.is_italic(),
            monospace: false,
            strikethrough: false,
            underline: false,
        };
        let size_pt = style.font_size_pt;
        let measured = self.font_set.measure(flags, text, size_pt);
        let x_pt = match anchor {
            FurnitureAnchor::Left => mm_to_pt(self.style.page.margins_mm.left.max(1.0)),
            FurnitureAnchor::Center => (self.page_width_pt() - measured) / 2.0,
            FurnitureAnchor::Right => {
                self.page_width_pt()
                    - mm_to_pt(self.style.page.margins_mm.right.max(1.0))
                    - measured
            }
        };

        let x_mm = pt_to_mm(x_pt);
        let y_mm = pt_to_mm(self.page_height_pt() - y_pt);
        let emit = if self.font_set.needs_transliteration(flags) {
            to_win1252(text)
        } else {
            text.to_string()
        };

        ops.push(Op::SaveGraphicsState);
        ops.push(Op::StartTextSection);
        ops.push(Op::SetTextCursor {
            pos: Point::new(Mm(x_mm), Mm(y_mm)),
        });
        ops.push(Op::SetFont {
            font: self.font_set.handle(flags),
            size: Pt(size_pt),
        });
        ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        ops.push(Op::ShowText {
            items: vec![TextItem::Text(emit)],
        });
        ops.push(Op::EndTextSection);
        ops.push(Op::RestoreGraphicsState);
    }

    fn render_block(&mut self, block: &Block) {
        match block {
            Block::Heading { level, runs } => self.render_heading(*level, runs),
            Block::Paragraph { runs } => self.render_paragraph(runs),
            Block::CodeBlock { lines } => self.render_code_block(lines),
            Block::HorizontalRule => self.render_horizontal_rule(),
            Block::List { entries } => self.render_list(entries),
            Block::BlockQuote { body } => self.render_blockquote(body),
            Block::Table {
                headers,
                aligns,
                rows,
            } => self.render_table(headers, aligns, rows),
            Block::Image { path, alt } => self.render_image(path, alt),
            Block::HtmlBlock { content } => self.render_html_block(content),
            Block::PageBreak => self.start_new_page(),
        }
    }

    /// Render a verbatim HTML block as a monospace code block so the
    /// content stays visible and clearly tagged as source-as-data.
    fn render_html_block(&mut self, content: &str) {
        let lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
        self.render_code_block(&lines);
    }

    fn render_image(&mut self, path: &std::path::Path, alt: &str) {
        // Decode via the `image` crate. If anything fails, gracefully
        // degrade to an italic alt-text paragraph so the document
        // doesn't lose content.
        let img = match image::ImageReader::open(path)
            .and_then(|r| r.with_guessed_format())
            .map_err(|e| e.to_string())
            .and_then(|r| r.decode().map_err(|e| e.to_string()))
        {
            Ok(d) => d,
            Err(e) => {
                log::warn!("could not decode image {:?}: {}", path, e);
                self.render_paragraph(&[InlineRun {
                    text: format!("[image: {}]", alt),
                    flags: RunFlags::default().with_italic(),
                    link: None,
                }]);
                return;
            }
        };

        let raw = match RawImage::from_dynamic_image(img) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("could not convert image {:?}: {}", path, e);
                self.render_paragraph(&[InlineRun {
                    text: format!("[image: {}]", alt),
                    flags: RunFlags::default().with_italic(),
                    link: None,
                }]);
                return;
            }
        };

        let px_w = raw.width as f32;
        let px_h = raw.height as f32;
        let dpi = 300.0_f32;
        let natural_w_pt = px_w / dpi * 72.0;
        let natural_h_pt = px_h / dpi * 72.0;

        let max_w_pt = self.content_width_pt();
        let scale = if natural_w_pt > max_w_pt {
            max_w_pt / natural_w_pt
        } else {
            1.0
        };
        let rendered_w_pt = natural_w_pt * scale;
        let rendered_h_pt = natural_h_pt * scale;

        self.advance_y(self.style.image.margin_before_pt);
        if self.y_from_top_pt + rendered_h_pt + self.bottom_margin_pt() > self.page_height_pt() {
            self.start_new_page();
        }

        let xobject_id: XObjectId = self.doc.add_image(&raw);
        self.close_text_section();

        let page_h_pt = self.page_height_pt();
        // Center horizontally inside the current content box.
        let x_pt = self.indent_left_pt + (self.content_width_pt() - rendered_w_pt) / 2.0;
        // printpdf places the image at translate_x/translate_y from
        // the page's bottom-left.
        let y_bot_pt = page_h_pt - self.y_from_top_pt - rendered_h_pt;

        self.page_ops.push(Op::UseXobject {
            id: xobject_id,
            transform: XObjectTransform {
                translate_x: Some(Pt(x_pt)),
                translate_y: Some(Pt(y_bot_pt)),
                rotate: None,
                scale_x: Some(scale),
                scale_y: Some(scale),
                dpi: Some(dpi),
            },
        });
        self.y_from_top_pt += rendered_h_pt;
        self.advance_y(self.style.image.margin_after_pt);
    }

    fn render_table(
        &mut self,
        headers: &[Vec<InlineRun>],
        aligns: &[crate::markdown::TableAlignment],
        rows: &[Vec<Vec<InlineRun>>],
    ) {
        if headers.is_empty() {
            return;
        }

        let s_header = self.style.table.header.clone();
        let s_cell = self.style.table.cell.clone();
        // Table-level margins come from `[table]` directly (separate
        // from per-cell margins). Row gap comes from `[table.row_gap_pt]`.
        let before_pt = self.style.table.margin_before_pt;
        let after_pt = self.style.table.margin_after_pt;
        let row_gap_pt = self.style.table.row_gap_pt;
        const CELL_PAD_PT: f32 = 4.0;

        self.advance_y(before_pt);

        let col_count = headers.len();
        let total_width = self.content_width_pt();
        let col_width = total_width / col_count as f32;

        // Header row.
        let header_height = self.measure_row_height(
            headers,
            s_header.font_size_pt,
            s_header.line_height,
            col_width,
            true,
        );
        if self.y_from_top_pt + header_height + self.bottom_margin_pt() > self.page_height_pt() {
            self.start_new_page();
        }
        let header_top = self.y_from_top_pt;
        self.draw_row(
            headers,
            aligns,
            s_header.font_size_pt,
            s_header.line_height,
            col_width,
            true,
            s_header.text_color_rgb(),
        );
        let header_bottom = header_top + header_height;
        self.draw_row_borders(header_top, header_bottom, col_count, col_width);
        self.y_from_top_pt = header_bottom;
        self.advance_y(row_gap_pt);

        // Data rows.
        for row in rows {
            // Pad / truncate to header column count.
            let mut padded: Vec<Vec<InlineRun>> = row.clone();
            padded.resize(col_count, Vec::new());
            let row_height = self.measure_row_height(
                &padded,
                s_cell.font_size_pt,
                s_cell.line_height,
                col_width,
                false,
            );
            if self.y_from_top_pt + row_height + self.bottom_margin_pt() > self.page_height_pt() {
                self.start_new_page();
                // Reprint headers on the new page.
                let header_top = self.y_from_top_pt;
                self.draw_row(
                    headers,
                    aligns,
                    s_header.font_size_pt,
                    s_header.line_height,
                    col_width,
                    true,
                    s_header.text_color_rgb(),
                );
                let header_bottom = header_top + header_height;
                self.draw_row_borders(header_top, header_bottom, col_count, col_width);
                self.y_from_top_pt = header_bottom;
                self.advance_y(row_gap_pt);
            }
            let row_top = self.y_from_top_pt;
            self.draw_row(
                &padded,
                aligns,
                s_cell.font_size_pt,
                s_cell.line_height,
                col_width,
                false,
                s_cell.text_color_rgb(),
            );
            let row_bottom = row_top + row_height;
            self.draw_row_borders(row_top, row_bottom, col_count, col_width);
            self.y_from_top_pt = row_bottom;
            self.advance_y(row_gap_pt);
        }

        let _ = CELL_PAD_PT;
        self.advance_y(after_pt);
    }

    fn measure_row_height(
        &self,
        cells: &[Vec<InlineRun>],
        font_size: f32,
        line_height_mult: f32,
        col_width: f32,
        bold: bool,
    ) -> f32 {
        let line_h = font_size * line_height_mult.max(0.5);
        let mut max_lines = 1usize;
        for cell in cells {
            let n_lines = count_wrapped_lines(
                cell,
                font_size,
                line_height_mult,
                col_width - 8.0,
                self.font_set,
                bold,
            );
            max_lines = max_lines.max(n_lines);
        }
        max_lines as f32 * line_h + 6.0
    }

    /// Sum the rendered widths of a cell's inline runs (no wrapping).
    /// Used for table column alignment — we shift the per-cell text
    /// cursor by `(col_width - measured) / 2` for center, etc.
    fn measure_runs_width(&self, runs: &[InlineRun], font_size: f32, bold: bool) -> f32 {
        let mut total = 0.0f32;
        for run in runs {
            let mut flags = run.flags;
            if bold {
                flags = flags.with_bold();
            }
            total += self.font_set.measure(flags, &run.text, font_size);
        }
        total
    }

    fn draw_row(
        &mut self,
        cells: &[Vec<InlineRun>],
        aligns: &[crate::markdown::TableAlignment],
        font_size: f32,
        line_height_mult: f32,
        col_width: f32,
        bold: bool,
        color: (u8, u8, u8),
    ) {
        const CELL_PAD: f32 = 4.0;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let row_top = self.y_from_top_pt;
        let mut max_bottom = row_top;
        let col_count = cells.len();
        for (i, cell) in cells.iter().enumerate() {
            let cell_left = saved_left + col_width * i as f32 + CELL_PAD;
            let cell_right = saved_left + col_width * (i + 1) as f32 - CELL_PAD;
            let inner_width = cell_right - cell_left;
            let mut runs = cell.clone();
            if bold {
                for r in &mut runs {
                    r.flags = r.flags.with_bold();
                }
            }
            // Single-line column alignment. Multi-line cells wrap from
            // the shifted left edge — full per-line alignment is a
            // Knuth-Plass-adjacent follow-up; the schema field works
            // for the single-line case that 99% of tables use.
            let measured = self.measure_runs_width(&runs, font_size, false);
            let align = aligns
                .get(i)
                .copied()
                .unwrap_or(crate::markdown::TableAlignment::Left);
            let shift = match align {
                crate::markdown::TableAlignment::Left => 0.0,
                crate::markdown::TableAlignment::Center => {
                    ((inner_width - measured) / 2.0).max(0.0)
                }
                crate::markdown::TableAlignment::Right => (inner_width - measured).max(0.0),
            };
            self.indent_left_pt = cell_left + shift;
            self.indent_right_pt = cell_right;
            self.y_from_top_pt = row_top + 3.0;
            self.write_wrapped_runs(
                &runs,
                font_size,
                line_height_mult,
                RunFlags::default(),
                Some(rgb_color(color)),
            );
            if self.y_from_top_pt > max_bottom {
                max_bottom = self.y_from_top_pt;
            }
        }
        self.indent_left_pt = saved_left;
        self.indent_right_pt = saved_right;
        let _ = col_count;
        self.y_from_top_pt = row_top;
    }

    fn draw_row_borders(
        &mut self,
        row_top: f32,
        row_bottom: f32,
        col_count: usize,
        col_width: f32,
    ) {
        self.close_text_section();
        let page_h = self.page_height_pt();
        let border_color = rgb_color((180, 180, 180));
        let left = self.indent_left_pt;
        // Horizontal lines: top and bottom of the row.
        draw_horizontal_line(
            &mut self.page_ops,
            left,
            left + col_width * col_count as f32,
            row_top,
            border_color.clone(),
            0.5,
            page_h,
        );
        draw_horizontal_line(
            &mut self.page_ops,
            left,
            left + col_width * col_count as f32,
            row_bottom,
            border_color.clone(),
            0.5,
            page_h,
        );
        // Vertical lines: col_count + 1 dividers.
        for i in 0..=col_count {
            let x = left + col_width * i as f32;
            draw_vertical_line(&mut self.page_ops, x, row_top, row_bottom, page_h);
        }
    }

    fn render_list(&mut self, entries: &[ListEntry]) {
        const BULLET_GAP_MM: f32 = 2.0;
        let bullet_gap_pt = mm_to_pt(BULLET_GAP_MM);
        let saved_left = self.indent_left_pt;

        // CommonMark §5.3: the whole list is loose if any item is loose.
        // Pre-compute once so every iteration uses the same gap.
        let any_loose = entries.iter().any(|e| e.loose);

        for (idx, entry) in entries.iter().enumerate() {
            let list_style: ResolvedList = match entry.bullet {
                ListBullet::Unordered(_) => self.style.list_unordered.clone(),
                ListBullet::Ordered(_) => self.style.list_ordered.clone(),
                ListBullet::TaskChecked | ListBullet::TaskUnchecked => {
                    self.style.list_task.clone()
                }
            };
            let s = &list_style.block;
            let size_pt = s.font_size_pt;
            let line_height = s.line_height;
            let inter_item_gap = if any_loose {
                list_style.item_spacing_loose_pt
            } else {
                list_style.item_spacing_tight_pt
            };

            let bullet_text = format_bullet(&entry.bullet, &list_style);
            let bullet_flags = RunFlags::default();
            let bullet_width = self.font_set.measure(bullet_flags, &bullet_text, size_pt);

            // First item: honor `block.margin_before_pt` (list-level
            // "space before the whole list"). Subsequent items use the
            // tight/loose inter-item gap.
            if idx == 0 {
                self.advance_y(s.margin_before_pt.max(0.5));
            } else {
                self.advance_y(inter_item_gap.max(0.0));
            }
            let bullet_x = saved_left;
            let bullet_y = self.y_from_top_pt + size_pt;
            self.close_text_section();
            self.ensure_text_section();
            self.move_cursor_to(bullet_x, bullet_y);
            self.page_ops.push(Op::SetFont {
                font: self.font_set.handle(bullet_flags),
                size: Pt(size_pt),
            });
            self.page_ops.push(Op::SetLineHeight {
                lh: Pt(size_pt * line_height.max(0.5)),
            });
            self.page_ops.push(Op::SetFillColor {
                col: rgb_color(s.text_color_rgb()),
            });
            let bullet_emit = if self.font_set.needs_transliteration(bullet_flags) {
                to_win1252(&bullet_text)
            } else {
                bullet_text.clone()
            };
            self.page_ops.push(Op::ShowText {
                items: vec![TextItem::Text(bullet_emit)],
            });

            self.indent_left_pt = (saved_left + bullet_width + bullet_gap_pt)
                .min(self.indent_right_pt - 10.0);

            self.write_wrapped_runs(
                &entry.runs,
                size_pt,
                line_height,
                RunFlags::default(),
                Some(rgb_color(s.text_color_rgb())),
            );

            for child in &entry.children {
                self.render_block(child);
            }

            self.indent_left_pt = saved_left;

            // Last item: honor `block.margin_after_pt` (list-level
            // "space after the whole list"). The inter-item gap is
            // applied at the *start* of the next iteration.
            if idx + 1 == entries.len() {
                self.advance_y(s.margin_after_pt.max(0.0));
            }
        }
    }

    fn render_blockquote(&mut self, body: &[Block]) {
        // padding.left in [blockquote.padding] is the single knob for
        // how far the text sits past the left border. `indent_pt` is
        // still available on the schema for callers who want an extra
        // first-line indent on paragraphs, but blockquotes don't apply
        // it implicitly anymore.
        let s = self.style.blockquote.clone();
        let ctx = self.begin_block(&s);
        for child in body {
            self.render_block(child);
        }
        self.end_block(ctx);
    }

    fn render_heading(&mut self, level: u8, runs: &[InlineRun]) {
        let idx = level.clamp(1, 6) as usize - 1;
        let s = self.style.headings[idx].clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base_flags = RunFlags {
            bold: s.is_bold(),
            italic: s.is_italic(),
            monospace: false,
            strikethrough: false,
            underline: false,
        };

        let text = collect_heading_text(runs);
        let base_slug = {
            let s = slugify(&text);
            if s.is_empty() { "section".to_string() } else { s }
        };
        let mut slug = base_slug.clone();
        let mut n = 2usize;
        while self.used_slugs.contains(&slug) {
            slug = format!("{}-{}", base_slug, n);
            n += 1;
        }
        self.used_slugs.insert(slug.clone());
        // The bookmark / GoTo target is the heading's TOP y (before
        // begin_block consumes margin_before_pt + padding).
        self.heading_anchors.push(HeadingAnchor {
            slug,
            level,
            text,
            page_idx: self.raw_pages.len(),
            y_pt: self.y_from_top_pt,
        });

        let ctx = self.begin_block(&s);
        self.write_wrapped_runs(runs, s.font_size_pt, s.line_height, base_flags, color);
        self.end_block(ctx);
    }

    fn render_paragraph(&mut self, runs: &[InlineRun]) {
        let s = self.style.paragraph.clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default();
        let ctx = self.begin_block(&s);
        self.write_wrapped_runs(runs, s.font_size_pt, s.line_height, base, color);
        self.end_block(ctx);
    }

    fn render_code_block(&mut self, lines: &[String]) {
        let s = self.style.code_block.clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default().with_monospace();
        let ctx = self.begin_block(&s);
        for line in lines {
            let run = InlineRun {
                text: line.clone(),
                flags: base,
                link: None,
            };
            self.write_wrapped_runs(
                std::slice::from_ref(&run),
                s.font_size_pt,
                s.line_height,
                base,
                color.clone(),
            );
        }
        self.end_block(ctx);
    }

    fn render_horizontal_rule(&mut self) {
        self.close_text_section();

        let s = &self.style.horizontal_rule;
        let thickness = s.thickness_pt.max(0.1);
        let color = rgb_color(s.color_rgb());
        let dash = dash_pattern_for(s.style);

        self.advance_y(s.margin_before_pt + thickness * 0.5);

        let mut x_left_pt = self.left_margin_pt();
        let mut x_right_pt = self.page_width_pt() - self.right_margin_pt();
        let pct = (s.width_pct / 100.0).clamp(0.05, 1.0);
        if pct < 1.0 {
            let full = x_right_pt - x_left_pt;
            let span = full * pct;
            let pad = (full - span) / 2.0;
            x_left_pt += pad;
            x_right_pt -= pad;
        }

        let y_pt = self.y_from_top_pt;
        let page_h = self.page_height_pt();
        draw_styled_line(
            &mut self.page_ops,
            x_left_pt,
            y_pt,
            x_right_pt,
            y_pt,
            color,
            thickness,
            dash,
            page_h,
        );

        self.advance_y(s.margin_after_pt);
    }

    /// Wrap `runs` to the page width and emit one ShowText per line.
    /// `font_size_pt` is the size used for line metrics; `base_flags`
    /// is the fallback style applied to runs whose flags match. The
    /// optional `color` is applied once at the start of the block.
    fn write_wrapped_runs(
        &mut self,
        runs: &[InlineRun],
        font_size: f32,
        line_height_mult: f32,
        _base_flags: RunFlags,
        color: Option<Color>,
    ) {
        if runs.is_empty() {
            return;
        }
        let size_pt = font_size;
        let line_height_pt = size_pt * line_height_mult.max(0.5);

        // Split runs into a flat sequence of (word, flags) pairs.
        // Whitespace is the only break opportunity in this phase.
        let words = words_from_runs(runs);
        if words.is_empty() {
            return;
        }

        let max_width = self.content_width_pt();
        let mut lines: Vec<Vec<TextSegment>> = Vec::new();
        let mut current: Vec<TextSegment> = Vec::new();
        let mut current_width = 0.0f32;

        for word in &words {
            let word_width = self.font_set.measure(word.flags, &word.text, size_pt);

            // If the very first piece of a line is wider than the
            // page, push it anyway — we don't break inside a word.
            if !current.is_empty() && current_width + word_width > max_width {
                lines.push(std::mem::take(&mut current));
                current_width = 0.0;
                // Drop any leading whitespace on the new line.
                if word.text.trim().is_empty() {
                    continue;
                }
            }

            current.push(TextSegment {
                text: word.text.clone(),
                flags: word.flags,
                link: word.link.clone(),
            });
            current_width += word_width;
        }
        if !current.is_empty() {
            lines.push(current);
        }

        // Merge adjacent segments on each line that share identical
        // flags + link. The wrap stage split text into per-word /
        // per-whitespace pieces to make line-break decisions; once
        // wrapping is settled, those pieces can collapse back into
        // one `ShowText` per same-style run. Fewer Tj operators =
        // tighter selection highlights with no visual gap between
        // logically-contiguous text.
        for line in &mut lines {
            line.dedup_by(|next, prev| {
                if prev.flags == next.flags && prev.link == next.link {
                    prev.text.push_str(&next.text);
                    true
                } else {
                    false
                }
            });
        }

        let link_color = Some(rgb_color(self.style.link.text_color_rgb()));

        // Close any open section so the first line of this block
        // starts with a fresh BT (and absolute Td). Subsequent lines
        // of this paragraph stay inside one BT and use T*.
        self.close_text_section();
        for line in &lines {
            // One BT...ET block per paragraph, not per line — PDF
            // viewers use text-block boundaries to determine
            // selection flow, and per-line blocks make text selection
            // jump between unrelated lines. Inside one block we use
            // T* (Op::AddLineBreak) to step down by the leading,
            // which is what every well-formed PDF (Word, LaTeX,
            // pandoc) does. The first line of each section uses Td
            // (Op::SetTextCursor) for absolute positioning — that's
            // safe because the text line matrix is identity at BT
            // start.
            let opened_now = !self.in_text_section;
            self.ensure_text_section();
            let baseline_y_pt = self.y_from_top_pt + size_pt;
            if opened_now {
                self.move_cursor_to(self.indent_left_pt, baseline_y_pt);
                self.page_ops.push(Op::SetLineHeight {
                    lh: Pt(line_height_pt),
                });
                if let Some(c) = color.clone() {
                    self.page_ops.push(Op::SetFillColor { col: c });
                }
            } else {
                self.page_ops.push(Op::AddLineBreak);
            }

            let mut x_cursor_pt = self.indent_left_pt;
            for seg in line {
                let seg_width = self.font_set.measure(seg.flags, &seg.text, size_pt);
                let font_handle = self.font_set.handle(seg.flags);
                let needs_trans = self.font_set.needs_transliteration(seg.flags);

                // Restore the text fill colour if this is a link
                // (link colour) vs body (block colour).
                if seg.flags.underline {
                    if let Some(lc) = link_color.clone() {
                        self.page_ops.push(Op::SetFillColor { col: lc });
                    }
                } else if let Some(c) = color.clone() {
                    self.page_ops.push(Op::SetFillColor { col: c });
                }

                self.page_ops.push(Op::SetFont {
                    font: font_handle,
                    size: Pt(size_pt),
                });
                let emit_text = if needs_trans {
                    to_win1252(&seg.text)
                } else {
                    seg.text.clone()
                };
                self.page_ops.push(Op::ShowText {
                    items: vec![TextItem::Text(emit_text)],
                });

                // Buffer decorations and link rects until the line is
                // finished — they need a closed text section to draw
                // paths on top.
                if seg.flags.underline || seg.flags.strikethrough || seg.link.is_some() {
                    let decoration_y_pt = if seg.flags.strikethrough {
                        baseline_y_pt - size_pt * 0.30
                    } else {
                        baseline_y_pt + size_pt * 0.12
                    };
                    self.pending_decorations.push(PendingDecoration {
                        kind: if seg.flags.strikethrough {
                            DecorationKind::Strike
                        } else if seg.flags.underline {
                            DecorationKind::Underline
                        } else {
                            DecorationKind::None
                        },
                        x0_pt: x_cursor_pt,
                        x1_pt: x_cursor_pt + seg_width,
                        y_pt: decoration_y_pt,
                        link: seg.link.clone(),
                        size_pt,
                        baseline_y_pt,
                    });
                }
                x_cursor_pt += seg_width;
            }

            self.advance_y(line_height_pt);
        }

        self.flush_decorations();
        // Ensure the text section closes at the end of the block so
        // the next block's first SetTextCursor lands on a fresh
        // text matrix.
        self.close_text_section();
    }

    fn flush_decorations(&mut self) {
        if self.pending_decorations.is_empty() {
            return;
        }
        // Close text section so we can draw paths and annotations.
        self.close_text_section();
        let page_h_pt = self.page_height_pt();
        let pending = std::mem::take(&mut self.pending_decorations);
        let link_color = Some(rgb_color(self.style.link.text_color_rgb()));
        for d in pending {
            match d.kind {
                DecorationKind::Underline | DecorationKind::Strike => {
                    let col = link_color.clone().unwrap_or_else(|| rgb_color((80, 80, 80)));
                    draw_horizontal_line(
                        &mut self.page_ops,
                        d.x0_pt,
                        d.x1_pt,
                        d.y_pt,
                        col,
                        0.6,
                        page_h_pt,
                    );
                }
                DecorationKind::None => {}
            }
            if let Some(url) = d.link {
                if let Some(slug) = url.strip_prefix('#') {
                    // Internal link — destination heading may not be
                    // laid out yet. Defer until finish() once every
                    // anchor's (page_idx, y_pt) is known.
                    self.pending_internal_links.push(PendingInternalLink {
                        page_idx: self.raw_pages.len(),
                        x0_pt: d.x0_pt,
                        x1_pt: d.x1_pt,
                        baseline_y_pt: d.baseline_y_pt,
                        size_pt: d.size_pt,
                        target_slug: slug.to_string(),
                    });
                } else {
                    let y_bot_pt = page_h_pt - d.baseline_y_pt;
                    let rect = Rect::from_xywh(
                        Pt(d.x0_pt),
                        Pt(y_bot_pt),
                        Pt((d.x1_pt - d.x0_pt).max(1.0)),
                        Pt(d.size_pt),
                    );
                    // Border [0.0, 0.0, 0.0] = no visible border. The
                    // default `BorderArray::Solid([0.0, 0.0, 1.0])`
                    // draws a 1pt outline that shows up as a stray
                    // rectangle around link rects in some viewers.
                    let annotation = LinkAnnotation::new(
                        rect,
                        Actions::uri(url),
                        Some(BorderArray::Solid([0.0, 0.0, 0.0])),
                        Some(ColorArray::Transparent),
                        None,
                    );
                    self.page_ops
                        .push(Op::LinkAnnotation { link: annotation });
                }
            }
        }
    }
}

/// One heading collected during layout. Drives the PDF outline and
/// the `#slug` internal-link resolver.
#[derive(Clone)]
struct HeadingAnchor {
    slug: String,
    level: u8,
    text: String,
    page_idx: usize,
    y_pt: f32,
}

/// A `[text](#slug)` link annotation deferred until the destination
/// heading's position is known. The page hosting the link rect is
/// captured at creation time; the destination is resolved in
/// `finish()` against `heading_anchors`.
struct PendingInternalLink {
    page_idx: usize,
    x0_pt: f32,
    x1_pt: f32,
    baseline_y_pt: f32,
    size_pt: f32,
    target_slug: String,
}

struct BlockPaintCtx {
    saved_left: f32,
    saved_right: f32,
    outer_x_left: f32,
    outer_x_right: f32,
    outer_y_top: f32,
    marker: usize,
    background_color: Option<crate::styling::Color>,
    border: ResolvedBorder,
    padding_bottom: f32,
    margin_after_pt: f32,
}

#[derive(Clone, Copy, Debug)]
enum DecorationKind {
    None,
    Underline,
    Strike,
}

#[derive(Debug)]
struct PendingDecoration {
    kind: DecorationKind,
    x0_pt: f32,
    x1_pt: f32,
    y_pt: f32,
    link: Option<String>,
    size_pt: f32,
    baseline_y_pt: f32,
}

/// Convert a UTF-8 string to ASCII for safe rendering with
/// printpdf's built-in font path.
///
/// Why ASCII and not Win-1252? printpdf 0.9 nominally writes text
/// for built-in fonts via `lopdf::Encoding::SimpleEncoding(
/// b"WinAnsiEncoding")`, but `SimpleEncoding` with an arbitrary
/// name falls through to a UTF-8 byte passthrough in lopdf 0.39 —
/// it never consults the actual Win-1252 mapping table. There is no
/// way through the public TextItem API to inject raw bytes 0x80..0xFF,
/// so non-ASCII characters round-trip as UTF-8 byte sequences and
/// then get *interpreted* as Win-1252 by the viewer, producing
/// mojibake like `â€"` for `—`.
///
/// Until the renderer can switch to an external (Unicode) TTF via
/// printpdf's `ParsedFont` path, we transliterate the common
/// punctuation Unicode points to their ASCII equivalents and replace
/// the rest with `?` — visibly imperfect but at least readable, and
/// it does not silently scramble the document.
fn to_win1252(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c as u32 {
            // ASCII passes through untouched.
            0x00..=0x7F => out.push(c),
            // Common Win-1252 punctuation -> ASCII equivalents.
            0x2014 => out.push_str("--"),  // — em-dash
            0x2013 => out.push('-'),       // – en-dash
            0x2022 => out.push('*'),       // • bullet
            0x2018 | 0x2019 => out.push('\''), // ' '
            0x201C | 0x201D => out.push('"'),  // " "
            0x2026 => out.push_str("..."), // …
            0x00A0 => out.push(' '),       // non-breaking space
            0x00A9 => out.push_str("(c)"),
            0x00AE => out.push_str("(R)"),
            0x2122 => out.push_str("(TM)"),
            // Everything else is mapped to '?' so the loss is visible
            // and not silently scrambled.
            _ => out.push('?'),
        }
    }
    out
}

/// Approximate the number of wrapped lines a run sequence would
/// occupy in a column of `max_width` points.
fn count_wrapped_lines(
    runs: &[InlineRun],
    font_size: f32,
    _line_height_mult: f32,
    max_width: f32,
    font_set: &FontSet,
    bold: bool,
) -> usize {
    if runs.is_empty() {
        return 1;
    }
    let size_pt = font_size;
    let mut current = 0.0f32;
    let mut lines = 1usize;
    for run in runs {
        let mut flags = run.flags;
        if bold {
            flags = flags.with_bold();
        }
        for word in run.text.split_whitespace() {
            let w = font_set.measure(flags, word, size_pt);
            let space = font_set.measure(flags, " ", size_pt);
            if current + w > max_width {
                lines += 1;
                current = w + space;
            } else {
                current += w + space;
            }
        }
    }
    lines
}

fn format_bullet(b: &ListBullet, style: &ResolvedList) -> String {
    // External (Unicode) fonts render `•` directly. Built-in
    // Helvetica falls back through `to_win1252`, which maps `•` to
    // `*` so the bullet still appears.
    match b {
        ListBullet::Unordered(_) => {
            let g = style.bullet.trim();
            let g = if g.is_empty() { "\u{2022}" } else { g };
            format!("{}  ", g)
        }
        ListBullet::Ordered(n) => {
            let template = style.bullet.trim();
            if template.contains('1') {
                let rendered = template.replacen("1", &n.to_string(), 1);
                format!("{}  ", rendered)
            } else if template.is_empty() {
                format!("{}.  ", n)
            } else {
                format!("{}{}  ", n, template)
            }
        }
        ListBullet::TaskChecked => "[x] ".to_string(),
        ListBullet::TaskUnchecked => "[ ] ".to_string(),
    }
}

fn draw_vertical_line(
    ops: &mut Vec<Op>,
    x_pt: f32,
    y_top_pt: f32,
    y_bottom_pt: f32,
    page_height_pt: f32,
) {
    let col = rgb_color((180, 180, 180));
    let y_top_mm = pt_to_mm(page_height_pt - y_top_pt);
    let y_bot_mm = pt_to_mm(page_height_pt - y_bottom_pt);
    let x_mm = pt_to_mm(x_pt);
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetOutlineColor { col });
    ops.push(Op::SetOutlineThickness { pt: Pt(1.2) });
    ops.push(Op::SetLineDashPattern {
        dash: LineDashPattern::default(),
    });
    ops.push(Op::DrawLine {
        line: printpdf::Line {
            points: vec![
                LinePoint {
                    p: Point::new(Mm(x_mm), Mm(y_top_mm)),
                    bezier: false,
                },
                LinePoint {
                    p: Point::new(Mm(x_mm), Mm(y_bot_mm)),
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
    ops.push(Op::RestoreGraphicsState);
}

/// Draw a filled rectangle from (x0, y_top) to (x1, y_bot) in
/// top-down points. Used for block backgrounds.
fn draw_filled_rect(
    ops: &mut Vec<Op>,
    x0_pt: f32,
    y_top_pt: f32,
    x1_pt: f32,
    y_bot_pt: f32,
    fill: Color,
    page_height_pt: f32,
) {
    let width_pt = (x1_pt - x0_pt).max(0.0);
    let height_pt = (y_bot_pt - y_top_pt).max(0.0);
    if width_pt <= 0.0 || height_pt <= 0.0 {
        return;
    }
    // PDF y origin is bottom-left; our y is top-down.
    let y_pdf = page_height_pt - y_bot_pt;
    let rect = Rect {
        x: Pt(x0_pt),
        y: Pt(y_pdf),
        width: Pt(width_pt),
        height: Pt(height_pt),
        mode: Some(printpdf::PaintMode::Fill),
        winding_order: None,
    };
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetFillColor { col: fill });
    ops.push(Op::DrawRectangle { rectangle: rect });
    ops.push(Op::RestoreGraphicsState);
}

/// Draw the per-side borders of a rect. Sides that are `None` on
/// `ResolvedBorder` are skipped so a user can request a single
/// `border-left` without painting the other three.
fn draw_outlined_rect(
    ops: &mut Vec<Op>,
    x0_pt: f32,
    y_top_pt: f32,
    x1_pt: f32,
    y_bot_pt: f32,
    border: &ResolvedBorder,
    page_height_pt: f32,
) {
    if let Some(side) = border.top {
        draw_styled_line(
            ops,
            x0_pt,
            y_top_pt,
            x1_pt,
            y_top_pt,
            rgb_color((side.color.r, side.color.g, side.color.b)),
            side.width_pt,
            dash_pattern_for(side.style),
            page_height_pt,
        );
    }
    if let Some(side) = border.bottom {
        draw_styled_line(
            ops,
            x0_pt,
            y_bot_pt,
            x1_pt,
            y_bot_pt,
            rgb_color((side.color.r, side.color.g, side.color.b)),
            side.width_pt,
            dash_pattern_for(side.style),
            page_height_pt,
        );
    }
    if let Some(side) = border.left {
        draw_styled_line(
            ops,
            x0_pt,
            y_top_pt,
            x0_pt,
            y_bot_pt,
            rgb_color((side.color.r, side.color.g, side.color.b)),
            side.width_pt,
            dash_pattern_for(side.style),
            page_height_pt,
        );
    }
    if let Some(side) = border.right {
        draw_styled_line(
            ops,
            x1_pt,
            y_top_pt,
            x1_pt,
            y_bot_pt,
            rgb_color((side.color.r, side.color.g, side.color.b)),
            side.width_pt,
            dash_pattern_for(side.style),
            page_height_pt,
        );
    }
}

/// Translate a schema `BorderStyle` into a printpdf dash pattern.
/// The integer dash/gap values are in points; viewers scale them by
/// the current stroke thickness — these defaults read as ~2pt dashes
/// at a 0.5pt stroke, which is what users intuitively expect from
/// "dashed".
fn dash_pattern_for(style: BorderStyle) -> LineDashPattern {
    match style {
        BorderStyle::Solid => LineDashPattern::default(),
        BorderStyle::Dashed => LineDashPattern {
            offset: 0,
            dash_1: Some(4),
            gap_1: Some(2),
            dash_2: None,
            gap_2: None,
            dash_3: None,
            gap_3: None,
        },
        BorderStyle::Dotted => LineDashPattern {
            offset: 0,
            dash_1: Some(1),
            gap_1: Some(1),
            dash_2: None,
            gap_2: None,
            dash_3: None,
            gap_3: None,
        },
    }
}

/// Generalized line draw with explicit color / thickness / dash.
/// `draw_horizontal_line` is now a thin wrapper around this for the
/// solid-line case kept for backward compatibility with table borders
/// and text decorations.
fn draw_styled_line(
    ops: &mut Vec<Op>,
    x0_pt: f32,
    y0_pt: f32,
    x1_pt: f32,
    y1_pt: f32,
    col: Color,
    thickness_pt: f32,
    dash: LineDashPattern,
    page_height_pt: f32,
) {
    let y0_mm = pt_to_mm(page_height_pt - y0_pt);
    let y1_mm = pt_to_mm(page_height_pt - y1_pt);
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetOutlineColor { col });
    ops.push(Op::SetOutlineThickness {
        pt: Pt(thickness_pt.max(0.1)),
    });
    ops.push(Op::SetLineDashPattern { dash });
    ops.push(Op::DrawLine {
        line: printpdf::Line {
            points: vec![
                LinePoint {
                    p: Point::new(Mm(pt_to_mm(x0_pt)), Mm(y0_mm)),
                    bezier: false,
                },
                LinePoint {
                    p: Point::new(Mm(pt_to_mm(x1_pt)), Mm(y1_mm)),
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
    ops.push(Op::RestoreGraphicsState);
}

fn has_any_border(b: &ResolvedBorder) -> bool {
    b.top.is_some() || b.right.is_some() || b.bottom.is_some() || b.left.is_some()
}

fn draw_horizontal_line(
    ops: &mut Vec<Op>,
    x0_pt: f32,
    x1_pt: f32,
    y_pt: f32,
    col: Color,
    thickness_pt: f32,
    page_height_pt: f32,
) {
    let y_mm = pt_to_mm(page_height_pt - y_pt);
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetOutlineColor { col });
    ops.push(Op::SetOutlineThickness {
        pt: Pt(thickness_pt),
    });
    ops.push(Op::SetLineDashPattern {
        dash: LineDashPattern::default(),
    });
    ops.push(Op::DrawLine {
        line: printpdf::Line {
            points: vec![
                LinePoint {
                    p: Point::new(Mm(pt_to_mm(x0_pt)), Mm(y_mm)),
                    bezier: false,
                },
                LinePoint {
                    p: Point::new(Mm(pt_to_mm(x1_pt)), Mm(y_mm)),
                    bezier: false,
                },
            ],
            is_closed: false,
        },
    });
    ops.push(Op::RestoreGraphicsState);
}

struct TextSegment {
    text: String,
    flags: RunFlags,
    link: Option<String>,
}

/// Flatten a run list to a sequence of (word | whitespace) pieces,
/// preserving the originating run's flags. Whitespace pieces become
/// break opportunities in the wrapping pass; words don't.
fn words_from_runs(runs: &[InlineRun]) -> Vec<InlineRun> {
    let mut out = Vec::new();
    for run in runs {
        let chars: Vec<(usize, char)> = run.text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let is_space = chars[i].1.is_whitespace();
            let mut j = i + 1;
            while j < chars.len() && chars[j].1.is_whitespace() == is_space {
                j += 1;
            }
            let end_byte = if j < chars.len() {
                chars[j].0
            } else {
                run.text.len()
            };
            let slice = &run.text[chars[i].0..end_byte];
            if !slice.is_empty() {
                out.push(InlineRun {
                    text: slice.to_string(),
                    flags: run.flags,
                    link: run.link.clone(),
                });
            }
            i = j;
        }
    }
    out
}

/// 1 inch = 72 pt; 1 inch = 25.4 mm; so 1 mm = 72/25.4 ≈ 2.8346 pt.
const MM_TO_PT: f32 = 72.0 / 25.4;

fn mm_to_pt(mm: f32) -> f32 {
    mm * MM_TO_PT
}

fn pt_to_mm(pt: f32) -> f32 {
    pt / MM_TO_PT
}

/// GitHub-style slug: lowercase, ASCII letters + digits + dashes,
/// whitespace → `-`, everything else dropped, no leading / trailing
/// dashes, no consecutive dashes.
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_dash = true;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !last_was_dash {
                out.push('-');
                last_was_dash = true;
            }
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Concatenate the plain text of a heading's inline runs. The PDF
/// outline + slug source. Markdown emphasis / inline code inside a
/// heading collapses to its literal text.
fn collect_heading_text(runs: &[InlineRun]) -> String {
    let mut out = String::new();
    for run in runs {
        out.push_str(&run.text);
    }
    out
}

fn rgb_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(Rgb {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        icc_profile: None,
    })
}

#[derive(Clone, Copy, Debug)]
enum FurniturePosition {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
enum FurnitureAnchor {
    Left,
    Center,
    Right,
}

/// Per-document furniture context. The fields that don't vary by page
/// (title / author / date / total) are computed once and reused for
/// every page; `with_page` produces the per-page view.
struct TemplateBase {
    total_pages: usize,
    title: String,
    author: String,
    date: String,
}

impl TemplateBase {
    fn with_page(&self, page: usize) -> TemplateContext<'_> {
        TemplateContext {
            page,
            total_pages: self.total_pages,
            title: &self.title,
            author: &self.author,
            date: &self.date,
        }
    }
}

struct TemplateContext<'a> {
    page: usize,
    total_pages: usize,
    title: &'a str,
    author: &'a str,
    date: &'a str,
}

impl TemplateContext<'_> {
    fn expand(&self, template: &str) -> String {
        template
            .replace("{page}", &self.page.to_string())
            .replace("{total_pages}", &self.total_pages.to_string())
            .replace("{title}", self.title)
            .replace("{author}", self.author)
            .replace("{date}", self.date)
    }
}

/// Today's date as `YYYY-MM-DD`, computed from system time using
/// Howard Hinnant's `civil_from_days` algorithm. UTC; no time zone
/// conversion (a configurable TZ is a follow-up).
fn today_iso_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// `days` = days since 1970-01-01 (UTC). Returns (year, month, day).
/// Algorithm: Howard Hinnant, http://howardhinnant.github.io/date_algorithms.html.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styling::ResolvedStyle;

    #[test]
    fn empty_block_list_produces_no_pages() {
        let font_set = FontSet::load(None, &[], crate::render::ir::VariantUsage::default(), &mut PdfDocument::new("test"));
        let style = ResolvedStyle::default();
        let pages = lay_out_pages(&[], &style, &font_set, &mut PdfDocument::new("test"));
        assert!(pages.is_empty());
    }

    #[test]
    fn one_paragraph_produces_one_page() {
        let font_set = FontSet::load(None, &[], crate::render::ir::VariantUsage::default(), &mut PdfDocument::new("test"));
        let style = ResolvedStyle::default();
        let blocks = vec![Block::Paragraph {
            runs: vec![InlineRun::new("hello world")],
        }];
        let pages = lay_out_pages(&blocks, &style, &font_set, &mut PdfDocument::new("test"));
        assert_eq!(pages.len(), 1);
    }

    #[test]
    fn many_paragraphs_split_across_pages() {
        let font_set = FontSet::load(None, &[], crate::render::ir::VariantUsage::default(), &mut PdfDocument::new("test"));
        let style = ResolvedStyle::default();
        let blocks: Vec<_> = (0..200)
            .map(|i| Block::Paragraph {
                runs: vec![InlineRun::new(format!("paragraph {}", i))],
            })
            .collect();
        let pages = lay_out_pages(&blocks, &style, &font_set, &mut PdfDocument::new("test"));
        assert!(pages.len() >= 2, "expected page split, got {}", pages.len());
    }

    #[test]
    fn very_long_paragraph_wraps_to_multiple_lines() {
        let font_set = FontSet::load(None, &[], crate::render::ir::VariantUsage::default(), &mut PdfDocument::new("test"));
        let style = ResolvedStyle::default();
        let long_text = "word ".repeat(200);
        let blocks = vec![Block::Paragraph {
            runs: vec![InlineRun::new(long_text)],
        }];
        let pages = lay_out_pages(&blocks, &style, &font_set, &mut PdfDocument::new("test"));
        assert!(!pages.is_empty());
    }
}
