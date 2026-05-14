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
