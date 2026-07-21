//! Intermediate representation for the renderer.
//!
//! The lowering pass ([`super::lower`]) converts a [`Token`] stream
//! into a flat `Vec<Block>` plus per-block inline runs. The layout
//! pass ([`super::layout`]) consumes that IR.
//!
//! The IR is intentionally smaller than the [`Token`] enum: it drops
//! anything the renderer doesn't need to distinguish at layout time.

/// A top-level block-level rendering unit.
#[derive(Debug, Clone)]
pub enum Block {
    /// A heading. `level` is 1..=6.
    Heading { level: u8, runs: Vec<InlineRun> },
    /// A paragraph of flowing text.
    Paragraph { runs: Vec<InlineRun> },
    /// A fenced or indented code block. One entry per source line.
    Code { lines: Vec<String> },
    /// A horizontal rule (`---`).
    HorizontalRule,
    /// A run of consecutive list items at the same level + marker
    /// shape. Ordered/unordered is determined by the entries.
    List { entries: Vec<ListEntry> },
    /// A block quote whose body is itself a sequence of [`Block`]s.
    Quote { body: Vec<Block> },
    /// A semantic callout / admonition block. `kind` is the canonical
    /// kind name (`note` / `info` / `tip` / `warning` / `danger` /
    /// `generic`) the renderer keys per-kind styling off; `raw_label`
    /// is the author's lowercased original kind word, surfaced as the
    /// header for unknown kinds so `!!! bug "…"` reads as a `BUG` box.
    /// `title` is the optional inline header override from the MkDocs
    /// `"Optional title"` form; when `None` the renderer falls back to
    /// the kind's default label. `body` is a nested block sequence so
    /// admonitions can contain lists, code, tables, even nested
    /// admonitions.
    Admonition {
        kind: String,
        raw_label: String,
        title: Option<Vec<InlineRun>>,
        body: Vec<Block>,
    },
    /// A GFM table.
    Table {
        headers: Vec<crate::markdown::TableCell<InlineRun>>,
        aligns: Vec<crate::markdown::TableAlignment>,
        rows: Vec<Vec<crate::markdown::TableCell<InlineRun>>>,
    },
    /// A block-level image. The lowering pass promotes a paragraph
    /// containing only an image to this variant; inline images keep
    /// their alt text in flow. The optional `caption` carries the
    /// markdown title attribute (`![alt](url "caption text")`) and is
    /// rendered as a small line beneath the image.
    Image {
        path: std::path::PathBuf,
        alt: String,
        caption: Option<String>,
    },
    /// Verbatim block-level raw HTML. Rendered as a monospace block
    /// so the source stays visible. CommonMark §4.6 lets us choose
    /// whether to interpret HTML or pass it through; we pass through.
    Html { content: String },
    /// A user-requested page break. Triggered by a standalone
    /// `<!-- pagebreak -->` block in the source. The renderer
    /// flushes the current page and starts a fresh one with no
    /// other side effects.
    PageBreak,
    /// Collected GFM footnote definitions, rendered as a "Footnotes"
    /// section at the end of the document. Numbers are assigned in
    /// first-reference order by the lower pass.
    FootnoteDefinitions { entries: Vec<FootnoteEntry> },
    /// PHP Markdown Extra-style definition list. Each entry pairs a
    /// term with one or more definitions.
    DefinitionList { entries: Vec<DefinitionEntry> },
    /// LaTeX display math (`$$ … $$`). `content` is the raw TeX,
    /// rendered centered in an italic monospace style. (v1 renders the
    /// source verbatim; full mathematical typesetting is a separate,
    /// larger effort tracked independently.)
    Math { content: String },
}

#[derive(Debug, Clone)]
pub struct DefinitionEntry {
    pub terms: Vec<Vec<InlineRun>>,
    pub definitions: Vec<Vec<Block>>,
}

#[derive(Debug, Clone)]
pub struct FootnoteEntry {
    /// Original markdown label (e.g. `1` or `note-a`). Retained for
    /// future use (e.g. footnote-to-bibliography lookups); not read
    /// by the v1 renderer.
    #[allow(dead_code)]
    pub label: String,
    pub number: usize,
    pub runs: Vec<InlineRun>,
}

/// One entry inside a [`Block::List`].
///
/// `runs` is the inline content of this entry (the visible text on
/// the same line as the bullet). `children` are nested block-level
/// elements (most commonly nested [`Block::List`]s, but headings or
/// paragraphs can appear inside loose list items too).
#[derive(Debug, Clone)]
pub struct ListEntry {
    pub bullet: ListBullet,
    pub runs: Vec<InlineRun>,
    pub children: Vec<Block>,
    /// CommonMark §5.3: a list is "loose" if any of its items has
    /// `loose = true` (i.e., is separated from another item by a
    /// blank line in the source). The renderer reads `any_loose`
    /// over the entries and picks `item_spacing_loose_pt` vs
    /// `item_spacing_tight_pt` for the inter-item gap.
    pub loose: bool,
}

/// The marker drawn at the start of a [`ListEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListBullet {
    /// `- `, `+ `, `* ` — the source marker is preserved here; the
    /// rendered bullet glyph comes from `[list.unordered.bullet]`
    /// regardless of which source marker was used.
    Unordered(char),
    /// `1.`, `2.` (or `1)`, `2)`).
    Ordered(usize),
    /// GFM task list item, checked.
    TaskChecked,
    /// GFM task list item, unchecked.
    TaskUnchecked,
}

/// A styled inline text run.
#[derive(Debug, Clone)]
pub struct InlineRun {
    pub text: String,
    pub flags: RunFlags,
    /// If `Some`, this run is the visible text of a hyperlink and the
    /// renderer emits a PDF link annotation pointing at the URL.
    pub link: Option<String>,
    /// If `Some`, this run is an inline math span: `text` is empty and
    /// the string is the raw TeX, typeset by the math engine as one
    /// indivisible box on the text baseline.
    pub math: Option<String>,
}

impl InlineRun {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            flags: RunFlags::default(),
            link: None,
            math: None,
        }
    }

    /// An inline-math run carrying raw TeX.
    pub fn math(tex: impl Into<String>, flags: RunFlags, link: Option<String>) -> Self {
        Self {
            text: String::new(),
            flags,
            link,
            math: Some(tex.into()),
        }
    }
}

/// Which font variants the document actually uses. Built by walking
/// the lowered IR once before font loading, so we can skip loading
/// (and embedding) weights that no run in the document references.
#[derive(Debug, Default, Clone, Copy)]
pub struct VariantUsage {
    pub body_bold: bool,
    pub body_italic: bool,
    pub body_bold_italic: bool,
    pub mono_regular: bool,
    pub mono_bold: bool,
    pub mono_italic: bool,
    pub mono_bold_italic: bool,
    /// Variants used by inline-code / `<kbd>` runs (separate from
    /// fenced code blocks so a configured `[code_inline].font_family`
    /// only loads the variants inline code actually references).
    pub inline_code_regular: bool,
    pub inline_code_bold: bool,
    pub inline_code_italic: bool,
    pub inline_code_bold_italic: bool,
}

impl VariantUsage {
    pub fn analyze(blocks: &[Block]) -> Self {
        let mut u = Self::default();
        for b in blocks {
            walk_block(b, &mut u);
        }
        u
    }
}

fn walk_block(block: &Block, u: &mut VariantUsage) {
    match block {
        Block::Heading { runs, .. } | Block::Paragraph { runs } => {
            for r in runs {
                walk_run(r, u);
            }
        }
        Block::List { entries } => {
            for entry in entries {
                for r in &entry.runs {
                    walk_run(r, u);
                }
                for child in &entry.children {
                    walk_block(child, u);
                }
            }
        }
        Block::Quote { body } => {
            for child in body {
                walk_block(child, u);
            }
        }
        Block::Admonition { title, body, .. } => {
            // The header label is rendered bold uppercase, so any
            // admonition contributes a body-bold requirement.
            u.body_bold = true;
            if let Some(runs) = title {
                for r in runs {
                    walk_run(r, u);
                }
            }
            for child in body {
                walk_block(child, u);
            }
        }
        Block::Table { headers, rows, .. } => {
            // The renderer ships the header cells through with_bold(),
            // so any table contributes a body-bold requirement.
            if !headers.is_empty() {
                u.body_bold = true;
            }
            for header in headers {
                for r in &header.content {
                    walk_run(r, u);
                }
            }
            for row in rows {
                for cell in row {
                    for r in &cell.content {
                        walk_run(r, u);
                    }
                }
            }
        }
        Block::Code { .. } | Block::Html { .. } => {
            u.mono_regular = true;
        }
        Block::FootnoteDefinitions { entries } => {
            for entry in entries {
                for r in &entry.runs {
                    walk_run(r, u);
                }
            }
        }
        Block::DefinitionList { entries } => {
            // Term is rendered bold, so any list contributes a
            // body-bold requirement.
            if !entries.is_empty() {
                u.body_bold = true;
            }
            for entry in entries {
                for term in &entry.terms {
                    for r in term {
                        walk_run(r, u);
                    }
                }
                for def in &entry.definitions {
                    for b in def {
                        walk_block(b, u);
                    }
                }
            }
        }
        Block::Math { .. } => {
            // Rendered as centered italic monospace.
            u.mono_italic = true;
        }
        Block::HorizontalRule | Block::Image { .. } | Block::PageBreak => {}
    }
}

fn walk_run(run: &InlineRun, u: &mut VariantUsage) {
    let f = run.flags;
    if f.inline_code {
        match (f.bold, f.italic) {
            (false, false) => u.inline_code_regular = true,
            (true, false) => u.inline_code_bold = true,
            (false, true) => u.inline_code_italic = true,
            (true, true) => u.inline_code_bold_italic = true,
        }
        return;
    }
    match (f.monospace, f.bold, f.italic) {
        (false, false, false) => {}
        (false, true, false) => u.body_bold = true,
        (false, false, true) => u.body_italic = true,
        (false, true, true) => u.body_bold_italic = true,
        (true, false, false) => u.mono_regular = true,
        (true, true, false) => u.mono_bold = true,
        (true, false, true) => u.mono_italic = true,
        (true, true, true) => u.mono_bold_italic = true,
    }
}

/// Per-run style flags. Combined orthogonally — e.g. `bold + italic`
/// resolves to a bold-italic font variant; `monospace` overrides the
/// family. `strikethrough`/`underline` are decorations drawn after
/// the glyphs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunFlags {
    pub bold: bool,
    pub italic: bool,
    pub monospace: bool,
    pub strikethrough: bool,
    pub underline: bool,
    /// `==text==` highlight. The renderer paints `style.mark`'s
    /// background behind the run's glyphs.
    pub highlight: bool,
    /// Renders the glyphs at ~70% size with a raised baseline. Used
    /// for footnote marker numbers and any `<sup>` HTML inline.
    pub superscript: bool,
    /// Renders the glyphs at ~70% size with a lowered baseline. Used
    /// for `<sub>` HTML inline (chemical formulas, indices).
    pub subscript: bool,
    /// Faux small caps — emit at ~78% size on the original baseline.
    /// Set per-segment by the layout's `expand_small_caps` pass, which
    /// upper-cases the source character and tags only the
    /// originally-lowercase characters with this flag.
    pub small_caps: bool,
    /// Renders at ~85% of body size on the original baseline. Set
    /// by `<small>` inline HTML.
    pub small: bool,
    /// True for monospace runs that originated from `` `inline code` ``
    /// or `<kbd>` (not fenced code blocks). When `[code_inline]
    /// font_family` is configured, these runs route through a separate
    /// `external_code_inline` family so inline code can use a different
    /// monospace face than block code.
    pub inline_code: bool,
}

impl RunFlags {
    pub fn with_bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn with_italic(mut self) -> Self {
        self.italic = true;
        self
    }
    pub fn with_monospace(mut self) -> Self {
        self.monospace = true;
        self
    }
    pub fn with_strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }
    pub fn with_highlight(mut self) -> Self {
        self.highlight = true;
        self
    }
    pub fn with_underline(mut self) -> Self {
        self.underline = true;
        self
    }
    pub fn with_superscript(mut self) -> Self {
        self.superscript = true;
        self
    }
    pub fn with_subscript(mut self) -> Self {
        self.subscript = true;
        self
    }
    pub fn with_small(mut self) -> Self {
        self.small = true;
        self
    }
    pub fn with_inline_code(mut self) -> Self {
        self.inline_code = true;
        self.monospace = true;
        self
    }

    /// OR every flag with `other`. Folds a block-level base style
    /// (e.g. a heading's bold weight) into per-run inline flags so
    /// the block style isn't lost when a run carries its own flags.
    pub fn or(self, other: Self) -> Self {
        Self {
            bold: self.bold || other.bold,
            italic: self.italic || other.italic,
            monospace: self.monospace || other.monospace,
            strikethrough: self.strikethrough || other.strikethrough,
            underline: self.underline || other.underline,
            highlight: self.highlight || other.highlight,
            superscript: self.superscript || other.superscript,
            subscript: self.subscript || other.subscript,
            small_caps: self.small_caps || other.small_caps,
            small: self.small || other.small,
            inline_code: self.inline_code || other.inline_code,
        }
    }
}
