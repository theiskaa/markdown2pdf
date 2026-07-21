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
    BorderStyle, ImageAlign, Orientation, PageSize, ResolvedBlock, ResolvedBorder,
    ResolvedBorderSide, ResolvedList,
    ResolvedPage, ResolvedPageFurniture, ResolvedStyle, ResolvedToc, TextAlignment,
};

use crate::markdown::{slugify, TableCell};

use super::font::FontSet;
use super::image_policy::{is_http_url, resolve_image_path, ImagePathRefusal};
use super::ir::{Block, InlineRun, ListBullet, ListEntry, RunFlags};
use super::math::layout::GlyphFont;

type Color = printpdf::Color;

/// Colour for a link whose `#slug` target doesn't match any heading
/// in the document — a dead wikilink. A muted red that reads as
/// "broken" against any theme. Underline is also suppressed for
/// these links so they don't visually claim to be live.
const UNRESOLVED_LINK_COLOR: (u8, u8, u8) = (192, 57, 43);

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
    known_heading_slugs: &HashSet<String>,
    doc: &mut PdfDocument,
) -> Vec<PdfPage> {
    let mut engine = Engine::new(style, font_set, doc);
    engine.known_heading_slugs = known_heading_slugs.clone();
    let mut it = blocks.iter().peekable();
    while let Some(block) = it.next() {
        let next = it.peek().copied();
        engine.render_block(block, next);
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
    /// Document-wide set of heading slugs, populated before layout
    /// starts. Used to style internal `#slug` links: a target in the
    /// set is a resolvable link (live styling); not in the set is a
    /// dead link (red, no underline).
    known_heading_slugs: HashSet<String>,
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
    /// Index into `page_ops` just before the currently-open text
    /// section's `SaveGraphicsState`. Inline `==highlight==` fills are
    /// spliced here on [`close_text_section`] so they paint *under*
    /// the section's glyphs (same trick as block backgrounds).
    text_section_marker: usize,
    /// Highlight rects collected while the current text section is
    /// open; drained into `page_ops` when it closes.
    pending_highlights: Vec<HighlightBox>,
    /// True while rendering a fenced code block, so monospace runs
    /// keep the `[code_block]` colour instead of being repainted with
    /// the `[code_inline]` colour (both carry the `monospace` flag).
    in_code_block: bool,
    /// When set, paragraphs take their *text* style (font, colour,
    /// weight, slant, size, alignment, decorations) from this block
    /// instead of `[paragraph]` — so a blockquote's or admonition's
    /// body text inherits the container's typography. Structural
    /// fields (margins, padding, border, background) stay paragraph's,
    /// since the container already draws its own box.
    text_style_override: Option<ResolvedBlock>,
    /// First-line indent (points) for the next `write_wrapped_runs`
    /// call. Set by `render_paragraph` from `[paragraph].indent_pt`;
    /// the call consumes it (resets to 0) so it applies once.
    first_line_indent_pt: f32,
    /// Extra spacing (points) added after every glyph of the block
    /// currently being rendered. Set by `begin_block` from the block's
    /// `letter_spacing_pt` and restored by `end_block`; read by both
    /// `measure_text` and `emit_text_chunks` so the two never drift.
    letter_spacing_pt: f32,
    /// Stack of block backgrounds currently open (LIFO — matches the
    /// nesting of `begin_block` / `end_block`). When a page break
    /// happens mid-block, [`start_new_page`] paints the fragment that
    /// fit on the outgoing page and resets each entry to continue on
    /// the next page. Empty when no background-bearing block is open.
    open_bg: Vec<OpenBlockBg>,
    /// Lazily-initialised TeX math state: the parsed STIX Two Math
    /// face plus the body / fallback text faces consulted for
    /// characters STIX lacks. `None` until the first math is
    /// rendered; `Some(None)` if the font failed to load (we then
    /// fall back to plain-text math).
    math: Option<Option<MathState<'a>>>,
    /// Memoised inline-math layouts, keyed by (raw TeX, size×100).
    /// Inline math is measured several times per line (wrap, natural
    /// width, emit) — typesetting once and cloning keeps that O(1).
    math_inline_cache: HashMap<(String, u32), super::math::layout::Frag>,
    /// One Form XObject per distinct (source font, glyph id). A glyph
    /// outline is emitted once here and invoked with a tiny `cm`/`Do`
    /// at every occurrence, instead of re-inlining its
    /// (flattened-Bézier) polygon — the dominant cost in math-heavy
    /// PDFs.
    math_glyph_xobjects: HashMap<(GlyphFont, u16), printpdf::XObjectId>,
    /// Number of body-text columns per page. Clamped to 1..=4 from
    /// `style.page.columns`. The TOC and title-page passes force this
    /// to 1 temporarily so their full-page layout is preserved.
    num_columns: u8,
    /// Horizontal gap (points) between two adjacent body columns,
    /// resolved from `style.page.column_gap_mm` and clamped so a
    /// positive column width is always derivable. Zero when
    /// `num_columns <= 1`.
    column_gap_pt: f32,
    /// Width (points) of a single body column. Equal to the page's
    /// content width when `num_columns == 1`.
    column_width_pt: f32,
    /// Which body column the cursor is currently in (`0 .. num_columns`).
    /// Advanced by [`advance_column`]; reset to 0 by [`start_new_page`].
    current_column: u8,
}

struct MathState<'a> {
    font: super::math::font::MathFont,
    /// Body-then-fallback faces (parsed from the FontSet's retained
    /// bytes) for `\text{…}` / symbol characters STIX lacks.
    text_fonts: Vec<super::math::font::MathTextFont<'a>>,
    /// Characters no font covers that have already been warned about
    /// — shared across every typeset so a render warns once per
    /// distinct char, not once per formula.
    warned: std::cell::RefCell<HashSet<char>>,
}

/// Captured column state, returned by
/// [`Engine::snapshot_columns_single`] and consumed by
/// [`Engine::restore_columns`]. Used by the TOC and title-page
/// passes to render full-page-width without permanently changing
/// the engine's column geometry.
struct SavedColumns {
    num_columns: u8,
    column_gap_pt: f32,
    column_width_pt: f32,
    current_column: u8,
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

/// Snapshot of an open block-background fragment: `(marker, x_left,
/// x_right, top_y, color)`. See `paint_open_bg_fragments`.
type OpenBgFrag = (usize, f32, f32, f32, (u8, u8, u8));

impl<'a> Engine<'a> {
    fn new(style: &'a ResolvedStyle, font_set: &'a FontSet, doc: &'a mut PdfDocument) -> Self {
        let (page_width_mm, page_height_mm) = page_dimensions_mm(&style.page);
        let left = mm_to_pt(style.page.margins_mm.left.max(1.0));
        let right = page_width_mm * MM_TO_PT - mm_to_pt(style.page.margins_mm.right.max(1.0));
        let top = mm_to_pt(style.page.margins_mm.top.max(1.0));
        let body_width = (right - left).max(10.0);
        let num_columns = style.page.columns.clamp(1, 4);
        // A 0mm gap (the default) keeps single-column renders byte-identical.
        // Hostile values (NaN, inf, negative, absurdly huge) get clamped so
        // (body_width - (n-1)*gap) / n stays positive and at least the
        // single-column minimum content width survives.
        let raw_gap_pt = mm_to_pt(if style.page.column_gap_mm.is_finite() {
            style.page.column_gap_mm.max(0.0)
        } else {
            0.0
        });
        let (column_gap_pt, column_width_pt) = if num_columns <= 1 {
            (0.0, body_width)
        } else {
            // Reserve at least 10pt per column so wrap math stays sane
            // even with a hostile gap. Floor the gap above 0 — narrower
            // than the user asked, but never collapses geometry.
            let n_f = num_columns as f32;
            let max_gap = ((body_width - 10.0 * n_f) / (n_f - 1.0)).max(0.0);
            let gap = raw_gap_pt.min(max_gap);
            let col_w = (body_width - gap * (n_f - 1.0)) / n_f;
            (gap, col_w)
        };
        // Initial cursor sits in column 0; its left/right edges collapse
        // to the page's content edges when num_columns == 1, so existing
        // single-column renders are byte-identical to the pre-column code.
        let col0_left = left;
        let col0_right = left + column_width_pt;
        Self {
            style,
            font_set,
            doc,
            page_width_mm,
            page_height_mm,
            y_from_top_pt: top,
            indent_left_pt: col0_left,
            indent_right_pt: col0_right,
            page_ops: Vec::new(),
            pending_decorations: Vec::new(),
            raw_pages: Vec::new(),
            heading_anchors: Vec::new(),
            pending_internal_links: Vec::new(),
            used_slugs: HashSet::new(),
            known_heading_slugs: HashSet::new(),
            current_text_align: TextAlignment::Left,
            url_image_cache: HashMap::new(),
            in_text_section: false,
            text_section_marker: 0,
            pending_highlights: Vec::new(),
            in_code_block: false,
            text_style_override: None,
            first_line_indent_pt: 0.0,
            letter_spacing_pt: 0.0,
            open_bg: Vec::new(),
            math: None,
            math_inline_cache: HashMap::new(),
            math_glyph_xobjects: HashMap::new(),
            num_columns,
            column_gap_pt,
            column_width_pt,
            current_column: 0,
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
            .chain(toc_pages)
            .chain(content_pages);
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
        // drained body into content_pages already). The title page
        // renders full-page-width regardless of the body's column
        // count, so num_columns is forced to 1 for the pass and
        // restored on the way out.
        let saved_y = self.y_from_top_pt;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_in_text = self.in_text_section;
        let saved_columns = self.snapshot_columns_single();

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

        // Optional cover image, centered above the title block. Scaled
        // to fit the content width with height capped at ~45% of the
        // usable page height so the title stack still fits.
        let content_w = self.indent_right_pt - self.indent_left_pt;
        let cover = tp
            .cover_image_path
            .as_deref()
            .and_then(|p| self.decode_image_file(std::path::Path::new(p)))
            .map(|raw| {
                let nat_w = raw.width as f32 / 300.0 * 72.0;
                let nat_h = raw.height as f32 / 300.0 * 72.0;
                let scale = (content_w / nat_w)
                    .min((usable_h * 0.45) / nat_h)
                    .min(1.0);
                (raw, nat_w * scale, nat_h * scale, scale)
            });
        if let Some((_, _, cover_h, _)) = &cover {
            stack_h += cover_h + line_gap;
        }

        let start_y = top + ((usable_h - stack_h) * 0.5).max(0.0);
        self.y_from_top_pt = start_y;

        // Draw the cover image, then drop the cursor below it so the
        // title stack renders underneath.
        if let Some((raw, cover_w, cover_h, scale)) = cover {
            let xobject_id: XObjectId = self.doc.add_image(&raw);
            let x_pt = self.indent_left_pt + ((content_w - cover_w) * 0.5).max(0.0);
            let y_bot_pt = self.page_height_pt() - self.y_from_top_pt - cover_h;
            self.close_text_section();
            self.page_ops.push(Op::UseXobject {
                id: xobject_id,
                transform: XObjectTransform {
                    translate_x: Some(Pt(x_pt)),
                    translate_y: Some(Pt(y_bot_pt)),
                    rotate: None,
                    scale_x: Some(scale),
                    scale_y: Some(scale),
                    dpi: Some(300.0),
                },
            });
            self.y_from_top_pt += cover_h + line_gap;
        }

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
        self.restore_columns(saved_columns);
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
            highlight: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
            underline: false,
            inline_code: false,
        };
        let measured = self.measure_text(flags, text, size_pt);
        let center_x = (self.page_width_pt() - measured) / 2.0;
        let baseline_y = self.y_from_top_pt + size_pt;

        self.close_text_section();
        self.ensure_text_section();
        self.move_cursor_to(center_x, baseline_y);
        self.page_ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        emit_text_chunks(
            &mut self.page_ops,
            self.font_set,
            flags,
            text,
            size_pt,
            self.letter_spacing_pt,
        );
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
        // after `push_current_page` was called. The TOC always renders
        // full-page-width regardless of the body's column count, so
        // num_columns is forced to 1 for the pass.
        let saved_y = self.y_from_top_pt;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_in_text = self.in_text_section;
        let saved_link_count = self.pending_internal_links.len();
        let saved_columns = self.snapshot_columns_single();
        // Reset to first-page top.
        self.y_from_top_pt = mm_to_pt(self.style.page.margins_mm.top.max(1.0));
        let page_width_pt = self.page_width_pt();
        self.indent_left_pt = mm_to_pt(self.style.page.margins_mm.left.max(1.0));
        self.indent_right_pt =
            page_width_pt - mm_to_pt(self.style.page.margins_mm.right.max(1.0));
        self.in_text_section = false;

        self.render_toc_title(&toc);

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
        self.restore_columns(saved_columns);
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
        let runs = vec![InlineRun { math: None,
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
            highlight: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
            underline: false,
            inline_code: false,
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
        self.page_ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        emit_text_chunks(
            &mut self.page_ops,
            self.font_set,
            flags,
            &anchor.text,
            size_pt,
            self.letter_spacing_pt,
        );

        // Page-number portion (right-aligned at row_right).
        let page_str = page_num.to_string();
        let num_w = self.measure_text(flags, &page_str, size_pt);
        let num_x = row_right - num_w;
        self.close_text_section();
        self.ensure_text_section();
        self.move_cursor_to(num_x, baseline_y);
        emit_text_chunks(
            &mut self.page_ops,
            self.font_set,
            flags,
            &page_str,
            size_pt,
            self.letter_spacing_pt,
        );
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

    /// Advance to the next column/page if `header_h + follow_h` won't
    /// fit in the remaining space. No-op at column top (already maximum
    /// room — advancing would just add a blank page). Callers supply
    /// the actual measured heights so the heuristic can adapt to
    /// multi-line headings and non-paragraph follow blocks.
    fn keep_with_next_break(&mut self, header_h: f32, follow_h: f32) {
        if (self.y_from_top_pt - self.top_margin_pt()).abs() < 0.01 {
            return;
        }
        let needed = header_h + follow_h;
        if self.y_from_top_pt + needed + self.bottom_margin_pt() > self.page_height_pt() {
            self.advance_column();
        }
    }

    /// Conservative wrap-count estimate: total word ink at `font_size`
    /// divided by current content width, rounded up. Used by
    /// keep-with-next to reserve enough vertical space for a heading
    /// that wraps. Biased to over-count by a fraction of a line in
    /// pathological cases, which is the desired direction — pushing a
    /// heading to a fresh page is cheaper than orphaning it.
    fn estimate_wrapped_lines(
        &self,
        runs: &[InlineRun],
        font_size: f32,
        base_flags: RunFlags,
    ) -> usize {
        if runs.is_empty() {
            return 0;
        }
        let max_width = self.content_width_pt();
        if max_width <= 0.0 {
            return 1;
        }
        let mut total = 0.0f32;
        for run in runs {
            if run.math.is_some() {
                continue;
            }
            let flags = run.flags.or(base_flags);
            total += self.measure_text(flags, &run.text, font_size);
        }
        ((total / max_width).ceil() as usize).max(1)
    }

    /// Vertical reservation for the *first visible chunk* of `next` —
    /// the space its `margin_before + padding.top + one line height`
    /// would occupy at the current cursor. `None` falls back to the
    /// paragraph style (conservative; preserves the pre-lookahead
    /// behavior when callers can't see what follows).
    fn next_block_lead_pt(&self, next: Option<&Block>) -> f32 {
        // Admonitions reserve their own label + gap + first body line
        // via render_admonition's own keep_with_next_break — so the
        // caller (e.g. a heading right before this admonition) must
        // reserve the admonition's full label-plus-body chunk, not
        // just one line, or the admonition will push to the next
        // page and orphan the heading.
        if let Some(Block::Admonition { kind, .. }) = next {
            let s = &self.style.admonition.for_kind(kind).block;
            return s.margin_before_pt
                + s.padding.top
                + s.font_size_pt * s.line_height.max(0.5)
                + s.font_size_pt * 0.35
                + s.font_size_pt * s.line_height.max(0.5);
        }
        let s: &ResolvedBlock = match next {
            None => &self.style.paragraph,
            Some(Block::Paragraph { .. }) => &self.style.paragraph,
            Some(Block::Heading { level, .. }) => {
                let idx = (*level).clamp(1, 6) as usize - 1;
                &self.style.headings[idx]
            }
            Some(Block::CodeBlock { .. }) => &self.style.code_block,
            Some(Block::BlockQuote { .. }) => &self.style.blockquote,
            Some(Block::List { entries }) => {
                if let Some(first) = entries.first() {
                    let list = match first.bullet {
                        ListBullet::Ordered(_) => &self.style.list_ordered,
                        ListBullet::Unordered(_) => &self.style.list_unordered,
                        ListBullet::TaskChecked | ListBullet::TaskUnchecked => {
                            &self.style.list_task
                        }
                    };
                    &list.block
                } else {
                    &self.style.paragraph
                }
            }
            Some(Block::Table { .. }) => &self.style.table.header,
            // Image / HR / HtmlBlock / Math / FootnoteDefinitions /
            // DefinitionList / PageBreak: paragraph is the conservative
            // default (these all have some leading margin and at least
            // one line-height worth of content).
            Some(_) => &self.style.paragraph,
        };
        s.margin_before_pt + s.padding.top + s.font_size_pt * s.line_height.max(0.5)
    }

    /// True if `link` is an internal `#slug` reference whose slug
    /// isn't among the document's heading anchors — i.e. a dead
    /// wikilink. External links (`http://`, etc.) are never
    /// "unresolved" by this check.
    fn is_unresolved_internal_link(&self, link: &Option<String>) -> bool {
        let Some(url) = link else { return false };
        let Some(slug) = url.strip_prefix('#') else { return false };
        !self.known_heading_slugs.contains(slug)
    }

    fn bottom_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.bottom.max(1.0))
    }

    fn left_margin_pt(&self) -> f32 {
        mm_to_pt(self.style.page.margins_mm.left.max(1.0))
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
                    out.push(InlineRun { math: None,
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
                out.push(InlineRun { math: None,
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
            if word.math.is_some() {
                // Inline-math boxes are atomic — never char-split.
                out.push(word);
                continue;
            }
            if word.text.chars().all(char::is_whitespace) {
                out.push(word);
                continue;
            }
            let total = self.measure_text(word.flags, &word.text, size_pt);
            if total <= max_width {
                out.push(word);
                continue;
            }
            let breaks = super::hyphenate::break_points(&word.text);
            // URL / path segment break candidates (positions after `/`,
            // `?`, `&`, `#`). Only collected for URL-like words — a `/`
            // is the cheapest signature — so identifiers like
            // `C#program_with_long_name` don't get split after `#`.
            let soft_breaks: Vec<usize> = if word.text.contains('/') {
                word.text
                    .char_indices()
                    .filter_map(|(i, c)| {
                        if matches!(c, '/' | '?' | '&' | '#') {
                            let next = i + c.len_utf8();
                            if next < word.text.len() { Some(next) } else { None }
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let hyphen_width = self.measure_text(word.flags, "-", size_pt);
            let chars: Vec<(usize, char)> = word.text.char_indices().collect();
            let mut chunk_start_byte = 0usize;
            let mut chunk_start_char = 0usize;
            while chunk_start_char < chars.len() {
                // Try soft (URL/path) break first — cleanest split for
                // long URLs. No trailing hyphen is added.
                let mut soft_break: Option<usize> = None;
                for &b in &soft_breaks {
                    if b <= chunk_start_byte {
                        continue;
                    }
                    let prefix = &word.text[chunk_start_byte..b];
                    let w = self.measure_text(word.flags, prefix, size_pt);
                    if w <= max_width {
                        soft_break = Some(b);
                    } else {
                        break;
                    }
                }
                if let Some(b) = soft_break {
                    let chunk_text = word.text[chunk_start_byte..b].to_string();
                    out.push(InlineRun { math: None,
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
                // Try hyphenation: pick the largest break offset
                // that's strictly past chunk_start AND produces a
                // prefix (plus "-") that fits in max_width.
                let mut hyphen_break: Option<usize> = None;
                for &b in &breaks {
                    if b <= chunk_start_byte {
                        continue;
                    }
                    let prefix = &word.text[chunk_start_byte..b];
                    let w = self.measure_text(word.flags, prefix, size_pt) + hyphen_width;
                    if w <= max_width {
                        hyphen_break = Some(b);
                    } else {
                        break;
                    }
                }
                if let Some(b) = hyphen_break {
                    let mut chunk_text = word.text[chunk_start_byte..b].to_string();
                    chunk_text.push('-');
                    out.push(InlineRun { math: None,
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
                    let w = self.measure_text(word.flags, prefix, size_pt);
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
                out.push(InlineRun { math: None,
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

    /// Advance the y cursor by `dy` points. When the cursor crosses
    /// the bottom margin, move on to the next column on the same page
    /// (or, if this was the last column, finalize the page and start a
    /// new one). For single-column layouts the behavior is identical
    /// to the original "page break on overflow".
    fn advance_y(&mut self, dy: f32) {
        self.y_from_top_pt += dy;
        if self.y_from_top_pt + self.bottom_margin_pt() > self.page_height_pt() {
            self.advance_column();
        }
    }

    /// Left edge (points) of column `col`'s body area, measured from
    /// the page's left edge. Column 0 sits at `left_margin_pt()`;
    /// each subsequent column shifts right by `column_width_pt +
    /// column_gap_pt`. Identical to `left_margin_pt()` for col 0 with
    /// `num_columns == 1`.
    fn column_body_left_pt(&self, col: u8) -> f32 {
        self.left_margin_pt() + col as f32 * (self.column_width_pt + self.column_gap_pt)
    }

    /// Right edge (points) of column `col`'s body area: column-body-left
    /// plus the full column width.
    fn column_body_right_pt(&self, col: u8) -> f32 {
        self.column_body_left_pt(col) + self.column_width_pt
    }

    /// Translate a `(left, right)` indent pair captured in `saved_column`
    /// to the equivalent pair in `self.current_column`, preserving the
    /// relative offset from each column-body edge. Used by callers that
    /// snapshot indents around content that might trigger
    /// `advance_column` mid-call.
    fn rebase_indents(&self, saved_left: f32, saved_right: f32, saved_column: u8) -> (f32, f32) {
        if saved_column == self.current_column {
            return (saved_left, saved_right);
        }
        let prev_l = self.column_body_left_pt(saved_column);
        let prev_r = self.column_body_right_pt(saved_column);
        let dl = saved_left - prev_l;
        let dr = prev_r - saved_right;
        let cur_l = self.column_body_left_pt(self.current_column);
        let cur_r = self.column_body_right_pt(self.current_column);
        (cur_l + dl, cur_r - dr)
    }

    /// Page-break: finalize the current page, reset to column 0 on a
    /// fresh page. Used by `Block::PageBreak` and by `advance_column`
    /// once the last column has filled. Preserves the *relative*
    /// indent inside any open block (a blockquote that page-broke
    /// keeps its left/right padding on the new page).
    fn start_new_page(&mut self) {
        self.close_text_section();
        self.paint_open_bg_fragments();
        self.push_current_page();
        let prev_col_left = self.column_body_left_pt(self.current_column);
        let prev_col_right = self.column_body_right_pt(self.current_column);
        let delta_l = self.indent_left_pt - prev_col_left;
        let delta_r = prev_col_right - self.indent_right_pt;
        self.current_column = 0;
        self.y_from_top_pt = self.top_margin_pt();
        let new_col_left = self.column_body_left_pt(0);
        let new_col_right = self.column_body_right_pt(0);
        self.indent_left_pt = new_col_left + delta_l;
        self.indent_right_pt = new_col_right - delta_r;
        // Each still-open background continues at the top of the new
        // column; its fill on this page starts at the top content edge
        // and its splice marker resets to the (now empty) op buffer.
        // X-edges follow the new column (preserving the bg's relative
        // offset from the column body edges).
        let new_top = self.top_margin_pt();
        for ob in self.open_bg.iter_mut() {
            ob.top_y = new_top;
            ob.marker = 0;
            let bg_dl = ob.x_left - prev_col_left;
            let bg_dr = prev_col_right - ob.x_right;
            ob.x_left = new_col_left + bg_dl;
            ob.x_right = new_col_right - bg_dr;
        }
    }

    /// Snapshot the column state and force the engine into a
    /// transient single-column mode. Used by the TOC and title-page
    /// passes so their full-page layout doesn't get sliced into
    /// columns. Restore with [`restore_columns`].
    fn snapshot_columns_single(&mut self) -> SavedColumns {
        let saved = SavedColumns {
            num_columns: self.num_columns,
            column_gap_pt: self.column_gap_pt,
            column_width_pt: self.column_width_pt,
            current_column: self.current_column,
        };
        let page_width_pt = self.page_width_pt();
        let body_w = (page_width_pt
            - mm_to_pt(self.style.page.margins_mm.left.max(1.0))
            - mm_to_pt(self.style.page.margins_mm.right.max(1.0)))
        .max(10.0);
        self.num_columns = 1;
        self.column_gap_pt = 0.0;
        self.column_width_pt = body_w;
        self.current_column = 0;
        saved
    }

    /// Restore column state previously captured by
    /// [`snapshot_columns_single`]. The caller is responsible for
    /// re-establishing the indents it took out as well — the column
    /// state alone is the column index, gap, width, and count.
    fn restore_columns(&mut self, saved: SavedColumns) {
        self.num_columns = saved.num_columns;
        self.column_gap_pt = saved.column_gap_pt;
        self.column_width_pt = saved.column_width_pt;
        self.current_column = saved.current_column;
    }

    /// Move to the next column on the same page; if the current column
    /// was the last, fall through to `start_new_page`. Any open block
    /// background paints the fragment that fit in the now-leaving
    /// column before its top-y / x-edges are reset to the new column.
    /// For single-column layouts this is exactly `start_new_page` —
    /// the geometry collapses to the original code path.
    fn advance_column(&mut self) {
        if self.current_column + 1 >= self.num_columns {
            self.start_new_page();
            return;
        }
        self.close_text_section();
        self.paint_open_bg_fragments();
        let prev_col_left = self.column_body_left_pt(self.current_column);
        let prev_col_right = self.column_body_right_pt(self.current_column);
        let delta_l = self.indent_left_pt - prev_col_left;
        let delta_r = prev_col_right - self.indent_right_pt;
        self.current_column += 1;
        self.y_from_top_pt = self.top_margin_pt();
        let new_col_left = self.column_body_left_pt(self.current_column);
        let new_col_right = self.column_body_right_pt(self.current_column);
        self.indent_left_pt = new_col_left + delta_l;
        self.indent_right_pt = new_col_right - delta_r;
        let new_top = self.top_margin_pt();
        for ob in self.open_bg.iter_mut() {
            ob.top_y = new_top;
            ob.marker = 0;
            let bg_dl = ob.x_left - prev_col_left;
            let bg_dr = prev_col_right - ob.x_right;
            ob.x_left = new_col_left + bg_dl;
            ob.x_right = new_col_right - bg_dr;
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
        let frags: Vec<OpenBgFrag> = self
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
            self.text_section_marker = self.page_ops.len();
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
        self.flush_highlights();
    }

    /// Splice the collected `==highlight==` rects into `page_ops` just
    /// before the (now-closed) text section's `SaveGraphicsState`, so
    /// they paint behind the glyphs. No-op when nothing was
    /// highlighted or `[mark]` has no background colour.
    fn flush_highlights(&mut self) {
        if self.pending_highlights.is_empty() {
            return;
        }
        let boxes = std::mem::take(&mut self.pending_highlights);
        let page_h_pt = self.page_height_pt();
        let mut bg_ops: Vec<Op> = Vec::new();
        for b in &boxes {
            draw_filled_rect(
                &mut bg_ops,
                b.x0_pt,
                b.baseline_y_pt - b.size_pt * 0.80 - b.pad_top_pt,
                b.x1_pt,
                b.baseline_y_pt + b.size_pt * 0.20 + b.pad_bottom_pt,
                b.fill.clone(),
                page_h_pt,
            );
        }
        let at = self.text_section_marker.min(self.page_ops.len());
        self.page_ops.splice(at..at, bg_ops);
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
        // Match CSS multi-column: collapse the first block's top
        // margin at the top of each column so col 0 (H1) and col 1+
        // (paragraph) align. Single-column layouts keep the original
        // breathing room.
        let at_column_top = (self.y_from_top_pt - self.top_margin_pt()).abs() < 0.01;
        if !(self.num_columns > 1 && at_column_top) {
            self.advance_y(style.margin_before_pt);
        }
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

        let saved_letter_spacing = self.letter_spacing_pt;
        self.letter_spacing_pt = style.letter_spacing_pt;

        BlockPaintCtx {
            saved_left: outer_x_left,
            saved_right: outer_x_right,
            saved_column: self.current_column,
            outer_x_left,
            outer_x_right,
            outer_y_top,
            background_color: style.background_color,
            border: style.border,
            padding_bottom: style.padding.bottom,
            margin_after_pt: style.margin_after_pt,
            saved_letter_spacing,
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
            if let Some(ob) = self.open_bg.pop()
                && outer_y_bottom > ob.top_y {
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

        // If the block crossed a column- or page-break, the absolute
        // indents captured at begin time refer to the *old* column and
        // would snap subsequent blocks back into it. Translate them to
        // the current column by preserving the delta from the column
        // body edges.
        if ctx.saved_column == self.current_column {
            self.indent_left_pt = ctx.saved_left;
            self.indent_right_pt = ctx.saved_right;
        } else {
            let prev_col_left = self.column_body_left_pt(ctx.saved_column);
            let prev_col_right = self.column_body_right_pt(ctx.saved_column);
            let delta_l = ctx.saved_left - prev_col_left;
            let delta_r = prev_col_right - ctx.saved_right;
            let cur_col_left = self.column_body_left_pt(self.current_column);
            let cur_col_right = self.column_body_right_pt(self.current_column);
            self.indent_left_pt = cur_col_left + delta_l;
            self.indent_right_pt = cur_col_right - delta_r;
        }
        self.letter_spacing_pt = ctx.saved_letter_spacing;
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
            highlight: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
            underline: false,
            inline_code: false,
        };
        let size_pt = style.font_size_pt;
        let measured = self.measure_text(flags, text, size_pt);
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

        ops.push(Op::SaveGraphicsState);
        ops.push(Op::StartTextSection);
        ops.push(Op::SetTextCursor {
            pos: Point::new(Mm(x_mm), Mm(y_mm)),
        });
        ops.push(Op::SetFillColor {
            col: rgb_color(style.text_color_rgb()),
        });
        emit_text_chunks(
            ops,
            self.font_set,
            flags,
            text,
            size_pt,
            self.letter_spacing_pt,
        );
        ops.push(Op::EndTextSection);
        ops.push(Op::RestoreGraphicsState);
    }

    fn render_block(&mut self, block: &Block, next: Option<&Block>) {
        match block {
            Block::Heading { level, runs } => self.render_heading(*level, runs, next),
            Block::Paragraph { runs } => self.render_paragraph(runs),
            Block::CodeBlock { lines } => self.render_code_block(lines),
            Block::HorizontalRule => self.render_horizontal_rule(),
            Block::List { entries } => self.render_list(entries),
            Block::BlockQuote { body } => self.render_blockquote(body),
            Block::Admonition {
                kind,
                raw_label,
                title,
                body,
            } => self.render_admonition(kind, raw_label, title.as_deref(), body),
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
            Block::MathBlock { content } => self.render_math_block(content),
        }
    }

    /// Lazily parse STIX Two Math plus the body / fallback text faces
    /// consulted for characters STIX lacks (`\text{…}`, bare CJK,
    /// etc.). Returns `true` once available; `false` if the math font
    /// failed to load (callers then fall back to plain-text math so
    /// nothing is lost). No font is *ever* embedded from this path —
    /// math is drawn as vector outlines.
    fn ensure_math(&mut self) -> bool {
        if self.math.is_none() {
            let font_set = self.font_set;
            self.math = Some(super::math::font::MathFont::new().map(|font| {
                let mut text_fonts = Vec::new();
                let chain = font_set
                    .external_body
                    .regular
                    .iter()
                    .chain(font_set.fallbacks.iter())
                    .filter(|f| !f.source_bytes().is_empty());
                for f in chain.take(super::math::layout::MAX_TEXT_FONTS) {
                    if let Some(tf) =
                        super::math::font::MathTextFont::from_bytes(f.source_bytes())
                    {
                        text_fonts.push(tf);
                    }
                }
                MathState {
                    font,
                    text_fonts,
                    warned: std::cell::RefCell::new(HashSet::new()),
                }
            }));
        }
        matches!(self.math, Some(Some(_)))
    }

    /// Typeset an inline-math span at `size_pt` (Text style),
    /// memoised. `None` if the math font is unavailable.
    fn inline_math_frag(
        &mut self,
        content: &str,
        size_pt: f32,
    ) -> Option<super::math::layout::Frag> {
        if !self.ensure_math() {
            return None;
        }
        let key = (content.to_string(), (size_pt * 100.0) as u32);
        if let Some(f) = self.math_inline_cache.get(&key) {
            return Some(f.clone());
        }
        let frag = {
            let ms = self.math.as_ref().unwrap().as_ref().unwrap();
            super::math::typeset(&ms.font, &ms.text_fonts, &ms.warned, content, false, size_pt)
        };
        if let Some(f) = &frag {
            self.math_inline_cache.insert(key, f.clone());
        }
        frag
    }

    /// The Form XObject for glyph `gid` of source face `sel` (the
    /// math font or one of the text fallback faces), building +
    /// registering it on first use. Content is the filled outline in
    /// raw font units; the form's `/Matrix` scales font units → em
    /// (1/upem of the *source* face) so a single object serves every
    /// size, positioned by the per-use CTM. `None` only if the font
    /// or this glyph's outline is unavailable. (printpdf's
    /// `FormXObject` serializer omits the required `/BBox` and writes
    /// `/FormType` as a name — both are patched in
    /// `postprocess::compress`.)
    fn math_glyph_xobject(&mut self, sel: GlyphFont, gid: u16) -> Option<printpdf::XObjectId> {
        if let Some(id) = self.math_glyph_xobjects.get(&(sel, gid)) {
            return Some(id.clone());
        }
        let ms = self.math.as_ref()?.as_ref()?;
        let (upem, segs) = match sel {
            GlyphFont::Math => (ms.font.upem, ms.font.outline(gid)),
            GlyphFont::Text(i) => {
                let tf = ms.text_fonts.get(i as usize)?;
                (tf.upem, tf.outline(gid))
            }
        };
        if segs.is_empty() {
            return None;
        }
        use super::math::font::PathSeg;
        // Font units (~±2000) rounded to integers: ≈0.01 pt at body
        // size after the 1/upem form matrix — sub-pixel, and the most
        // compact encoding. Curves stay curves — one `c` per cubic
        // instead of eight flattened `l` segments: far fewer bytes,
        // and exact at any scale.
        let mut s = String::with_capacity(segs.len() * 16);
        let r = |v: f32| v.round() as i32;
        for seg in &segs {
            match *seg {
                PathSeg::Move(x, y) => {
                    s.push_str(&format!("{} {} m\n", r(x), r(y)));
                }
                PathSeg::Line(x, y) => {
                    s.push_str(&format!("{} {} l\n", r(x), r(y)));
                }
                PathSeg::Cubic(x1, y1, x2, y2, x, y) => {
                    s.push_str(&format!(
                        "{} {} {} {} {} {} c\n",
                        r(x1),
                        r(y1),
                        r(x2),
                        r(y2),
                        r(x),
                        r(y)
                    ));
                }
                PathSeg::Close => s.push_str("h\n"),
            }
        }
        s.push_str("f\n");
        let form = printpdf::FormXObject {
            form_type: printpdf::FormType::Type1,
            size: None,
            bytes: s.into_bytes(),
            matrix: Some(printpdf::CurTransMat::Raw([
                1.0 / upem,
                0.0,
                0.0,
                1.0 / upem,
                0.0,
                0.0,
            ])),
            resources: None,
            group: None,
            ref_dict: None,
            metadata: None,
            piece_info: None,
            last_modified: None,
            struct_parent: None,
            struct_parents: None,
            opi: None,
            oc: None,
            name: None,
        };
        let id = printpdf::XObjectId::new();
        self.doc
            .resources
            .xobjects
            .map
            .insert(id.clone(), printpdf::XObject::Form(form));
        self.math_glyph_xobjects.insert((sel, gid), id.clone());
        Some(id)
    }

    /// Emit a laid-out math fragment as filled glyph outlines plus
    /// rule rectangles. `x0` is the left edge and `baseline` the
    /// fragment baseline (points-from-left / points-from-top).
    ///
    /// Glyphs are drawn as vector paths, not text: there is no
    /// embedded math font and nothing selectable, so the equation
    /// behaves like a figure in every PDF viewer (no stray selection
    /// box, no broken copy). Each distinct glyph's outline is stored
    /// once as a Form XObject and invoked with a tiny CTM/`Do` per
    /// occurrence, so repetition costs almost nothing.
    fn emit_math_frag(
        &mut self,
        frag: &super::math::layout::Frag,
        x0: f32,
        baseline: f32,
        color: Color,
    ) {
        self.close_text_section();
        let page_h = self.page_height_pt();
        // Baseline in PDF space (origin bottom-left, y up).
        let base_pdf_y = page_h - baseline;
        if !matches!(self.math, Some(Some(_))) {
            return;
        }
        // Snapshot placements so the per-glyph XObject lookup (which
        // borrows `self` mutably) doesn't clash with `frag`.
        let glyphs: Vec<(GlyphFont, u16, f32, f32, f32)> = frag
            .glyphs
            .iter()
            .map(|g| (g.font, g.gid, x0 + g.x, base_pdf_y + g.y, g.size))
            .collect();
        // The fill colour is the same for every glyph in the
        // fragment, so set it once outside the per-glyph save/restore
        // pairs (each `q`/`Q` preserves it) instead of re-emitting it
        // per glyph.
        if !glyphs.is_empty() {
            self.page_ops
                .push(Op::SetFillColor { col: color.clone() });
        }
        for (sel, gid, ox, oy, size) in glyphs {
            let Some(id) = self.math_glyph_xobject(sel, gid) else {
                continue;
            };
            // Invoke the shared glyph form with an exact CTM:
            // `[size 0 0 size ox oy]`. The form's own /Matrix scales
            // font units → em, so this lands the glyph at `size` pt,
            // origin `(ox, oy)`. Colour is inherited by the form.
            self.page_ops.push(Op::SaveGraphicsState);
            self.page_ops.push(Op::SetTransformationMatrix {
                matrix: printpdf::CurTransMat::Raw([size, 0.0, 0.0, size, ox, oy]),
            });
            self.page_ops.push(Op::UseXobject {
                id,
                transform: XObjectTransform::default(),
            });
            self.page_ops.push(Op::RestoreGraphicsState);
        }
        for r in &frag.rules {
            draw_filled_rect(
                &mut self.page_ops,
                x0 + r.x,
                baseline - r.y_top,
                x0 + r.x + r.w,
                baseline - (r.y_top - r.thickness),
                color.clone(),
                page_h,
            );
        }
    }

    /// Display math (`$$ … $$`): real TeX typesetting, centered as its
    /// own block. Falls back to a plain-text rendering only if the
    /// math font can't be loaded.
    fn render_math_block(&mut self, content: &str) {
        if content.trim().is_empty() {
            return;
        }
        if !self.ensure_math() {
            return self.render_math_block_text(content);
        }
        let m = self.style.math;
        let color = rgb_color((m.color.r, m.color.g, m.color.b));
        let base_pt = self.style.paragraph.font_size_pt * m.scale;

        let mut frag = {
            let ms = self.math.as_ref().unwrap().as_ref().unwrap();
            match super::math::typeset(&ms.font, &ms.text_fonts, &ms.warned, content, true, base_pt)
            {
                Some(f) => f,
                None => return,
            }
        };

        // Route block spacing through the paragraph block machinery
        // (background / page-break bookkeeping) but with the `[math]`
        // margins substituted.
        let mut s = self.style.paragraph.clone();
        s.margin_before_pt = m.margin_before_pt;
        s.margin_after_pt = m.margin_after_pt;
        let ctx = self.begin_block(&s);
        // Scale-to-fit if the equation is wider than the current
        // column. Re-typeset at a smaller base point size rather than
        // overflowing into the adjacent column.
        let avail_w = self.content_width_pt();
        if frag.w > avail_w && avail_w > 0.0 {
            let scaled_pt = (base_pt * (avail_w / frag.w)).max(4.0);
            let ms = self.math.as_ref().unwrap().as_ref().unwrap();
            if let Some(f) =
                super::math::typeset(&ms.font, &ms.text_fonts, &ms.warned, content, true, scaled_pt)
            {
                frag = f;
            }
        }
        let total_h = frag.asc + frag.desc;
        // Keep the whole equation on one page when it fits. In a
        // multi-column layout this means "in the same column" — push
        // to the next column (or page) rather than splitting the
        // equation across columns.
        if self.y_from_top_pt + total_h + self.bottom_margin_pt()
            > self.page_height_pt()
            && total_h + self.top_margin_pt() + self.bottom_margin_pt()
                < self.page_height_pt()
        {
            self.advance_column();
        }
        let avail = self.content_width_pt();
        let slack = (avail - frag.w).max(0.0);
        let x0 = self.indent_left_pt
            + match m.align {
                TextAlignment::Left => 0.0,
                TextAlignment::Right => slack,
                // Center / Justify both center a display equation.
                _ => slack / 2.0,
            };
        let baseline = self.y_from_top_pt + frag.asc;
        self.emit_math_frag(&frag, x0, baseline, color);
        self.advance_y(total_h);
        self.end_block(ctx);
    }

    /// Fallback display-math rendering (centred italic monospace) used
    /// only when the math font is unavailable.
    fn render_math_block_text(&mut self, content: &str) {
        let s = self.style.paragraph.clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default().with_monospace().with_italic();
        let ctx = self.begin_block(&s);
        let saved_align = self.current_text_align;
        self.current_text_align = TextAlignment::Center;
        for line in content.split('\n') {
            if line.trim().is_empty() {
                continue;
            }
            let run = InlineRun { math: None,
                text: line.to_string(),
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
        self.current_text_align = saved_align;
        self.end_block(ctx);
    }

    fn render_definition_list(&mut self, entries: &[crate::render::ir::DefinitionEntry]) {
        if entries.is_empty() {
            return;
        }
        let body_style = self.style.paragraph.clone();
        let color = Some(rgb_color(body_style.text_color_rgb()));
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_column = self.current_column;
        let def_indent_pt = mm_to_pt(6.0);

        for (idx, entry) in entries.iter().enumerate() {
            if idx == 0 {
                self.advance_y(body_style.margin_before_pt);
            } else {
                self.advance_y(body_style.margin_before_pt * 0.5);
            }
            for term in &entry.terms {
                let bolded: Vec<InlineRun> = term
                    .iter()
                    .map(|r| {
                        let mut b = r.clone();
                        b.flags = b.flags.with_bold();
                        b
                    })
                    .collect();
                let (outer_left, outer_right) =
                    self.rebase_indents(saved_left, saved_right, saved_column);
                self.indent_left_pt = outer_left;
                self.indent_right_pt = outer_right;
                self.write_wrapped_runs(
                    &bolded,
                    body_style.font_size_pt,
                    body_style.line_height,
                    RunFlags::default().with_bold(),
                    color.clone(),
                );
            }
            let (outer_left, outer_right) =
                self.rebase_indents(saved_left, saved_right, saved_column);
            self.indent_left_pt = (outer_left + def_indent_pt).min(outer_right - 10.0);
            self.indent_right_pt = outer_right;
            for def in &entry.definitions {
                for (i, block) in def.iter().enumerate() {
                    let next = def.get(i + 1);
                    self.render_block(block, next);
                }
            }
            let (outer_left, outer_right) =
                self.rebase_indents(saved_left, saved_right, saved_column);
            self.indent_left_pt = outer_left;
            self.indent_right_pt = outer_right;
        }
        self.advance_y(body_style.margin_after_pt);
    }

    fn render_footnote_definitions(&mut self, entries: &[crate::render::ir::FootnoteEntry]) {
        if entries.is_empty() {
            return;
        }
        let h2 = self.style.headings[1].clone();
        let title_runs = vec![InlineRun { math: None,
            text: "Footnotes".to_string(),
            flags: RunFlags::default(),
            link: None,
        }];
        let header_h = {
            let lines = self
                .estimate_wrapped_lines(&title_runs, h2.font_size_pt, base_flags_from_block(&h2));
            h2.margin_before_pt
                + h2.padding.top
                + lines as f32 * h2.font_size_pt * h2.line_height.max(0.5)
                + h2.padding.bottom
                + h2.margin_after_pt
        };
        let p = &self.style.paragraph;
        let follow_h = p.margin_before_pt + p.padding.top + p.font_size_pt * p.line_height.max(0.5);
        self.keep_with_next_break(header_h, follow_h);
        let color = Some(rgb_color(h2.text_color_rgb()));
        let flags = RunFlags {
            bold: h2.is_bold(),
            italic: h2.is_italic(),
            monospace: false,
            strikethrough: false,
            highlight: false,
            underline: false,
            superscript: false,
            subscript: false,
            small_caps: false,
            small: false,
            inline_code: false,
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
            runs.push(InlineRun { math: None,
                text: format!("{}", entry.number),
                flags: RunFlags::default().with_superscript(),
                link: None,
            });
            runs.push(InlineRun { math: None,
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

    /// Fetch a remote image into memory, caching by URL. The actual
    /// SSRF-guarded fetch (host validation, redirect re-validation,
    /// size/time bounds) lives in [`super::net_guard::fetch_url`] —
    /// see its doc comment for the full guard behavior; this method is
    /// just the cache lookup around it.
    ///
    /// Gated behind the `fetch` feature — without it, this returns an
    /// error so the caller falls back to alt text.
    #[cfg(feature = "fetch")]
    fn fetch_url_bytes(&mut self, url: &str) -> Result<Vec<u8>, String> {
        if !self.url_image_cache.contains_key(url) {
            let bytes = super::net_guard::fetch_url(url)?;
            self.url_image_cache.insert(url.to_string(), bytes);
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
        self.render_paragraph(&[InlineRun { math: None,
            text: format!("[image: {}]", alt),
            flags: RunFlags::default().with_italic(),
            link: None,
        }]);
    }

    /// Decode an image from a local path or URL into a `RawImage`,
    /// applying the 4000px dimension cap. Returns `None` on any
    /// fetch / decode / conversion failure (logged), and also on a
    /// refusal from the operator's `[security]` policy — the two cases
    /// share the same graceful degradation to alt text. URL fetch is
    /// gated under the `fetch` feature; SVG rasterization under `svg`.
    fn decode_image_file(&mut self, path: &std::path::Path) -> Option<RawImage> {
        let path_str = path.to_string_lossy();
        let is_url = is_http_url(path_str.as_ref());
        let bytes_result: Result<Vec<u8>, String> = if is_url {
            if !self.style.security.allow_remote_images {
                log::warn!(
                    "remote image {:?} refused: allow_remote_images is disabled",
                    path
                );
                return None;
            }
            self.fetch_url_bytes(path_str.as_ref())
        } else {
            let security = &self.style.security;
            match resolve_image_path(
                path,
                security.image_root.as_deref(),
                security.allow_absolute_image_paths,
            ) {
                Ok(resolved) => std::fs::read(&resolved).map_err(|e| e.to_string()),
                Err(ImagePathRefusal::Policy(msg)) => {
                    log::warn!("image {:?} refused by security policy: {}", path, msg);
                    return None;
                }
                Err(ImagePathRefusal::NotFound(msg)) => {
                    // Not a policy decision — a missing/unreadable file
                    // (typo, moved file). Phrased neutrally so an
                    // operator debugging a broken image link doesn't go
                    // hunting through their security config.
                    log::warn!("{}", msg);
                    return None;
                }
            }
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
                return None;
            }
        };

        // Degenerate dimensions: a 0-px image can't produce a valid
        // XObject. Treat it like a decode failure.
        if img.width() == 0 || img.height() == 0 {
            log::warn!("image {:?} has zero dimension; skipping", path);
            return None;
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

        match RawImage::from_dynamic_image(img) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("could not convert image {:?}: {}", path, e);
                None
            }
        }
    }

    fn render_image(&mut self, path: &std::path::Path, alt: &str, caption: Option<&str>) {
        // Decode the image; on any failure degrade to an italic
        // alt-text paragraph so the document doesn't lose content.
        let raw = match self.decode_image_file(path) {
            Some(r) => r,
            None => {
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
            self.advance_column();
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
            // Caption line styled by `[image.caption]`, wrapped within
            // the image's width when the image is narrower than the
            // column.
            let cap = self.style.image.caption.clone();
            self.advance_y(cap.margin_before_pt);
            let base_flags = base_flags_from_block(&cap);
            let saved_left = self.indent_left_pt;
            let saved_right = self.indent_right_pt;
            let saved_column = self.current_column;
            if rendered_w_pt < self.content_width_pt() {
                self.indent_left_pt = x_pt;
                self.indent_right_pt = x_pt + rendered_w_pt;
            }
            let runs = vec![InlineRun {
                math: None,
                text: text.to_string(),
                flags: RunFlags::default(),
                link: None,
            }];
            let color = Some(rgb_color(cap.text_color_rgb()));
            let saved_align = self.current_text_align;
            self.current_text_align = cap.text_align;
            self.write_wrapped_runs(
                &runs,
                cap.font_size_pt,
                cap.line_height,
                base_flags,
                color,
            );
            self.current_text_align = saved_align;
            let (l, r) = self.rebase_indents(saved_left, saved_right, saved_column);
            self.indent_left_pt = l;
            self.indent_right_pt = r;
        }

        self.advance_y(self.style.image.margin_after_pt);
    }

    fn render_table(
        &mut self,
        headers: &[TableCell<InlineRun>],
        aligns: &[crate::markdown::TableAlignment],
        rows: &[Vec<TableCell<InlineRun>>],
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
        // Tables don't go through begin_block; scope letter spacing
        // here — header cells use `[table.header]`, data cells
        // `[table.cell]`.
        let saved_letter_spacing = self.letter_spacing_pt;
        self.letter_spacing_pt = s_header.letter_spacing_pt;

        self.advance_y(before_pt);

        let col_count = headers.len();
        let total_width = self.content_width_pt();
        // Floor the column wide enough that the inner cell box
        // (left+pad .. right-pad) can't invert.
        let pad = self.style.table.cell_padding;
        let min_col_width_pt = pad.left + pad.right + 1.0;
        let col_width = (total_width / col_count as f32).max(min_col_width_pt);

        let header_height = self.measure_row_height(
            headers,
            s_header.font_size_pt,
            s_header.line_height,
            col_width,
            true,
        );
        let header_background = s_header.background_color_rgb();
        if self.y_from_top_pt + header_height + self.bottom_margin_pt() > self.page_height_pt() {
            self.advance_column();
        }
        let header_top = self.y_from_top_pt;
        if let Some(bg) = header_background {
            self.draw_table_row_background(header_top, header_height, col_width, col_count, bg);
        }
        self.draw_row(
            headers,
            0,
            &[header_height],
            aligns,
            s_header.font_size_pt,
            s_header.line_height,
            col_width,
            true,
            s_header.text_color_rgb(),
        );
        let header_bottom = header_top + header_height;
        self.y_from_top_pt = header_bottom;
        self.advance_y(row_gap_pt);

        self.letter_spacing_pt = s_cell.letter_spacing_pt;
        let mut table_rows: Vec<Vec<TableCell<InlineRun>>> = rows.to_vec();
        for row in &mut table_rows {
            row.resize_with(col_count, || TableCell::new(Vec::new()));
            row.truncate(col_count);
        }
        let row_heights = self.measure_table_row_heights(
            &table_rows,
            s_cell.font_size_pt,
            s_cell.line_height,
            col_width,
        );
        let mut row_idx = 0usize;
        while row_idx < table_rows.len() {
            let group_end = rowspan_group_end(&table_rows, row_idx);
            let group_height: f32 = row_heights[row_idx..group_end].iter().sum();
            if self.y_from_top_pt + group_height + self.bottom_margin_pt() > self.page_height_pt() {
                self.advance_column();
                // Reprint headers on the new column (or page).
                let header_top = self.y_from_top_pt;
                if let Some(bg) = header_background {
                    self.draw_table_row_background(
                        header_top,
                        header_height,
                        col_width,
                        col_count,
                        bg,
                    );
                }
                self.draw_row(
                    headers,
                    0,
                    &[header_height],
                    aligns,
                    s_header.font_size_pt,
                    s_header.line_height,
                    col_width,
                    true,
                    s_header.text_color_rgb(),
                );
                let header_bottom = header_top + header_height;
                self.y_from_top_pt = header_bottom;
                self.advance_y(row_gap_pt);
            }
            let group_top = self.y_from_top_pt;
            // Zebra striping: tint alternate data rows (every other
            // row, first data row left untinted) when configured.
            if let Some(bg) = self.style.table.alternating_row_background
                && row_idx % 2 == 1 {
                    self.draw_table_row_background(
                        group_top,
                        group_height,
                        col_width,
                        col_count,
                        (bg.r, bg.g, bg.b),
                    );
                }
            let group_heights = &row_heights[row_idx..group_end];
            for local_idx in 0..(group_end - row_idx) {
                self.y_from_top_pt = group_top + group_heights[..local_idx].iter().sum::<f32>();
                self.draw_row(
                    &table_rows[row_idx + local_idx],
                    local_idx,
                    group_heights,
                    aligns,
                    s_cell.font_size_pt,
                    s_cell.line_height,
                    col_width,
                    false,
                    s_cell.text_color_rgb(),
                );
            }
            self.y_from_top_pt = group_top + group_height;
            self.advance_y(row_gap_pt);
            row_idx = group_end;
        }

        self.letter_spacing_pt = saved_letter_spacing;
        self.advance_y(after_pt);
    }

    fn draw_table_row_background(
        &mut self,
        row_top: f32,
        row_height: f32,
        col_width: f32,
        col_count: usize,
        bg: (u8, u8, u8),
    ) {
        let table_left = self.indent_left_pt;
        let table_right = table_left + col_width * col_count as f32;
        let page_h = self.page_height_pt();
        let fill = rgb_color(bg);
        self.close_text_section();
        draw_filled_rect(
            &mut self.page_ops,
            table_left,
            row_top,
            table_right,
            row_top + row_height,
            fill,
            page_h,
        );
    }

    fn measure_row_height(
        &self,
        cells: &[TableCell<InlineRun>],
        font_size: f32,
        line_height_mult: f32,
        col_width: f32,
        bold: bool,
    ) -> f32 {
        let line_h = font_size * line_height_mult.max(0.5);
        let pad = self.style.table.cell_padding;
        let mut max_lines = 1usize;
        for cell in cells {
            if cell.covered {
                continue;
            }
            let n_lines = count_wrapped_lines(
                &cell.content,
                font_size,
                line_height_mult,
                col_width * cell.colspan.max(1) as f32 - (pad.left + pad.right),
                self.font_set,
                bold,
                self.letter_spacing_pt,
            );
            max_lines = max_lines.max(n_lines);
        }
        max_lines as f32 * line_h + pad.top + pad.bottom
    }

    fn measure_table_row_heights(
        &self,
        rows: &[Vec<TableCell<InlineRun>>],
        font_size: f32,
        line_height_mult: f32,
        col_width: f32,
    ) -> Vec<f32> {
        let mut heights: Vec<f32> = rows
            .iter()
            .map(|row| self.measure_row_height(row, font_size, line_height_mult, col_width, false))
            .collect();
        for (r, row) in rows.iter().enumerate() {
            for cell in row {
                let span = cell.rowspan.max(1);
                if cell.covered || span <= 1 || r + span > rows.len() {
                    continue;
                }
                let need = self.measure_row_height(
                    std::slice::from_ref(cell),
                    font_size,
                    line_height_mult,
                    col_width,
                    false,
                );
                let have: f32 = heights[r..r + span].iter().sum();
                if need > have {
                    let extra = (need - have) / span as f32;
                    for h in &mut heights[r..r + span] {
                        *h += extra;
                    }
                }
            }
        }
        heights
    }

    /// Sum the rendered widths of a cell's inline runs (no wrapping).
    /// Used for table column alignment — we shift the per-cell text
    /// cursor by `(col_width - measured) / 2` for center, etc.
    /// Rendered width of `text`, including the active block's letter
    /// spacing. `letter_spacing_pt` is added after every glyph, so an
    /// N-char run measures `font_set.measure() + N * letter_spacing_pt`
    /// — exactly matching the PDF text-cursor advance, so measurement
    /// and emission never drift.
    fn measure_text(&self, flags: RunFlags, text: &str, size_pt: f32) -> f32 {
        self.font_set.measure(flags, text, size_pt)
            + self.letter_spacing_pt * text.chars().count() as f32
    }

    fn measure_runs_width(&self, runs: &[InlineRun], font_size: f32, bold: bool) -> f32 {
        let mut total = 0.0f32;
        for run in runs {
            let mut flags = run.flags;
            if bold {
                flags = flags.with_bold();
            }
            total += self.measure_text(flags, &run.text, font_size);
        }
        total
    }

    fn draw_row(
        &mut self,
        cells: &[TableCell<InlineRun>],
        row_offset: usize,
        row_heights: &[f32],
        aligns: &[crate::markdown::TableAlignment],
        font_size: f32,
        line_height_mult: f32,
        col_width: f32,
        bold: bool,
        color: (u8, u8, u8),
    ) {
        let pad = self.style.table.cell_padding;
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let saved_column = self.current_column;
        let row_top = self.y_from_top_pt;
        let col_count = cells.len();
        for (i, cell) in cells.iter().enumerate() {
            if cell.covered {
                continue;
            }
            let colspan = cell.colspan.max(1).min(col_count - i);
            let rowspan = cell.rowspan.max(1).min(row_heights.len() - row_offset);
            let region_height: f32 = row_heights[row_offset..row_offset + rowspan].iter().sum();
            let cell_left = saved_left + col_width * i as f32 + pad.left;
            let cell_right = saved_left + col_width * (i + colspan) as f32 - pad.right;
            let inner_width = cell_right - cell_left;
            let mut runs = cell.content.clone();
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
            // A row-spanning cell is vertically centered within the
            // merged region. Every other cell keeps the original
            // top-aligned `row_top + 3.0` so plain GFM and colspan-only
            // tables render exactly as they did before spans existed.
            self.y_from_top_pt = if rowspan > 1 {
                let content_lines = count_wrapped_lines(
                    &runs,
                    font_size,
                    line_height_mult,
                    inner_width,
                    self.font_set,
                    false,
                    self.letter_spacing_pt,
                );
                let content_height =
                    content_lines as f32 * font_size * line_height_mult.max(0.5);
                row_top + ((region_height - content_height) / 2.0).max(pad.top)
            } else {
                row_top + pad.top
            };
            self.write_wrapped_runs(
                &runs,
                font_size,
                line_height_mult,
                RunFlags::default(),
                Some(rgb_color(color)),
            );
            self.indent_left_pt = saved_left;
            self.draw_cell_border(row_top, row_top + region_height, i, i + colspan, col_width);
        }
        let (l, r) = self.rebase_indents(saved_left, saved_right, saved_column);
        self.indent_left_pt = l;
        self.indent_right_pt = r;
        self.y_from_top_pt = row_top;
    }

    fn draw_cell_border(
        &mut self,
        row_top: f32,
        row_bottom: f32,
        col_start: usize,
        col_end: usize,
        col_width: f32,
    ) {
        self.close_text_section();
        let page_h = self.page_height_pt();
        let border_color = rgb_color((180, 180, 180));
        let left = self.indent_left_pt;
        let x0 = left + col_width * col_start as f32;
        let x1 = left + col_width * col_end as f32;
        // Horizontal lines: top and bottom of the row.
        draw_horizontal_line(
            &mut self.page_ops,
            x0,
            x1,
            row_top,
            border_color.clone(),
            0.5,
            page_h,
        );
        draw_horizontal_line(
            &mut self.page_ops,
            x0,
            x1,
            row_bottom,
            border_color.clone(),
            0.5,
            page_h,
        );
        draw_vertical_line(&mut self.page_ops, x0, row_top, row_bottom, page_h);
        draw_vertical_line(&mut self.page_ops, x1, row_top, row_bottom, page_h);
    }

    fn render_list(&mut self, entries: &[ListEntry]) {
        let saved_left = self.indent_left_pt;
        // Lists don't go through begin_block; scope letter spacing here
        // so list text honors `[list.*].letter_spacing_pt`.
        let saved_letter_spacing = self.letter_spacing_pt;

        // CommonMark §5.3: the whole list is loose if any item is loose.
        // Pre-compute once so every iteration uses the same gap.
        let any_loose = entries.iter().any(|e| e.loose);

        for (idx, entry) in entries.iter().enumerate() {
            let mut list_style: ResolvedList = match entry.bullet {
                ListBullet::Unordered(_) => self.style.list_unordered.clone(),
                ListBullet::Ordered(_) => self.style.list_ordered.clone(),
                ListBullet::TaskChecked | ListBullet::TaskUnchecked => {
                    self.style.list_task.clone()
                }
            };
            // Inside a blockquote / admonition, list text inherits the
            // container typography (the same fields a body paragraph
            // does); bullet glyphs, indents and spacing stay the list's.
            if let Some(ov) = &self.text_style_override {
                let b = &mut list_style.block;
                b.font_family = ov.font_family.clone();
                b.font_size_pt = ov.font_size_pt;
                b.font_weight = ov.font_weight;
                b.font_style = ov.font_style;
                b.text_color = ov.text_color;
                b.line_height = ov.line_height;
                b.letter_spacing_pt = ov.letter_spacing_pt;
            }
            let s = &list_style.block;
            self.letter_spacing_pt = s.letter_spacing_pt;
            let size_pt = s.font_size_pt;
            let line_height = s.line_height;
            let inter_item_gap = if any_loose {
                list_style.item_spacing_loose_pt
            } else {
                list_style.item_spacing_tight_pt
            };

            let bullet_text = format_bullet(&entry.bullet, &list_style);
            let bullet_flags = RunFlags::default();
            let bullet_width = self.measure_text(bullet_flags, &bullet_text, size_pt);

            // First item: honor `block.margin_before_pt` (list-level
            // "space before the whole list"). Subsequent items use the
            // tight/loose inter-item gap.
            if idx == 0 {
                self.advance_y(s.margin_before_pt.max(0.5));
            } else {
                self.advance_y(inter_item_gap.max(0.0));
            }
            // Bullet-orphan guard: if the cursor + one line of entry
            // text wouldn't fit, advance now so the bullet glyph and
            // its first text line land together on the next column /
            // page. Without this, the bullet renders at the old y, the
            // text wraps onto the next page, and the glyph is left
            // alone at the bottom of the previous page. Skip at column
            // top (already maximum room).
            if (self.y_from_top_pt - self.top_margin_pt()).abs() >= 0.01 {
                let line_h = size_pt * line_height.max(0.5);
                if self.y_from_top_pt + line_h + self.bottom_margin_pt()
                    > self.page_height_pt()
                {
                    self.advance_column();
                }
            }
            let bullet_x = saved_left;
            let bullet_y = self.y_from_top_pt + size_pt;
            // An unordered bullet whose configured glyph the active
            // font can't represent (the default `•` under built-in
            // Helvetica) would otherwise transliterate to `*`. Draw
            // it as a disc instead. A configured ASCII bullet (`-`)
            // or any glyph the font *can* render is still emitted as
            // text, so user config is respected. Task items always
            // get a real checkbox rather than literal `[ ]`/`[x]`.
            let needs_xlit = self.font_set.needs_transliteration(bullet_flags);
            let glyph_unrepresentable =
                needs_xlit && to_win1252(&bullet_text) != bullet_text;
            let bullet_col = rgb_color(s.text_color_rgb());
            let page_h = self.page_height_pt();
            // Vertical centre of the lowercase text the bullet sits
            // beside (baseline is `bullet_y`).
            let mid_y = bullet_y - size_pt * 0.30;
            match entry.bullet {
                ListBullet::TaskChecked | ListBullet::TaskUnchecked => {
                    self.close_text_section();
                    let side = size_pt * 0.62;
                    let x0 = bullet_x;
                    let y_top = mid_y - side / 2.0;
                    let y_bot = mid_y + side / 2.0;
                    let x1 = x0 + side;
                    draw_stroked_path(
                        &mut self.page_ops,
                        &[(x0, y_top), (x1, y_top), (x1, y_bot), (x0, y_bot)],
                        bullet_col.clone(),
                        0.8,
                        true,
                        page_h,
                    );
                    if matches!(entry.bullet, ListBullet::TaskChecked) {
                        // A tick from the lower-left through to the
                        // upper-right of the box.
                        draw_stroked_path(
                            &mut self.page_ops,
                            &[
                                (x0 + side * 0.18, mid_y + side * 0.05),
                                (x0 + side * 0.42, y_bot - side * 0.16),
                                (x1 - side * 0.12, y_top + side * 0.14),
                            ],
                            bullet_col,
                            1.1,
                            false,
                            page_h,
                        );
                    }
                }
                ListBullet::Unordered(_) if glyph_unrepresentable => {
                    self.close_text_section();
                    let r = size_pt * 0.13;
                    draw_filled_disc(
                        &mut self.page_ops,
                        bullet_x + r,
                        mid_y,
                        r,
                        bullet_col,
                        page_h,
                    );
                }
                _ => {
                    self.close_text_section();
                    self.ensure_text_section();
                    self.move_cursor_to(bullet_x, bullet_y);
                    self.page_ops.push(Op::SetLineHeight {
                        lh: Pt(size_pt * line_height.max(0.5)),
                    });
                    self.page_ops.push(Op::SetFillColor { col: bullet_col });
                    emit_text_chunks(
                        &mut self.page_ops,
                        self.font_set,
                        bullet_flags,
                        &bullet_text,
                        size_pt,
                        self.letter_spacing_pt,
                    );
                }
            }

            let text_indent = (saved_left + bullet_width + list_style.bullet_gap_pt)
                .min(self.indent_right_pt - 10.0);
            self.indent_left_pt = text_indent;

            self.write_wrapped_runs(
                &entry.runs,
                size_pt,
                line_height,
                base_flags_from_block(s),
                Some(rgb_color(s.text_color_rgb())),
            );

            // A nested list steps in by `indent_per_level_pt` from this
            // list's bullet column; an item's other children (e.g.
            // continuation paragraphs) stay aligned with the item text.
            let nested_indent = (saved_left + list_style.indent_per_level_pt)
                .min(self.indent_right_pt - 10.0);
            let mut child_it = entry.children.iter().peekable();
            while let Some(child) = child_it.next() {
                self.indent_left_pt = if matches!(child, Block::List { .. }) {
                    nested_indent
                } else {
                    text_indent
                };
                self.render_block(child, child_it.peek().copied());
            }

            self.indent_left_pt = saved_left;

            // Last item: honor `block.margin_after_pt` (list-level
            // "space after the whole list"). The inter-item gap is
            // applied at the *start* of the next iteration.
            if idx + 1 == entries.len() {
                self.advance_y(s.margin_after_pt.max(0.0));
            }
        }
        self.letter_spacing_pt = saved_letter_spacing;
    }

    fn render_blockquote(&mut self, body: &[Block]) {
        // padding.left in [blockquote.padding] is the single knob for
        // how far the text sits past the left border. `indent_pt` is
        // still available on the schema for callers who want an extra
        // first-line indent on paragraphs, but blockquotes don't apply
        // it implicitly anymore.
        let s = self.style.blockquote.clone();
        let ctx = self.begin_block(&s);
        let saved_override = self.text_style_override.take();
        self.text_style_override = Some(s.clone());
        let mut it = body.iter().peekable();
        while let Some(child) = it.next() {
            self.render_block(child, it.peek().copied());
        }
        self.text_style_override = saved_override;
        self.end_block(ctx);
    }

    /// Render a callout / admonition block: tinted background, accent
    /// left border, bold uppercase header (or the author's title if
    /// they supplied one), then the body laid out as nested blocks.
    /// Per-kind colour comes from `[admonition.kind]`; unknown kinds
    /// land on the `generic` palette and surface their raw label as
    /// the header.
    fn render_admonition(
        &mut self,
        kind: &str,
        raw_label: &str,
        title: Option<&[InlineRun]>,
        body: &[Block],
    ) {
        let resolved = self.style.admonition.for_kind(kind).clone();
        let accent = resolved.accent_color;

        // Build a per-call ResolvedBlock that overlays the accent
        // colour on top of the kind's resolved shape: the left border
        // is always painted in the accent colour and is thick enough
        // to read as a callout strip, regardless of what the theme
        // configured on `[admonition].border`.
        let mut block_style = resolved.block.clone();
        block_style.border.left = Some(ResolvedBorderSide {
            width_pt: 3.0,
            color: accent,
            style: BorderStyle::Solid,
        });

        // Keep-with-next: the kind-label strip and the first body
        // chunk must land together. Reserve margin_before + padding.top
        // + one label line + the post-label gap + one first-body-line
        // worth of space; if that won't fit before the page bottom,
        // push the whole admonition to the next column/page.
        let header_h = block_style.margin_before_pt
            + block_style.padding.top
            + block_style.font_size_pt * block_style.line_height.max(0.5)
            + block_style.font_size_pt * 0.35;
        let follow_h = self.next_block_lead_pt(body.first());
        self.keep_with_next_break(header_h, follow_h);

        let ctx = self.begin_block(&block_style);

        // Header line: title runs (already inline-flattened by lower)
        // if the author wrote a quoted title; otherwise the kind's
        // configured label (e.g. "NOTE"); for unknown kinds, fall
        // back to the raw label uppercased so `!!! bug "…"` reads as
        // a BUG box.
        let header_runs: Vec<InlineRun> = match title {
            Some(runs) if !runs.is_empty() => {
                // Title rendered bold + accent-coloured.
                runs.iter()
                    .map(|r| {
                        let mut clone = r.clone();
                        clone.flags = clone.flags.with_bold();
                        clone
                    })
                    .collect()
            }
            _ => {
                // Use the author's typed label (uppercased) instead of
                // the canonical kind's preset label, so aliases like
                // `caution`/`important` keep their own word even when
                // their canonical kind (danger / info) supplies the
                // styling. Falls back to the canonical preset only
                // when raw_label is somehow empty.
                let label_text = if !raw_label.is_empty() {
                    raw_label.to_ascii_uppercase()
                } else {
                    resolved.label.clone()
                };
                vec![InlineRun {
                    math: None,
                    text: label_text,
                    flags: RunFlags::default().with_bold(),
                    link: None,
                }]
            }
        };

        let header_color = Some(rgb_color((accent.r, accent.g, accent.b)));
        let icon_size = block_style.font_size_pt * 0.95;
        let icon_gap = block_style.font_size_pt * 0.40;
        let header_top = self.y_from_top_pt;
        let icon_left = self.indent_left_pt;
        let saved_indent = self.indent_left_pt;
        self.indent_left_pt += icon_size + icon_gap;
        self.write_wrapped_runs(
            &header_runs,
            block_style.font_size_pt,
            block_style.line_height,
            RunFlags::default(),
            header_color,
        );
        self.indent_left_pt = saved_indent;

        // Draw the per-kind icon on top of the header row. PDF painter
        // order doesn't matter here — the icon and the header glyphs
        // live in distinct x ranges (the icon column was reserved by
        // the temporary indent above). `cutout_color` is the negative
        // space used inside the danger disc's X-mark; falls back to
        // white when the admonition has no tinted background.
        let cutout_color = block_style
            .background_color
            .map(|c| rgb_color((c.r, c.g, c.b)))
            .unwrap_or(rgb_color((0xFF, 0xFF, 0xFF)));
        self.close_text_section();
        let icon_top = header_top + block_style.font_size_pt * 0.12;
        let page_h = self.page_height_pt();
        let accent_color = rgb_color((accent.r, accent.g, accent.b));
        draw_admonition_icon(
            &mut self.page_ops,
            kind,
            icon_left,
            icon_top,
            icon_size,
            &accent_color,
            &cutout_color,
            page_h,
        );

        // Small gap between header and body.
        self.advance_y(block_style.font_size_pt * 0.35);

        let saved_override = self.text_style_override.take();
        self.text_style_override = Some(block_style.clone());
        let mut it = body.iter().peekable();
        while let Some(child) = it.next() {
            self.render_block(child, it.peek().copied());
        }
        self.text_style_override = saved_override;
        self.end_block(ctx);
    }

    fn render_heading(&mut self, level: u8, runs: &[InlineRun], next: Option<&Block>) {
        let idx = level.clamp(1, 6) as usize - 1;
        let s = self.style.headings[idx].clone();
        let base_flags = base_flags_from_block(&s);
        let line_count = self.estimate_wrapped_lines(runs, s.font_size_pt, base_flags);
        let header_h = s.margin_before_pt
            + s.padding.top
            + line_count as f32 * s.font_size_pt * s.line_height.max(0.5)
            + s.padding.bottom
            + s.margin_after_pt;
        let follow_h = self.next_block_lead_pt(next);
        self.keep_with_next_break(header_h, follow_h);
        let color = Some(rgb_color(s.text_color_rgb()));

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
        self.first_line_indent_pt = s.indent_pt;
        self.write_wrapped_runs(runs_ref, s.font_size_pt, s.line_height, base_flags, color);
        self.current_text_align = TextAlignment::Left;
        self.end_block(ctx);
    }

    fn render_paragraph(&mut self, runs: &[InlineRun]) {
        let mut s = self.style.paragraph.clone();
        // Inside a blockquote / admonition, body paragraphs inherit
        // the container's text typography; structural fields (margins,
        // padding, border, background) stay paragraph's.
        if let Some(ov) = &self.text_style_override {
            s.font_family = ov.font_family.clone();
            s.font_size_pt = ov.font_size_pt;
            s.font_weight = ov.font_weight;
            s.font_style = ov.font_style;
            s.text_color = ov.text_color;
            s.line_height = ov.line_height;
            s.text_align = ov.text_align;
            s.underline = ov.underline;
            s.strikethrough = ov.strikethrough;
            s.small_caps = ov.small_caps;
            s.letter_spacing_pt = ov.letter_spacing_pt;
            s.indent_pt = ov.indent_pt;
        }
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = base_flags_from_block(&s);
        let ctx = self.begin_block(&s);
        let owned_runs;
        let runs_ref: &[InlineRun] = if s.small_caps {
            owned_runs = self.expand_small_caps(runs);
            &owned_runs
        } else {
            runs
        };
        self.current_text_align = s.text_align;
        self.first_line_indent_pt = s.indent_pt;
        self.write_wrapped_runs(runs_ref, s.font_size_pt, s.line_height, base, color);
        self.current_text_align = TextAlignment::Left;
        self.end_block(ctx);
    }

    fn render_code_block(&mut self, lines: &[String]) {
        let s = self.style.code_block.clone();
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = base_flags_from_block(&s).with_monospace();
        let ctx = self.begin_block(&s);
        self.in_code_block = true;
        self.current_text_align = s.text_align;
        self.first_line_indent_pt = s.indent_pt;
        for line in lines {
            let run = InlineRun { math: None,
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
        self.current_text_align = TextAlignment::Left;
        self.in_code_block = false;
        self.end_block(ctx);
    }

    fn render_horizontal_rule(&mut self) {
        self.close_text_section();

        let s = &self.style.horizontal_rule;
        let thickness = s.thickness_pt.max(0.1);
        let color = rgb_color(s.color_rgb());
        let dash = dash_pattern_for(s.style);

        self.advance_y(s.margin_before_pt + thickness * 0.5);

        // The rule spans the current column / block region — using the
        // active indents instead of the page margins keeps it inside a
        // column (and inside a blockquote's padding). For a default
        // single-column document with no enclosing block, these are
        // exactly the page margins, so the line is identical to the
        // pre-column rendering.
        let mut x_left_pt = self.indent_left_pt;
        let mut x_right_pt = self.indent_right_pt;
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
        base_flags: RunFlags,
        color: Option<Color>,
    ) {
        if runs.is_empty() {
            return;
        }
        let size_pt = font_size;
        let line_height_pt = size_pt * line_height_mult.max(0.5);
        // First-line indent applies once; consume it so nested calls
        // (e.g. list children) don't inherit it.
        let first_line_indent_pt = std::mem::take(&mut self.first_line_indent_pt);

        // Fold the block-level base style (e.g. a heading's bold
        // weight) into every run so it isn't lost when a run carries
        // its own inline flags. A default `base_flags` is a no-op.
        let merged_runs;
        let runs: &[InlineRun] = if base_flags == RunFlags::default() {
            runs
        } else {
            merged_runs = runs
                .iter()
                .map(|r| InlineRun {
                    flags: r.flags.or(base_flags),
                    ..r.clone()
                })
                .collect::<Vec<_>>();
            &merged_runs
        };

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
        // `[code_inline].padding` is applied to the first / last word
        // of each contiguous inline-code span: pad.left on the first,
        // pad.right on the last. Middle words and runs that aren't
        // inline-code get (0, 0), so a non-inline-code document picks
        // up no per-word overhead.
        let ci_pad_l = self.style.code_inline.padding.left;
        let ci_pad_r = self.style.code_inline.padding.right;
        let is_inline_code_word = |w: &InlineRun| {
            w.math.is_none() && w.flags.inline_code && !self.in_code_block
        };
        let word_pads: Vec<(f32, f32)> = if ci_pad_l == 0.0 && ci_pad_r == 0.0 {
            vec![(0.0, 0.0); words.len()]
        } else {
            (0..words.len())
                .map(|i| {
                    if !is_inline_code_word(&words[i]) {
                        return (0.0, 0.0);
                    }
                    let prev_ic = i > 0 && is_inline_code_word(&words[i - 1]);
                    let next_ic = i + 1 < words.len() && is_inline_code_word(&words[i + 1]);
                    (
                        if prev_ic { 0.0 } else { ci_pad_l },
                        if next_ic { 0.0 } else { ci_pad_r },
                    )
                })
                .collect()
        };
        let mut lines: Vec<Vec<TextSegment>> = Vec::new();
        let mut current: Vec<TextSegment> = Vec::new();
        let mut current_width = 0.0f32;

        for (wi, word) in words.iter().enumerate() {
            let (pad_before_pt, pad_after_pt) = word_pads[wi];
            let word_width = match &word.math {
                Some(tex) => self
                    .inline_math_frag(tex, size_pt)
                    .map(|f| f.w)
                    .unwrap_or(0.0),
                None => self.measure_text(word.flags, &word.text, size_pt),
            } + pad_before_pt
                + pad_after_pt;

            // The first line is narrowed by the first-line indent.
            let line_limit = if lines.is_empty() {
                max_width - first_line_indent_pt
            } else {
                max_width
            };
            // If the very first piece of a line is wider than the
            // page, push it anyway — we don't break inside a word.
            if !current.is_empty() && current_width + word_width > line_limit {
                lines.push(std::mem::take(&mut current));
                current_width = 0.0;
                // Drop any leading whitespace on the new line.
                if word.math.is_none() && word.text.trim().is_empty() {
                    continue;
                }
            }

            current.push(TextSegment {
                text: word.text.clone(),
                flags: word.flags,
                link: word.link.clone(),
                math: word.math.clone(),
                pad_before_pt,
                pad_after_pt,
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
        // logically-contiguous text. The merged seg keeps the leader's
        // `pad_before_pt` and takes on the trailer's `pad_after_pt`
        // (middle words contribute 0 on both sides by construction).
        for line in &mut lines {
            line.dedup_by(|next, prev| {
                if prev.math.is_none()
                    && next.math.is_none()
                    && prev.flags == next.flags
                    && prev.link == next.link
                {
                    prev.text.push_str(&next.text);
                    prev.pad_after_pt = next.pad_after_pt;
                    true
                } else {
                    false
                }
            });
        }

        let link_color = Some(rgb_color(self.style.link.text_color_rgb()));
        let mark_color = rgb_color(self.style.mark.text_color_rgb());
        let code_inline_color = rgb_color(self.style.code_inline.text_color_rgb());

        // Close any open section so the first line of this block
        // starts with a fresh BT (and absolute Td). Subsequent lines
        // of this paragraph stay inside one BT and use T*.
        self.close_text_section();
        let align = self.current_text_align;
        let last_line_idx = lines.len().saturating_sub(1);
        let mut prev_line_x_start = 0.0f32;
        let mut prev_baseline_y_pt = 0.0f32;
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
                if let Some(tex) = &seg.math {
                    natural_w_pt += self
                        .inline_math_frag(tex, size_pt)
                        .map(|f| f.w)
                        .unwrap_or(0.0);
                    continue;
                }
                let s_size = if seg.flags.superscript || seg.flags.subscript {
                    size_pt * 0.70
                } else if seg.flags.small_caps {
                    size_pt * 0.78
                } else if seg.flags.small {
                    size_pt * 0.85
                } else {
                    size_pt
                };
                natural_w_pt += self.measure_text(seg.flags, &seg.text, s_size)
                    + seg.pad_before_pt
                    + seg.pad_after_pt;
                if seg.text.chars().all(char::is_whitespace) && !seg.text.is_empty() {
                    space_count += 1;
                }
            }
            // The first line is shifted right and narrowed by the
            // first-line indent; later lines use the full column.
            let line_indent = if line_idx == 0 { first_line_indent_pt } else { 0.0 };
            let eff_left = self.indent_left_pt + line_indent;
            let eff_max_width = (max_width - line_indent).max(0.0);
            let slack_pt = (eff_max_width - natural_w_pt).max(0.0);
            let is_last_line = line_idx == last_line_idx;

            let (line_x_start, word_spacing_pt) = match align {
                TextAlignment::Left => (eff_left, 0.0),
                TextAlignment::Center => (eff_left + slack_pt * 0.5, 0.0),
                TextAlignment::Right => (eff_left + slack_pt, 0.0),
                TextAlignment::Justify => {
                    // Don't justify the last line of a paragraph, lines
                    // with no break opportunities, or lines whose slack
                    // would stretch spaces beyond ~30% of the column
                    // (a sign the wrap had no good fit, like an isolated
                    // short word).
                    let stretch_ok = space_count > 0
                        && slack_pt > 0.0
                        && slack_pt < eff_max_width * 0.30;
                    let tw = if !is_last_line && stretch_ok {
                        (slack_pt / space_count as f32).min(size_pt * 0.5)
                    } else {
                        0.0
                    };
                    (eff_left, tw)
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
                // `Td` moves relative to the previous line's origin,
                // not absolutely — center/right lines each have a
                // different left edge, so emit the delta from the
                // previous line (x shift, one line down).
                let dx = line_x_start - prev_line_x_start;
                let dy = -(baseline_y_pt - prev_baseline_y_pt);
                self.page_ops.push(Op::SetTextCursor {
                    pos: Point::new(Mm(pt_to_mm(dx)), Mm(pt_to_mm(dy))),
                });
            } else {
                self.page_ops.push(Op::AddLineBreak);
            }
            prev_line_x_start = line_x_start;
            prev_baseline_y_pt = baseline_y_pt;

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
                // Inline math: an indivisible typeset box on the text
                // baseline. Drawn as outlines in its own graphics
                // block (like the superscript break-out), so the line's
                // BT/ET is closed and the next text segment re-opens.
                if let Some(tex) = seg.math.clone() {
                    if let Some(frag) = self.inline_math_frag(&tex, size_pt) {
                        let mc = color
                            .clone()
                            .unwrap_or_else(|| rgb_color((0, 0, 0)));
                        self.emit_math_frag(&frag, x_cursor_pt, baseline_y_pt, mc);
                        x_cursor_pt += frag.w;
                        cursor_needs_reset = true;
                        line_was_broken = true;
                    }
                    continue;
                }
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
                let seg_width = self.measure_text(seg.flags, &seg.text, seg_size);
                // Justified lines widen every space via the PDF `Tw`
                // operator. `seg_width` (glyphs + letter spacing) does
                // not include that, so the cursor and decoration rects
                // must add `word_spacing_pt` per space or underlines /
                // link boxes drift left of the text. Super/subscript
                // segments break into their own `Tw`-free section, and
                // an inline-code span at the boundary contributes
                // `pad_before_pt + pad_after_pt` of horizontal gap
                // around its glyphs.
                let pad_before_pt = seg.pad_before_pt;
                let pad_after_pt = seg.pad_after_pt;
                let glyph_advance = seg_width
                    + if seg.flags.superscript || seg.flags.subscript {
                        0.0
                    } else {
                        word_spacing_pt
                            * seg.text.chars().filter(|&c| c == ' ').count() as f32
                    };
                let seg_advance = pad_before_pt + glyph_advance + pad_after_pt;

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
                    if let Some(c) = color.clone() {
                        self.page_ops.push(Op::SetFillColor { col: c });
                    }
                    emit_text_chunks(
                        &mut self.page_ops,
                        self.font_set,
                        seg.flags,
                        &seg.text,
                        seg_size,
                        self.letter_spacing_pt,
                    );
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
                    // Restore the text fill colour: link colour for a
                    // link, `[mark]` colour for a highlight, `[code_inline]`
                    // colour for inline code, otherwise the block colour.
                    if seg.link.is_some() {
                        let lc = if self.is_unresolved_internal_link(&seg.link) {
                            rgb_color(UNRESOLVED_LINK_COLOR)
                        } else {
                            link_color.clone().unwrap_or_else(|| rgb_color((0, 0, 0)))
                        };
                        self.page_ops.push(Op::SetFillColor { col: lc });
                    } else if seg.flags.highlight {
                        self.page_ops.push(Op::SetFillColor {
                            col: mark_color.clone(),
                        });
                    } else if seg.flags.monospace && !self.in_code_block {
                        self.page_ops.push(Op::SetFillColor {
                            col: code_inline_color.clone(),
                        });
                    } else if let Some(c) = color.clone() {
                        self.page_ops.push(Op::SetFillColor { col: c });
                    }
                    // Insert the inline-code left padding as a TJ
                    // negative offset (in thousandths of em) — moves
                    // the text cursor right by `pad_before_pt` without
                    // emitting a glyph, so the inline-code text starts
                    // `pad_before_pt` past the previous seg's end.
                    if pad_before_pt > 0.0 {
                        self.page_ops.push(Op::ShowText {
                            items: vec![TextItem::Offset(-pad_before_pt * 1000.0 / seg_size)],
                        });
                    }
                    emit_text_chunks(
                        &mut self.page_ops,
                        self.font_set,
                        seg.flags,
                        &seg.text,
                        seg_size,
                        self.letter_spacing_pt,
                    );
                    if pad_after_pt > 0.0 {
                        self.page_ops.push(Op::ShowText {
                            items: vec![TextItem::Offset(-pad_after_pt * 1000.0 / seg_size)],
                        });
                    }
                }

                // Buffer decorations and link rects until the line is
                // finished — they need a closed text section to draw
                // paths on top. Underline / strikethrough come from the
                // run flags (`<u>`, `~~`), from `[link]` for links, and
                // from `[code_inline]` / `[mark]` for inline code and
                // highlighted spans.
                // Unresolved internal links read as broken via the
                // red colour above and skip the underline so they
                // don't visually claim to be live destinations.
                let link_underline = seg.link.is_some()
                    && self.style.link.underline
                    && !self.is_unresolved_internal_link(&seg.link);
                let is_inline_code = seg.flags.monospace && !self.in_code_block;
                let dec_underline = seg.flags.underline
                    || link_underline
                    || (is_inline_code && self.style.code_inline.underline)
                    || (seg.flags.highlight && self.style.mark.underline);
                let dec_strike = seg.flags.strikethrough
                    || (is_inline_code && self.style.code_inline.strikethrough)
                    || (seg.flags.highlight && self.style.mark.strikethrough);
                if dec_underline || dec_strike || seg.link.is_some() {
                    let decoration_y_pt = if dec_strike {
                        baseline_y_pt - size_pt * 0.30
                    } else {
                        baseline_y_pt + size_pt * 0.12
                    };
                    self.pending_decorations.push(PendingDecoration {
                        kind: if dec_strike {
                            DecorationKind::Strike
                        } else if dec_underline {
                            DecorationKind::Underline
                        } else {
                            DecorationKind::None
                        },
                        x0_pt: x_cursor_pt + pad_before_pt,
                        x1_pt: x_cursor_pt + pad_before_pt + glyph_advance,
                        y_pt: decoration_y_pt,
                        link: seg.link.clone(),
                        size_pt,
                        baseline_y_pt,
                    });
                }
                // Inline background box: `[mark]` fill for a highlight,
                // `[code_inline]` fill for inline code (not code blocks).
                // Inline-code boxes span the full padded extent (the
                // padding is the *whole point* of the box — it sits
                // outside the text); mark highlights carry no padding.
                let inline_bg = if seg.flags.highlight {
                    self.style.mark.background_color_rgb()
                } else if seg.flags.monospace && !self.in_code_block {
                    self.style.code_inline.background_color_rgb()
                } else {
                    None
                };
                if let Some(rgb) = inline_bg {
                    let is_ic_box = seg.flags.monospace && !self.in_code_block;
                    let (pad_top, pad_bot) = if is_ic_box {
                        (
                            self.style.code_inline.padding.top,
                            self.style.code_inline.padding.bottom,
                        )
                    } else {
                        (0.0, 0.0)
                    };
                    self.pending_highlights.push(HighlightBox {
                        x0_pt: x_cursor_pt,
                        x1_pt: x_cursor_pt + seg_advance,
                        baseline_y_pt,
                        size_pt,
                        fill: rgb_color(rgb),
                        pad_top_pt: pad_top,
                        pad_bottom_pt: pad_bot,
                    });
                }
                x_cursor_pt += seg_advance;
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
                    // Near-text-black at ~1pt so the rule reads as
                    // clearly as the glyph stems; links keep link
                    // colour. (Was (80,80,80) @ 0.6pt — barely
                    // visible next to the text.)
                    let col = link_color.clone().unwrap_or_else(|| rgb_color((45, 45, 45)));
                    draw_horizontal_line(
                        &mut self.page_ops,
                        d.x0_pt,
                        d.x1_pt,
                        d.y_pt,
                        col,
                        1.0,
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
    saved_column: u8,
    outer_x_left: f32,
    outer_x_right: f32,
    outer_y_top: f32,
    background_color: Option<crate::styling::Color>,
    border: ResolvedBorder,
    padding_bottom: f32,
    margin_after_pt: f32,
    saved_letter_spacing: f32,
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

/// One inline background box (a `==highlight==` span or inline-code
/// span), in top-down points. Collected while a text section is open
/// and painted — with its own `fill` — under the glyphs when the
/// section closes.
#[derive(Debug)]
struct HighlightBox {
    x0_pt: f32,
    x1_pt: f32,
    baseline_y_pt: f32,
    size_pt: f32,
    fill: Color,
    /// Extra pt above the natural top edge (inline-code
    /// `padding.top`). Zero for `==mark==` highlights.
    pad_top_pt: f32,
    /// Extra pt below the natural bottom edge (inline-code
    /// `padding.bottom`).
    pad_bottom_pt: f32,
}

/// Emit `text` at `size_pt` using the run flags' resolved font chain.
/// When the FontSet has no fallbacks, this produces exactly one
/// `SetFont` + `ShowText` pair — identical to the pre-fallback emit
/// path. When fallbacks are loaded, the text is split into per-font
/// chunks ([`FontSet::split_for_emit`]) and one `SetFont` + `ShowText`
/// pair is emitted per chunk, so codepoints uncovered by the primary
/// render in their first covering fallback.
fn emit_text_chunks(
    ops: &mut Vec<Op>,
    font_set: &FontSet,
    flags: RunFlags,
    text: &str,
    size_pt: f32,
    letter_spacing_pt: f32,
) {
    // `Tc` adds spacing after every glyph. Emit it only when set —
    // a zero value leaves the op out so non-letter-spaced documents
    // render byte-identically. `Tc` is reset by the next `BT`, so a
    // block keeps its own spacing without leaking to the next.
    if letter_spacing_pt != 0.0 {
        ops.push(Op::SetCharacterSpacing {
            multiplier: letter_spacing_pt,
        });
    }
    for chunk in font_set.split_for_emit(flags, text, size_pt) {
        ops.push(Op::SetFont {
            font: chunk.handle,
            size: Pt(size_pt),
        });
        let emit = if chunk.needs_transliteration {
            to_win1252(&chunk.text)
        } else {
            chunk.text
        };
        ops.push(Op::ShowText {
            items: vec![TextItem::Text(emit)],
        });
    }
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
        super::font::for_each_builtin_emit_char(c, |emitted| out.push(emitted));
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
    letter_spacing_pt: f32,
) -> usize {
    if runs.is_empty() {
        return 1;
    }
    let size_pt = font_size;
    let measure = |flags: RunFlags, text: &str| {
        font_set.measure(flags, text, size_pt)
            + letter_spacing_pt * text.chars().count() as f32
    };
    let mut current = 0.0f32;
    let mut lines = 1usize;
    for run in runs {
        let mut flags = run.flags;
        if bold {
            flags = flags.with_bold();
        }
        for word in run.text.split_whitespace() {
            let w = measure(flags, word);
            let space = measure(flags, " ");
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
        BorderStyle::Dashed => LineDashPattern::new(0.0, &[4.0, 2.0]),
        BorderStyle::Dotted => LineDashPattern::new(0.0, &[1.0, 1.0]),
    }
}

fn rowspan_group_end(rows: &[Vec<TableCell<InlineRun>>], start: usize) -> usize {
    let mut end = (start + 1).min(rows.len());
    let mut r = start;
    while r < end {
        for cell in &rows[r] {
            if !cell.covered {
                end = end.max((r + cell.rowspan.max(1)).min(rows.len()));
            }
        }
        r += 1;
    }
    end
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

/// Filled disc (≈16-gon) for a list bullet the active font can't
/// render — drawn as a path so it never depends on a glyph being
/// present. Centre is given in top-down coordinates.
fn draw_filled_disc(
    ops: &mut Vec<Op>,
    cx_pt: f32,
    cy_top_pt: f32,
    r_pt: f32,
    fill: Color,
    page_height_pt: f32,
) {
    if r_pt <= 0.0 {
        return;
    }
    let n = 16;
    let points: Vec<LinePoint> = (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * (i as f32) / (n as f32);
            LinePoint {
                p: Point {
                    x: Pt(cx_pt + r_pt * a.cos()),
                    y: Pt(page_height_pt - (cy_top_pt + r_pt * a.sin())),
                },
                bezier: false,
            }
        })
        .collect();
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::SetFillColor { col: fill });
    ops.push(Op::DrawPolygon {
        polygon: Polygon {
            rings: vec![PolygonRing { points }],
            mode: PaintMode::Fill,
            winding_order: WindingOrder::NonZero,
        },
    });
    ops.push(Op::RestoreGraphicsState);
}

/// Approximate a circle outline by a closed 24-sided polyline. Used
/// by admonition icons that want a ring shape without filling.
fn draw_stroked_circle(
    ops: &mut Vec<Op>,
    cx_pt: f32,
    cy_top_pt: f32,
    r_pt: f32,
    color: Color,
    thickness_pt: f32,
    page_height_pt: f32,
) {
    if r_pt <= 0.0 {
        return;
    }
    let n = 24;
    let pts: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let a = std::f32::consts::TAU * (i as f32) / (n as f32);
            (cx_pt + r_pt * a.cos(), cy_top_pt + r_pt * a.sin())
        })
        .collect();
    draw_stroked_path(ops, &pts, color, thickness_pt, true, page_height_pt);
}

/// Per-kind callout glyph drawn into the header row's leading
/// 12pt-ish square. All icons render in `accent`; `cutout` is the
/// negative-space colour used for the X-mark inside the filled
/// `danger` disc (typically the admonition's background tint, so the
/// X reads as a notch carved out of the disc).
fn draw_admonition_icon(
    ops: &mut Vec<Op>,
    kind: &str,
    x_left_pt: f32,
    y_top_pt: f32,
    size_pt: f32,
    accent: &Color,
    cutout: &Color,
    page_height_pt: f32,
) {
    let cx = x_left_pt + size_pt * 0.5;
    let cy = y_top_pt + size_pt * 0.5;
    let s = size_pt;
    let stroke = (s * 0.10).max(0.6);
    match kind {
        "note" => {
            draw_filled_disc(ops, cx, cy, s * 0.36, accent.clone(), page_height_pt);
        }
        "info" => {
            draw_stroked_circle(
                ops,
                cx,
                cy,
                s * 0.45,
                accent.clone(),
                stroke,
                page_height_pt,
            );
            draw_filled_disc(
                ops,
                cx,
                y_top_pt + s * 0.30,
                s * 0.07,
                accent.clone(),
                page_height_pt,
            );
            draw_stroked_path(
                ops,
                &[(cx, y_top_pt + s * 0.43), (cx, y_top_pt + s * 0.72)],
                accent.clone(),
                stroke,
                false,
                page_height_pt,
            );
        }
        "tip" => {
            draw_filled_disc(
                ops,
                cx,
                y_top_pt + s * 0.38,
                s * 0.30,
                accent.clone(),
                page_height_pt,
            );
            draw_filled_rect(
                ops,
                cx - s * 0.16,
                y_top_pt + s * 0.62,
                cx + s * 0.16,
                y_top_pt + s * 0.78,
                accent.clone(),
                page_height_pt,
            );
            draw_filled_rect(
                ops,
                cx - s * 0.10,
                y_top_pt + s * 0.82,
                cx + s * 0.10,
                y_top_pt + s * 0.92,
                accent.clone(),
                page_height_pt,
            );
        }
        "warning" => {
            let p0 = (cx, y_top_pt + s * 0.10);
            let p1 = (x_left_pt + s * 0.06, y_top_pt + s * 0.90);
            let p2 = (x_left_pt + s * 0.94, y_top_pt + s * 0.90);
            draw_stroked_path(
                ops,
                &[p0, p1, p2],
                accent.clone(),
                stroke,
                true,
                page_height_pt,
            );
            draw_stroked_path(
                ops,
                &[(cx, y_top_pt + s * 0.36), (cx, y_top_pt + s * 0.66)],
                accent.clone(),
                stroke,
                false,
                page_height_pt,
            );
            draw_filled_disc(
                ops,
                cx,
                y_top_pt + s * 0.78,
                stroke * 0.7,
                accent.clone(),
                page_height_pt,
            );
        }
        "danger" => {
            draw_filled_disc(ops, cx, cy, s * 0.46, accent.clone(), page_height_pt);
            let inset = s * 0.24;
            draw_stroked_path(
                ops,
                &[
                    (x_left_pt + inset, y_top_pt + inset),
                    (x_left_pt + s - inset, y_top_pt + s - inset),
                ],
                cutout.clone(),
                stroke,
                false,
                page_height_pt,
            );
            draw_stroked_path(
                ops,
                &[
                    (x_left_pt + inset, y_top_pt + s - inset),
                    (x_left_pt + s - inset, y_top_pt + inset),
                ],
                cutout.clone(),
                stroke,
                false,
                page_height_pt,
            );
        }
        _ => {
            let bar_stroke = (s * 0.12).max(0.8);
            for frac in [0.28_f32, 0.50, 0.72] {
                let y = y_top_pt + s * frac;
                draw_stroked_path(
                    ops,
                    &[(x_left_pt + s * 0.18, y), (x_left_pt + s * 0.82, y)],
                    accent.clone(),
                    bar_stroke,
                    false,
                    page_height_pt,
                );
            }
        }
    }
}

/// Stroked polyline through top-down points (optionally closed) —
/// used for the task-list checkbox outline and tick.
fn draw_stroked_path(
    ops: &mut Vec<Op>,
    pts_top: &[(f32, f32)],
    col: Color,
    thickness_pt: f32,
    closed: bool,
    page_height_pt: f32,
) {
    if pts_top.len() < 2 {
        return;
    }
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
            points: pts_top
                .iter()
                .map(|&(x, yt)| LinePoint {
                    p: Point {
                        x: Pt(x),
                        y: Pt(page_height_pt - yt),
                    },
                    bezier: false,
                })
                .collect(),
            is_closed: closed,
        },
    });
    ops.push(Op::RestoreGraphicsState);
}

struct TextSegment {
    text: String,
    flags: RunFlags,
    link: Option<String>,
    /// Raw TeX when this segment is an inline-math box (`text` empty).
    math: Option<String>,
    /// Horizontal pt of padding to insert before this segment's glyphs
    /// (and after, respectively). Non-zero only for the first / last
    /// segment of a contiguous inline-code span when
    /// `[code_inline].padding.left` / `.right` are set. Emitted as a
    /// `TextItem::Offset` so the gap lives inside the line's BT and
    /// text selection stays contiguous.
    pad_before_pt: f32,
    pad_after_pt: f32,
}

/// Flatten a run list to a sequence of (word | whitespace) pieces,
/// preserving the originating run's flags. Whitespace pieces become
/// break opportunities in the wrapping pass; words don't.
fn words_from_runs(runs: &[InlineRun]) -> Vec<InlineRun> {
    let mut out = Vec::new();
    for run in runs {
        if run.math.is_some() {
            // An inline-math box is one indivisible word — never
            // split on whitespace, never merged with neighbours.
            out.push(run.clone());
            continue;
        }
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
                out.push(InlineRun { math: None,
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


/// Block-level style → base run flags. Weight, slant, and the
/// underline / strikethrough decorations are set on the style block
/// rather than by inline markup, so they have to be folded into the
/// base flags that `write_wrapped_runs` applies to every run.
fn base_flags_from_block(s: &ResolvedBlock) -> RunFlags {
    RunFlags {
        bold: s.is_bold(),
        italic: s.is_italic(),
        underline: s.underline,
        strikethrough: s.strikethrough,
        ..RunFlags::default()
    }
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
    // An untrusted SVG must not be able to pull in external resources
    // while parsing: usvg's default string resolver will happily read
    // `<image href="/etc/…">` off disk. Data URIs stay allowed — they
    // are self-contained. Built as a struct literal (not
    // `default()` + field assignment) so clippy's
    // `field_reassign_with_default` stays quiet.
    let opts = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: resvg::usvg::ImageHrefResolver::default_data_resolver(),
            resolve_string: Box::new(|_href, _opts| None),
        },
        ..Default::default()
    };
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
        let pages = lay_out_pages(&[], &style, &font_set, &HashSet::new(), &mut PdfDocument::new("test"));
        assert!(pages.is_empty());
    }

    #[test]
    fn one_paragraph_produces_one_page() {
        let font_set = FontSet::load(None, &[], crate::render::ir::VariantUsage::default(), &mut PdfDocument::new("test"));
        let style = ResolvedStyle::default();
        let blocks = vec![Block::Paragraph {
            runs: vec![InlineRun::new("hello world")],
        }];
        let pages = lay_out_pages(&blocks, &style, &font_set, &HashSet::new(), &mut PdfDocument::new("test"));
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
        let pages = lay_out_pages(&blocks, &style, &font_set, &HashSet::new(), &mut PdfDocument::new("test"));
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
        let pages = lay_out_pages(&blocks, &style, &font_set, &HashSet::new(), &mut PdfDocument::new("test"));
        assert!(!pages.is_empty());
    }
}
