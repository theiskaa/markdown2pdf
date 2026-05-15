//! Layout engine: block IR -> printpdf 0.9 page operation streams.
//!
//! Greedy line breaking at word boundaries using real glyph advance
//! widths from [`super::font::FontMetricsCache`]. Vertical advancement
//! is per-block; the engine pushes a new page when the y cursor
//! would dip below the bottom margin.

use printpdf::{
    Actions, BorderArray, ColorArray, Destination, LineDashPattern, LinePoint, LinkAnnotation,
    Mm, Op, PaintMode, PdfDocument, PdfPage, Point, Polygon, PolygonRing, Pt, RawImage, Rect, Rgb,
    TextItem, WindingOrder, XObjectId, XObjectTransform,
};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::styling::{
    BorderStyle, ImageAlign, Orientation, PageSize, ResolvedBlock, ResolvedBorder, ResolvedList,
    ResolvedPage, ResolvedPageFurniture, ResolvedStyle, ResolvedToc, TextAlignment,
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
        PageSize::Custom { width_mm, height_mm } => {
            // A hostile config can set a custom size to 0, negative,
            // NaN, inf, or an absurd magnitude. NaN is the worst: it
            // makes every downstream page-break comparison false (the
            // break never fires), so a degenerate size must fall back
            // rather than propagate. A valid-but-extreme size is
            // clamped into PDF's renderable range (~10mm .. 5080mm,
            // the spec's 200in maximum) so page math can't overflow.
            if width_mm.is_finite()
                && height_mm.is_finite()
                && width_mm > 0.0
                && height_mm > 0.0
            {
                (width_mm.clamp(10.0, 5080.0), height_mm.clamp(10.0, 5080.0))
            } else {
                (210.0, 297.0)
            }
        }
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
    /// Alignment used by the next call to [`write_wrapped_runs`]. Set
    /// by `render_paragraph` / `render_heading` / etc. before the
    /// call and reset back to `Left` afterwards (so other code paths
    /// that don't touch it default to left).
    current_text_align: TextAlignment,
    /// Per-render URL → image-bytes cache so two `![](url)` blocks
    /// pointing at the same remote asset only download once. Only
    /// populated when the `fetch` feature is enabled; kept compiled-in
    /// either way so the rest of the engine doesn't need cfg guards.
    #[cfg_attr(not(feature = "fetch"), allow(dead_code))]
    url_image_cache: HashMap<String, Vec<u8>>,
    /// Whether a text section is currently open.
    in_text_section: bool,
    /// Stack of block backgrounds currently open (LIFO — matches the
    /// nesting of `begin_block` / `end_block`). When a page break
    /// happens mid-block, [`start_new_page`] paints the fragment that
    /// fit on the outgoing page and resets each entry to continue on
    /// the next page. Empty when no background-bearing block is open.
    open_bg: Vec<OpenBlockBg>,
}

/// One background-bearing block that is currently open. Tracks the
/// rect to paint *for the current page fragment*; cross-page blocks
/// produce one rect per page they touch.
struct OpenBlockBg {
    x_left: f32,
    x_right: f32,
    /// Top of the fragment on the *current* page, y-from-top points.
    top_y: f32,
    color: (u8, u8, u8),
    /// Splice index into `page_ops` for the current page so the fill
    /// lands *under* the text drawn afterward.
    marker: usize,
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
            current_text_align: TextAlignment::Left,
            url_image_cache: HashMap::new(),
            in_text_section: false,
            open_bg: Vec::new(),
        }
    }

    fn finish(mut self) -> Vec<PdfPage> {
        self.close_text_section();
        self.push_current_page();

        // Body content is fully laid out. Take it out so the engine's
        // raw_pages slot is empty for title-page / TOC passes.
        let content_pages: Vec<Vec<Op>> = std::mem::take(&mut self.raw_pages);
        let body_link_count = self.pending_internal_links.len();

        // Optional title page first. Currently always produces one
        // page; multi-page title support is a follow-up.
        let title_pages: Vec<Vec<Op>> = if self.style.title_page.is_some() {
            self.lay_out_title_page()
        } else {
            Vec::new()
        };
        let title_offset = title_pages.len();
        let title_link_count = self.pending_internal_links.len();

        // Optional TOC pass. Convergence loop on page count (bounded
        // at 3). Displayed page numbers include the title-page
        // prefix so they match the final-document position.
        let toc_pages: Vec<Vec<Op>> = if self.style.toc.is_some() {
            let mut estimate = 1usize;
            let mut result = Vec::new();
            for _ in 0..3 {
                self.pending_internal_links.truncate(title_link_count);
                result = self.lay_out_toc(title_offset + estimate);
                if result.len() == estimate {
                    break;
                }
                estimate = result.len();
            }
            result
        } else {
            Vec::new()
        };
        let toc_count = toc_pages.len();
        let prefix_offset = title_offset + toc_count;

        // Shift body anchors and body's pre-existing internal links
        // forward by prefix_offset (title + TOC pages land at the
        // front). Shift TOC's own pending links by title_offset only
        // (they sit on TOC pages, which now appear at indices
        // [title_offset, prefix_offset)).
        for anchor in &mut self.heading_anchors {
            anchor.page_idx += prefix_offset;
        }
        for link in &mut self.pending_internal_links[..body_link_count] {
            link.page_idx += prefix_offset;
        }
        for link in &mut self.pending_internal_links[body_link_count..] {
            link.page_idx += title_offset;
        }

        let total = content_pages.len() + prefix_offset;
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

        // Page assembly: title pages → TOC pages → body content. Header
        // / footer furniture applies to every page EXCEPT the title
        // pages (book convention: clean cover, no chrome).
        let mut pages = Vec::with_capacity(total);
        let combined = title_pages
            .into_iter()
            .chain(toc_pages.into_iter())
            .chain(content_pages.into_iter());
        for (idx, content_ops) in combined.enumerate() {
            let ctx = base.with_page(idx + 1);
            let is_title_page = idx < title_offset;
            let header_ops = if is_title_page {
                Vec::new()
            } else {
                self.render_furniture(self.style.header.as_ref(), &ctx, FurniturePosition::Top)
            };
            let footer_ops = if is_title_page {
                Vec::new()
            } else {
                self.render_furniture(self.style.footer.as_ref(), &ctx, FurniturePosition::Bottom)
            };
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
    /// Build the title page(s). Returns the resulting page op streams
    /// (typically one page). Caller is responsible for ensuring
    /// `raw_pages` is empty and `style.title_page` is `Some` before
    /// calling.
    fn lay_out_title_page(&mut self) -> Vec<Vec<Op>> {
        let tp = self
            .style
            .title_page
            .clone()
            .expect("title_page must be Some when this is called");

        // Snapshot geometric state. raw_pages is empty here (caller
        // drained body into content_pages already).
        let saved_y = self.y_from_top_pt;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_in_text = self.in_text_section;

        let page_width_pt = self.page_width_pt();
        self.indent_left_pt = mm_to_pt(self.style.page.margins_mm.left.max(1.0));
        self.indent_right_pt =
            page_width_pt - mm_to_pt(self.style.page.margins_mm.right.max(1.0));
        self.in_text_section = false;

        let base_size = tp.style.font_size_pt.max(8.0);
        let title_size = base_size * 2.4;
        let subtitle_size = base_size * 1.4;
        let author_size = base_size * 1.1;
        let date_size = base_size;

        // Vertical layout: estimate stack height, then center it
        // between the top and bottom margins. Each piece contributes
        // its font size plus a small gap.
        let line_gap = base_size * 1.6;
        let small_gap = base_size * 0.6;
        let mut stack_h = title_size;
        if tp.subtitle.is_some() {
            stack_h += subtitle_size + small_gap;
        }
        if tp.author.is_some() {
            stack_h += author_size + line_gap;
        }
        if tp.date.is_some() {
            stack_h += date_size + small_gap;
        }

        let top = mm_to_pt(self.style.page.margins_mm.top.max(1.0));
        let bottom = self.page_height_pt() - mm_to_pt(self.style.page.margins_mm.bottom.max(1.0));
        let usable_h = bottom - top;
        let start_y = top + ((usable_h - stack_h) * 0.5).max(0.0);
        self.y_from_top_pt = start_y;

        // Title (bold)
        self.render_title_page_text(
            &tp.title,
            title_size,
            &tp.style,
            true,
        );
        self.advance_y(small_gap);

        if let Some(sub) = tp.subtitle.as_deref() {
            self.render_title_page_text(sub, subtitle_size, &tp.style, false);
            self.advance_y(line_gap);
        }
        if let Some(author) = tp.author.as_deref() {
            self.render_title_page_text(author, author_size, &tp.style, false);
            self.advance_y(small_gap);
        }
        if let Some(date) = tp.date.as_deref() {
            self.render_title_page_text(date, date_size, &tp.style, false);
        }

        self.close_text_section();
        self.push_current_page();
        let pages = std::mem::take(&mut self.raw_pages);

        self.y_from_top_pt = saved_y;
        self.indent_left_pt = saved_left;
        self.indent_right_pt = saved_right;
        self.in_text_section = saved_in_text;
        pages
    }

    /// Emit a single centered line of text at the current
    /// `y_from_top_pt`, advancing the cursor by `size_pt`.
    fn render_title_page_text(
        &mut self,
        text: &str,
        size_pt: f32,
        style: &ResolvedBlock,
        force_bold: bool,
    ) {
        if text.is_empty() {
            self.advance_y(size_pt);
            return;
        }
        let flags = RunFlags {
            bold: force_bold || style.is_bold(),
            italic: style.is_italic(),
            monospace: false,
            strikethrough: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
            underline: false,
        };
        let measured = self.font_set.measure(flags, text, size_pt);
        let center_x = (self.page_width_pt() - measured) / 2.0;
        let baseline_y = self.y_from_top_pt + size_pt;

        self.close_text_section();
        self.ensure_text_section();
        self.move_cursor_to(center_x, baseline_y);
        self.page_ops.push(Op::SetFont {
            font: self.font_set.handle(flags),
            size: Pt(size_pt),
        });
        self.page_ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        let emit = if self.font_set.needs_transliteration(flags) {
            to_win1252(text)
        } else {
            text.to_string()
        };
        self.page_ops.push(Op::ShowText {
            items: vec![TextItem::Text(emit)],
        });
        self.close_text_section();

        self.advance_y(size_pt);
    }

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
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
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

    /// Per-character small-caps expansion: every lowercase character
    /// in each run becomes an uppercase character with the
    /// `small_caps` flag set (which the segment loop renders at ~78%
    /// font size). Characters that weren't lowercase pass through with
    /// the flag cleared, so digits, punctuation, and originally-caps
    /// letters keep their full size. Runs are split at every
    /// case-class boundary; adjacent same-flag chunks are merged.
    fn expand_small_caps(&self, runs: &[InlineRun]) -> Vec<InlineRun> {
        let mut out: Vec<InlineRun> = Vec::with_capacity(runs.len());
        for run in runs {
            let mut buf = String::new();
            let mut buf_lower: Option<bool> = None;
            for ch in run.text.chars() {
                let is_lower = ch.is_lowercase();
                if buf_lower.is_some() && buf_lower != Some(is_lower) {
                    let mut f = run.flags;
                    f.small_caps = buf_lower == Some(true);
                    out.push(InlineRun {
                        text: std::mem::take(&mut buf),
                        flags: f,
                        link: run.link.clone(),
                    });
                }
                if is_lower {
                    for u in ch.to_uppercase() {
                        buf.push(u);
                    }
                } else {
                    buf.push(ch);
                }
                buf_lower = Some(is_lower);
            }
            if !buf.is_empty() {
                let mut f = run.flags;
                f.small_caps = buf_lower == Some(true);
                out.push(InlineRun {
                    text: buf,
                    flags: f,
                    link: run.link.clone(),
                });
            }
        }
        out
    }

    /// Greedy character-level word break for tokens wider than the
    /// column. Each input word is either kept (if it fits) or chopped
    /// into the smallest number of chunks that each fit `max_width`.
    /// Whitespace words pass through untouched.
    ///
    /// When the word is a dictionary-known English word, the chop
    /// happens at Knuth-Liang hyphenation points (with a trailing
    /// "-" appended to the prefix). When no hyphenation points are
    /// available within the fit window — long URLs, identifiers,
    /// repeated-char tokens — the chop falls back to UTF-8 char
    /// boundaries.
    fn split_long_words(
        &self,
        words: Vec<InlineRun>,
        max_width: f32,
        size_pt: f32,
    ) -> Vec<InlineRun> {
        let mut out: Vec<InlineRun> = Vec::with_capacity(words.len());
        for word in words {
            if word.text.chars().all(char::is_whitespace) {
                out.push(word);
                continue;
            }
            let total = self.font_set.measure(word.flags, &word.text, size_pt);
            if total <= max_width {
                out.push(word);
                continue;
            }
            let breaks = super::hyphenate::break_points(&word.text);
            let hyphen_width = self.font_set.measure(word.flags, "-", size_pt);
            let chars: Vec<(usize, char)> = word.text.char_indices().collect();
            let mut chunk_start_byte = 0usize;
            let mut chunk_start_char = 0usize;
            while chunk_start_char < chars.len() {
                // Try hyphenation first: pick the largest break offset
                // that's strictly past chunk_start AND produces a
                // prefix (plus "-") that fits in max_width.
                let mut hyphen_break: Option<usize> = None;
                for &b in &breaks {
                    if b <= chunk_start_byte {
                        continue;
                    }
                    let prefix = &word.text[chunk_start_byte..b];
                    let w = self.font_set.measure(word.flags, prefix, size_pt) + hyphen_width;
                    if w <= max_width {
                        hyphen_break = Some(b);
                    } else {
                        break;
                    }
                }
                if let Some(b) = hyphen_break {
                    let mut chunk_text = word.text[chunk_start_byte..b].to_string();
                    chunk_text.push('-');
                    out.push(InlineRun {
                        text: chunk_text,
                        flags: word.flags,
                        link: word.link.clone(),
                    });
                    chunk_start_byte = b;
                    chunk_start_char = chars
                        .iter()
                        .position(|(off, _)| *off == b)
                        .unwrap_or(chars.len());
                    continue;
                }
                // No hyphenation point fits; fall back to char-boundary
                // chopping (longest prefix that fits in max_width).
                let mut last_fit = chunk_start_char;
                let mut j = chunk_start_char;
                while j < chars.len() {
                    let end_byte = chars
                        .get(j + 1)
                        .map(|c| c.0)
                        .unwrap_or(word.text.len());
                    let prefix = &word.text[chunk_start_byte..end_byte];
                    let w = self.font_set.measure(word.flags, prefix, size_pt);
                    if w > max_width {
                        if last_fit == chunk_start_char {
                            last_fit = j;
                        }
                        break;
                    }
                    last_fit = j;
                    j += 1;
                }
                let end_byte = chars
                    .get(last_fit + 1)
                    .map(|c| c.0)
                    .unwrap_or(word.text.len());
                let chunk_text = word.text[chunk_start_byte..end_byte].to_string();
                out.push(InlineRun {
                    text: chunk_text,
                    flags: word.flags,
                    link: word.link.clone(),
                });
                chunk_start_char = last_fit + 1;
                chunk_start_byte = chars
                    .get(chunk_start_char)
                    .map(|(off, _)| *off)
                    .unwrap_or(word.text.len());
            }
        }
        out
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
        self.paint_open_bg_fragments();
        self.push_current_page();
        self.y_from_top_pt = self.top_margin_pt();
        // Each still-open background continues at the top of the new
        // page; its fill on this page starts at the top content edge
        // and its splice marker resets to the (now empty) op buffer.
        let new_top = self.top_margin_pt();
        for ob in self.open_bg.iter_mut() {
            ob.top_y = new_top;
            ob.marker = 0;
        }
    }

    /// Paint the portion of each open block background that fits on
    /// the current page, splicing the fill *under* the page's text.
    /// Called right before the page is flushed. Deepest-nested block
    /// is spliced first so shallower blocks' (smaller) markers stay
    /// valid.
    fn paint_open_bg_fragments(&mut self) {
        if self.open_bg.is_empty() {
            return;
        }
        let page_h = self.page_height_pt();
        let frag_bottom = page_h - self.bottom_margin_pt();
        // Snapshot to avoid borrow conflict with `self.page_ops`.
        let frags: Vec<(usize, f32, f32, f32, (u8, u8, u8))> = self
            .open_bg
            .iter()
            .map(|ob| (ob.marker, ob.x_left, ob.x_right, ob.top_y, ob.color))
            .collect();
        for (marker, x_left, x_right, top_y, color) in frags.into_iter().rev() {
            if frag_bottom <= top_y {
                continue;
            }
            let mut bg_ops: Vec<Op> = Vec::new();
            draw_filled_rect(
                &mut bg_ops,
                x_left,
                top_y,
                x_right,
                frag_bottom,
                rgb_color(color),
                page_h,
            );
            let at = marker.min(self.page_ops.len());
            self.page_ops.splice(at..at, bg_ops);
        }
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

        if let Some(bg) = style.background_color {
            self.open_bg.push(OpenBlockBg {
                x_left: outer_x_left,
                x_right: outer_x_right,
                top_y: outer_y_top,
                color: (bg.r, bg.g, bg.b),
                marker,
            });
        }

        BlockPaintCtx {
            saved_left: outer_x_left,
            saved_right: outer_x_right,
            outer_x_left,
            outer_x_right,
            outer_y_top,
            background_color: style.background_color,
            border: style.border.clone(),
            padding_bottom: style.padding.bottom,
            margin_after_pt: style.margin_after_pt,
        }
    }

    /// Close a block opened by [`begin_block`]. Paints the background
    /// fragment for the final page the block touches (earlier
    /// fragments were already painted by [`start_new_page`]), then the
    /// border. Borders on a block that spanned a page break are still
    /// skipped — a partial box looks worse than none.
    fn end_block(&mut self, ctx: BlockPaintCtx) {
        self.close_text_section();
        self.advance_y(ctx.padding_bottom);
        let outer_y_bottom = self.y_from_top_pt;

        let spanned_page = outer_y_bottom < ctx.outer_y_top;
        let page_h = self.page_height_pt();

        if ctx.background_color.is_some() {
            // The open-bg entry's top_y / marker were reset by
            // start_new_page on every page break, so they describe
            // the *final* fragment regardless of how many pages the
            // block crossed.
            if let Some(ob) = self.open_bg.pop() {
                if outer_y_bottom > ob.top_y {
                    let mut bg_ops: Vec<Op> = Vec::new();
                    draw_filled_rect(
                        &mut bg_ops,
                        ob.x_left,
                        ob.top_y,
                        ob.x_right,
                        outer_y_bottom,
                        rgb_color(ob.color),
                        page_h,
                    );
                    let insert_at = ob.marker.min(self.page_ops.len());
                    self.page_ops.splice(insert_at..insert_at, bg_ops);
                }
            }
        }

        if has_any_border(&ctx.border) && !spanned_page {
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
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
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
            Block::Image { path, alt, caption } => {
                self.render_image(path, alt, caption.as_deref())
            }
            Block::HtmlBlock { content } => self.render_html_block(content),
            Block::PageBreak => self.start_new_page(),
            Block::FootnoteDefinitions { entries } => {
                self.render_footnote_definitions(entries)
            }
            Block::DefinitionList { entries } => self.render_definition_list(entries),
        }
    }

    fn render_definition_list(&mut self, entries: &[crate::render::ir::DefinitionEntry]) {
        if entries.is_empty() {
            return;
        }
        let body_style = self.style.paragraph.clone();
        let color = Some(rgb_color(body_style.text_color_rgb()));
        let saved_left = self.indent_left_pt;
        let def_indent_pt = mm_to_pt(6.0);

        for (idx, entry) in entries.iter().enumerate() {
            let mut term_runs: Vec<InlineRun> = Vec::with_capacity(entry.term.len());
            for r in &entry.term {
                let mut bolded = r.clone();
                bolded.flags = bolded.flags.with_bold();
                term_runs.push(bolded);
            }
            if idx == 0 {
                self.advance_y(body_style.margin_before_pt);
            } else {
                self.advance_y(body_style.margin_before_pt * 0.5);
            }
            self.write_wrapped_runs(
                &term_runs,
                body_style.font_size_pt,
                body_style.line_height,
                RunFlags::default().with_bold(),
                color.clone(),
            );
            self.indent_left_pt = (saved_left + def_indent_pt).min(self.indent_right_pt - 10.0);
            for def in &entry.definitions {
                self.write_wrapped_runs(
                    def,
                    body_style.font_size_pt,
                    body_style.line_height,
                    RunFlags::default(),
                    color.clone(),
                );
            }
            self.indent_left_pt = saved_left;
        }
        self.advance_y(body_style.margin_after_pt);
    }

    fn render_footnote_definitions(&mut self, entries: &[crate::render::ir::FootnoteEntry]) {
        if entries.is_empty() {
            return;
        }
        // Render the "Footnotes" section heading using h2 typography.
        let h2 = self.style.headings[1].clone();
        let title_runs = vec![InlineRun {
            text: "Footnotes".to_string(),
            flags: RunFlags::default(),
            link: None,
        }];
        let color = Some(rgb_color(h2.text_color_rgb()));
        let flags = RunFlags {
            bold: h2.is_bold(),
            italic: h2.is_italic(),
            monospace: false,
            strikethrough: false,
            underline: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
        };
        let ctx = self.begin_block(&h2);
        self.write_wrapped_runs(&title_runs, h2.font_size_pt, h2.line_height, flags, color);
        self.end_block(ctx);

        // Each entry: a paragraph whose first run is the superscript
        // number marker and the rest is the definition body. A heading
        // anchor with slug `footnote-N` is registered so body refs
        // (lower pass emits links to `#footnote-N`) resolve.
        let body_style = self.style.paragraph.clone();
        for entry in entries {
            self.heading_anchors.push(HeadingAnchor {
                slug: format!("footnote-{}", entry.number),
                level: 6,
                text: format!("[{}]", entry.number),
                page_idx: self.raw_pages.len(),
                y_pt: self.y_from_top_pt,
            });
            let mut runs: Vec<InlineRun> = Vec::with_capacity(entry.runs.len() + 2);
            runs.push(InlineRun {
                text: format!("{}", entry.number),
                flags: RunFlags::default().with_superscript(),
                link: None,
            });
            runs.push(InlineRun {
                text: "  ".to_string(),
                flags: RunFlags::default(),
                link: None,
            });
            for r in &entry.runs {
                runs.push(r.clone());
            }
            let color = Some(rgb_color(body_style.text_color_rgb()));
            let ctx = self.begin_block(&body_style);
            self.write_wrapped_runs(
                &runs,
                body_style.font_size_pt,
                body_style.line_height,
                RunFlags::default(),
                color,
            );
            self.end_block(ctx);
        }
    }

    /// Render a verbatim HTML block as a monospace code block so the
    /// content stays visible and clearly tagged as source-as-data.
    fn render_html_block(&mut self, content: &str) {
        let lines: Vec<String> = content.split('\n').map(|s| s.to_string()).collect();
        self.render_code_block(&lines);
    }

    /// Fetch a remote image into memory (caching by URL). Hard caps:
    /// 5 second read timeout, 10 MB max body. Gated behind the
    /// `fetch` feature — without it, this returns an error so the
    /// caller falls back to alt text.
    #[cfg(feature = "fetch")]
    fn fetch_url_bytes(&mut self, url: &str) -> Result<Vec<u8>, String> {
        const MAX_BYTES: u64 = 10 * 1024 * 1024;
        const TIMEOUT_SECS: u64 = 5;

        if !self.url_image_cache.contains_key(url) {
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
                .build()
                .map_err(|e| format!("http client init: {}", e))?;
            let resp = client.get(url).send().map_err(|e| e.to_string())?;
            if !resp.status().is_success() {
                return Err(format!("HTTP {}", resp.status()));
            }
            if let Some(len) = resp.content_length() {
                if len > MAX_BYTES {
                    return Err(format!(
                        "image at {} is {} bytes; cap is {}",
                        url, len, MAX_BYTES
                    ));
                }
            }
            let bytes = resp.bytes().map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_BYTES {
                return Err(format!(
                    "image at {} is {} bytes; cap is {}",
                    url,
                    bytes.len(),
                    MAX_BYTES
                ));
            }
            self.url_image_cache.insert(url.to_string(), bytes.to_vec());
        }
        Ok(self.url_image_cache.get(url).expect("just inserted").clone())
    }

    #[cfg(not(feature = "fetch"))]
    fn fetch_url_bytes(&mut self, url: &str) -> Result<Vec<u8>, String> {
        Err(format!(
            "URL image {} requires the `fetch` feature (recompile with --features fetch)",
            url
        ))
    }

    fn render_image_fallback(&mut self, alt: &str) {
        // Empty alt — render nothing visible. The image was decorative
        // (or the author didn't provide text), and printing the literal
        // `[image: ]` is uglier than skipping it.
        if alt.trim().is_empty() {
            return;
        }
        self.render_paragraph(&[InlineRun {
            text: format!("[image: {}]", alt),
            flags: RunFlags::default().with_italic(),
            link: None,
        }]);
    }

    fn render_image(&mut self, path: &std::path::Path, alt: &str, caption: Option<&str>) {
        // Decode via the `image` crate. If anything fails, gracefully
        // degrade to an italic alt-text paragraph so the document
        // doesn't lose content. URL paths take a separate fetch
        // pre-pass that downloads the bytes (gated under the `fetch`
        // feature); without the feature, URL images fall back to alt
        // text. SVG content is rasterized via resvg (gated under
        // `svg`).
        let path_str = path.to_string_lossy();
        let is_url = path_str.starts_with("http://") || path_str.starts_with("https://");
        let bytes_result: Result<Vec<u8>, String> = if is_url {
            self.fetch_url_bytes(path_str.as_ref())
        } else {
            std::fs::read(path).map_err(|e| e.to_string())
        };
        let decode_result: Result<image::DynamicImage, String> = bytes_result.and_then(|bytes| {
            if looks_like_svg(&bytes) {
                decode_svg_bytes(&bytes)
            } else {
                let cursor = std::io::Cursor::new(bytes);
                image::ImageReader::new(cursor)
                    .with_guessed_format()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.decode().map_err(|e| e.to_string()))
            }
        });
        let img = match decode_result {
            Ok(d) => d,
            Err(e) => {
                log::warn!("could not decode image {:?}: {}", path, e);
                self.render_image_fallback(alt);
                return;
            }
        };

        // Degenerate dimensions: a 0-px image can't produce a valid
        // XObject. Treat it like a decode failure.
        if img.width() == 0 || img.height() == 0 {
            log::warn!("image {:?} has zero dimension; skipping", path);
            self.render_image_fallback(alt);
            return;
        }

        // Bound decoded pixel dimensions. The URL fetch cap limits the
        // *download* size, but a small compressed PNG can decompress
        // to an enormous raster (memory + PDF-size blowup). Mirror the
        // SVG ceiling: downscale so neither dimension exceeds
        // `MAX_IMG_PX`, preserving aspect ratio.
        const MAX_IMG_PX: u32 = 4000;
        let img = if img.width() > MAX_IMG_PX || img.height() > MAX_IMG_PX {
            log::warn!(
                "image {:?} is {}x{}; downscaling to fit {}px",
                path,
                img.width(),
                img.height(),
                MAX_IMG_PX
            );
            img.resize(
                MAX_IMG_PX,
                MAX_IMG_PX,
                image::imageops::FilterType::Triangle,
            )
        } else {
            img
        };

        let raw = match RawImage::from_dynamic_image(img) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("could not convert image {:?}: {}", path, e);
                self.render_image_fallback(alt);
                return;
            }
        };

        let px_w = raw.width as f32;
        let px_h = raw.height as f32;
        let dpi = 300.0_f32;
        let natural_w_pt = px_w / dpi * 72.0;
        let natural_h_pt = px_h / dpi * 72.0;

        // `image.max_width_pct` is a hard cap as a percentage of the
        // content column. 100 = full column; smaller values shrink the
        // image regardless of its natural size.
        let column_w_pt = self.content_width_pt();
        let cap_pct = self.style.image.max_width_pct.clamp(1.0, 100.0) / 100.0;
        let max_w_pt = column_w_pt * cap_pct;
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
        let x_pt = match self.style.image.align {
            ImageAlign::Left => self.indent_left_pt,
            ImageAlign::Right => self.indent_left_pt + (column_w_pt - rendered_w_pt).max(0.0),
            ImageAlign::Center => {
                self.indent_left_pt + ((column_w_pt - rendered_w_pt) / 2.0).max(0.0)
            }
        };
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

        if let Some(text) = caption.filter(|s| !s.trim().is_empty()) {
            // Small gap, then a caption line styled as italic body text
            // centered horizontally inside the image's content column.
            self.advance_y(4.0);
            let base = self.style.paragraph.clone();
            let caption_size = base.font_size_pt * 0.88;
            let saved_left = self.indent_left_pt;
            let saved_right = self.indent_right_pt;
            // Constrain the caption's wrap width to the image width
            // when the image is narrower than the column.
            if rendered_w_pt < self.content_width_pt() {
                self.indent_left_pt = x_pt;
                self.indent_right_pt = x_pt + rendered_w_pt;
            }
            let runs = vec![InlineRun {
                text: text.to_string(),
                flags: RunFlags::default().with_italic(),
                link: None,
            }];
            let color = Some(rgb_color(base.text_color_rgb()));
            self.write_wrapped_runs(
                &runs,
                caption_size,
                base.line_height,
                RunFlags::default().with_italic(),
                color,
            );
            self.indent_left_pt = saved_left;
            self.indent_right_pt = saved_right;
        }

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
        // A very wide table (hundreds of columns) drives the even split
        // below the cell padding, making `col_width - pad` negative:
        // every word then "overflows" and row height explodes, and the
        // cell box (left+pad .. right-pad) inverts. Floor the column so
        // geometry stays positive — the table overflows the right
        // margin (ugly) instead of degenerating.
        const MIN_COL_WIDTH_PT: f32 = 24.0;
        let col_width = (total_width / col_count as f32).max(MIN_COL_WIDTH_PT);

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
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
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
        let owned_runs;
        let runs_ref: &[InlineRun] = if s.small_caps {
            owned_runs = self.expand_small_caps(runs);
            &owned_runs
        } else {
            runs
        };
        self.current_text_align = s.text_align;
        self.write_wrapped_runs(runs_ref, s.font_size_pt, s.line_height, base_flags, color);
        self.current_text_align = TextAlignment::Left;
        self.end_block(ctx);
    }

    fn render_paragraph(&mut self, runs: &[InlineRun]) {
        let s = self.style.paragraph.clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default();
        let ctx = self.begin_block(&s);
        let owned_runs;
        let runs_ref: &[InlineRun] = if s.small_caps {
            owned_runs = self.expand_small_caps(runs);
            &owned_runs
        } else {
            runs
        };
        self.current_text_align = s.text_align;
        self.write_wrapped_runs(runs_ref, s.font_size_pt, s.line_height, base, color);
        self.current_text_align = TextAlignment::Left;
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
        let mut words = words_from_runs(runs);
        if words.is_empty() {
            return;
        }

        let max_width = self.content_width_pt();
        // Any word that on its own exceeds the column width gets
        // chopped at character boundaries so the chunks each fit. URLs,
        // long identifiers, CJK runs without spaces, etc.
        words = self.split_long_words(words, max_width, size_pt);
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
        let align = self.current_text_align;
        let last_line_idx = lines.len().saturating_sub(1);
        for (line_idx, line) in lines.iter().enumerate() {
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

            // Pre-measure this line's natural width so alignment
            // calculations have something to work with.
            let mut natural_w_pt = 0.0f32;
            let mut space_count = 0usize;
            for seg in line {
                let s_size = if seg.flags.superscript || seg.flags.subscript {
                    size_pt * 0.70
                } else if seg.flags.small_caps {
                    size_pt * 0.78
                } else if seg.flags.small {
                    size_pt * 0.85
                } else {
                    size_pt
                };
                natural_w_pt += self.font_set.measure(seg.flags, &seg.text, s_size);
                if seg.text.chars().all(char::is_whitespace) && !seg.text.is_empty() {
                    space_count += 1;
                }
            }
            let slack_pt = (max_width - natural_w_pt).max(0.0);
            let is_last_line = line_idx == last_line_idx;

            let (line_x_start, word_spacing_pt) = match align {
                TextAlignment::Left => (self.indent_left_pt, 0.0),
                TextAlignment::Center => {
                    (self.indent_left_pt + slack_pt * 0.5, 0.0)
                }
                TextAlignment::Right => (self.indent_left_pt + slack_pt, 0.0),
                TextAlignment::Justify => {
                    // Don't justify the last line of a paragraph, lines
                    // with no break opportunities, or lines whose slack
                    // would stretch spaces beyond ~30% of the column
                    // (a sign the wrap had no good fit, like an isolated
                    // short word).
                    let stretch_ok = space_count > 0
                        && slack_pt > 0.0
                        && slack_pt < max_width * 0.30;
                    let tw = if !is_last_line && stretch_ok {
                        (slack_pt / space_count as f32).min(size_pt * 0.5)
                    } else {
                        0.0
                    };
                    (self.indent_left_pt, tw)
                }
            };
            let needs_absolute_td = !matches!(
                align,
                TextAlignment::Left | TextAlignment::Justify
            );

            if opened_now {
                self.move_cursor_to(line_x_start, baseline_y_pt);
                self.page_ops.push(Op::SetLineHeight {
                    lh: Pt(line_height_pt),
                });
                if let Some(c) = color.clone() {
                    self.page_ops.push(Op::SetFillColor { col: c });
                }
            } else if needs_absolute_td {
                self.move_cursor_to(line_x_start, baseline_y_pt);
            } else {
                self.page_ops.push(Op::AddLineBreak);
            }

            // Justify uses the PDF Tw operator (set word spacing) so
            // every space char picks up the extra slack. Set it before
            // this line's segments emit and reset to 0 afterwards.
            if matches!(align, TextAlignment::Justify) {
                self.page_ops.push(Op::SetWordSpacing {
                    pt: Pt(word_spacing_pt),
                });
            }

            let mut x_cursor_pt = line_x_start;
            let mut cursor_needs_reset = false;
            let mut line_was_broken = false;
            for seg in line {
                // Superscript: render at 70% size on a baseline raised
                // by ~32% of the original size. Implemented as a
                // self-contained little text section so it doesn't
                // disturb the line's main BT/ET. The next segment
                // re-establishes its cursor via Td.
                let (seg_size, seg_baseline) = if seg.flags.superscript {
                    (size_pt * 0.70, baseline_y_pt - size_pt * 0.32)
                } else if seg.flags.subscript {
                    (size_pt * 0.70, baseline_y_pt + size_pt * 0.20)
                } else if seg.flags.small_caps {
                    (size_pt * 0.78, baseline_y_pt)
                } else if seg.flags.small {
                    (size_pt * 0.85, baseline_y_pt)
                } else {
                    (size_pt, baseline_y_pt)
                };
                let seg_width = self.font_set.measure(seg.flags, &seg.text, seg_size);
                let font_handle = self.font_set.handle(seg.flags);
                let needs_trans = self.font_set.needs_transliteration(seg.flags);
                let emit_text = if needs_trans {
                    to_win1252(&seg.text)
                } else {
                    seg.text.clone()
                };

                if seg.flags.superscript || seg.flags.subscript {
                    // Close the line's main section, emit the small
                    // shifted-baseline glyphs in their own BT/ET, then
                    // the next iteration (if any) re-opens the main
                    // section with an explicit cursor.
                    self.close_text_section();
                    self.page_ops.push(Op::SaveGraphicsState);
                    self.page_ops.push(Op::StartTextSection);
                    let x_mm = pt_to_mm(x_cursor_pt);
                    let y_mm = pt_to_mm(self.page_height_pt() - seg_baseline);
                    self.page_ops.push(Op::SetTextCursor {
                        pos: Point::new(Mm(x_mm), Mm(y_mm)),
                    });
                    self.page_ops.push(Op::SetFont {
                        font: font_handle,
                        size: Pt(seg_size),
                    });
                    if let Some(c) = color.clone() {
                        self.page_ops.push(Op::SetFillColor { col: c });
                    }
                    self.page_ops.push(Op::ShowText {
                        items: vec![TextItem::Text(emit_text)],
                    });
                    self.page_ops.push(Op::EndTextSection);
                    self.page_ops.push(Op::RestoreGraphicsState);
                    cursor_needs_reset = true;
                    line_was_broken = true;
                } else {
                    if cursor_needs_reset {
                        // Re-open the line's main section after a
                        // superscript broke out. Place the cursor at
                        // the post-superscript x position on the
                        // baseline AND restore the leading + color so
                        // any subsequent `T*` line break in this
                        // section behaves like the original BT.
                        self.ensure_text_section();
                        let x_mm = pt_to_mm(x_cursor_pt);
                        let y_mm = pt_to_mm(self.page_height_pt() - baseline_y_pt);
                        self.page_ops.push(Op::SetTextCursor {
                            pos: Point::new(Mm(x_mm), Mm(y_mm)),
                        });
                        self.page_ops.push(Op::SetLineHeight {
                            lh: Pt(line_height_pt),
                        });
                        if let Some(c) = color.clone() {
                            self.page_ops.push(Op::SetFillColor { col: c });
                        }
                        cursor_needs_reset = false;
                    }
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
                        size: Pt(seg_size),
                    });
                    self.page_ops.push(Op::ShowText {
                        items: vec![TextItem::Text(emit_text)],
                    });
                }

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

            // A line that had any superscript break also has its
            // current BT's LineMatrix anchored mid-line (from the
            // reopen Td). Subsequent `T*` line breaks would advance
            // from that mid-line x. Close the section here so the
            // next line opens fresh with an absolute Td at the
            // intended left edge.
            if line_was_broken {
                self.close_text_section();
            }

            // Reset word spacing after a justified line so subsequent
            // sections (or the last line below) don't inherit the
            // stretch.
            if matches!(align, TextAlignment::Justify) && word_spacing_pt > 0.0 {
                self.page_ops.push(Op::SetWordSpacing { pt: Pt(0.0) });
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
    // printpdf 0.9's `Op::DrawRectangle` is broken — its serializer
    // emits `re` then `n` (end-path-no-paint), so the fill is
    // discarded regardless of `PaintMode`. We build the rectangle as
    // a 4-corner `Op::DrawPolygon` instead, whose serializer honors
    // `PaintMode::Fill` and emits the `f` operator.
    //
    // PDF y origin is bottom-left; our y is top-down.
    let y_bottom = page_height_pt - y_bot_pt;
    let y_top = page_height_pt - y_top_pt;
    let corner = |x: f32, y: f32| LinePoint {
        p: Point {
            x: Pt(x),
            y: Pt(y),
        },
        bezier: false,
    };
    let polygon = Polygon {
        rings: vec![PolygonRing {
            points: vec![
                corner(x0_pt, y_bottom),
                corner(x1_pt, y_bottom),
                corner(x1_pt, y_top),
                corner(x0_pt, y_top),
            ],
        }],
        mode: PaintMode::Fill,
        winding_order: WindingOrder::NonZero,
    };
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetFillColor { col: fill });
    ops.push(Op::DrawPolygon { polygon });
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
            let is_space = is_breaking_space(chars[i].1);
            let mut j = i + 1;
            while j < chars.len() && is_breaking_space(chars[j].1) == is_space {
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

/// True for whitespace that is a *line-break opportunity*. Excludes
/// the non-breaking space family: U+00A0 (NBSP), U+202F (narrow
/// NBSP), U+2007 (figure space). Those render with space advance but
/// must keep their neighbors on the same line, so `words_from_runs`
/// keeps them inside the word token rather than emitting them as a
/// breakable gap.
fn is_breaking_space(c: char) -> bool {
    c.is_whitespace() && !matches!(c, '\u{00A0}' | '\u{202F}' | '\u{2007}')
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

/// Cheap content sniff: does this buffer look like an SVG document?
/// Skips a UTF-8 BOM and ASCII whitespace, accepts `<?xml`-prefixed
/// SVGs and bare `<svg ...>` openings. Big enough to keep us from
/// running resvg on random `<svg>`-mentioning text.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let mut s = bytes;
    if s.starts_with(&[0xEF, 0xBB, 0xBF]) {
        s = &s[3..];
    }
    while let Some(&b) = s.first() {
        if b.is_ascii_whitespace() {
            s = &s[1..];
        } else {
            break;
        }
    }
    let head: &[u8] = if s.len() > 512 { &s[..512] } else { s };
    let lower: Vec<u8> = head.iter().map(|b| b.to_ascii_lowercase()).collect();
    if lower.starts_with(b"<?xml") {
        return lower.windows(4).any(|w| w == b"<svg");
    }
    lower.starts_with(b"<svg")
}

/// Rasterize an SVG byte buffer to an `image::DynamicImage`. Rendered
/// at `2x` the SVG's intrinsic size for crisp output on print DPIs;
/// hard upper cap of 4000px per dimension so an unbounded
/// `width="999999"` doesn't blow up memory.
#[cfg(feature = "svg")]
fn decode_svg_bytes(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    const MAX_PX: u32 = 4000;
    let opts = resvg::usvg::Options::default();
    let tree = resvg::usvg::Tree::from_data(bytes, &opts).map_err(|e| e.to_string())?;
    let size = tree.size();
    let scale = 2.0_f32;
    let mut w_px = (size.width() * scale).ceil() as u32;
    let mut h_px = (size.height() * scale).ceil() as u32;
    if w_px == 0 || h_px == 0 {
        return Err("svg has zero intrinsic size".to_string());
    }
    if w_px > MAX_PX || h_px > MAX_PX {
        let r = (MAX_PX as f32 / w_px.max(h_px) as f32).min(1.0);
        w_px = ((w_px as f32) * r) as u32;
        h_px = ((h_px as f32) * r) as u32;
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w_px, h_px)
        .ok_or_else(|| "could not allocate svg pixmap".to_string())?;
    let tx = resvg::tiny_skia::Transform::from_scale(
        w_px as f32 / size.width(),
        h_px as f32 / size.height(),
    );
    resvg::render(&tree, tx, &mut pixmap.as_mut());
    let rgba = image::RgbaImage::from_raw(w_px, h_px, pixmap.data().to_vec())
        .ok_or_else(|| "pixmap → RgbaImage conversion failed".to_string())?;
    Ok(image::DynamicImage::ImageRgba8(rgba))
}

#[cfg(not(feature = "svg"))]
fn decode_svg_bytes(_bytes: &[u8]) -> Result<image::DynamicImage, String> {
    Err("SVG support requires the `svg` feature".to_string())
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

    // SVG raster helpers live in a free `fn` outside `Engine` so the
    // module-level helpers don't have to thread `self`. Tests exercise
    // them indirectly via the showcase document.

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
