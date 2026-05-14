//! Intermediate representation for the renderer.
//!
//! The lowering pass ([`super::lower`]) converts a [`Token`] stream
//! into a flat `Vec<Block>` plus per-block inline runs. The layout
//! pass ([`super::layout`]) consumes that IR.
//!
//! The IR is intentionally smaller than the [`Token`] enum — it drops
//! constructs the renderer doesn't handle yet (lists, blockquotes,
//! tables, images, links) and degrades them to plain paragraphs.
//! Phase 2+ will expand it.

/// A top-level block-level rendering unit.
#[derive(Debug, Clone)]
pub enum Block {
    /// A heading. `level` is 1..=6.
    Heading { level: u8, runs: Vec<InlineRun> },
    /// A paragraph of flowing text.
    Paragraph { runs: Vec<InlineRun> },
    /// A fenced or indented code block. One entry per source line.
    CodeBlock { lines: Vec<String> },
    /// A horizontal rule (`---`).
    HorizontalRule,
    /// A run of consecutive list items at the same level + marker
    /// shape. Ordered/unordered is determined by the entries.
    List { entries: Vec<ListEntry> },
    /// A block quote whose body is itself a sequence of [`Block`]s.
    BlockQuote { body: Vec<Block> },
    /// A GFM table.
    Table {
        headers: Vec<Vec<InlineRun>>,
        aligns: Vec<crate::markdown::TableAlignment>,
        rows: Vec<Vec<Vec<InlineRun>>>,
    },
    /// A block-level image. The lowering pass promotes a paragraph
    /// containing only an image to this variant; inline images keep
    /// their alt text in flow.
    Image {
        path: std::path::PathBuf,
        alt: String,
    },
    /// Verbatim block-level raw HTML. Rendered as a monospace block
    /// so the source stays visible. CommonMark §4.6 lets us choose
    /// whether to interpret HTML or pass it through; we pass through.
    HtmlBlock { content: String },
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
}

/// The marker drawn at the start of a [`ListEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListBullet {
    /// `- `, `+ `, `* ` — phase 2 renders them as a centered dot
    /// regardless of source marker. The original marker is preserved
    /// in case a later phase wants to honor it.
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
}

impl InlineRun {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            flags: RunFlags::default(),
            link: None,
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
        Block::BlockQuote { body } => {
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
                for r in header {
                    walk_run(r, u);
                }
            }
            for row in rows {
                for cell in row {
                    for r in cell {
                        walk_run(r, u);
                    }
                }
            }
        }
        Block::CodeBlock { .. } | Block::HtmlBlock { .. } => {
            u.mono_regular = true;
        }
        Block::HorizontalRule | Block::Image { .. } => {}
    }
}

fn walk_run(run: &InlineRun, u: &mut VariantUsage) {
    let f = run.flags;
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
    pub fn with_underline(mut self) -> Self {
        self.underline = true;
        self
    }
}
