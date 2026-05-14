//! Layout engine: block IR -> printpdf 0.9 page operation streams.
//!
//! Greedy line breaking at word boundaries using real glyph advance
//! widths from [`super::font::FontMetricsCache`]. Vertical advancement
//! is per-block; the engine pushes a new page when the y cursor
//! would dip below the bottom margin.

use printpdf::{
    Actions, BorderArray, ColorArray, LineDashPattern, LinePoint, LinkAnnotation, Mm, Op,
    PdfDocument, PdfPage, Point, Pt, RawImage, Rect, Rgb, TextItem, XObjectId, XObjectTransform,
};

use crate::styling::ResolvedStyle;

use super::font::FontSet;
use super::ir::{Block, InlineRun, ListBullet, ListEntry, RunFlags};

type Color = printpdf::Color;

const PAGE_WIDTH_MM: f32 = 210.0;
const PAGE_HEIGHT_MM: f32 = 297.0;

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
    /// Distance from the top of the page to the current text baseline
    /// in points. Grows downward.
    y_from_top_pt: f32,
    /// Left content edge for the *current* block in points, measured
    /// from the page's left edge. Updated when entering lists,
    /// blockquotes, or other indented contexts; restored when leaving.
    indent_left_pt: f32,
    /// Right content edge for the *current* block.
    indent_right_pt: f32,
    /// Stack of (y_pt, indent_left_pt) markers — one per active
    /// blockquote frame — used to draw the left rule on
    /// [`pop_blockquote`].
    blockquote_stack: Vec<BlockQuoteFrame>,
    /// Page-local Op stream we're currently appending to.
    page_ops: Vec<Op>,
    /// Pending text decorations (underline / strikethrough lines and
    /// link annotation rects) collected while a text section is open.
    /// Drawn together when the section closes so they don't fight the
    /// text section's graphics state.
    pending_decorations: Vec<PendingDecoration>,
    /// Finished pages.
    pages: Vec<PdfPage>,
    /// Whether a text section is currently open.
    in_text_section: bool,
}

struct BlockQuoteFrame {
    /// Point at which the blockquote started on the *current* page.
    /// Reset to `y_from_top_pt` whenever a page break happens inside
    /// the blockquote so we can draw the rule per page.
    page_start_y_pt: f32,
    /// x position (from the page's left edge) at which to draw the
    /// vertical rule.
    rule_x_pt: f32,
}

impl<'a> Engine<'a> {
    fn new(style: &'a ResolvedStyle, font_set: &'a FontSet, doc: &'a mut PdfDocument) -> Self {
        let left = mm_to_pt(style.page.margins_mm.left.max(1.0));
        let right = PAGE_WIDTH_MM * MM_TO_PT - mm_to_pt(style.page.margins_mm.right.max(1.0));
        let top = mm_to_pt(style.page.margins_mm.top.max(1.0));
        Self {
            style,
            font_set,
            doc,
            y_from_top_pt: top,
            indent_left_pt: left,
            indent_right_pt: right,
            blockquote_stack: Vec::new(),
            page_ops: Vec::new(),
            pending_decorations: Vec::new(),
            pages: Vec::new(),
            in_text_section: false,
        }
    }

    fn finish(mut self) -> Vec<PdfPage> {
        self.close_text_section();
        self.flush_blockquote_rules();
        self.push_current_page();
        self.pages
    }

    fn push_current_page(&mut self) {
        if self.page_ops.is_empty() {
            return;
        }
        let ops = std::mem::take(&mut self.page_ops);
        let page = PdfPage::new(Mm(PAGE_WIDTH_MM), Mm(PAGE_HEIGHT_MM), ops);
        self.pages.push(page);
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
        PAGE_HEIGHT_MM * MM_TO_PT
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
        self.flush_blockquote_rules();
        self.push_current_page();
        self.y_from_top_pt = self.top_margin_pt();
        // Restart any active blockquote frames at the new top margin
        // so the left rule continues on the new page.
        for frame in &mut self.blockquote_stack {
            frame.page_start_y_pt = self.y_from_top_pt;
        }
    }

    fn flush_blockquote_rules(&mut self) {
        let page_h = self.page_height_pt();
        let y = self.y_from_top_pt;
        // Snapshot the frames so we can borrow page_ops mutably below.
        let frames: Vec<(f32, f32)> = self
            .blockquote_stack
            .iter()
            .map(|f| (f.rule_x_pt, f.page_start_y_pt))
            .collect();
        for (rule_x, y_top) in frames {
            draw_vertical_line(&mut self.page_ops, rule_x, y_top, y, page_h);
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

        // Spacing before the image.
        self.advance_y(2.0);
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
        self.advance_y(2.0);
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
        let before_pt = (s_cell.margin_before_pt + 0.5).max(0.5);
        let after_pt = (s_cell.margin_after_pt + 0.5).max(0.5);
        const CELL_PAD_PT: f32 = 4.0;
        const ROW_GAP_PT: f32 = 2.0;

        self.advance_y(before_pt);

        let col_count = headers.len();
        let total_width = self.content_width_pt();
        let col_width = total_width / col_count as f32;

        // Header row.
        let header_height = self.measure_row_height(headers, s_header.font_size_pt, col_width, true);
        if self.y_from_top_pt + header_height + self.bottom_margin_pt() > self.page_height_pt() {
            self.start_new_page();
        }
        let header_top = self.y_from_top_pt;
        self.draw_row(
            headers,
            aligns,
            s_header.font_size_pt,
            col_width,
            true,
            s_header.text_color_rgb(),
        );
        let header_bottom = header_top + header_height;
        self.draw_row_borders(header_top, header_bottom, col_count, col_width);
        self.y_from_top_pt = header_bottom;
        self.advance_y(ROW_GAP_PT);

        // Data rows.
        for row in rows {
            // Pad / truncate to header column count.
            let mut padded: Vec<Vec<InlineRun>> = row.clone();
            padded.resize(col_count, Vec::new());
            let row_height = self.measure_row_height(&padded, s_cell.font_size_pt, col_width, false);
            if self.y_from_top_pt + row_height + self.bottom_margin_pt() > self.page_height_pt() {
                self.start_new_page();
                // Reprint headers on the new page.
                let header_top = self.y_from_top_pt;
                self.draw_row(
                    headers,
                    aligns,
                    s_header.font_size_pt,
                    col_width,
                    true,
                    s_header.text_color_rgb(),
                );
                let header_bottom = header_top + header_height;
                self.draw_row_borders(header_top, header_bottom, col_count, col_width);
                self.y_from_top_pt = header_bottom;
                self.advance_y(ROW_GAP_PT);
            }
            let row_top = self.y_from_top_pt;
            self.draw_row(
                &padded,
                aligns,
                s_cell.font_size_pt,
                col_width,
                false,
                s_cell.text_color_rgb(),
            );
            let row_bottom = row_top + row_height;
            self.draw_row_borders(row_top, row_bottom, col_count, col_width);
            self.y_from_top_pt = row_bottom;
            self.advance_y(ROW_GAP_PT);
        }

        let _ = CELL_PAD_PT;
        self.advance_y(after_pt);
    }

    fn measure_row_height(
        &self,
        cells: &[Vec<InlineRun>],
        font_size: f32,
        col_width: f32,
        bold: bool,
    ) -> f32 {
        let size_pt = f32::from(font_size);
        let line_h = size_pt * 1.4;
        let mut max_lines = 1usize;
        for cell in cells {
            let n_lines =
                count_wrapped_lines(cell, font_size, col_width - 8.0, self.font_set, bold);
            max_lines = max_lines.max(n_lines);
        }
        max_lines as f32 * line_h + 6.0
    }

    fn draw_row(
        &mut self,
        cells: &[Vec<InlineRun>],
        aligns: &[crate::markdown::TableAlignment],
        font_size: f32,
        col_width: f32,
        bold: bool,
        color: (u8, u8, u8),
    ) {
        let saved_left = self.indent_left_pt;
        let saved_right = self.indent_right_pt;
        let row_top = self.y_from_top_pt;
        let mut max_bottom = row_top;
        let col_count = cells.len();
        for (i, cell) in cells.iter().enumerate() {
            let cell_left = saved_left + col_width * i as f32 + 4.0;
            let cell_right = saved_left + col_width * (i + 1) as f32 - 4.0;
            self.indent_left_pt = cell_left;
            self.indent_right_pt = cell_right;
            self.y_from_top_pt = row_top + 3.0;
            let mut runs = cell.clone();
            if bold {
                for r in &mut runs {
                    r.flags = r.flags.with_bold();
                }
            }
            // Alignment is hinted via the lexer but printpdf's
            // Op::ShowText doesn't support per-line alignment — phase 3
            // keeps everything left-aligned. Aligns are accepted as
            // input but ignored for now; phase 5 adds real alignment.
            let _ = aligns.get(i);
            self.write_wrapped_runs(&runs, font_size, RunFlags::default(), Some(rgb_color(color)));
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
        // Per-item indent. Honors block-quote / outer list indent by
        // adding to the current `indent_left_pt`.
        const BULLET_GAP_MM: f32 = 2.0;
        let bullet_gap_pt = mm_to_pt(BULLET_GAP_MM);

        let saved_left = self.indent_left_pt;
        let s = &self.style.list_unordered.block;
        let size_pt = s.font_size_pt;

        for entry in entries {
            let bullet_text = format_bullet(&entry.bullet);
            let bullet_flags = RunFlags::default();
            let bullet_width = self.font_set.measure(bullet_flags, &bullet_text, size_pt);

            // Reserve bullet column at the *current* left edge.
            // Then indent inline content past it.
            self.advance_y(s.margin_before_pt.max(0.5));
            let bullet_x = saved_left;
            let bullet_y = self.y_from_top_pt + size_pt;
            // Force a fresh BT before the bullet's Td so absolute
            // placement works — otherwise the previous item's text
            // section is still open and `Td` compounds against the
            // current text matrix.
            self.close_text_section();
            self.ensure_text_section();
            self.move_cursor_to(bullet_x, bullet_y);
            self.page_ops.push(Op::SetFont {
                font: self.font_set.handle(bullet_flags),
                size: Pt(size_pt),
            });
            self.page_ops.push(Op::SetLineHeight {
                lh: Pt(size_pt * 1.4),
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

            // Indent inline content past the bullet column.
            self.indent_left_pt = (saved_left + bullet_width + bullet_gap_pt)
                .min(self.indent_right_pt - 10.0);

            self.write_wrapped_runs(&entry.runs, s.font_size_pt, RunFlags::default(), Some(rgb_color(s.text_color_rgb())));

            // Render any nested children (sub-lists, paragraphs from
            // a loose list item) at this same indent.
            for child in &entry.children {
                self.render_block(child);
            }

            self.indent_left_pt = saved_left;
            self.advance_y(s.margin_after_pt.max(0.0));
        }
    }

    fn render_blockquote(&mut self, body: &[Block]) {
        const INDENT_MM: f32 = 6.0;
        const RULE_OFFSET_MM: f32 = 1.5;
        let indent_pt = mm_to_pt(INDENT_MM);
        let rule_x_pt = self.indent_left_pt + mm_to_pt(RULE_OFFSET_MM);

        let saved_left = self.indent_left_pt;
        let s = &self.style.blockquote;
        let before_pt = s.margin_before_pt.max(0.5);
        let after_pt = s.margin_after_pt.max(0.5);

        self.advance_y(before_pt);

        let frame = BlockQuoteFrame {
            page_start_y_pt: self.y_from_top_pt,
            rule_x_pt,
        };
        self.blockquote_stack.push(frame);
        self.indent_left_pt = saved_left + indent_pt;

        for child in body {
            self.render_block(child);
        }

        // Draw the rule on the current page, then pop.
        let frame = self.blockquote_stack.pop().unwrap();
        self.close_text_section();
        let page_h = self.page_height_pt();
        let y = self.y_from_top_pt;
        draw_vertical_line(
            &mut self.page_ops,
            frame.rule_x_pt,
            frame.page_start_y_pt,
            y,
            page_h,
        );

        self.indent_left_pt = saved_left;
        self.advance_y(after_pt);
    }

    fn render_heading(&mut self, level: u8, runs: &[InlineRun]) {
        let s = match level {
            1 => &self.style.headings[0],
            2 => &self.style.headings[1],
            3 => &self.style.headings[2],
            4 => &self.style.headings[3],
            5 => &self.style.headings[4],
            _ => &self.style.headings[5],
        };

        let color = Some(rgb_color(s.text_color_rgb()));
        let base_flags = RunFlags {
            bold: s.is_bold(),
            italic: s.is_italic(),
            monospace: false,
            strikethrough: false,
            underline: false,
        };
        let before_pt = s.margin_before_pt;
        let after_pt = s.margin_after_pt;

        self.advance_y(before_pt);
        self.write_wrapped_runs(runs, s.font_size_pt, base_flags, color);
        self.advance_y(after_pt);
    }

    fn render_paragraph(&mut self, runs: &[InlineRun]) {
        let s = &self.style.paragraph;
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default();
        let before_pt = s.margin_before_pt;
        let after_pt = s.margin_after_pt;

        self.advance_y(before_pt);
        self.write_wrapped_runs(runs, s.font_size_pt, base, color);
        self.advance_y(after_pt);
    }

    fn render_code_block(&mut self, lines: &[String]) {
        let s = &self.style.code_block;
        let color = Some(rgb_color(s.text_color_rgb()));
        let base = RunFlags::default().with_monospace();
        let before_pt = s.margin_before_pt;
        let after_pt = s.margin_after_pt;
        let size_pt = s.font_size_pt;

        self.advance_y(before_pt);
        for line in lines {
            let run = InlineRun {
                text: line.clone(),
                flags: base,
                link: None,
            };
            self.write_wrapped_runs(std::slice::from_ref(&run), size_pt, base, color.clone());
        }
        self.advance_y(after_pt);
    }

    fn render_horizontal_rule(&mut self) {
        self.close_text_section();

        let s = &self.style.horizontal_rule;
        let before_pt = s.margin_before_pt;
        let after_pt = s.margin_after_pt;
        let line_color = s.color_rgb();
        let thickness = s.thickness_pt.max(0.1);

        self.advance_y(before_pt + thickness * 0.5);

        let x_left_pt = self.left_margin_pt();
        let x_right_pt = PAGE_WIDTH_MM * MM_TO_PT - self.right_margin_pt();
        let y_pt = self.y_from_top_pt;
        let y_mm = pt_to_mm(self.page_height_pt() - y_pt);

        self.page_ops.push(Op::SaveGraphicsState);
        self.page_ops.push(Op::SetOutlineColor {
            col: rgb_color(line_color),
        });
        self.page_ops.push(Op::SetOutlineThickness {
            pt: Pt(0.5),
        });
        self.page_ops.push(Op::SetLineDashPattern {
            dash: LineDashPattern::default(),
        });
        self.page_ops.push(Op::DrawLine {
            line: printpdf::Line {
                points: vec![
                    LinePoint {
                        p: Point::new(Mm(pt_to_mm(x_left_pt)), Mm(y_mm)),
                        bezier: false,
                    },
                    LinePoint {
                        p: Point::new(Mm(pt_to_mm(x_right_pt)), Mm(y_mm)),
                        bezier: false,
                    },
                ],
                is_closed: false,
            },
        });
        self.page_ops.push(Op::RestoreGraphicsState);

        self.advance_y(after_pt);
    }

    /// Wrap `runs` to the page width and emit one ShowText per line.
    /// `font_size_pt` is the size used for line metrics; `base_flags`
    /// is the fallback style applied to runs whose flags match. The
    /// optional `color` is applied once at the start of the block.
    fn write_wrapped_runs(
        &mut self,
        runs: &[InlineRun],
        font_size: f32,
        _base_flags: RunFlags,
        color: Option<Color>,
    ) {
        if runs.is_empty() {
            return;
        }
        let size_pt = f32::from(font_size);
        let line_height_pt = size_pt * 1.4;

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
            // page, push it anyway — we don't break inside a word in
            // phase 1.
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
                // Annotation rect spans baseline_y - size .. baseline_y
                // in our top-down coords. printpdf wants the rect in
                // bottom-up Pt space.
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
    max_width: f32,
    font_set: &FontSet,
    bold: bool,
) -> usize {
    if runs.is_empty() {
        return 1;
    }
    let size_pt = f32::from(font_size);
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

fn format_bullet(b: &ListBullet) -> String {
    // External (Unicode) fonts render '•' directly. Built-in
    // Helvetica falls back through `to_win1252`, which maps '•' to
    // '*' so the bullet still appears.
    match b {
        ListBullet::Unordered(_) => "\u{2022}  ".to_string(),
        ListBullet::Ordered(n) => format!("{}.  ", n),
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

fn rgb_color((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(Rgb {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        icc_profile: None,
    })
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
