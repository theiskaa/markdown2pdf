//! Markdown lexical analysis and token representation.
//!
//! This module provides the core lexical analysis functionality for parsing Markdown text into a
//! structured token stream. It handles both block-level elements like headings and lists, as well
//! as inline formatting like emphasis and links.
//!
//! The lexer maintains proper nesting of elements and handles edge cases around delimiter matching
//! and whitespace handling according to .
//!
//! # Examples
//! ```rust
//! use markdown2pdf::markdown::Token;
//!
//! // Heading token with nested content (level 1-6 is valid)
//! let heading = Token::Heading(vec![Token::Text("Title".to_string())], 1);
//! assert!(matches!(heading, Token::Heading(_, 1)));
//!
//! // Emphasis token with nested content (level 1-3 is valid)
//! let emphasis = Token::Emphasis {
//!     level: 1,
//!     content: vec![Token::Text("italic".to_string())]
//! };
//! assert!(matches!(emphasis, Token::Emphasis { level: 1, .. }));
//!
//! // Link token: parsed inline content + URL + optional title
//! let link = Token::Link {
//!     content: vec![Token::Text("Click here".to_string())],
//!     url: "https://example.com".to_string(),
//!     title: None,
//! };
//! assert!(matches!(link, Token::Link { .. }));
//! ```
//!
//! Token (nested) structure looks like:
//! Token::Heading
//! └── Vec<Token>
//!     ├── Token::Text
//!     ├── Token::Emphasis
//!     │   └── Vec<Token>
//!     │       └── Token::Text
//!     └── Token::Link
//!         ├── content: Vec<Token>
//!         ├── url: String
//!         └── title: Option<String>

use genpdfi::Alignment;
use std::collections::HashMap;

include!(concat!(env!("OUT_DIR"), "/entities_table.rs"));

/// Tag names that open a raw-content HTML block (CommonMark §4.6).
/// Body runs verbatim until a matching closer appears on any line.
const RAW_HTML_BLOCK_TAG_NAMES: &[&str] = &["script", "pre", "style", "textarea"];

/// Tag names that open a block-element HTML block (CommonMark §4.6).
/// Used for two purposes today:
///   1. To EXCLUDE these names from the standalone-tag arm (so a
///      `<div>` line doesn't get claimed by the wrong arm).
///   2. As the basis for the dedicated block-element arm that will
///      land in a follow-up commit (with its own opener-completeness
///      and paragraph-interrupt rules).
const BLOCK_ELEMENT_TAG_NAMES: &[&str] = &[
    "address", "article", "aside", "base", "basefont", "blockquote",
    "body", "caption", "center", "col", "colgroup", "dd", "details",
    "dialog", "dir", "div", "dl", "dt", "fieldset", "figcaption",
    "figure", "footer", "form", "frame", "frameset", "h1", "h2", "h3",
    "h4", "h5", "h6", "head", "header", "hr", "html", "iframe",
    "legend", "li", "link", "main", "menu", "menuitem", "nav",
    "noframes", "ol", "optgroup", "option", "p", "param", "search",
    "section", "summary", "table", "tbody", "td", "tfoot", "th",
    "thead", "title", "tr", "track", "ul",
];

/// Parsing context — determines which tokens are valid in the current location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseContext {
    Root,       // top-level document
    ListItem,   // inside a list item (block context)
    TableCell,  // inside a table cell (restrict block-level tokens)
    BlockQuote, // inside a blockquote (we'll treat as block-level but still disallow headings inside cells)
    Inline,     // inline parsing context (e.g., inside emphasis / link)
}

/// Represents the different types of tokens that can be parsed from Markdown text.
/// Each variant captures both the semantic meaning and associated content/metadata
/// needed to properly render the element.
#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    /// A heading with nested content and level (e.g., # h1, ## h2)
    Heading(Vec<Token>, usize),
    /// Emphasized text with configurable level (1-3) for * or _ delimiters
    Emphasis { level: usize, content: Vec<Token> },
    /// Strong emphasis (bold) text using ** or __ delimiters
    StrongEmphasis(Vec<Token>),
    /// Code construct. `block: true` for indented or fenced code blocks
    /// (rendered as `<pre><code>…</code></pre>`); `block: false` for inline
    /// code spans (`<code>…</code>`). `language` is the info-string first
    /// word for fenced blocks; empty for inline spans and indented blocks.
    Code {
        language: String,
        content: String,
        block: bool,
    },
    /// Block quote whose body is itself a sequence of tokens (so emphasis,
    /// links, code, etc. inside `> …` lines are properly parsed).
    BlockQuote(Vec<Token>),
    /// List item with nested content and type information
    ListItem {
        content: Vec<Token>,
        ordered: bool,
        number: Option<usize>, // For ordered lists (e.g., "1.", "2.")
        /// The marker character used for this item: `-`, `+`, `*` for
        /// bulleted lists; `.` or `)` for ordered lists. Renderers should
        /// split a run of sibling items into separate lists whenever the
        /// marker changes — `- foo\n+ bar` is two lists, not one.
        marker: char,
        /// GFM task list state: `None` = regular item, `Some(false)` = `[ ]`,
        /// `Some(true)` = `[x]` / `[X]`.
        checked: Option<bool>,
        /// True when this item is part of a *loose* list: any pair of
        /// sibling items at the same level is separated by a blank line.
        /// All items in the same list share the same value. Renderers
        /// should add paragraph spacing around the content of loose-list
        /// items and keep tight-list items inline.
        loose: bool,
    },
    /// Inline link. `content` is the parsed inline children of the link text
    /// (so emphasis, code spans, entities, etc. inside `[...]` are honored);
    /// `url` is the destination with escapes/entities already decoded;
    /// `title` is the optional title from `(url "title")` / `(url 'title')`
    /// / `(url (title))` syntax (also decoded). Autolinks produce a Link with
    /// a single Text child mirroring the URL.
    Link {
        content: Vec<Token>,
        url: String,
        title: Option<String>,
    },
    /// Inline image. `alt` is the parsed inline children of the alt text
    /// (renderers typically flatten this to plain text). `url` and `title`
    /// follow the same rules as `Link`.
    Image {
        alt: Vec<Token>,
        url: String,
        title: Option<String>,
    },
    /// Plain text content
    Text(String),
    /// Internal: a run of `*` or `_` delimiter characters before emphasis
    /// matching. After `resolve_emphasis` runs, unmatched runs are flattened
    /// into `Text` and matched runs become `Emphasis`. Should never escape
    /// the lexer to consumers.
    DelimRun {
        ch: char,
        count: usize,
    },
    /// Table with header, alignment info, and rows
    Table {
        headers: Vec<Vec<Token>>,
        aligns: Vec<Alignment>,
        rows: Vec<Vec<Vec<Token>>>,
    },
    /// Text alignment for table columns
    TableAlignment(Alignment),
    /// HTML comment content
    HtmlComment(String),
    /// Raw inline HTML (`<span>`, `</span>`, `<br/>`, etc.)
    /// Stored verbatim including the angle brackets.
    HtmlInline(String),
    /// Block-level raw HTML (CommonMark §4.6). Content is the verbatim
    /// block text including all original whitespace and line endings.
    /// Renderers should pass it through unmodified or skip it — no
    /// markdown parsing happens inside.
    HtmlBlock(String),
    /// Soft line break (single `\n`).
    Newline,
    /// Hard line break: two-or-more trailing spaces or a
    /// trailing backslash before the line terminator.
    HardBreak,
    /// Horizontal rule (---)
    HorizontalRule,
    /// GFM strikethrough (`~~text~~`).
    Strikethrough(Vec<Token>),
    /// Unknown or malformed token
    Unknown(String),
}

impl Token {
    /// Recursively extracts all text content from a token and its nested tokens.
    /// This is useful for collecting all characters used in a document for font subsetting.
    ///
    /// # Returns
    /// A string containing all text content from this token and any nested tokens.
    ///
    /// # Example
    /// ```
    /// use markdown2pdf::markdown::Token;
    ///
    /// let tokens = vec![
    ///     Token::Heading(vec![Token::Text("Title".to_string())], 1),
    ///     Token::Text("Body text with ăâîșț".to_string()),
    /// ];
    ///
    /// let all_text = Token::collect_all_text(&tokens);
    /// assert!(all_text.contains("Title"));
    /// assert!(all_text.contains("ăâîșț"));
    /// ```
    pub fn collect_all_text(tokens: &[Token]) -> String {
        let mut result = String::new();
        for token in tokens {
            token.collect_text_recursive(&mut result);
        }
        result
    }

    fn collect_text_recursive(&self, result: &mut String) {
        match self {
            Token::Text(s) => result.push_str(s),
            Token::DelimRun { ch, count } => {
                for _ in 0..*count {
                    result.push(*ch);
                }
            }
            Token::Heading(nested, _) => {
                for token in nested {
                    token.collect_text_recursive(result);
                }
            }
            Token::Emphasis { content, .. } => {
                for token in content {
                    token.collect_text_recursive(result);
                }
            }
            Token::StrongEmphasis(nested) => {
                for token in nested {
                    token.collect_text_recursive(result);
                }
            }
            Token::Code { content, .. } => result.push_str(content),
            Token::BlockQuote(body) => {
                for token in body {
                    token.collect_text_recursive(result);
                }
            }
            Token::ListItem { content, .. } => {
                for token in content {
                    token.collect_text_recursive(result);
                }
            }
            Token::Link { content, .. } => {
                for token in content {
                    token.collect_text_recursive(result);
                }
            }
            Token::Image { alt, .. } => {
                for token in alt {
                    token.collect_text_recursive(result);
                }
            }
            Token::HtmlComment(comment) => result.push_str(comment),
            Token::HtmlInline(html) => result.push_str(html),
            Token::HtmlBlock(html) => result.push_str(html),
            Token::Unknown(text) => result.push_str(text),
            Token::Newline | Token::HardBreak | Token::HorizontalRule => {
                // These don't contain text
            }
            Token::Strikethrough(nested) => {
                for token in nested {
                    token.collect_text_recursive(result);
                }
            }
            Token::Table {
                headers,
                aligns: _,
                rows,
            } => {
                for header in headers {
                    for token in header {
                        token.collect_text_recursive(result);
                    }
                }
                for row in rows {
                    for cell in row {
                        for token in cell {
                            token.collect_text_recursive(result);
                        }
                    }
                }
            }
            Token::TableAlignment(_) => {
                // These don't contain text
            }
        }
    }
}

/// Tries to decode an HTML/CommonMark entity reference starting at
/// `chars[start]` (which must be `&`). On success returns
/// `Some((decoded_string, length_consumed))` so the caller can advance.
/// Returns `None` if the sequence isn't a valid recognized entity, in which
/// case the caller should emit `&` as a literal char.
///
/// Only semicolon-terminated references are valid. Numeric references for
/// code point 0, surrogates, or values above 0x10FFFF decode to U+FFFD
/// (REPLACEMENT CHARACTER) rather than failing — only syntactically
/// invalid references (empty digits, non-hex digits, missing `;`) fall
/// back to a literal `&`.
fn try_decode_entity(chars: &[char], start: usize) -> Option<(String, usize)> {
    if chars.get(start) != Some(&'&') {
        return None;
    }
    // The longest CommonMark-valid entity name is 31 chars
    // (`CounterClockwiseContourIntegral`); plus `&` and `;` that's 33.
    // Numeric refs can reach `&#xXXXXXXXX;` = 12 chars. 64 leaves headroom
    // and rules out runaway scans through giant paragraphs.
    let mut end = start + 1;
    while end < chars.len() && end - start < 64 {
        if chars[end] == ';' {
            break;
        }
        end += 1;
    }
    if end >= chars.len() || chars[end] != ';' {
        return None;
    }
    let body: String = chars[start + 1..end].iter().collect();
    let consumed = end - start + 1;

    // Numeric reference: &#NNN; or &#xHH; / &#XHH;
    if let Some(rest) = body.strip_prefix('#') {
        let (radix, digits) = if rest.starts_with('x') || rest.starts_with('X') {
            (16, &rest[1..])
        } else {
            (10, rest)
        };
        if digits.is_empty() {
            return None;
        }
        let max_digits = if radix == 16 { 6 } else { 7 };
        if digits.len() > max_digits {
            return None;
        }
        // Invalid Unicode code points (including the null character,
        // surrogates, and out-of-range values) are replaced with U+FFFD.
        // Only a *syntactic* failure (overflowing u32, non-digits in the
        // chosen radix) falls back to literal.
        let Ok(code) = u32::from_str_radix(digits, radix) else {
            return None;
        };
        let ch = if code == 0 || (0xD800..=0xDFFF).contains(&code) || code > 0x10FFFF {
            '\u{FFFD}'
        } else {
            match char::from_u32(code) {
                Some(c) => c,
                None => '\u{FFFD}',
            }
        };
        return Some((ch.to_string(), consumed));
    }

    // Named entity — full CommonMark / WHATWG HTML5 table (~2,125 entries)
    // built at compile time by build.rs.
    NAMED_ENTITIES
        .get(body.as_str())
        .map(|s| ((*s).to_string(), consumed))
}

/// Processes backslash escapes (`\<punct>` → `<punct>`) and entity / numeric
/// character references in a flat string. Used where escape/entity decoding
/// must happen but we don't have a Lexer in scope (reference-definition
/// pre-pass, fenced code info string capture, etc.).
/// Strips `strip_cols` columns of leading whitespace from `chars[from..to]`,
/// honoring tab-stop expansion. When a tab boundary lands inside the strip
/// target, the leftover portion of the tab is emitted as the appropriate
/// number of spaces in the output. Any non-whitespace content (and any
/// in-line tabs) past the strip target is preserved verbatim.
fn strip_leading_cols(chars: &[char], from: usize, to: usize, strip_cols: usize) -> String {
    // Expand leading whitespace tabs to spaces (tab-stop = 4), strip
    // `strip_cols` from the result, then append the rest of the line
    // verbatim. Expanding at the leading edge is what keeps column-aligned
    // stripping correct under cumulative passes (e.g. list-item content
    // strip → indented-code strip). Without it, the second pass measures
    // the post-strip tab as 2 cols instead of its original 4, silently
    // dropping 2 cols of width.
    let mut leading = String::new();
    let mut col = 0usize;
    let mut i = from;
    while i < to {
        match chars[i] {
            ' ' => {
                leading.push(' ');
                col += 1;
                i += 1;
            }
            '\t' => {
                let span = 4 - (col % 4);
                for _ in 0..span {
                    leading.push(' ');
                }
                col += span;
                i += 1;
            }
            _ => break,
        }
    }
    let stripped: String = leading.chars().skip(strip_cols).collect();
    let mut out = stripped;
    while i < to {
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// Strips the optional closing `#` sequence from an ATX heading line per
/// CommonMark §4.2. The trailing run of unescaped `#` chars is removed, plus
/// any whitespace that immediately preceded it. An odd-length run of `\`
/// chars directly before the trailing `#`s escapes them — in that case
/// nothing is stripped. If the heading line is all `#`s and trailing
/// whitespace, returns the empty string (empty heading).
fn strip_atx_trailing_hashes(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut end = chars.len();
    while end > 0 && (chars[end - 1] == ' ' || chars[end - 1] == '\t') {
        end -= 1;
    }
    let mut hash_run_start = end;
    while hash_run_start > 0 && chars[hash_run_start - 1] == '#' {
        hash_run_start -= 1;
    }
    if hash_run_start == end {
        // No trailing # run — pass through.
        return chars.iter().collect();
    }
    // If the # immediately before the run is escaped by an odd number of
    // backslashes, treat the # as content and don't strip.
    let mut backslashes = 0;
    let mut p = hash_run_start;
    while p > 0 && chars[p - 1] == '\\' {
        backslashes += 1;
        p -= 1;
    }
    if backslashes % 2 == 1 {
        return chars.iter().collect();
    }
    if hash_run_start == 0 {
        // Heading is all `#` + trailing whitespace — empty content.
        return String::new();
    }
    let prev = chars[hash_run_start - 1];
    if prev != ' ' && prev != '\t' {
        // The # run must be preceded by whitespace to count as closing.
        return chars.iter().collect();
    }
    let mut new_end = hash_run_start;
    while new_end > 0 && (chars[new_end - 1] == ' ' || chars[new_end - 1] == '\t') {
        new_end -= 1;
    }
    chars[..new_end].iter().collect()
}

fn decode_escapes_and_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() && is_ascii_punctuation(chars[i + 1]) {
            out.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '&' {
            if let Some((decoded, consumed)) = try_decode_entity(&chars, i) {
                out.push_str(&decoded);
                i += consumed;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Tries to parse a single line as a CommonMark link reference definition:
/// `(spaces 0-3)[label]:(spaces)url(spaces title)?(spaces)?`.
/// Returns `(label, url, optional_title)` if the whole line matches.
/// Tries to parse a link reference definition starting at `chars[start]`.
/// Definitions may span multiple source lines:
///   `[label]:` followed by optional whitespace (possibly newlines, no blank
///   line), then URL, then optional whitespace (possibly newlines, no blank
///   line), then optional title. The title body itself may span lines as
///   long as no blank line appears inside it.
///
/// Returns `(label, url, optional_title, end_position_in_chars)` on success,
/// where `end_position` is the index past the trailing newline of the
/// definition. `None` means no valid definition at this position.
fn try_parse_definition(
    chars: &[char],
    start: usize,
) -> Option<(String, String, Option<String>, usize)> {
    let mut i = start;

    // Up to 3 leading spaces.
    let mut leading = 0usize;
    while i < chars.len() && chars[i] == ' ' && leading < 3 {
        i += 1;
        leading += 1;
    }

    // `[`
    if chars.get(i) != Some(&'[') {
        return None;
    }
    i += 1;

    // Label body: read until unescaped `]`. May contain newlines but no
    // blank line. Unescaped `[` is also disallowed.
    let label_start = i;
    loop {
        if i >= chars.len() {
            return None;
        }
        let c = chars[i];
        if c == ']' {
            break;
        }
        if c == '[' {
            return None;
        }
        if c == '\\' && i + 1 < chars.len() && is_ascii_punctuation(chars[i + 1]) {
            i += 2;
            continue;
        }
        if c == '\n' {
            // No blank line inside label.
            let mut j = i + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j >= chars.len() || chars[j] == '\n' {
                return None;
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    let label: String = chars[label_start..i].iter().collect();
    if label.trim().is_empty() {
        return None;
    }
    // Label must contain at least one non-whitespace char (already checked).
    i += 1; // past ]

    // `:`
    if chars.get(i) != Some(&':') {
        return None;
    }
    i += 1;

    // Whitespace before URL — at most one newline (no blank line).
    let mut newlines = 0usize;
    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' => i += 1,
            '\n' => {
                newlines += 1;
                if newlines > 1 {
                    return None;
                }
                i += 1;
            }
            _ => break,
        }
    }
    if i >= chars.len() {
        return None;
    }

    // URL — angle-bracket form or plain.
    let url = if chars[i] == '<' {
        i += 1;
        let s = i;
        loop {
            if i >= chars.len() {
                return None;
            }
            let c = chars[i];
            if c == '>' {
                break;
            }
            if c == '<' || c == '\n' {
                return None;
            }
            if c == '\\' && i + 1 < chars.len() && is_ascii_punctuation(chars[i + 1]) {
                i += 2;
                continue;
            }
            i += 1;
        }
        let raw: String = chars[s..i].iter().collect();
        i += 1; // past >
        decode_escapes_and_entities(&raw)
    } else {
        let s = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            if chars[i] == '\\' && i + 1 < chars.len() && is_ascii_punctuation(chars[i + 1]) {
                i += 2;
                continue;
            }
            i += 1;
        }
        if i == s {
            return None;
        }
        let raw: String = chars[s..i].iter().collect();
        decode_escapes_and_entities(&raw)
    };

    // After URL, optionally one or more whitespace chars (possibly newline,
    // no blank line) before title.
    let after_url = i;
    let mut newlines_after_url = 0usize;
    let mut q = after_url;
    while q < chars.len() {
        match chars[q] {
            ' ' | '\t' => q += 1,
            '\n' => {
                newlines_after_url += 1;
                if newlines_after_url > 1 {
                    break;
                }
                q += 1;
            }
            _ => break,
        }
    }

    // If we hit blank line / EOF, the definition has no title — but only if
    // the URL was followed by valid line-ending whitespace.
    let no_title_def = || -> Option<(String, String, Option<String>, usize)> {
        // Validate that the URL line had only whitespace before its end.
        let mut k = after_url;
        while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t') {
            k += 1;
        }
        if k < chars.len() && chars[k] != '\n' {
            return None;
        }
        let end = if k < chars.len() { k + 1 } else { k };
        Some((label.clone(), url.clone(), None, end))
    };

    if q >= chars.len() || (newlines_after_url > 1) {
        return no_title_def();
    }

    let title_open = chars[q];
    if !matches!(title_open, '"' | '\'' | '(') {
        return no_title_def();
    }
    // The title must be separated from the URL by at least one whitespace
    // char (space, tab, or newline). Without that gap, e.g. `<bar>(baz)`,
    // the `(...)` is not a title — and since the URL line then has
    // unexpected non-whitespace content, the whole definition is invalid.
    if q == after_url {
        return None;
    }
    let close = match title_open {
        '"' => '"',
        '\'' => '\'',
        '(' => ')',
        _ => unreachable!(),
    };

    // Try to read title from position q.
    let mut t = q + 1;
    let title_start = t;
    loop {
        if t >= chars.len() {
            return no_title_def();
        }
        let c = chars[t];
        if c == close {
            break;
        }
        if c == '\\' && t + 1 < chars.len() && is_ascii_punctuation(chars[t + 1]) {
            t += 2;
            continue;
        }
        if c == '\n' {
            // No blank line inside title.
            let mut j = t + 1;
            while j < chars.len() && (chars[j] == ' ' || chars[j] == '\t') {
                j += 1;
            }
            if j >= chars.len() || chars[j] == '\n' {
                return no_title_def();
            }
        }
        t += 1;
    }
    let title_raw: String = chars[title_start..t].iter().collect();
    let title = decode_escapes_and_entities(&title_raw);
    t += 1; // past close
    // Trailing chars on title-end line must be whitespace ending in \n / EOF.
    let mut k = t;
    while k < chars.len() && (chars[k] == ' ' || chars[k] == '\t') {
        k += 1;
    }
    if k < chars.len() && chars[k] != '\n' {
        return no_title_def();
    }
    let end = if k < chars.len() { k + 1 } else { k };
    Some((label, url, Some(title), end))
}

/// Walks the token tree and marks list items as loose when a sibling pair is
/// separated by one or more blank lines. Operates on the top-level vec and
/// recurses into each `ListItem.content` to handle nested lists.
///
/// A list is loose iff any pair of consecutive sibling items in that list is
/// blank-line separated. All items in the same list share the resulting
/// `loose` flag. Blank-line separation surfaces in the token stream as one
/// or more `Token::Newline` tokens between two `Token::ListItem` siblings.
fn propagate_loose_tight(tokens: &mut [Token]) {
    let mut i = 0;
    while i < tokens.len() {
        if !matches!(tokens[i], Token::ListItem { .. }) {
            i += 1;
            continue;
        }
        // Start of a run. Walk forward over (ListItem | Newline*) sequences,
        // tracking whether any blank line separated two sibling items.
        let run_start = i;
        let mut has_blank_between = false;
        let mut last_item_end = i;
        // An item internally containing a blank-line paragraph break also
        // makes the whole list loose (per spec "any item directly contains
        // two block-level elements with a blank line between them").
        if item_has_internal_blank(&tokens[i]) {
            has_blank_between = true;
        }
        loop {
            i += 1;
            // Skip across any Newlines — but if there are ≥2 in a row between
            // two list items, that's a blank line and the list is loose.
            let mut newlines = 0;
            while i < tokens.len() && matches!(tokens[i], Token::Newline) {
                newlines += 1;
                i += 1;
            }
            if i >= tokens.len() || !matches!(tokens[i], Token::ListItem { .. }) {
                // Run terminated. The outer reassignment below handles position.
                break;
            }
            // Another sibling. If we crossed at least one Newline to reach it,
            // that's a blank-line separation per the lexer's emission shape.
            if newlines >= 1 {
                has_blank_between = true;
            }
            if item_has_internal_blank(&tokens[i]) {
                has_blank_between = true;
            }
            last_item_end = i;
        }
        // Backpatch all items in [run_start, last_item_end] with the loose flag.
        let loose = has_blank_between;
        for tok in &mut tokens[run_start..=last_item_end] {
            if let Token::ListItem { loose: l, .. } = tok {
                *l = *l || loose;
            }
        }
        i = last_item_end + 1;
    }
    // Recurse into children of every ListItem and every BlockQuote so nested
    // lists also get their loose flag computed.
    for tok in tokens.iter_mut() {
        match tok {
            Token::ListItem { content, .. } => propagate_loose_tight(content),
            Token::BlockQuote(body) => propagate_loose_tight(body),
            _ => {}
        }
    }
}

/// Returns true if a ListItem's content contains a blank-line break — i.e.,
/// two or more consecutive `Token::Newline` tokens, which `parse_list_item`
/// emits when a continuation paragraph rejoins the item after a blank gap.
fn item_has_internal_blank(tok: &Token) -> bool {
    let Token::ListItem { content, .. } = tok else {
        return false;
    };
    let mut run = 0;
    for t in content {
        if matches!(t, Token::Newline) {
            run += 1;
            if run >= 2 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// Normalizes a reference-link label ASCII case-fold
/// plus internal-whitespace collapse plus leading/trailing trim.
/// True if the line `chars[start..end]` is blank or is a single-line block
/// construct (ATX heading, thematic break) — i.e., it does NOT leave an open
/// paragraph behind. Used by `extract_definitions` to decide whether the
/// next line is allowed to begin a link-reference definition.
fn is_paragraph_breaking_line_chars(chars: &[char], start: usize, end: usize) -> bool {
    let mut p = start;
    let mut indent = 0;
    while p < end && chars[p] == ' ' && indent < 3 {
        p += 1;
        indent += 1;
    }
    if p >= end {
        return true; // blank line
    }
    // ATX heading: 1-6 `#` then space/tab/EOL.
    if chars[p] == '#' {
        let mut h = 0;
        while p + h < end && chars[p + h] == '#' {
            h += 1;
        }
        if (1..=6).contains(&h) {
            let after = chars.get(p + h);
            if after.is_none()
                || matches!(after, Some(' ') | Some('\t') | Some('\n'))
            {
                return true;
            }
        }
    }
    // Thematic break: 3+ matching markers from `-`/`*`/`_`, with allowed
    // interspersed whitespace.
    if matches!(chars[p], '-' | '*' | '_') {
        let marker = chars[p];
        let mut count = 0;
        let mut q = p;
        while q < end {
            if chars[q] == marker {
                count += 1;
            } else if chars[q] != ' ' && chars[q] != '\t' {
                return false;
            }
            q += 1;
        }
        if count >= 3 {
            return true;
        }
    }
    false
}

fn normalize_label(s: &str) -> String {
    // Per CommonMark, label comparison is the case-folded, whitespace-
    // collapsed RAW string — no backslash-escape or entity decoding. So
    // `[foo\!]` and `[foo!]` do NOT match, even though `\!` would decode
    // to `!`. Both ref and def labels must keep their literal source chars
    // before this normalize.
    let mut out = String::new();
    let mut prev_ws = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            case_fold_char(c, &mut out);
            prev_ws = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Approximates full Unicode case folding (per TR21) using `to_lowercase`
/// for the common range plus a few special-case mappings that diverge
/// from lowercase. Notably `ẞ`/`ß` fold to `ss` so `[ẞ]` matches a
/// `[SS]: …` reference definition.
fn case_fold_char(c: char, out: &mut String) {
    match c {
        'ẞ' | 'ß' => out.push_str("ss"),
        '\u{0130}' => {
            out.push('i');
            out.push('\u{0307}');
        }
        '\u{0149}' => {
            out.push('\u{02BC}');
            out.push('n');
        }
        '\u{017F}' => out.push('s'),
        _ => {
            for ch in c.to_lowercase() {
                out.push(ch);
            }
        }
    }
}

/// If a code-span body begins AND ends with a space (and is not entirely
/// composed of spaces), strip exactly one leading and one trailing space.
/// Otherwise leave content untouched.
fn strip_code_span_outer_space(s: String) -> String {
    if s.len() >= 2 && s.starts_with(' ') && s.ends_with(' ') && !s.chars().all(|c| c == ' ') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

/// CommonMark "Unicode punctuation" predicate, used by the left/right-
/// flanking-run rules. Per CommonMark 0.31.2, this covers all ASCII
/// punctuation plus the Unicode general categories `P*` (punctuation) and
/// `S*` (symbols — currency, math, modifier, other). Without `S*` coverage,
/// emphasis flanking misclassifies inputs like `*£*alpha.` (currency `£`
/// is `Sc`).
fn is_md_punctuation(c: char) -> bool {
    if is_ascii_punctuation(c) {
        return true;
    }
    if (c as u32) < 0x80 {
        return false;
    }
    matches!(c,
        '\u{00A1}'..='\u{00BF}'
            | '\u{00D7}'
            | '\u{00F7}'
            | '\u{2000}'..='\u{206F}'
            | '\u{2070}'..='\u{209F}'
            | '\u{20A0}'..='\u{20CF}'
            | '\u{2100}'..='\u{214F}'
            | '\u{2150}'..='\u{218F}'
            | '\u{2190}'..='\u{21FF}'
            | '\u{2200}'..='\u{22FF}'
            | '\u{2300}'..='\u{23FF}'
            | '\u{2400}'..='\u{243F}'
            | '\u{2500}'..='\u{257F}'
            | '\u{2580}'..='\u{259F}'
            | '\u{25A0}'..='\u{25FF}'
            | '\u{2600}'..='\u{26FF}'
            | '\u{2700}'..='\u{27BF}'
            | '\u{27C0}'..='\u{27EF}'
            | '\u{27F0}'..='\u{27FF}'
            | '\u{2800}'..='\u{28FF}'
            | '\u{2900}'..='\u{297F}'
            | '\u{2980}'..='\u{29FF}'
            | '\u{2A00}'..='\u{2AFF}'
            | '\u{2B00}'..='\u{2BFF}'
            | '\u{3000}'..='\u{303F}'
            | '\u{FE30}'..='\u{FE4F}'
            | '\u{FE50}'..='\u{FE6F}'
            | '\u{FF00}'..='\u{FFEF}'
    ) && !matches!(c, '\u{00A0}' | '\u{2028}' | '\u{2029}')
}

#[derive(Debug, Clone)]
struct EmDelim {
    pos: usize,
    ch: char,
    count: usize,
    can_open: bool,
    can_close: bool,
}

/// CommonMark emphasis algorithm. Scans `tokens` for delimiter-run text
/// tokens (`Text("***")`, `Text("___")`, etc.), computes left/right
/// flanking using neighboring tokens, and matches opener/closer pairs
/// per the stack-based algorithm with Rule 9/10 (mod-3) and Rule 13
/// (`n = 2` when both opener and closer ≥ 2). Matched pairs become
/// `Token::Emphasis { level, content }`; unmatched delim runs stay as
/// literal `Text`. Paragraph boundaries (consecutive `Newline` tokens)
/// split the scan so emphasis can't cross a blank line.
fn resolve_emphasis(tokens: &mut Vec<Token>) {
    // Split tokens into paragraph-bounded chunks at any pair of consecutive
    // `Newline` tokens (CommonMark: the delimiter stack resets across blank
    // lines). Build the result Vec by extending it chunk-by-chunk; doing
    // this in place via `drain` + `insert` is O(N²) because each insert
    // shifts the tail.
    let mut result: Vec<Token> = Vec::with_capacity(tokens.len());
    let mut chunk: Vec<Token> = Vec::new();
    let original = std::mem::take(tokens);
    let mut i = 0;
    while i < original.len() {
        if i + 1 < original.len()
            && matches!(original[i], Token::Newline)
            && matches!(original[i + 1], Token::Newline)
        {
            if !chunk.is_empty() {
                resolve_emphasis_chunk(&mut chunk);
                result.append(&mut chunk);
            }
            // Run of newlines passes through verbatim.
            while i < original.len() && matches!(original[i], Token::Newline) {
                result.push(Token::Newline);
                i += 1;
            }
            continue;
        }
        chunk.push(original[i].clone());
        i += 1;
    }
    if !chunk.is_empty() {
        resolve_emphasis_chunk(&mut chunk);
        result.append(&mut chunk);
    }
    *tokens = result;
}

fn resolve_emphasis_chunk(tokens: &mut Vec<Token>) {
    // Canonical CommonMark emphasis algorithm (cmark's `process_emphasis`):
    // walk closers left-to-right ONCE, maintain `openers_bottom` per
    // (delim-char, count%3, can-open-too) to short-circuit Rule 9/10
    // failures. Total work is O(D) amortized over D delimiter runs —
    // dramatically better than the previous O(D³) "rebuild + scan" loop
    // which timed out on inputs with thousands of delimiters.
    let mut delims = find_em_delims(tokens);
    if delims.is_empty() {
        return;
    }
    // active[i] = false means delim i has been consumed or deactivated.
    let mut active: Vec<bool> = vec![true; delims.len()];
    // openers_bottom[ch_idx][count%3][can-also-close as 0/1] = minimum
    // delim index a future closer must search back to. Below this we
    // know no opener of the matching (ch, count%3, can-also-close)
    // shape exists.
    let mut openers_bottom = [[[0usize; 2]; 3]; 2];

    fn ch_idx(c: char) -> usize {
        if c == '*' {
            0
        } else {
            1
        }
    }

    let mut ci = 0;
    while ci < delims.len() {
        if !active[ci] || !delims[ci].can_close {
            ci += 1;
            continue;
        }
        let c_ch = delims[ci].ch;
        let c_can_open = delims[ci].can_open;
        let c_count = delims[ci].count;
        let bottom = openers_bottom[ch_idx(c_ch)][c_count % 3]
            [if c_can_open { 1 } else { 0 }];
        let mut oi_found: Option<usize> = None;
        let mut oi = ci;
        while oi > bottom {
            oi -= 1;
            if !active[oi] {
                continue;
            }
            let o = &delims[oi];
            if !o.can_open || o.ch != c_ch {
                continue;
            }
            // Rule 9/10
            if (o.can_open && o.can_close) || c_can_open {
                let sum = o.count + c_count;
                if sum % 3 == 0
                    && !(o.count % 3 == 0 && c_count % 3 == 0)
                {
                    continue;
                }
            }
            oi_found = Some(oi);
            break;
        }
        if let Some(oi) = oi_found {
            let opener = delims[oi].clone();
            let closer = delims[ci].clone();
            let n = if opener.count >= 2 && closer.count >= 2 { 2 } else { 1 };
            // Deactivate delims between opener and closer (they're inside
            // the wrapped Emphasis content; an inner pair may still match
            // via the recursive call below in wrap_emphasis_pair).
            for k in (oi + 1)..ci {
                active[k] = false;
            }
            wrap_emphasis_pair(
                tokens,
                opener.pos,
                closer.pos,
                opener.count,
                closer.count,
                n,
            );
            // Update delim positions after the splice. The splice replaced
            // tokens[opener.pos..closer.pos+1] with 1-3 new tokens.
            let old_len = closer.pos - opener.pos + 1;
            let new_len = 1
                + (if opener.count > n { 1 } else { 0 })
                + (if closer.count > n { 1 } else { 0 });
            let shift: i64 = new_len as i64 - old_len as i64;
            for d in delims.iter_mut().skip(ci + 1) {
                d.pos = ((d.pos as i64) + shift) as usize;
            }
            // The opener's token position: if opener kept characters, the
            // opener delim is at opener.pos; else removed.
            let new_opener_pos = opener.pos;
            // The closer's token position: opener.pos + (opener has remainder ? 1 : 0) + 1 (for Emphasis).
            let new_closer_pos = opener.pos
                + (if opener.count > n { 1 } else { 0 })
                + 1;
            delims[oi].count -= n;
            delims[oi].pos = new_opener_pos;
            if delims[oi].count == 0 {
                active[oi] = false;
            }
            delims[ci].count -= n;
            delims[ci].pos = new_closer_pos;
            if delims[ci].count == 0 {
                active[ci] = false;
                ci += 1;
            }
            // Otherwise: closer has chars left — recheck it as closer next
            // iter (don't advance ci).
        } else {
            openers_bottom[ch_idx(c_ch)][c_count % 3]
                [if c_can_open { 1 } else { 0 }] = ci;
            if !c_can_open {
                active[ci] = false;
            }
            ci += 1;
        }
    }

    // Any remaining DelimRun (active or not — same data either way) at the
    // token level becomes literal text. wrap_emphasis_pair already recurses
    // into Emphasis content, so leftover DelimRuns there are also handled.
    for t in tokens.iter_mut() {
        if let Token::DelimRun { ch, count } = t {
            *t = Token::Text(ch.to_string().repeat(*count));
        }
    }
}

fn find_em_delims(tokens: &[Token]) -> Vec<EmDelim> {
    let mut out = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        let Token::DelimRun { ch, count } = tok else { continue };
        let (can_open, can_close) = compute_em_flanking(tokens, i, *ch);
        out.push(EmDelim { pos: i, ch: *ch, count: *count, can_open, can_close });
    }
    out
}

fn compute_em_flanking(tokens: &[Token], idx: usize, ch: char) -> (bool, bool) {
    let before = char_before_token(tokens, idx);
    let after = char_after_token(tokens, idx);
    let lf = em_is_left_flanking(before, after);
    let rf = em_is_right_flanking(before, after);
    if ch == '*' {
        (lf, rf)
    } else {
        let can_open =
            lf && (!rf || matches!(before, Some(c) if is_md_punctuation(c)));
        let can_close =
            rf && (!lf || matches!(after, Some(c) if is_md_punctuation(c)));
        (can_open, can_close)
    }
}

fn em_is_left_flanking(before: Option<char>, after: Option<char>) -> bool {
    let Some(a) = after else { return false };
    if a.is_whitespace() {
        return false;
    }
    if !is_md_punctuation(a) {
        return true;
    }
    match before {
        None => true,
        Some(b) => b.is_whitespace() || is_md_punctuation(b),
    }
}

fn em_is_right_flanking(before: Option<char>, after: Option<char>) -> bool {
    let Some(b) = before else { return false };
    if b.is_whitespace() {
        return false;
    }
    if !is_md_punctuation(b) {
        return true;
    }
    match after {
        None => true,
        Some(a) => a.is_whitespace() || is_md_punctuation(a),
    }
}

fn char_before_token(tokens: &[Token], idx: usize) -> Option<char> {
    for i in (0..idx).rev() {
        if let Some(c) = last_meaningful_char(&tokens[i]) {
            return Some(c);
        }
    }
    None
}

fn char_after_token(tokens: &[Token], idx: usize) -> Option<char> {
    for i in (idx + 1)..tokens.len() {
        if let Some(c) = first_meaningful_char(&tokens[i]) {
            return Some(c);
        }
    }
    None
}

fn last_meaningful_char(tok: &Token) -> Option<char> {
    match tok {
        Token::Text(s) => s.chars().last(),
        Token::DelimRun { ch, .. } => Some(*ch),
        Token::Code { content, .. } => content.chars().last().or(Some('`')),
        Token::HtmlInline(s) | Token::HtmlComment(s) => s.chars().last(),
        Token::Emphasis { content, .. } => last_meaningful_in_slice(content),
        Token::StrongEmphasis(content) => last_meaningful_in_slice(content),
        Token::Strikethrough(content) => last_meaningful_in_slice(content),
        Token::Link { content, .. } => last_meaningful_in_slice(content),
        Token::Image { alt, .. } => last_meaningful_in_slice(alt),
        Token::Heading(content, _) => last_meaningful_in_slice(content),
        Token::Newline | Token::HardBreak => Some(' '),
        Token::Unknown(s) => s.chars().last(),
        _ => None,
    }
}

fn first_meaningful_char(tok: &Token) -> Option<char> {
    match tok {
        Token::Text(s) => s.chars().next(),
        Token::DelimRun { ch, .. } => Some(*ch),
        Token::Code { content, .. } => content.chars().next().or(Some('`')),
        Token::HtmlInline(s) | Token::HtmlComment(s) => s.chars().next(),
        Token::Emphasis { content, .. } => first_meaningful_in_slice(content),
        Token::StrongEmphasis(content) => first_meaningful_in_slice(content),
        Token::Strikethrough(content) => first_meaningful_in_slice(content),
        Token::Link { content, .. } => first_meaningful_in_slice(content),
        Token::Image { alt, .. } => first_meaningful_in_slice(alt),
        Token::Heading(content, _) => first_meaningful_in_slice(content),
        Token::Newline | Token::HardBreak => Some(' '),
        Token::Unknown(s) => s.chars().next(),
        _ => None,
    }
}

fn last_meaningful_in_slice(slice: &[Token]) -> Option<char> {
    for t in slice.iter().rev() {
        if let Some(c) = last_meaningful_char(t) {
            return Some(c);
        }
    }
    None
}

fn first_meaningful_in_slice(slice: &[Token]) -> Option<char> {
    for t in slice {
        if let Some(c) = first_meaningful_char(t) {
            return Some(c);
        }
    }
    None
}

fn wrap_emphasis_pair(
    tokens: &mut Vec<Token>,
    opener_pos: usize,
    closer_pos: usize,
    opener_count: usize,
    closer_count: usize,
    n: usize,
) {
    let opener_ch = match &tokens[opener_pos] {
        Token::DelimRun { ch, .. } => *ch,
        _ => return,
    };
    let closer_ch = match &tokens[closer_pos] {
        Token::DelimRun { ch, .. } => *ch,
        _ => return,
    };
    let opener_remaining = opener_count - n;
    let closer_remaining = closer_count - n;
    let mut inside: Vec<Token> = tokens[opener_pos + 1..closer_pos].to_vec();
    // Inner pairs may still match among themselves once the outer pair has
    // been wrapped (`**a*b*c**` keeps `*b*` as a nested em). Recurse so
    // any leftover `DelimRun` becomes either `Emphasis` or `Text`, never
    // escaping into a final `Emphasis.content` slot.
    resolve_emphasis_chunk(&mut inside);
    let emph = Token::Emphasis { level: n, content: inside };
    let mut replacement = Vec::new();
    if opener_remaining > 0 {
        replacement.push(Token::DelimRun { ch: opener_ch, count: opener_remaining });
    }
    replacement.push(emph);
    if closer_remaining > 0 {
        replacement.push(Token::DelimRun { ch: closer_ch, count: closer_remaining });
    }
    tokens.splice(opener_pos..closer_pos + 1, replacement);
}

/// True for the 32 ASCII punctuation characters that allows
/// to be backslash-escaped. Backslash before any other char (letters, digits,
/// whitespace, end-of-input) leaves the backslash as literal text.
fn is_ascii_punctuation(c: char) -> bool {
    matches!(
        c,
        '!' | '"'
            | '#'
            | '$'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | '+'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '<'
            | '='
            | '>'
            | '?'
            | '@'
            | '['
            | '\\'
            | ']'
            | '^'
            | '_'
            | '`'
            | '{'
            | '|'
            | '}'
            | '~'
    )
}

/// Error types that can occur during lexical analysis
#[derive(Debug)]
pub enum LexerError {
    /// Input ended unexpectedly while parsing a token
    UnexpectedEndOfInput,
    /// Encountered an invalid or malformed token
    UnknownToken(String),
}

/// A lexical analyzer that converts Markdown text into a sequence of tokens.
/// Handles nested structures and special Markdown syntax elements while maintaining
/// proper context and state during parsing.
pub struct Lexer {
    /// Input text as character vector for efficient parsing
    input: Vec<char>,
    /// Current position in the input stream
    position: usize,
    /// Set by `parse_text` when it consumes a hard-break-triggering line
    /// ending (two trailing spaces or a trailing backslash). Read and
    /// cleared by the next `next_token` call so the break is emitted as
    /// `Token::HardBreak`.
    pending_hard_break: bool,
    /// True while we're parsing the inline content of an ATX/setext heading.
    /// Hard breaks (`  \n` or `\\\n`) are not valid inside headings, so
    /// `parse_text` skips its hard-break promotion when this is set.
    in_heading: bool,
    /// True when the most recently emitted top-level token was a ListItem.
    /// Used by the dispatcher to decide whether `-\n` (empty list marker)
    /// should open / continue a list, which is only legal as a sibling of an
    /// existing list item — never as the first interrupter of a paragraph.
    last_emitted_list_item: bool,
    /// True when the most recent top-level emission was an inline-content
    /// token (paragraph text). Reset by block emissions and blank lines.
    /// Used by indented-code detection: per spec, indented code blocks may
    /// not interrupt a paragraph, but can follow headings, HRs, etc.
    last_emitted_was_paragraph_text: bool,
    /// Reference-link definitions collected by `extract_definitions()` in
    /// the pre-pass. Keys are normalized (lowercased, whitespace-collapsed);
    /// values are `(url, title)`.
    definitions: HashMap<String, (String, Option<String>)>,
    /// When true, `peek_setext_level` returns None. Set by parsers that
    /// build a sub-Lexer input from lines that include lazy-continuation
    /// content (e.g. blockquote body): per CommonMark, a setext-heading
    /// underline cannot come from a lazy line, but our sub-Lex would
    /// otherwise see the underline at block-start position and wrap the
    /// preceding paragraph in a Heading.
    suppress_setext: bool,
    /// Tokens emitted out-of-band by parsers that need to surface multiple
    /// tokens from a single `next_token` call. Drained before regular
    /// dispatch. Currently used by `parse_link`'s fallback paths to emit
    /// `Text("[")` + the inner already-parsed content tokens without
    /// rewinding (which would exponentially re-parse deeply-nested
    /// brackets like `[[[…]]]`).
    pending: std::collections::VecDeque<Token>,
}

impl Lexer {
    /// Creates a new lexer instance from input string. A leading BOM
    /// (U+FEFF) is stripped so it doesn't interfere with block-start
    /// detection on the first line. CRLF and bare CR line endings are
    /// normalized to LF up-front so the rest of the lexer only
    /// needs to reason about `\n`.
    pub fn new(input: String) -> Self {
        // Strip a single leading BOM. Without this, the BOM character
        // sits at position 0 and `is_at_line_start` reports false for
        // any non-BOM block marker that follows, so a doc starting with
        // `\u{FEFF}# Heading` would never dispatch the heading.
        let input = if let Some(stripped) = input.strip_prefix('\u{FEFF}') {
            stripped.to_string()
        } else {
            input
        };
        let normalized: String = input.replace("\r\n", "\n").replace('\r', "\n");
        Lexer {
            input: normalized.chars().collect(),
            position: 0,
            pending_hard_break: false,
            in_heading: false,
            last_emitted_list_item: false,
            last_emitted_was_paragraph_text: false,
            definitions: HashMap::new(),
            suppress_setext: false,
            pending: std::collections::VecDeque::new(),
        }
    }

    /// Parses the entire input string into a sequence of tokens.
    /// Returns a Result containing either a Vec of parsed tokens or a LexerError.
    pub fn parse(&mut self) -> Result<Vec<Token>, LexerError> {
        // Pre-pass: collect reference-link definitions and strip those lines
        // so the main lexer doesn't see them as paragraph text.
        self.extract_definitions();
        let mut tokens = self.parse_with_context(ParseContext::Root)?;
        propagate_loose_tight(&mut tokens);
        Ok(tokens)
    }

    /// Pre-pass: scans the input line-by-line for `[label]: url "title"`
    /// definitions, removes those lines from `self.input`, and stores the
    /// result in `self.definitions` for later resolution by `parse_link` /
    /// `parse_image`. Idempotent: safe to call multiple times.
    fn extract_definitions(&mut self) {
        let chars = self.input.clone();
        let mut definitions = HashMap::new();
        let mut kept: Vec<char> = Vec::with_capacity(chars.len());
        let mut i = 0usize;
        // A link-reference definition cannot interrupt a paragraph. It is
        // only valid at a position where a new block can begin — meaning
        // the previous line was blank, was a single-line block (ATX
        // heading, thematic break), was itself a definition, or we're at
        // BOF.
        let mut may_start_def = true;
        let mut line_start = 0usize;
        while i < chars.len() {
            let at_line_start = i == 0 || chars[i - 1] == '\n';
            if at_line_start {
                line_start = i;
                if may_start_def {
                    if let Some((label, url, title, end)) = try_parse_definition(&chars, i) {
                        definitions
                            .entry(normalize_label(&label))
                            .or_insert((url, title));
                        i = end;
                        may_start_def = true;
                        continue;
                    }
                    // Definitions nested inside a blockquote register
                    // globally too. Peel `>` markers (with their optional
                    // single-space separator) and retry, then keep the
                    // markers in place so the blockquote still parses as
                    // an empty container. Limited to single-line defs to
                    // avoid the multi-line-with-`>`-prefix complexity.
                    let mut peel = i;
                    let mut leading = 0usize;
                    while peel < chars.len() && chars[peel] == ' ' && leading < 3 {
                        peel += 1;
                        leading += 1;
                    }
                    let prefix_start = peel;
                    let mut any_marker = false;
                    while peel < chars.len() && chars[peel] == '>' {
                        any_marker = true;
                        peel += 1;
                        if peel < chars.len()
                            && (chars[peel] == ' ' || chars[peel] == '\t')
                        {
                            peel += 1;
                        }
                    }
                    if any_marker {
                        let line_end = (peel..chars.len())
                            .find(|&j| chars[j] == '\n')
                            .unwrap_or(chars.len());
                        if let Some((label, url, title, def_end)) =
                            try_parse_definition(&chars, peel)
                        {
                            let single_line = def_end <= line_end
                                || (def_end == line_end + 1
                                    && line_end < chars.len());
                            if single_line {
                                definitions
                                    .entry(normalize_label(&label))
                                    .or_insert((url, title));
                                for c in &chars[i..prefix_start] {
                                    kept.push(*c);
                                }
                                for c in &chars[prefix_start..peel] {
                                    if *c == '>' {
                                        kept.push('>');
                                    }
                                }
                                i = if line_end < chars.len() {
                                    kept.push('\n');
                                    line_end + 1
                                } else {
                                    line_end
                                };
                                may_start_def = true;
                                continue;
                            }
                        }
                    }
                }
            }
            if chars[i] == '\n' {
                may_start_def =
                    is_paragraph_breaking_line_chars(&chars, line_start, i);
            }
            kept.push(chars[i]);
            i += 1;
        }
        self.input = kept;
        self.position = 0;
        self.definitions = definitions;
    }

    /// Parses the entire input string into a sequence of tokens for a given context.
    /// Returns a Result containing either a Vec of parsed tokens or a LexerError.
    /// Takes in a `ParseContext` that determines which tokens are valid in the current location.
    pub fn parse_with_context(&mut self, ctx: ParseContext) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        while self.position < self.input.len() || !self.pending.is_empty() {
            if let Some(token) = self.next_token(ctx)? {
                // Track whether the most recent top-level emission was a
                // ListItem (Newline doesn't reset it) and whether it was
                // inline paragraph text (a blank line resets both).
                let mut newlines_in_a_row = 0;
                match &token {
                    Token::ListItem { .. } => {
                        self.last_emitted_list_item = true;
                        self.last_emitted_was_paragraph_text = false;
                    }
                    Token::Newline => {
                        // Count consecutive Newlines so a blank line (≥2)
                        // can reset paragraph state.
                        let mut n = tokens.len();
                        while n > 0 && matches!(tokens[n - 1], Token::Newline) {
                            newlines_in_a_row += 1;
                            n -= 1;
                        }
                        if newlines_in_a_row >= 1 {
                            // The Newline we're about to push makes ≥2.
                            self.last_emitted_was_paragraph_text = false;
                        }
                    }
                    Token::Heading(_, _)
                    | Token::HorizontalRule
                    | Token::BlockQuote(_)
                    | Token::Table { .. }
                    | Token::HtmlComment(_) => {
                        self.last_emitted_list_item = false;
                        self.last_emitted_was_paragraph_text = false;
                    }
                    Token::Code { block: true, .. } => {
                        self.last_emitted_list_item = false;
                        self.last_emitted_was_paragraph_text = false;
                    }
                    _ => {
                        self.last_emitted_list_item = false;
                        self.last_emitted_was_paragraph_text = true;
                    }
                }
                tokens.push(token);
            }
        }

        resolve_emphasis(&mut tokens);
        Ok(tokens)
    }

    /// Helper function to parse nested content until a delimiter is encountered.
    /// Used for parsing content within emphasis, headings, and list items.
    fn parse_nested_content<F>(
        &mut self,
        is_delimiter: F,
        ctx: ParseContext,
    ) -> Result<Vec<Token>, LexerError>
    where
        F: Fn(char) -> bool,
    {
        let mut content = Vec::new();
        let initial_indent = self.get_current_indent();

        loop {
            // Drain any pending queued tokens before reading the next char.
            while let Some(tok) = self.pending.pop_front() {
                content.push(tok);
            }
            if self.position >= self.input.len() {
                break;
            }
            let ch = self.current_char();

            // Inline runs (emphasis, strikethrough) cannot span paragraph
            // boundaries. A blank line forces parse_emphasis /
            // parse_strikethrough into their literal-text fallback so the
            // opener doesn't gobble subsequent paragraphs / headings.
            if ch == '\n' && self.input.get(self.position + 1) == Some(&'\n') {
                break;
            }

            if is_delimiter(ch) {
                break;
            }

            // Handle nested content
            if self.is_at_line_start() {
                let current_indent = self.get_current_indent();

                // If more indented than parent, treat as nested content
                if current_indent > initial_indent
                    && !matches!(ctx, ParseContext::Inline | ParseContext::TableCell)
                {
                    self.position += current_indent;

                    match self.current_char() {
                        '-' | '+' => {
                            if !self.check_horizontal_rule()? {
                                content.push(self.parse_list_item(false, ctx)?);
                                continue;
                            }
                        }
                        '*' => {
                            if self.is_list_marker('*') {
                                content.push(self.parse_list_item(false, ctx)?);
                                continue;
                            }
                        }
                        '0'..='9' => {
                            if self.check_ordered_list_marker().is_some() {
                                content.push(self.parse_list_item(true, ctx)?);
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Parse regular content
            if let Some(token) = self.next_token(ctx)? {
                content.push(token);
            }
        }

        // Resolve emphasis here only for non-link contexts (heading content,
        // strikethrough body, etc.). Link/Image content uses Inline context
        // and must keep its DelimRun tokens intact through the link's
        // fallback decision — otherwise a failed link silently flattens its
        // emphasis delimiters and outer emphasis can't reach across them.
        if !matches!(ctx, ParseContext::Inline) {
            resolve_emphasis(&mut content);
        }
        Ok(content)
    }

    /// Determines the next token in the input stream based on the current character
    /// and context. Handles special cases like line starts differently.
    fn next_token(&mut self, ctx: ParseContext) -> Result<Option<Token>, LexerError> {
        // Drain queued tokens before regular dispatch. Parsers push here
        // when they need to emit multiple tokens from a single position
        // (e.g. parse_link's no-`]` fallback emits `Text("[")` + already-
        // parsed inner content). Otherwise the rewind alternative
        // exponentially re-parses deeply-nested brackets.
        if let Some(tok) = self.pending.pop_front() {
            return Ok(Some(tok));
        }
        // A pending hard break overrides the usual dispatch — emit it before
        // looking at the next character.
        if self.pending_hard_break {
            self.pending_hard_break = false;
            return Ok(Some(Token::HardBreak));
        }

        // An indented (4-column) code block. Triggers at line start in Root
        // or BlockQuote context AND only when the previous line is blank or
        // we're at start-of-document, so list-item
        // continuations and post-paragraph-without-blank lines aren't
        // mis-routed to code.
        if matches!(ctx, ParseContext::Root | ParseContext::BlockQuote)
            && self.is_at_line_start()
            && self.get_current_indent() >= 4
            && self.can_start_indented_code()
        {
            return Ok(Some(self.parse_indented_code_block()));
        }

        // Only skip whitespace if we're not immediately after a special token
        if !self.is_after_special_token() {
            self.skip_whitespace();
        }

        if self.position >= self.input.len() {
            return Ok(None);
        }

        let current_char = self.current_char();
        let is_line_start = self.is_at_line_start();
        // Block markers accept up to 3 columns of leading space indent. We
        // call `skip_whitespace` above, so by the time we land here the
        // position has already moved past those spaces; `is_block_marker_start`
        // walks backward to recover the "would-have-been at line start" check.
        let is_block_start = self.is_block_marker_start();

        // Helper closures to check whether a certain token is allowed in this context.
        let allow_block_tokens = |context: ParseContext| -> bool {
            // Block tokens are allowed in Root, ListItem, BlockQuote.
            matches!(
                context,
                ParseContext::Root | ParseContext::ListItem | ParseContext::BlockQuote
            )
        };

        // CommonMark setext heading: paragraph line followed by `===` / `---`.
        // Must run before the regular dispatch so that `Title\n---` becomes an
        // H2 instead of being consumed as Text + HorizontalRule. Allowed in
        // Root and BlockQuote contexts (a blockquote sub-lexer also needs
        // setext detection for `> Title\n> ---`). Skip detection when the
        // CURRENT line is itself a thematic break — `***\n---\n___` is three
        // HRs, not a setext-H2 with `***` content.
        if is_block_start
            && matches!(ctx, ParseContext::Root | ParseContext::BlockQuote)
            && !(matches!(current_char, '*' | '_' | '-')
                && self.is_thematic_break_line())
        {
            if let Some(level) = self.peek_setext_level() {
                return Ok(Some(self.consume_setext_heading(level)?));
            }
        }

        let token = match current_char {
            '#' if is_block_start && allow_block_tokens(ctx) && self.is_atx_heading_start() => {
                self.parse_heading()?
            }
            '*' if is_block_start && allow_block_tokens(ctx) && self.is_thematic_break_line() => {
                self.consume_current_line();
                Token::HorizontalRule
            }
            '_' if is_block_start && allow_block_tokens(ctx) && self.is_thematic_break_line() => {
                self.consume_current_line();
                Token::HorizontalRule
            }
            '*' if is_block_start && allow_block_tokens(ctx) && self.is_list_marker('*') => {
                self.parse_list_item(false, ctx)?
            }
            '*' => self.parse_emphasis()?,
            '_' if !self.is_intra_word_underscore_run(self.position) => {
                self.parse_emphasis()?
            }
            '_' => self.parse_text(ctx)?,
            '`' => self.parse_code()?,
            '~' if is_block_start
                && allow_block_tokens(ctx)
                && self.count_consecutive('~') >= 3 =>
            {
                self.parse_tilde_fence()?
            }
            '~' if self.count_consecutive('~') >= 2 => self.parse_strikethrough()?,
            '~' => self.parse_text(ctx)?,
            '>' if is_block_start && allow_block_tokens(ctx) => self.parse_blockquote()?,
            '-' | '+' if is_block_start && allow_block_tokens(ctx) => {
                if self.is_thematic_break_line() {
                    self.consume_current_line();
                    Token::HorizontalRule
                } else if self.check_horizontal_rule()? {
                    Token::HorizontalRule
                } else if self.is_list_marker(current_char) {
                    self.parse_list_item(false, ctx)?
                } else {
                    self.parse_text(ctx)?
                }
            }
            '0'..='9' if is_block_start && allow_block_tokens(ctx) => {
                if let Some(n) = self.check_ordered_list_marker() {
                    // An ordered marker with start != 1 cannot interrupt
                    // an open paragraph. Only `1.` / `1)` is allowed in
                    // that position; everything else falls through to text.
                    let can_open = n == 1
                        || self.last_emitted_list_item
                        || self.previous_line_is_blank_or_bof();
                    if can_open {
                        self.parse_list_item(true, ctx)?
                    } else {
                        self.parse_text(ctx)?
                    }
                } else {
                    self.parse_text(ctx)?
                }
            }
            '[' => self.parse_link()?,
            '!' => {
                // Check if this is a valid image start (! followed by [)
                if self.position + 1 < self.input.len() && self.input[self.position + 1] == '[' {
                    self.parse_image()?
                } else {
                    self.parse_text(ctx)?
                }
            }
            '<' if is_block_start && allow_block_tokens(ctx) => {
                // HTML block detection runs first at line start; falls
                // through to inline-comment / autolink / inline-tag /
                // text if no block construct fits.
                if let Some(block) = self.try_parse_html_block() {
                    block
                } else if self.is_html_comment_start() {
                    self.parse_html_comment()?
                } else if let Some(autolink) = self.try_parse_autolink() {
                    autolink
                } else if let Some(len) = self.try_match_html_tag_len() {
                    let html: String = self.input[self.position..self.position + len]
                        .iter()
                        .collect();
                    self.position += len;
                    Token::HtmlInline(html)
                } else if let Some(len) = self.try_match_inline_raw_html_special() {
                    let html: String = self.input[self.position..self.position + len]
                        .iter()
                        .collect();
                    self.position += len;
                    Token::HtmlInline(html)
                } else {
                    self.parse_text(ctx)?
                }
            }
            '<' if self.is_html_comment_start() => self.parse_html_comment()?,
            '<' => {
                if let Some(autolink) = self.try_parse_autolink() {
                    autolink
                } else if let Some(len) = self.try_match_html_tag_len() {
                    let html: String = self.input[self.position..self.position + len]
                        .iter()
                        .collect();
                    self.position += len;
                    Token::HtmlInline(html)
                } else if let Some(len) = self.try_match_inline_raw_html_special() {
                    let html: String = self.input[self.position..self.position + len]
                        .iter()
                        .collect();
                    self.position += len;
                    Token::HtmlInline(html)
                } else {
                    self.parse_text(ctx)?
                }
            }
            '\n' => self.parse_newline()?,
            '|' if is_line_start => {
                if self.is_table_start() {
                    self.parse_table()?
                } else {
                    self.parse_text(ctx)?
                }
            }
            _ => self.parse_text(ctx)?,
        };

        Ok(Some(token))
    }

    /// An ATX heading opener must be 1-6 `#` chars
    /// followed by a space, tab, end-of-line, or end-of-input. This guard
    /// runs before `parse_heading` so `#hello` (no space) and `####### too`
    /// (more than 6 `#`s) fall through to paragraph text.
    fn is_atx_heading_start(&self) -> bool {
        if self.current_char() != '#' {
            return false;
        }
        let mut p = self.position;
        let mut count = 0usize;
        while p < self.input.len() && self.input[p] == '#' {
            count += 1;
            p += 1;
        }
        if !(1..=6).contains(&count) {
            return false;
        }
        match self.input.get(p) {
            None => true,
            Some(&c) => c == ' ' || c == '\t' || c == '\n',
        }
    }

    /// Parses a heading token. Counts up to 6 `#` chars (caller has already
    /// validated it's a real ATX heading via `is_atx_heading_start`), then
    /// collects nested inline content. an optional
    /// closing run of `#`s preceded by a space and followed only by spaces
    /// is stripped from the heading content.
    fn parse_heading(&mut self) -> Result<Token, LexerError> {
        let mut level = 0usize;
        while self.current_char() == '#' && level < 6 {
            level += 1;
            self.advance();
        }
        self.skip_whitespace();
        // Read the rest of the line raw, strip optional trailing closing
        // `#` sequence (only when the run is unescaped and preceded by
        // whitespace or comprises the whole line), then sub-lex the
        // remainder as inline content.
        let line_start = self.position;
        while self.position < self.input.len() && self.current_char() != '\n' {
            self.advance();
        }
        let raw_line: String = self.input[line_start..self.position].iter().collect();
        let stripped = strip_atx_trailing_hashes(&raw_line);
        let mut sub = Lexer::new(stripped);
        sub.in_heading = true;
        sub.definitions = self.definitions.clone();
        let content = sub.parse_with_context(ParseContext::Inline)?;
        Ok(Token::Heading(content, level))
    }

    /// Emits a `*`/`_` delimiter run as a `Token::DelimRun`. The matching
    /// itself happens after the inline scan, in `resolve_emphasis` —
    /// CommonMark's emphasis rules (stack-based matching, Rule 9/10 mod-3,
    /// the `*** → <em><strong>` double-wrap) can't be implemented with
    /// greedy scanning. A `DelimRun` is distinct from a literal `*` or
    /// `_` produced by `\*` escape decoding, which stays as `Text`.
    fn parse_emphasis(&mut self) -> Result<Token, LexerError> {
        let delimiter = self.current_char();
        let mut count = 0;
        while self.position < self.input.len() && self.current_char() == delimiter {
            count += 1;
            self.advance();
        }
        Ok(Token::DelimRun { ch: delimiter, count })
    }

    /// Parses code blocks, handling both inline code and fenced code blocks
    fn parse_code(&mut self) -> Result<Token, LexerError> {
        let opener_pos = self.position;
        // Fence openers accept up to 3 columns of leading-space indent.
        let is_block = self.is_block_marker_start();
        let opener_indent_cols = if is_block {
            // Walk back from opener_pos to line start, count cols.
            let mut p = opener_pos;
            while p > 0 && self.input[p - 1] != '\n' {
                p -= 1;
            }
            let mut col = 0usize;
            for &c in &self.input[p..opener_pos] {
                match c {
                    ' ' => col += 1,
                    '\t' => col += 4 - (col % 4),
                    _ => col += 1,
                }
            }
            col
        } else {
            0
        };
        let start_backticks = self.count_backticks();

        let is_fence = start_backticks >= 3
            && is_block
            && self.no_backticks_on_rest_of_line(opener_pos, start_backticks);

        if !is_fence {
            return Ok(self.parse_inline_code_span_body(start_backticks));
        }

        // Fenced code block. Info string spans to end of line; the
        // *language* is the first whitespace-delimited word, with any
        // remaining metadata discarded (we have no consumer for it).
        self.skip_whitespace();
        let info_string = self.read_until_newline();
        let language = decode_escapes_and_entities(
            info_string.split_whitespace().next().unwrap_or(""),
        );
        // Consume the opener line's newline.
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        let mut content_lines: Vec<String> = Vec::new();
        loop {
            if self.position >= self.input.len() {
                break;
            }
            let line_start = self.position;
            // Measure leading whitespace cols on this line.
            let mut col = 0usize;
            let mut q = line_start;
            while q < self.input.len()
                && (self.input[q] == ' ' || self.input[q] == '\t')
                && col < 4
            {
                if self.input[q] == ' ' {
                    col += 1;
                } else {
                    col += 4 - (col % 4);
                }
                q += 1;
            }
            // Check for a closing fence: ≤3 cols of indent, then ≥start_backticks
            // backticks, then optional whitespace, then end-of-line.
            if col < 4 {
                let mut close_count = 0usize;
                let mut r = q;
                while r < self.input.len() && self.input[r] == '`' {
                    close_count += 1;
                    r += 1;
                }
                if close_count >= start_backticks {
                    let mut tail = r;
                    while tail < self.input.len()
                        && (self.input[tail] == ' ' || self.input[tail] == '\t')
                    {
                        tail += 1;
                    }
                    if tail >= self.input.len() || self.input[tail] == '\n' {
                        // Leave the closer line's `\n` for the outer
                        // dispatcher: emitting it as a separate Newline
                        // means a following blank line surfaces as two
                        // consecutive Newlines, which is what
                        // `propagate_loose_tight` keys on to mark
                        // multi-block list items as loose.
                        self.position = tail;
                        let body = content_lines.join("\n");
                        return Ok(Token::Code {
                            language,
                            content: body,
                            block: true,
                        });
                    }
                }
            }
            // Not a closer — accumulate this line as content, stripping up
            // to opener_indent_cols leading cols (with partial-tab → spaces).
            let mut p = line_start;
            while p < self.input.len() && self.input[p] != '\n' {
                p += 1;
            }
            content_lines.push(strip_leading_cols(
                &self.input,
                line_start,
                p,
                opener_indent_cols,
            ));
            if p < self.input.len() {
                self.position = p + 1;
            } else {
                self.position = p;
            }
        }
        // EOF without closer — emit what we have.
        let body = content_lines.join("\n");
        Ok(Token::Code {
            language,
            content: body,
            block: true,
        })
    }

    /// Walks from `opener_pos + count` to end of line and returns true only
    /// if *no* backtick character is present. A backtick fence opener's
    /// info string must be backtick-free, which also rules out an
    /// inline-span closer on the same line.
    fn no_backticks_on_rest_of_line(&self, opener_pos: usize, count: usize) -> bool {
        let mut p = opener_pos + count;
        while p < self.input.len() && self.input[p] != '\n' {
            if self.input[p] == '`' {
                return false;
            }
            p += 1;
        }
        true
    }

    /// Reads an inline code span body. The opener has already been consumed
    /// by `count_backticks`. Closes on the next backtick run of exactly
    /// `opener_count` chars; runs of a different size are content. A single
    /// `\n` is converted to a space. A blank line (`\n\n`)
    /// or EOF before a closer triggers a literal-text fallback so an
    /// unclosed run can't gobble across paragraphs.
    fn parse_inline_code_span_body(&mut self, opener_count: usize) -> Token {
        let body_start = self.position;
        let mut content = String::new();
        while self.position < self.input.len() {
            let ch = self.current_char();
            if ch == '\n' {
                // Blank line ends the search and falls back to literal text.
                if self.input.get(self.position + 1) == Some(&'\n') {
                    self.position = body_start;
                    return Token::Text("`".repeat(opener_count));
                }
                // A line starting a new block (list marker, ATX heading,
                // thematic break, fenced code) terminates the surrounding
                // paragraph, so the code span cannot reach across it.
                let next_line_start = self.position + 1;
                let mut p = next_line_start;
                let mut cols = 0usize;
                while p < self.input.len() && cols < 3 {
                    match self.input[p] {
                        ' ' => {
                            cols += 1;
                            p += 1;
                        }
                        '\t' => {
                            cols += 4 - (cols % 4);
                            p += 1;
                        }
                        _ => break,
                    }
                }
                if p < self.input.len() && self.line_starts_new_block_at(p) {
                    self.position = body_start;
                    return Token::Text("`".repeat(opener_count));
                }
                content.push(' ');
                self.advance();
                continue;
            }
            if ch == '`' {
                let close_count = self.count_consecutive('`');
                if close_count == opener_count {
                    for _ in 0..close_count {
                        self.advance();
                    }
                    return Token::Code {
                        language: String::new(),
                        content: strip_code_span_outer_space(content),
                        block: false,
                    };
                }
                for _ in 0..close_count {
                    content.push('`');
                    self.advance();
                }
                continue;
            }
            content.push(ch);
            self.advance();
        }
        // EOF without finding a closer — fall back to literal text.
        self.position = body_start;
        Token::Text("`".repeat(opener_count))
    }

    /// Returns the number of consecutive `c` chars starting at `self.position`,
    /// without advancing.
    fn count_consecutive(&self, c: char) -> usize {
        let mut count = 0;
        let mut p = self.position;
        while p < self.input.len() && self.input[p] == c {
            count += 1;
            p += 1;
        }
        count
    }

    /// Parses a GFM strikethrough run (`~~text~~`). Falls back to literal
    /// text if the closer isn't found, mirroring the emphasis fallback.
    fn parse_strikethrough(&mut self) -> Result<Token, LexerError> {
        let mut level = 0;
        while self.current_char() == '~' {
            level += 1;
            self.advance();
        }
        let after_opener = self.position;

        // Strikethrough opens with at least 2 tildes; we always close with 2.
        let close_level = 2;
        let content = self.parse_nested_content(|c| c == '~', ParseContext::Inline)?;

        let mut found = 0usize;
        while found < close_level && self.current_char() == '~' {
            self.advance();
            found += 1;
        }

        if found < close_level {
            // Fallback: rewind and emit opener as literal text.
            self.position = after_opener;
            let mut run = "~".repeat(level);
            if self.position < self.input.len() && self.current_char() == ' ' {
                run.push(' ');
                self.advance();
            }
            return Ok(Token::Text(run));
        }

        let mut content = content;
        resolve_emphasis(&mut content);
        Ok(Token::Strikethrough(content))
    }

    /// Parses a `~~~`-fenced code block. Mirrors the backtick fence path but
    /// distinct so the two fences don't accidentally close each other.
    fn parse_tilde_fence(&mut self) -> Result<Token, LexerError> {
        let opener_pos = self.position;
        // Measure opener-line indent in columns (for stripping content).
        let opener_indent_cols = {
            let mut p = opener_pos;
            while p > 0 && self.input[p - 1] != '\n' {
                p -= 1;
            }
            let mut col = 0usize;
            for &c in &self.input[p..opener_pos] {
                match c {
                    ' ' => col += 1,
                    '\t' => col += 4 - (col % 4),
                    _ => col += 1,
                }
            }
            col
        };
        let mut start_tildes = 0;
        while self.current_char() == '~' {
            start_tildes += 1;
            self.advance();
        }
        // Tilde fences: info strings may contain backticks, but
        // the language is still the first whitespace-delimited word.
        self.skip_whitespace();
        let info_string = self.read_until_newline();
        let language = decode_escapes_and_entities(
            info_string.split_whitespace().next().unwrap_or(""),
        );
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        let mut content_lines: Vec<String> = Vec::new();
        loop {
            if self.position >= self.input.len() {
                break;
            }
            let line_start = self.position;
            // Measure leading whitespace cols.
            let mut col = 0usize;
            let mut q = line_start;
            while q < self.input.len()
                && (self.input[q] == ' ' || self.input[q] == '\t')
                && col < 4
            {
                if self.input[q] == ' ' {
                    col += 1;
                } else {
                    col += 4 - (col % 4);
                }
                q += 1;
            }
            // Closing fence check.
            if col < 4 {
                let mut close_count = 0usize;
                let mut r = q;
                while r < self.input.len() && self.input[r] == '~' {
                    close_count += 1;
                    r += 1;
                }
                if close_count >= start_tildes {
                    let mut tail = r;
                    while tail < self.input.len() && self.input[tail] != '\n' {
                        tail += 1;
                    }
                    self.position = tail;
                    if self.position < self.input.len() && self.current_char() == '\n' {
                        self.advance();
                    }
                    return Ok(Token::Code {
                        language,
                        content: content_lines.join("\n"),
                        block: true,
                    });
                }
            }
            // Content line — strip up to opener_indent_cols (partial-tab → spaces).
            let mut p = line_start;
            while p < self.input.len() && self.input[p] != '\n' {
                p += 1;
            }
            content_lines.push(strip_leading_cols(
                &self.input,
                line_start,
                p,
                opener_indent_cols,
            ));
            if p < self.input.len() {
                self.position = p + 1;
            } else {
                self.position = p;
            }
        }
        // EOF without closer.
        Ok(Token::Code {
            language,
            content: content_lines.join("\n"),
            block: true,
        })
    }

    /// Helper method to count consecutive backticks
    fn count_backticks(&mut self) -> usize {
        let mut count = 0;
        while self.position < self.input.len() && self.current_char() == '`' {
            count += 1;
            self.advance();
        }
        count
    }

    /// Parses a blockquote, consuming consecutive `>`-prefixed lines and
    /// recursively lexing the body so inline formatting works. Supports
    /// lazy continuation: a non-`>`-prefixed line that doesn't itself
    /// start a new block construct joins the open paragraph.
    ///
    /// A line terminates the quote when it is blank at top level, or when it
    /// begins a new block construct that interrupts paragraphs (ATX heading,
    /// thematic break, list marker, fenced code). Lazy continuation is only
    /// permitted when the most recent body line was non-blank (a blank `>`
    /// line closes the paragraph, after which a top-level line is no longer
    /// lazy). Nesting works because the sub-lexer reruns this logic.
    ///
    /// Known limitation: lazy continuation does NOT track which kind of block
    /// is open inside the quote. If the inner block is an indented code block
    /// or heading rather than a paragraph, lazy lines will incorrectly extend
    /// it. Fixing that needs full block-state tracking and is deferred.
    fn parse_blockquote(&mut self) -> Result<Token, LexerError> {
        // Caller may have skipped up to 3 leading spaces during dispatch.
        // Rewind to the actual line start so the per-iteration leading-
        // space scan inside the loop sees those spaces; otherwise the
        // `!is_at_line_start()` guard exits before consuming the `>` and
        // the outer dispatch loops forever on the same byte.
        while self.position > 0 && self.input[self.position - 1] != '\n' {
            self.position -= 1;
        }
        let mut body_lines: Vec<String> = Vec::new();
        let mut had_lazy = false;

        loop {
            if self.position >= self.input.len() || !self.is_at_line_start() {
                break;
            }
            let line_start = self.position;
            // Skip up to 3 leading spaces for marker detection — a block
            // marker may have 0-3 spaces of indent.
            let mut peek = line_start;
            let mut leading = 0usize;
            while peek < self.input.len() && self.input[peek] == ' ' && leading < 3 {
                peek += 1;
                leading += 1;
            }
            let is_marked =
                peek < self.input.len() && self.input[peek] == '>';

            if is_marked {
                self.position = peek;
                self.advance(); // '>'
                // Optional single space or tab after `>`. A tab at this
                // position would expand to column 4 (the `>` consumed col 0,
                // tab spans cols 1-3 to reach col 4); the marker consumes one
                // of those columns as its own padding slot and leaves the
                // remaining 3 columns of indent in the body. Inject them as
                // literal spaces on the body line so the sub-lexer's indent
                // counting sees the correct content offset.
                // `>` consumed col 0. After it, an optional single space or
                // tab acts as the marker separator. Track original column so
                // that tabs in the body's leading whitespace expand to spaces
                // with correct column alignment (a tab at original col N
                // spans to the next multiple of 4).
                let mut body_prefix = String::new();
                let mut orig_col: usize = 1;
                if self.position < self.input.len() {
                    match self.current_char() {
                        ' ' => {
                            self.advance();
                            orig_col += 1;
                        }
                        '\t' => {
                            self.advance();
                            let span = 4 - (orig_col % 4);
                            // 1 col is the separator slot; the rest is body indent.
                            for _ in 0..(span - 1) {
                                body_prefix.push(' ');
                            }
                            orig_col += span;
                        }
                        _ => {}
                    }
                }
                while self.position < self.input.len() {
                    match self.current_char() {
                        '\t' => {
                            let span = 4 - (orig_col % 4);
                            for _ in 0..span {
                                body_prefix.push(' ');
                            }
                            orig_col += span;
                            self.advance();
                        }
                        ' ' => {
                            body_prefix.push(' ');
                            orig_col += 1;
                            self.advance();
                        }
                        _ => break,
                    }
                }
                let rest = self.read_until_newline();
                body_lines.push(body_prefix + &rest);
                if self.position < self.input.len() && self.current_char() == '\n' {
                    self.advance();
                }
                continue;
            }

            // Non-`>` line. parse_blockquote is only invoked after we've
            // seen a `>` at the entry point, so on iteration 0 the marker
            // path above must have fired — if body_lines is still empty,
            // something is off and we bail rather than mislabel content.
            if body_lines.is_empty() {
                break;
            }
            // Blank line or EOF closes the quote at top level.
            if peek >= self.input.len() || self.input[peek] == '\n' {
                break;
            }
            // Lazy continuation requires an OPEN paragraph — a blank `>`
            // line closes the paragraph, so we forbid lazy after that.
            // An indented-code-like body line (4+ leading spaces) or a
            // fence opener (`` ```... `` / `~~~...`) is also not a
            // paragraph; lazy lines must not extend it.
            let last_was_paragraph = body_lines
                .last()
                .map(|l| {
                    if l.trim().is_empty() {
                        return false;
                    }
                    let leading: usize =
                        l.chars().take_while(|&c| c == ' ').count();
                    if leading >= 4 {
                        return false;
                    }
                    let chars: Vec<char> = l.chars().collect();
                    let p = leading;
                    if p < chars.len()
                        && (chars[p] == '`' || chars[p] == '~')
                    {
                        let marker = chars[p];
                        let mut cnt = 0;
                        while p + cnt < chars.len() && chars[p + cnt] == marker {
                            cnt += 1;
                        }
                        if cnt >= 3 {
                            return false;
                        }
                    }
                    true
                })
                .unwrap_or(false);
            if !last_was_paragraph {
                break;
            }
            // Block-starters interrupt a paragraph.
            if self.line_starts_new_block_at(peek) {
                break;
            }
            // Accept as lazy continuation: capture the raw line (including
            // its 0-3 leading spaces) and append.
            self.position = line_start;
            let lazy_line = self.read_until_newline();
            body_lines.push(lazy_line);
            had_lazy = true;
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            }
        }

        let body_text = body_lines.join("\n");
        let mut sub = Lexer::new(body_text);
        // Lazy continuation lines can't form a setext underline (CommonMark
        // example 93): if the blockquote pulled in any lazy line, suppress
        // setext detection during the sub-lex so the trailing `===`/`---`
        // stays as paragraph text.
        sub.suppress_setext = had_lazy;
        let body = sub.parse_with_context(ParseContext::BlockQuote)?;
        Ok(Token::BlockQuote(body))
    }

    /// Returns true if the line beginning at `pos` (already past any 0-3
    /// leading spaces) starts a new block-level construct that interrupts
    /// an open paragraph. Covers ATX heading, thematic break, list marker,
    /// and fenced code. Does NOT detect indented-code interruptions, since
    /// they only apply when the open block in the surrounding context is
    /// itself a paragraph.
    fn line_starts_new_block_at(&mut self, pos: usize) -> bool {
        if pos >= self.input.len() {
            return false;
        }
        let c = self.input[pos];
        match c {
            '#' | '-' | '+' | '*' | '_' | '`' | '~' | '0'..='9' => {}
            _ => return false,
        }
        if self.line_starts_with_list_marker(pos) {
            // A list marker terminates an open paragraph — but only if it
            // doesn't conflict with a thematic break (handled below).
            // Bullet markers `-`, `*` can clash with `---`/`***` thematic
            // breaks; the thematic-break check below covers those.
            if !matches!(c, '-' | '*' | '_') {
                return true;
            }
        }
        let savepos = self.position;
        self.position = pos;
        if c == '#' && self.is_atx_heading_start() {
            self.position = savepos;
            return true;
        }
        if (c == '-' || c == '*' || c == '_') && self.is_thematic_break_line() {
            self.position = savepos;
            return true;
        }
        self.position = savepos;
        // After thematic-break check, re-test list marker for `-`/`*`
        // (which we deferred above).
        if matches!(c, '-' | '*') && self.line_starts_with_list_marker(pos) {
            return true;
        }
        // Fenced code: 3+ run of `` ` `` or `~`.
        if c == '`' || c == '~' {
            let mut p = pos;
            while p < self.input.len() && self.input[p] == c {
                p += 1;
            }
            if p - pos >= 3 {
                return true;
            }
        }
        false
    }

    /// Parses a link token, extracting display text and URL. Supports inline
    /// `[text](url "title")`, full reference `[text][label]`, collapsed
    /// `[text][]`, and shortcut `[text]` (the last only when the label
    /// resolves; otherwise emits the brackets literally).
    fn parse_link(&mut self) -> Result<Token, LexerError> {
        let bracket_pos = self.position;
        self.advance(); // skip '['
        let label_text_start = self.position;
        let content = self.parse_nested_content(|c| c == ']', ParseContext::Inline)?;
        let label_text_end = self.position;
        if self.position >= self.input.len() || self.current_char() != ']' {
            // No closing bracket. If the body parsed cleanly into text +
            // newlines, flatten the whole `[…` run into one Text — both
            // approaches give the same rendered output and the flat form
            // is faster. Newlines must round-trip as `\n` so multi-line
            // link-text fragments preserve the line break. If the body
            // produced structured inlines (Code, HtmlInline, Emphasis,
            // sub-Link, etc.), push the already-parsed content tokens to
            // the pending queue and return `Text("[")`. The queue
            // approach replaces an older position-rewind that re-parsed
            // the body and went exponential on inputs like `[[[[…alt](u)`.
            let only_text = content
                .iter()
                .all(|t| matches!(t, Token::Text(_) | Token::Newline));
            if only_text {
                let mut s = String::from("[");
                for t in &content {
                    match t {
                        Token::Text(t) => s.push_str(t),
                        Token::Newline => s.push('\n'),
                        _ => {}
                    }
                }
                return Ok(Token::Text(s));
            }
            self.pending_hard_break = false;
            for t in content {
                self.pending.push_back(t);
            }
            return Ok(Token::Text("[".to_string()));
        }
        // Nested-link prevention: if the parsed inline content already
        // contains a Link, the outer `[…]` must NOT form a link. Push
        // `Text("[")` + already-parsed content + `Text("]")` to the
        // pending queue and advance past the `]`. Any trailing `(url)`
        // or `[label]` becomes literal text via the dispatcher (because
        // the outer wasn't a link, we don't consume them as link suffix).
        if content
            .iter()
            .any(|t| matches!(t, Token::Link { .. }))
        {
            self.advance(); // skip ']'
            for t in content {
                self.pending.push_back(t);
            }
            self.pending.push_back(Token::Text("]".to_string()));
            return Ok(Token::Text("[".to_string()));
        }
        self.advance(); // skip ']'

        // Inline: [text](url "title")
        if self.position < self.input.len() && self.current_char() == '(' {
            let save = self.position;
            self.advance(); // skip '('
            let (url, title) = self.read_link_destination_and_title();
            if self.position < self.input.len() && self.current_char() == ')' {
                self.advance(); // skip ')'
                let mut content = content;
                resolve_emphasis(&mut content);
                return Ok(Token::Link { content, url, title });
            }
            // The `(…)` form didn't close cleanly — rewind to just before
            // `(` and fall through to the reference / shortcut forms below.
            // The `(` becomes literal text, but `[text]` itself may still
            // resolve as a shortcut reference.
            self.position = save;
        }

        // Raw label text from the source for collapsed/shortcut reference
        // lookup. Comparison labels are the formatting-stripped source chars
        // (e.g. `*foo*` in a label normalizes to `*foo*`, not `foo`), so we
        // can't use `collect_all_text(&content)` which folds emphasis away.
        let raw_label_text: String = self.input[label_text_start..label_text_end]
            .iter()
            .collect();

        // Full or collapsed reference: [text][label] or [text][]
        if self.position < self.input.len() && self.current_char() == '[' {
            let second_bracket = self.position;
            self.advance(); // skip [
            let label_str = self.read_until_char_with_escapes(']');
            let saw_closing_bracket = self.position < self.input.len()
                && self.current_char() == ']';
            if saw_closing_bracket {
                self.advance();
            }
            let key = if label_str.trim().is_empty() {
                normalize_label(&raw_label_text)
            } else {
                normalize_label(&label_str)
            };
            if let Some((url, title)) = self.definitions.get(&key).cloned() {
                let mut content = content;
                resolve_emphasis(&mut content);
                return Ok(Token::Link { content, url, title });
            }
            // Lookup failed. For COLLAPSED `[text][]` the empty label is
            // effectively the text and the shortcut form is identical, so
            // there's nothing further to try — emit literally. For FULL
            // `[text][label]` rewind to just before the second `[` so the
            // trailing `[label]` can re-parse as a fresh candidate, and the
            // outer `[text]` becomes literal text. CommonMark example 571
            // shows this even overrides a successful shortcut: `[foo][bar]`
            // with both `foo` and `baz` defined still emits `[foo]` literal.
            //
            // If the body parsed into structured inlines (DelimRun, Code,
            // emphasis-eligible content), rewind to just past the opening
            // `[` so the dispatcher re-emits the inner tokens — flattening
            // them now would prevent outer emphasis like `*foo [bar*` from
            // resolving once the link form is rejected.
            let only_text = content
                .iter()
                .all(|t| matches!(t, Token::Text(_) | Token::Newline));
            if label_str.trim().is_empty() {
                let text_str = Token::collect_all_text(&content);
                let bracket_label = if !saw_closing_bracket {
                    String::new()
                } else {
                    "[]".to_string()
                };
                if only_text {
                    return Ok(Token::Text(format!(
                        "[{}]{}",
                        text_str, bracket_label
                    )));
                }
                self.pending_hard_break = false;
                self.position = bracket_pos + 1;
                return Ok(Token::Text("[".to_string()));
            }
            self.position = second_bracket;
            let text_str = Token::collect_all_text(&content);
            if only_text {
                return Ok(Token::Text(format!("[{}]", text_str)));
            }
            self.pending_hard_break = false;
            self.position = bracket_pos + 1;
            return Ok(Token::Text("[".to_string()));
        }

        // Shortcut: [text] alone — only a link if the label resolves.
        let key = normalize_label(&raw_label_text);
        if let Some((url, title)) = self.definitions.get(&key).cloned() {
            let mut content = content;
            resolve_emphasis(&mut content);
            return Ok(Token::Link { content, url, title });
        }

        // Unresolved — emit `[text]` literally so the brackets aren't lost.
        let only_text = content
            .iter()
            .all(|t| matches!(t, Token::Text(_) | Token::Newline));
        if only_text {
            let text_str = Token::collect_all_text(&content);
            return Ok(Token::Text(format!("[{}]", text_str)));
        }
        self.pending_hard_break = false;
        self.position = bracket_pos + 1;
        Ok(Token::Text("[".to_string()))
    }

    /// Reads a link destination plus an optional CommonMark-style title.
    /// The title may be delimited by `"…"`, `'…'`, or `(…)` and must be
    /// separated from the URL by at least one ASCII whitespace char. Returns
    /// the URL (with any trailing whitespace trimmed) and the title (if any).
    /// On exit, `self.position` is at the closing `)` or end of input.
    /// Reads a plain (non-angle-bracket) link destination: stops at
    /// unmatched `)`, whitespace introducing a title, or `\n`. Handles
    /// backslash escapes, entity decoding, and balanced parens.
    fn read_link_url_plain(&mut self) -> String {
        let mut url = String::new();
        let mut depth: i32 = 0;
        while self.position < self.input.len() {
            let c = self.current_char();
            if c == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
                    url.push(next);
                    self.advance();
                    self.advance();
                    continue;
                }
            }
            if c == '&' {
                if let Some((decoded, consumed)) =
                    try_decode_entity(&self.input, self.position)
                {
                    url.push_str(&decoded);
                    for _ in 0..consumed {
                        self.advance();
                    }
                    continue;
                }
            }
            if c == '\n' {
                break;
            }
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            } else if (c == ' ' || c == '\t') && depth == 0 {
                // Whitespace at depth 0 may introduce a title — peek ahead.
                let mut p = self.position;
                while p < self.input.len()
                    && (self.input[p] == ' ' || self.input[p] == '\t')
                {
                    p += 1;
                }
                if p < self.input.len() {
                    let next = self.input[p];
                    if next == '"' || next == '\'' || next == '(' {
                        break;
                    }
                }
                // Otherwise whitespace ends the URL too — plain URLs may not
                // contain spaces.
                break;
            }
            url.push(c);
            self.advance();
        }
        url.trim_end().to_string()
    }

    fn read_link_destination_and_title(&mut self) -> (String, Option<String>) {
        // Skip leading whitespace before destination (CommonMark allows it).
        while self.position < self.input.len()
            && (self.current_char() == ' ' || self.current_char() == '\t')
        {
            self.advance();
        }

        let url = if self.position < self.input.len() && self.current_char() == '<' {
            // Angle-bracket form: `<destination>`. Reads up to matching `>`.
            // Spaces are allowed inside; `\<` and `\>` allowed via escape.
            // Newlines / unescaped `<` / `>` end the form invalidly — we
            // bail by treating the `<` as literal in that case.
            let save_pos = self.position;
            self.advance(); // past '<'
            let mut s = String::new();
            let mut ok = false;
            while self.position < self.input.len() {
                let c = self.current_char();
                if c == '\\' && self.position + 1 < self.input.len() {
                    let next = self.input[self.position + 1];
                    if is_ascii_punctuation(next) {
                        s.push(next);
                        self.advance();
                        self.advance();
                        continue;
                    }
                }
                if c == '&' {
                    if let Some((decoded, consumed)) =
                        try_decode_entity(&self.input, self.position)
                    {
                        s.push_str(&decoded);
                        for _ in 0..consumed {
                            self.advance();
                        }
                        continue;
                    }
                }
                if c == '>' {
                    self.advance();
                    ok = true;
                    break;
                }
                if c == '<' || c == '\n' {
                    break;
                }
                s.push(c);
                self.advance();
            }
            if ok {
                s
            } else {
                // Angle form started but didn't close cleanly — the whole
                // link destination is invalid. Don't fall back to plain
                // reading (which would silently re-accept the `<` as URL
                // content). Leave position at the failure point so the
                // caller's "ended at `)`" check fails the link.
                let _ = save_pos;
                s
            }
        } else {
            self.read_link_url_plain()
        };

        // Skip whitespace between URL and potential title. A single newline
        // is allowed (multi-line link form `[text](url\n"title")`); two
        // consecutive newlines terminate the link.
        let mut newlines_between = 0usize;
        while self.position < self.input.len() {
            match self.current_char() {
                ' ' | '\t' => self.advance(),
                '\n' => {
                    newlines_between += 1;
                    if newlines_between > 1 {
                        break;
                    }
                    self.advance();
                }
                _ => break,
            }
        }

        let title = if self.position < self.input.len() && newlines_between <= 1 {
            match self.current_char() {
                '"' => Some(self.read_title_delimited('"', '"')),
                '\'' => Some(self.read_title_delimited('\'', '\'')),
                '(' => Some(self.read_title_delimited('(', ')')),
                _ => None,
            }
        } else {
            None
        };

        // Skip whitespace between title and the final `)` of the link.
        // Allow a single newline (multi-line link form).
        let mut trailing_newlines = 0usize;
        while self.position < self.input.len() {
            match self.current_char() {
                ' ' | '\t' => self.advance(),
                '\n' => {
                    trailing_newlines += 1;
                    if trailing_newlines > 1 {
                        break;
                    }
                    self.advance();
                }
                _ => break,
            }
        }

        (url, title)
    }

    /// Reads a quoted/parenthesised title body. Assumes `self.current_char()`
    /// is the opening delimiter; advances past the closing delimiter.
    /// Backslash escapes apply (so `\"` produces a literal `"` in a
    /// double-quoted title) and entity references decode.
    fn read_title_delimited(&mut self, _open: char, close: char) -> String {
        self.advance(); // past opener
        let mut out = String::new();
        while self.position < self.input.len() && self.current_char() != close {
            let ch = self.current_char();
            if ch == '\n' {
                break;
            }
            if ch == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
                    out.push(next);
                    self.advance();
                    self.advance();
                    continue;
                }
            }
            if ch == '&' {
                if let Some((decoded, consumed)) =
                    try_decode_entity(&self.input, self.position)
                {
                    out.push_str(&decoded);
                    for _ in 0..consumed {
                        self.advance();
                    }
                    continue;
                }
            }
            out.push(ch);
            self.advance();
        }
        if self.position < self.input.len() && self.current_char() == close {
            self.advance(); // past closer
        }
        out
    }

    /// Parses an image token, supporting inline, reference, collapsed, and
    /// shortcut forms (mirrors `parse_link`).
    fn parse_image(&mut self) -> Result<Token, LexerError> {
        let start_pos = self.position;
        self.advance();

        if self.position >= self.input.len() || self.current_char() != '[' {
            // Bare `!` not followed by `[` — treat as regular text.
            self.position = start_pos;
            return self.parse_text(ParseContext::Inline);
        }

        self.advance();
        let alt_text_start = self.position;
        // Image alt scan tracks bracket depth so the OUTERMOST `]` (the one
        // matching the image's opening `[`) becomes the alt closer, even
        // when the alt body itself contains nested link `[…](…)` pairs.
        // Unlike a link, an image is allowed to enclose links inside its
        // alt (they get flattened to text in the rendered alt attribute).
        let alt_text_end = {
            let mut depth: i32 = 1;
            let mut p = self.position;
            while p < self.input.len() {
                match self.input[p] {
                    '\\' if p + 1 < self.input.len()
                        && is_ascii_punctuation(self.input[p + 1]) =>
                    {
                        p += 2;
                        continue;
                    }
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                p += 1;
            }
            if depth != 0 {
                let alt = self.parse_nested_content(|c| c == ']', ParseContext::Inline)?;
                let mut s = String::from("![");
                s.push_str(&Token::collect_all_text(&alt));
                return Ok(Token::Text(s));
            }
            p
        };
        let alt_chars: Vec<char> = self.input[alt_text_start..alt_text_end]
            .iter()
            .copied()
            .collect();
        let alt_input: String = alt_chars.iter().collect();
        let mut sub_alt = Lexer::new(alt_input);
        sub_alt.definitions = self.definitions.clone();
        let alt = sub_alt.parse_with_context(ParseContext::Inline)?;
        self.position = alt_text_end;
        self.advance(); // skip ']'

        // Inline: ![alt](url "title")
        if self.position < self.input.len() && self.current_char() == '(' {
            self.advance(); // skip '('
            let (url, title) = self.read_link_destination_and_title();
            if self.position < self.input.len() && self.current_char() == ')' {
                self.advance(); // skip ')'
            }
            let mut alt = alt;
            resolve_emphasis(&mut alt);
            return Ok(Token::Image { alt, url, title });
        }

        let raw_alt_text: String = self.input[alt_text_start..alt_text_end]
            .iter()
            .collect();

        // Reference / collapsed: ![alt][label] or ![alt][]
        if self.position < self.input.len() && self.current_char() == '[' {
            self.advance();
            let label_str = self.read_until_char_with_escapes(']');
            if self.position < self.input.len() && self.current_char() == ']' {
                self.advance();
            }
            let alt_text = Token::collect_all_text(&alt);
            let key = if label_str.trim().is_empty() {
                normalize_label(&raw_alt_text)
            } else {
                normalize_label(&label_str)
            };
            if let Some((url, title)) = self.definitions.get(&key).cloned() {
                let mut alt = alt;
                resolve_emphasis(&mut alt);
                return Ok(Token::Image { alt, url, title });
            }
            let display_label = decode_escapes_and_entities(&label_str);
            let bracket_label = if label_str.is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", display_label)
            };
            return Ok(Token::Text(format!("![{}]{}", alt_text, bracket_label)));
        }

        // Shortcut: ![alt]
        let key = normalize_label(&raw_alt_text);
        if let Some((url, title)) = self.definitions.get(&key).cloned() {
            let mut alt = alt;
            resolve_emphasis(&mut alt);
            return Ok(Token::Image { alt, url, title });
        }

        // Unresolved shortcut — emit literally instead of erroring.
        let alt_text = Token::collect_all_text(&alt);
        Ok(Token::Text(format!("![{}]", alt_text)))
    }

    /// Tries to recognize a raw inline HTML tag (open tag, closing tag,
    /// or self-closing) starting at the current `<`. Returns the matched
    /// length (including angle brackets) on success. Pragmatic subset of
    /// — comments, processing instructions, declarations,
    /// and CDATA sections are handled elsewhere or fall through to text.
    fn try_match_html_tag_len(&self) -> Option<usize> {
        if self.current_char() != '<' {
            return None;
        }
        let chars = &self.input;
        let start = self.position;
        let mut p = start + 1;
        if p >= chars.len() {
            return None;
        }

        // Closing tag: `</name>`.
        let is_closing = chars[p] == '/';
        if is_closing {
            p += 1;
            if p >= chars.len() || !chars[p].is_ascii_alphabetic() {
                return None;
            }
        } else {
            // Open tag: must start with ASCII letter.
            if !chars[p].is_ascii_alphabetic() {
                return None;
            }
        }

        // Tag name: letters/digits/-.
        while p < chars.len()
            && (chars[p].is_ascii_alphanumeric() || chars[p] == '-')
        {
            p += 1;
        }

        if is_closing {
            // Optional whitespace then `>`.
            while p < chars.len() && (chars[p] == ' ' || chars[p] == '\t') {
                p += 1;
            }
            if chars.get(p) == Some(&'>') {
                return Some(p - start + 1);
            }
            return None;
        }

        // Open tag: optional attributes, optional `/`, then `>`.
        loop {
            // Skip whitespace between attributes.
            let ws_start = p;
            while p < chars.len()
                && (chars[p] == ' ' || chars[p] == '\t' || chars[p] == '\n')
            {
                p += 1;
            }
            if p >= chars.len() {
                return None;
            }
            // End of tag.
            if chars[p] == '>' {
                return Some(p - start + 1);
            }
            if chars[p] == '/' {
                p += 1;
                if chars.get(p) == Some(&'>') {
                    return Some(p - start + 1);
                }
                return None;
            }
            // Need at least one whitespace before an attribute (after the
            // tag name or after the previous attribute).
            if p == ws_start {
                return None;
            }
            // Attribute name: letter or `_` or `:`, then alphanum/_/:/-/.
            if !(chars[p].is_ascii_alphabetic() || chars[p] == '_' || chars[p] == ':') {
                return None;
            }
            p += 1;
            while p < chars.len()
                && (chars[p].is_ascii_alphanumeric()
                    || chars[p] == '_'
                    || chars[p] == ':'
                    || chars[p] == '-'
                    || chars[p] == '.')
            {
                p += 1;
            }
            // Optional value: `= "..."` / `'...'` / unquoted.
            let attr_end = p;
            while p < chars.len() && (chars[p] == ' ' || chars[p] == '\t') {
                p += 1;
            }
            if chars.get(p) == Some(&'=') {
                p += 1;
                while p < chars.len() && (chars[p] == ' ' || chars[p] == '\t') {
                    p += 1;
                }
                if p >= chars.len() {
                    return None;
                }
                match chars[p] {
                    '"' => {
                        p += 1;
                        while p < chars.len() && chars[p] != '"' {
                            p += 1;
                        }
                        if chars.get(p) != Some(&'"') {
                            return None;
                        }
                        p += 1;
                    }
                    '\'' => {
                        p += 1;
                        while p < chars.len() && chars[p] != '\'' {
                            p += 1;
                        }
                        if chars.get(p) != Some(&'\'') {
                            return None;
                        }
                        p += 1;
                    }
                    _ => {
                        // Unquoted attribute value per CommonMark §6.6:
                        // a nonempty string of characters excluding
                        // whitespace, `"`, `'`, `=`, `<`, `>`, `` ` ``.
                        // Note `/` IS allowed (e.g. `href=/path`); the
                        // outer attribute loop separately handles `/>`.
                        if "\"'=<>`".contains(chars[p]) {
                            return None;
                        }
                        while p < chars.len()
                            && !chars[p].is_whitespace()
                            && !"\"'=<>`".contains(chars[p])
                        {
                            p += 1;
                        }
                    }
                }
            } else {
                // Value-less attribute; restore p.
                p = attr_end;
            }
        }
    }

    /// Recognizes inline raw HTML that isn't an open / close tag —
    /// processing instructions (`<?…?>`), declarations (`<!LETTER…>`),
    /// and CDATA sections (`<![CDATA[…]]>`). Returns the matched
    /// length on success or `None` if the current position doesn't
    /// open one of these constructs (or the terminator never appears).
    ///
    /// Block-level forms are handled by `try_parse_html_block`; this
    /// method covers the inline (mid-paragraph) case.
    fn try_match_inline_raw_html_special(&self) -> Option<usize> {
        if self.current_char() != '<' {
            return None;
        }
        let pos = self.position;
        let chars = &self.input;
        if pos + 1 >= chars.len() {
            return None;
        }

        // Processing instruction: `<?…?>`.
        if chars[pos + 1] == '?' {
            let mut p = pos + 2;
            while p + 1 < chars.len() {
                if chars[p] == '?' && chars[p + 1] == '>' {
                    return Some(p + 2 - pos);
                }
                p += 1;
            }
            return None;
        }

        // CDATA section: `<![CDATA[…]]>`.
        if pos + 8 < chars.len()
            && chars[pos + 1] == '!'
            && chars[pos + 2] == '['
            && chars[pos + 3] == 'C'
            && chars[pos + 4] == 'D'
            && chars[pos + 5] == 'A'
            && chars[pos + 6] == 'T'
            && chars[pos + 7] == 'A'
            && chars[pos + 8] == '['
        {
            let mut p = pos + 9;
            while p + 2 < chars.len() {
                if chars[p] == ']' && chars[p + 1] == ']' && chars[p + 2] == '>' {
                    return Some(p + 3 - pos);
                }
                p += 1;
            }
            return None;
        }

        // Declaration: `<!LETTER…>`. Distinguished from CDATA by the
        // third character (`[` vs ASCII letter) and from comments by
        // the third character (`-` for comments, letter here).
        if pos + 2 < chars.len()
            && chars[pos + 1] == '!'
            && chars[pos + 2].is_ascii_alphabetic()
        {
            let mut p = pos + 3;
            while p < chars.len() {
                if chars[p] == '>' {
                    return Some(p + 1 - pos);
                }
                p += 1;
            }
            return None;
        }

        None
    }

    /// Cheap predicate used by `is_start_of_special_token`: scans the chars
    /// after `<` looking for a closing `>` on the same line and a viable
    /// autolink shape (URL scheme or `local@domain.tld`).
    fn looks_like_autolink_start(&self) -> bool {
        if self.current_char() != '<' {
            return false;
        }
        let start = self.position + 1;
        let mut p = start;
        while p < self.input.len() {
            let c = self.input[p];
            if c == '>' {
                break;
            }
            if c == '\n' || c == ' ' || c == '\t' || c == '<' {
                return false;
            }
            p += 1;
        }
        if p >= self.input.len() || self.input[p] != '>' {
            return false;
        }
        let body: String = self.input[start..p].iter().collect();
        if body.is_empty() {
            return false;
        }
        // URL scheme prefix?
        let has_scheme = {
            let mut chars = body.chars();
            let first = chars.next();
            matches!(first, Some(c) if c.is_ascii_alphabetic())
                && body.contains(':')
        };
        if has_scheme {
            return true;
        }
        // Email-ish?
        if let Some(at_pos) = body.find('@') {
            let (local, domain) = body.split_at(at_pos);
            let domain = &domain[1..];
            if !local.is_empty() && domain.contains('.') {
                return true;
            }
        }
        false
    }

    /// Tries to parse an autolink (`<https://…>` or `<user@host>`) at the
    /// current `<`. Returns `Some(Token)` if successful, otherwise `None` so
    /// the caller can dispatch to HTML-comment / text fallback.
    fn try_parse_autolink(&mut self) -> Option<Token> {
        if self.current_char() != '<' {
            return None;
        }
        let start = self.position + 1;
        let mut p = start;
        // Body must not contain whitespace, `<`, or `>`.
        while p < self.input.len() {
            let c = self.input[p];
            if c == '>' {
                break;
            }
            if c == '\n' || c == ' ' || c == '\t' || c == '<' {
                return None;
            }
            p += 1;
        }
        if p >= self.input.len() || self.input[p] != '>' {
            return None;
        }
        let body: String = self.input[start..p].iter().collect();
        if body.is_empty() {
            return None;
        }

        // URL autolink: scheme = ALPHA + 1+ of [ALPHA|DIGIT|+|-|.] then `:`.
        let mut chars = body.chars();
        let first = chars.next();
        let is_url_scheme = matches!(first, Some(c) if c.is_ascii_alphabetic())
            && {
                let mut found_colon = false;
                let mut scheme_len = 1;
                for c in chars {
                    if c == ':' {
                        found_colon = true;
                        break;
                    }
                    if c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.' {
                        scheme_len += 1;
                    } else {
                        break;
                    }
                }
                found_colon && scheme_len >= 2
            };

        let is_email = !is_url_scheme && body.contains('@') && {
            let mut parts = body.splitn(2, '@');
            let local = parts.next().unwrap_or("");
            let domain = parts.next().unwrap_or("");
            // Email autolinks restrict the local part to a specific char set
            // (no `\`, no parentheses, etc.) and require the domain to look
            // like one-or-more dot-separated labels of ASCII alphanumerics
            // and hyphens.
            let local_ok = !local.is_empty()
                && local.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || matches!(
                            c,
                            '.' | '!' | '#' | '$' | '%' | '&' | '\'' | '*'
                            | '+' | '/' | '=' | '?' | '^' | '_' | '`' | '{'
                            | '|' | '}' | '~' | '-'
                        )
                });
            let domain_ok = !domain.is_empty()
                && domain.split('.').all(|label| {
                    !label.is_empty()
                        && label.len() <= 63
                        && !label.starts_with('-')
                        && !label.ends_with('-')
                        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                });
            local_ok && domain_ok && domain.contains('.')
        };

        if !is_url_scheme && !is_email {
            return None;
        }

        self.position = p + 1; // skip past '>'

        Some(if is_email {
            Token::Link {
                content: vec![Token::Text(body.clone())],
                url: format!("mailto:{}", body),
                title: None,
            }
        } else {
            Token::Link {
                content: vec![Token::Text(body.clone())],
                url: body,
                title: None,
            }
        })
    }

    /// Parses a newline token
    fn parse_newline(&mut self) -> Result<Token, LexerError> {
        self.advance();
        Ok(Token::Newline)
    }

    /// Parses regular text until a special token start or newline is encountered.
    /// Returns an error if no text could be parsed.
    fn parse_text(&mut self, ctx: ParseContext) -> Result<Token, LexerError> {
        let mut content = String::new();
        let start_pos = self.position;

        // If we're starting with a space after a special token, include it
        if self.position > 0 && self.current_char() == ' ' {
            content.push(' ');
            self.advance();
        }

        let mut last_was_escape = false;
        while self.position < self.input.len() {
            let ch = self.current_char();

            // `\` before any ASCII punctuation char emits the
            // punctuation as literal text (so `\*`, `\#`, `\[` etc. don't open
            // their respective constructs). `\` before a non-punctuation char
            // stays literal.
            if ch == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
                    content.push(next);
                    self.advance();
                    self.advance();
                    last_was_escape = true;
                    continue;
                }
            }

            // HTML entity / numeric character references.
            if ch == '&' {
                if let Some((decoded, consumed)) =
                    try_decode_entity(&self.input, self.position)
                {
                    content.push_str(&decoded);
                    for _ in 0..consumed {
                        self.advance();
                    }
                    last_was_escape = false;
                    continue;
                }
            }

            if ch == '\n' || self.is_start_of_special_token(ctx) {
                break;
            }

            content.push(ch);
            self.advance();
            last_was_escape = false;
        }

        // Hard line break: 2+ trailing spaces or a lone trailing backslash
        // before `\n`. Valid in any inline run except inside a heading
        // (which is a single-line construct per spec). A hard break only
        // forms when actual content follows on the next line — at EOF or
        // before a blank line the trailing `\` is just literal text.
        if self.position < self.input.len()
            && self.current_char() == '\n'
            && !self.in_heading
        {
            let has_follow = self.has_content_after_newline(self.position);
            if content.ends_with("  ") && has_follow {
                while content.ends_with(' ') {
                    content.pop();
                }
                self.advance();
                self.pending_hard_break = true;
            } else if !last_was_escape
                && content.ends_with('\\')
                && has_follow
            {
                content.pop();
                self.advance();
                self.pending_hard_break = true;
            } else {
                // Soft break: strip trailing spaces so they don't render in
                // the paragraph text. Leading spaces on the continuation line
                // are already eaten by `skip_whitespace` before the next
                // parse_text call.
                while content.ends_with(' ') {
                    content.pop();
                }
            }
        }

        if content.is_empty() {
            // Dispatcher routed an unhandled character here (e.g. `]` in the
            // Inline context, where it's a special-token boundary but has no
            // dedicated arm). Consume the char as literal text so the lexer
            // always makes progress and never bubbles a UnknownToken error
            // up from inside a nested parse — that left whole inputs
            // un-rendered when one token couldn't be classified.
            if self.position < self.input.len() {
                let c = self.current_char();
                content.push(c);
                self.advance();
            } else {
                let (line, col) = self.pos_to_line_col(start_pos);
                return Err(LexerError::UnknownToken(format!(
                    "Unexpected character at line {}, column {}",
                    line, col
                )));
            }
        }
        Ok(Token::Text(content))
    }

    /// Parses an HTML comment, extracting the comment content
    fn parse_html_comment(&mut self) -> Result<Token, LexerError> {
        // Assumes current position at '<' and '!--' follows. An unterminated
        // comment (no closing `-->` before EOF) is treated as literal text
        // — bubbling an error up here would abort parsing of any document
        // that happens to contain a partial comment.
        let opener = self.position;
        self.position += 4; // Skip past '<', '!', '-', '-'

        // Short-form comments per CommonMark §6.6:
        //   <!-->   — empty body
        //   <!--->  — body is a single hyphen
        // Both must close immediately at this position; otherwise we
        // fall through to the regular `-->` scan below.
        if self.position < self.input.len() && self.input[self.position] == '>' {
            self.position += 1;
            return Ok(Token::HtmlComment(String::new()));
        }
        if self.position + 1 < self.input.len()
            && self.input[self.position] == '-'
            && self.input[self.position + 1] == '>'
        {
            self.position += 2;
            return Ok(Token::HtmlComment("-".to_string()));
        }

        let start = self.position;
        while self.position + 2 < self.input.len() {
            if self.input[self.position] == '-'
                && self.input[self.position + 1] == '-'
                && self.input[self.position + 2] == '>'
            {
                break;
            }
            self.advance();
        }

        if self.position + 2 < self.input.len() {
            let comment: String = self.input[start..self.position].iter().collect();
            self.position += 3; // Skip past '-', '-', '>'
            Ok(Token::HtmlComment(comment))
        } else {
            let raw: String = self.input[opener..].iter().collect();
            self.position = self.input.len();
            Ok(Token::Text(raw))
        }
    }

    /// Tries to consume an HTML block (CommonMark §4.6) starting at the
    /// current position. Returns `Some(Token::HtmlBlock(content))` on a
    /// successful match — content is the verbatim block text including
    /// the opening line and any trailing newline — or `None` if no HTML
    /// block fits, in which case the caller can fall through to inline
    /// HTML handling (comment / autolink / inline tag / text).
    ///
    /// Caller is responsible for verifying line-start and 0-3-space
    /// indent before calling.
    ///
    /// Recognizes any of the seven CommonMark HTML block kinds by
    /// matching its opener, then consuming the body per kind-specific
    /// termination rules:
    ///
    ///   Raw-content blocks (`<script|pre|style|textarea …>`):
    ///       until a line containing `</script>` / `</pre>` /
    ///       `</style>` / `</textarea>` (case-insensitive) appears.
    ///   HTML block comments (`<!--…-->`): until `-->`.
    ///   Processing instructions (`<?…?>`): until `?>`.
    ///   Declarations (`<!LETTER…>`, e.g. DOCTYPE): until `>`.
    ///   CDATA sections (`<![CDATA[…]]>`): until `]]>`.
    ///   Block-element tags (whitelisted: `<div>`, `<table>`, …):
    ///       until blank line or EOF (NOT YET IMPLEMENTED).
    ///   Standalone tags (any complete tag on its own line):
    ///       until blank line or EOF (NOT YET IMPLEMENTED).
    ///
    /// Openers that share the `<!` prefix (comments, declarations,
    /// CDATA) are distinguished by the third character (`-`, ASCII
    /// letter, or `[`); the checks below sit in this dispatch order
    /// to mirror spec ordering even where patterns are technically
    /// disjoint.
    fn try_parse_html_block(&mut self) -> Option<Token> {
        // We're called from `next_token` at the current `<`. The caller
        // has already verified `is_block_marker_start()` (line start +
        // 0-3 space indent), but `self.position` itself sits at the `<`
        // because `skip_whitespace` ran first. We need to capture the
        // ORIGINAL line start (including the up-to-3 spaces of indent)
        // so the block body preserves that indent verbatim per spec
        // (example 184: `  <div>\n…` keeps the 2-space indent in the
        // emitted HtmlBlock content).
        let block_start = {
            let mut p = self.position;
            while p > 0 && self.input[p - 1] == ' ' {
                p -= 1;
            }
            p
        };

        // Raw-content blocks (`<script>`, `<pre>`, `<style>`,
        // `<textarea>`). Opener is the tag name (case-insensitive)
        // followed by space, tab, `>`, or end-of-line. Body terminates
        // at a line that *contains* any of `</script>` / `</pre>` /
        // `</style>` / `</textarea>` (case-insensitive — per spec the
        // closer "need not match the start tag"). If no closer ever
        // appears, the block runs to EOF. Body is verbatim — no
        // markdown parsing happens inside.
        if self.input[self.position] == '<'
            && self.is_raw_html_block_opener_at(self.position + 1)
        {
            let end = self.scan_to_raw_html_block_close(self.position);
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // HTML block comments: opener `<!--` at line start; body
        // terminates at `-->` (multi-line allowed). Distinct from the
        // declaration / CDATA arms below by the third char (`-` vs
        // ASCII-letter vs `[`).
        if self.position + 3 < self.input.len()
            && self.input[self.position] == '<'
            && self.input[self.position + 1] == '!'
            && self.input[self.position + 2] == '-'
            && self.input[self.position + 3] == '-'
        {
            let end = self.scan_html_block_to_terminator(self.position, "-->")?;
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // Declarations (`<!DOCTYPE`, `<!ELEMENT`, `<!ATTLIST`, …):
        // body terminates at the first `>` on this or a subsequent
        // line. Opener pattern is `<!` followed by an ASCII letter.
        if self.position + 2 < self.input.len()
            && self.input[self.position] == '<'
            && self.input[self.position + 1] == '!'
            && self.input[self.position + 2].is_ascii_alphabetic()
        {
            let end = self.scan_html_block_to_terminator(self.position, ">")?;
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // CDATA sections (`<![CDATA[…]]>`): body terminates at `]]>`
        // (multi-line allowed). Opener is the literal 9-char sequence
        // `<![CDATA[`. Doesn't overlap with the declaration arm because
        // `<!` is followed by `[`, not a letter.
        if self.position + 8 < self.input.len()
            && self.input[self.position] == '<'
            && self.input[self.position + 1] == '!'
            && self.input[self.position + 2] == '['
            && self.input[self.position + 3] == 'C'
            && self.input[self.position + 4] == 'D'
            && self.input[self.position + 5] == 'A'
            && self.input[self.position + 6] == 'T'
            && self.input[self.position + 7] == 'A'
            && self.input[self.position + 8] == '['
        {
            let end = self.scan_html_block_to_terminator(self.position, "]]>")?;
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // Processing instructions (`<?…?>`, e.g. PHP, XML PIs):
        // body terminates at `?>` (multi-line allowed). Opener is the
        // 2-char sequence `<?`.
        if self.position + 1 < self.input.len()
            && self.input[self.position] == '<'
            && self.input[self.position + 1] == '?'
        {
            let end = self.scan_html_block_to_terminator(self.position, "?>")?;
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // Block-element HTML blocks: opener is `<NAME` or `</NAME`
        // where NAME (case-insensitive) is in BLOCK_ELEMENT_TAG_NAMES,
        // followed by space, tab, end-of-line, `>`, or `/>`. The opener
        // does NOT have to be syntactically complete — a line like
        // `<div id="foo"` with no closing `>` is still a valid block
        // start. Body runs to the next blank line or EOF; content is
        // verbatim. This kind CAN interrupt an open paragraph (no
        // previous_line_is_blank_or_bof check).
        if self.input[self.position] == '<'
            && self.is_block_element_opener_at(self.position)
        {
            let mut after_opener_line = self.position;
            while after_opener_line < self.input.len()
                && self.input[after_opener_line] != '\n'
            {
                after_opener_line += 1;
            }
            if after_opener_line < self.input.len() {
                after_opener_line += 1;
            }
            let end = self.scan_to_blank_line(after_opener_line);
            let content: String = self.input[block_start..end].iter().collect();
            self.position = end;
            return Some(Token::HtmlBlock(content));
        }

        // Standalone HTML tag blocks: any complete open or close tag,
        // followed by ONLY spaces or tabs to the end of the line, whose
        // tag name is not in the raw-content list (handled above) and
        // not in the block-element whitelist (handled by the
        // not-yet-implemented block-element arm — once that lands,
        // these two arms split cleanly). Body runs until a blank line
        // or EOF; content is verbatim.
        //
        // Precedence rule: this kind CANNOT interrupt an open paragraph
        // (per CommonMark spec). When the previous line is non-blank,
        // we fall through so the line stays part of the paragraph.
        if self.input[self.position] == '<' && self.previous_line_is_blank_or_bof() {
            if let Some(tag_name) = self.extract_html_tag_name_at(self.position) {
                let name_lower = tag_name.to_ascii_lowercase();
                let is_block_element = BLOCK_ELEMENT_TAG_NAMES
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(&name_lower));
                let is_raw_content = RAW_HTML_BLOCK_TAG_NAMES
                    .iter()
                    .any(|t| t.eq_ignore_ascii_case(&name_lower));
                if !is_block_element && !is_raw_content {
                    if let Some(tag_len) = self.try_match_html_tag_len() {
                        let after_tag = self.position + tag_len;
                        // The complete tag must fit on a single line.
                        // CommonMark's Type 7 start condition is "line
                        // begins with a complete open tag ... followed
                        // by ... the end of the line" — a tag whose
                        // attributes wrap onto a continuation line
                        // (e.g. `<a href="foo\nbar">`) doesn't qualify
                        // and stays as inline HTML inside a paragraph.
                        let tag_spans_newline = self.input[self.position..after_tag]
                            .iter()
                            .any(|c| *c == '\n');
                        if !tag_spans_newline
                            && self.is_only_whitespace_to_eol(after_tag)
                        {
                            // Move past the opener line's `\n` so the
                            // blank-line scan starts on the next line.
                            let mut after_opener_line = after_tag;
                            while after_opener_line < self.input.len()
                                && self.input[after_opener_line] != '\n'
                            {
                                after_opener_line += 1;
                            }
                            if after_opener_line < self.input.len() {
                                after_opener_line += 1;
                            }
                            let end = self.scan_to_blank_line(after_opener_line);
                            let content: String = self.input[block_start..end].iter().collect();
                            self.position = end;
                            return Some(Token::HtmlBlock(content));
                        }
                    }
                }
            }
        }

        None
    }

    /// Scans forward from `start` looking for `terminator` (e.g. `>`
    /// for a declaration, `?>` for a processing instruction, `]]>` for
    /// CDATA, `-->` for a block comment) and returns the byte index
    /// AFTER the terminator + the trailing newline (if any) — caller can
    /// slice `[block_start..returned]` as the verbatim block content
    /// and set `self.position = returned` to move past the block.
    ///
    /// Returns `None` if the terminator is never reached before EOF —
    /// the caller falls through to inline HTML handling so an
    /// unterminated declaration / PI / CDATA / comment doesn't consume
    /// the rest of the document as a block.
    fn scan_html_block_to_terminator(&self, start: usize, terminator: &str) -> Option<usize> {
        let term: Vec<char> = terminator.chars().collect();
        let mut p = start;
        while p + term.len() <= self.input.len() {
            if self.input[p..p + term.len()] == term[..] {
                let after = p + term.len();
                // Consume the rest of the line (terminator may be
                // followed by whitespace + newline, all of which is
                // part of the block per spec).
                let mut tail = after;
                while tail < self.input.len() && self.input[tail] != '\n' {
                    tail += 1;
                }
                if tail < self.input.len() {
                    tail += 1; // include the `\n`
                }
                return Some(tail);
            }
            p += 1;
        }
        None
    }

    /// Returns true if `chars[pos..]` starts with one of the four
    /// raw-content HTML block tag names (`script`, `pre`, `style`,
    /// `textarea`, case-insensitive) followed by a valid opener
    /// delimiter (space, tab, `>`, or end-of-line). Used by
    /// `try_parse_html_block` after the initial `<` is consumed
    /// (caller passes `self.position + 1`).
    fn is_raw_html_block_opener_at(&self, pos: usize) -> bool {
        const TAGS: &[&str] = &["script", "pre", "style", "textarea"];
        for &tag in TAGS {
            let len = tag.chars().count();
            if pos + len > self.input.len() {
                continue;
            }
            // Case-insensitive ASCII match on the tag name.
            let ok = self.input[pos..pos + len]
                .iter()
                .zip(tag.chars())
                .all(|(a, b)| a.eq_ignore_ascii_case(&b));
            if !ok {
                continue;
            }
            // Validate the char after the tag name.
            match self.input.get(pos + len).copied() {
                None | Some(' ') | Some('\t') | Some('\n') | Some('>') => return true,
                _ => continue,
            }
        }
        false
    }

    /// Returns the (lowercase) tag name at `pos` if the position sits
    /// at the start of a complete or partial HTML tag. Handles both
    /// open tags `<name>` and close tags `</name>`. Returns `None` if
    /// the position is not at a recognizable tag opener.
    fn extract_html_tag_name_at(&self, pos: usize) -> Option<String> {
        if pos >= self.input.len() || self.input[pos] != '<' {
            return None;
        }
        let mut p = pos + 1;
        if p < self.input.len() && self.input[p] == '/' {
            p += 1;
        }
        if p >= self.input.len() || !self.input[p].is_ascii_alphabetic() {
            return None;
        }
        let name_start = p;
        while p < self.input.len()
            && (self.input[p].is_ascii_alphanumeric() || self.input[p] == '-')
        {
            p += 1;
        }
        let name: String = self.input[name_start..p].iter().collect();
        Some(name.to_ascii_lowercase())
    }

    /// Returns true if `chars[pos..]` matches the opener of a
    /// block-element HTML block — `<NAME` or `</NAME` where NAME (case-
    /// insensitive) is in BLOCK_ELEMENT_TAG_NAMES, followed by space,
    /// tab, end-of-line, `>`, or `/>`. The opener does NOT need to be
    /// syntactically complete; this check stops right after the tag
    /// name and validates the trailing delimiter.
    fn is_block_element_opener_at(&self, pos: usize) -> bool {
        if pos >= self.input.len() || self.input[pos] != '<' {
            return false;
        }
        let mut p = pos + 1;
        if p < self.input.len() && self.input[p] == '/' {
            p += 1;
        }
        if p >= self.input.len() || !self.input[p].is_ascii_alphabetic() {
            return false;
        }
        let name_start = p;
        while p < self.input.len()
            && (self.input[p].is_ascii_alphanumeric() || self.input[p] == '-')
        {
            p += 1;
        }
        let name: String = self.input[name_start..p].iter().collect();
        let name_lower = name.to_ascii_lowercase();
        if !BLOCK_ELEMENT_TAG_NAMES
            .iter()
            .any(|t| *t == name_lower.as_str())
        {
            return false;
        }
        match self.input.get(p).copied() {
            None | Some(' ') | Some('\t') | Some('\n') | Some('>') => true,
            Some('/') => self.input.get(p + 1).copied() == Some('>'),
            _ => false,
        }
    }

    /// True if every character from `pos` until the next `\n` (or EOF)
    /// is a space or tab. Used by the standalone-tag arm to enforce
    /// the "complete tag followed by ONLY whitespace to end-of-line"
    /// rule.
    fn is_only_whitespace_to_eol(&self, pos: usize) -> bool {
        let mut p = pos;
        while p < self.input.len() && self.input[p] != '\n' {
            if self.input[p] != ' ' && self.input[p] != '\t' {
                return false;
            }
            p += 1;
        }
        true
    }

    /// Scans from `start` (must be at line-start) line by line until a
    /// blank line (only whitespace + newline, or empty line). Returns
    /// the position at the start of that blank line — the blank line
    /// itself is NOT included in the result. If no blank line ever
    /// appears, returns `self.input.len()` so the caller treats the
    /// rest of the document as block content.
    fn scan_to_blank_line(&self, start: usize) -> usize {
        let mut p = start;
        while p < self.input.len() {
            let line_start = p;
            let mut line_end = line_start;
            while line_end < self.input.len() && self.input[line_end] != '\n' {
                line_end += 1;
            }
            let is_blank = self.input[line_start..line_end]
                .iter()
                .all(|c| *c == ' ' || *c == '\t');
            if is_blank {
                return line_start;
            }
            p = if line_end < self.input.len() {
                line_end + 1
            } else {
                line_end
            };
        }
        self.input.len()
    }

    /// Scans line-by-line from `start` looking for any of the four
    /// raw-content closing tags (`</script>`, `</pre>`, `</style>`,
    /// `</textarea>`, case-insensitive) anywhere on a line. Returns the
    /// position just past the newline of the closing line, or
    /// `self.input.len()` if no closer is found — in that case the
    /// block runs to EOF, which the CommonMark spec explicitly allows.
    fn scan_to_raw_html_block_close(&self, start: usize) -> usize {
        const CLOSERS: &[&str] = &["</script>", "</pre>", "</style>", "</textarea>"];
        let mut line_start = start;
        while line_start < self.input.len() {
            let mut line_end = line_start;
            while line_end < self.input.len() && self.input[line_end] != '\n' {
                line_end += 1;
            }
            let line_lower: String = self.input[line_start..line_end]
                .iter()
                .flat_map(|c| c.to_lowercase())
                .collect();
            for &closer in CLOSERS {
                if line_lower.contains(closer) {
                    return if line_end < self.input.len() {
                        line_end + 1
                    } else {
                        line_end
                    };
                }
            }
            line_start = if line_end < self.input.len() {
                line_end + 1
            } else {
                line_end
            };
        }
        self.input.len()
    }

    /// Checks if current position is at the start of a line
    fn is_at_line_start(&self) -> bool {
        self.position == 0 || self.input.get(self.position - 1) == Some(&'\n')
    }

    /// True when the current position sits at line start *modulo* up to 3
    /// leading spaces of indent — i.e., every char between this position and
    /// the previous `\n` (or beginning of input) is a space, and there are at
    /// most 3 of them. Per CommonMark, all block markers (ATX heading,
    /// thematic break, list marker, blockquote, fence) accept up to 3 columns
    /// of leading whitespace. A leading tab disqualifies the line (a tab
    /// expands to 4 columns).
    fn is_block_marker_start(&self) -> bool {
        let mut p = self.position;
        let mut spaces = 0usize;
        while p > 0 {
            match self.input[p - 1] {
                ' ' => {
                    spaces += 1;
                    if spaces > 3 {
                        return false;
                    }
                    p -= 1;
                }
                '\n' => return true,
                _ => return false,
            }
        }
        true
    }

    /// Skips whitespace characters except newlines
    fn skip_whitespace(&mut self) {
        while self.position < self.input.len()
            && self.current_char().is_whitespace()
            && self.current_char() != '\n'
        {
            self.advance();
        }
    }

    /// Advances the position counter by one
    fn advance(&mut self) {
        self.position += 1;
    }

    /// Returns the current character or '\0' if at end of input
    fn current_char(&self) -> char {
        *self.input.get(self.position).unwrap_or(&'\0')
    }

    /// Reads characters until a newline is encountered
    fn read_until_newline(&mut self) -> String {
        let start = self.position;
        while self.position < self.input.len() && self.current_char() != '\n' {
            self.advance();
        }
        self.input[start..self.position].iter().collect()
    }

    /// Reads link/image text or label content, honoring backslash escapes
    /// for ASCII punctuation and entity references. Stops at the closing
    /// delimiter (which is NOT consumed). `\<close>` and `\\` produce
    /// literal chars; `\<punct>` produces the punctuation; `\<other>`
    /// remains a literal backslash followed by the char. `&name;` /
    /// `&#dd;` / `&#xHH;` decode to their character(s); unrecognized `&…`
    /// sequences pass through literally.
    fn read_until_char_with_escapes(&mut self, delimiter: char) -> String {
        // Reads raw chars up to `delimiter`, treating `\<punct>` as a literal
        // two-character sequence (so an escaped `\]` doesn't terminate a
        // `[…]` label scan). Used for reference-link labels, where the
        // CommonMark comparison rule is on the raw source string — NO
        // backslash-escape or entity decoding is applied here.
        let mut out = String::new();
        while self.position < self.input.len() {
            let ch = self.current_char();
            if ch == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
                    out.push('\\');
                    out.push(next);
                    self.advance();
                    self.advance();
                    continue;
                }
            }
            if ch == delimiter {
                break;
            }
            out.push(ch);
            self.advance();
        }
        out
    }

    /// Checks if current position starts an HTML comment
    fn is_html_comment_start(&self) -> bool {
        // Per-character compare avoids the O(n) full-tail allocation
        // that the old `iter().collect::<String>().starts_with("<!--")`
        // shape produced. `parse_text` calls this once per character,
        // so an O(n) probe here would turn the whole parse quadratic.
        let p = self.position;
        p + 3 < self.input.len()
            && self.input[p] == '<'
            && self.input[p + 1] == '!'
            && self.input[p + 2] == '-'
            && self.input[p + 3] == '-'
    }

    /// Checks if current position could start a special token given a context
    fn is_start_of_special_token(&self, ctx: ParseContext) -> bool {
        let ch = self.current_char();
        match ch {
            // `#` is not listed: heading detection is gated on `is_line_start` in
            // `next_token`, and `parse_text` already breaks on '\n'. Treating mid-paragraph
            // `#` as special caused inputs like "C# is great" to fail with UnknownToken.

            // inline-compatible tokens
            '*' | '`' | '[' => true,

            // `]` only acts as a special-token boundary inside the Inline
            // context, where parse_nested_content uses it as the closing
            // delimiter for link text / image alt. In Root and other block
            // contexts a bare `]` is just literal text.
            ']' if matches!(ctx, ParseContext::Inline) => true,

            // `_` only opens emphasis when the run is not intra-word.
            // `phpmyadmin/localized_docs` keeps the underscore as literal text.
            '_' => !self.is_intra_word_underscore_run(self.position),

            // `~~` opens GFM strikethrough; lone `~` is literal text but we
            // still break here so the dispatcher can decide.
            '~' => self.count_consecutive('~') >= 2,

            '!' => {
                if self.position + 1 < self.input.len() {
                    self.input[self.position + 1] == '['
                } else {
                    false
                }
            }

            '<' => {
                if matches!(ctx, ParseContext::Root) && self.is_html_comment_start() {
                    return true;
                }
                // Autolinks (`<scheme:…>`, `<user@host>`) can appear inline.
                if self.looks_like_autolink_start() {
                    return true;
                }
                // Raw HTML tags (`<span>`, `</span>`, `<br/>`).
                if self.try_match_html_tag_len().is_some() {
                    return true;
                }
                // Inline raw-HTML specials: processing instructions
                // `<?…?>`, declarations `<!LETTER…>`, CDATA sections
                // `<![CDATA[…]]>`.
                self.try_match_inline_raw_html_special().is_some()
            }

            _ => false,
        }
    }

    /// Converts a flat character offset into a 1-based (line, column) pair so
    /// error messages point users at a specific spot in the input.
    pub fn pos_to_line_col(&self, pos: usize) -> (usize, usize) {
        let mut line = 1usize;
        let mut col = 1usize;
        let limit = pos.min(self.input.len());
        for ch in &self.input[..limit] {
            if *ch == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Returns true when the `_` run at `pos` is "intra-word" — i.e. flanked on
    /// both sides by alphanumeric characters. CommonMark forbids such runs from
    /// opening or closing emphasis, so things like `foo_bar` and `foo__bar`
    /// stay as literal text.
    fn is_intra_word_underscore_run(&self, pos: usize) -> bool {
        if self.input.get(pos) != Some(&'_') {
            return false;
        }

        let mut start = pos;
        while start > 0 && self.input[start - 1] == '_' {
            start -= 1;
        }

        let mut end = pos;
        while end + 1 < self.input.len() && self.input[end + 1] == '_' {
            end += 1;
        }

        let before = if start == 0 {
            None
        } else {
            self.input.get(start - 1).copied()
        };
        let after = self.input.get(end + 1).copied();

        matches!(
            (before, after),
            (Some(a), Some(b)) if a.is_alphanumeric() && b.is_alphanumeric()
        )
    }

    /// Checks if we're immediately after a special token that should preserve
    /// following spaces. Includes the closing chars of every inline construct:
    /// `` ` `` (code span), `)` (inline link/image), `]` (reference /
    /// shortcut), `>` (autolink / inline HTML), `*` and `_` (emphasis), `~`
    /// (strikethrough). Without these, `next_token`'s leading whitespace
    /// skip eats the space between e.g. `*foo*` and the next word.
    fn is_after_special_token(&self) -> bool {
        if self.position == 0 {
            return false;
        }
        matches!(
            self.input[self.position - 1],
            '`' | ')' | ']' | '>' | '*' | '_' | '~'
        )
    }

    /// True if the byte at `pos` is `\n` AND the next line contains some
    /// non-whitespace character before its terminating newline. Used to gate
    /// hard-line-break formation at end-of-paragraph: a trailing `\` or two
    /// trailing spaces only form a `<br />` when there's an actual following
    /// line of inline content to break to.
    fn has_content_after_newline(&self, pos: usize) -> bool {
        let mut p = pos + 1;
        while p < self.input.len() {
            match self.input[p] {
                '\n' => return false,
                ' ' | '\t' => p += 1,
                _ => return true,
            }
        }
        false
    }

    /// Checks if the current position contains a horizontal rule (---).
    /// Requires ≥3 consecutive hyphens AND only whitespace before the next
    /// `\n` — otherwise inputs like `---a---` would split into HR + text.
    fn check_horizontal_rule(&mut self) -> Result<bool, LexerError> {
        if self.current_char() == '-' {
            let mut count = 1;
            let mut pos = self.position + 1;
            while pos < self.input.len() && self.input[pos] == '-' {
                count += 1;
                pos += 1;
            }
            if count < 3 {
                return Ok(false);
            }
            let mut tail = pos;
            while tail < self.input.len() && self.input[tail] != '\n' {
                if self.input[tail] != ' ' && self.input[tail] != '\t' {
                    return Ok(false);
                }
                tail += 1;
            }
            self.position = pos;
            return Ok(true);
        }
        Ok(false)
    }

    /// A thematic break is a line of 3+ matching markers from `-`/`*`/`_`
    /// (with optional internal/leading whitespace, up to 3 leading spaces).
    /// Caller must already be at line start.
    fn is_thematic_break_line(&self) -> bool {
        let mut p = self.position;
        let mut leading = 0usize;
        while p < self.input.len() && self.input[p] == ' ' && leading < 3 {
            p += 1;
            leading += 1;
        }
        let marker = match self.input.get(p) {
            Some(&c) if c == '-' || c == '*' || c == '_' => c,
            _ => return false,
        };
        let mut count = 0usize;
        while p < self.input.len() && self.input[p] != '\n' {
            let c = self.input[p];
            if c == marker {
                count += 1;
            } else if c == ' ' || c == '\t' {
                // permitted between markers
            } else {
                return false;
            }
            p += 1;
        }
        count >= 3
    }

    /// Advances `self.position` past the current line and the trailing `\n`.
    fn consume_current_line(&mut self) {
        while self.position < self.input.len() && self.current_char() != '\n' {
            self.advance();
        }
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }
    }

    /// If the line following the current line is a setext underline
    /// (`===…` for H1, `---…` for H2, with optional 3-space indent and
    /// trailing whitespace), returns the heading level. The current line
    /// must contain non-whitespace content.
    fn peek_setext_level(&self) -> Option<usize> {
        if self.suppress_setext {
            return None;
        }
        // Setext doesn't apply when the current line is itself the start of
        // another block construct (list item, ATX heading, blockquote,
        // thematic break, fenced code). The most common false-positive in
        // our setting is a list-marker line: `- foo\n---` is a list item +
        // thematic break, not setext H2.
        let scan_start = {
            let mut p = self.position;
            let mut leading = 0usize;
            while p < self.input.len() && self.input[p] == ' ' && leading < 3 {
                p += 1;
                leading += 1;
            }
            p
        };
        if scan_start < self.input.len() {
            let c = self.input[scan_start];
            if c == '-' || c == '+' || c == '*' {
                if let Some(&n) = self.input.get(scan_start + 1) {
                    if n == ' ' || n == '\t' || n == '\n' {
                        return None;
                    }
                } else {
                    return None;
                }
            }
            if c.is_ascii_digit() {
                let mut q = scan_start;
                while q < self.input.len() && self.input[q].is_ascii_digit() {
                    q += 1;
                }
                if q < self.input.len()
                    && (self.input[q] == '.' || self.input[q] == ')')
                {
                    if let Some(&n) = self.input.get(q + 1) {
                        if n == ' ' || n == '\t' || n == '\n' {
                            return None;
                        }
                    }
                }
            }
            if c == '#' {
                // ATX heading line — not a paragraph for setext purposes.
                let savepos = self.position;
                let _ = savepos;
                // Inline check without mutating: walk #s, see trailing.
                let mut q = scan_start;
                let mut hashes = 0usize;
                while q < self.input.len() && self.input[q] == '#' {
                    hashes += 1;
                    q += 1;
                }
                if (1..=6).contains(&hashes) {
                    if q >= self.input.len() {
                        return None;
                    }
                    let n = self.input[q];
                    if n == ' ' || n == '\t' || n == '\n' {
                        return None;
                    }
                }
            }
            if c == '>' {
                return None;
            }
        }

        // Walk one or more paragraph-like lines until we find either a
        // setext underline or a line that's not a valid paragraph
        // continuation. Each line must contain non-whitespace content and
        // not itself open a block construct.
        let mut p = self.position;
        let mut lines_seen = 0usize;
        loop {
            // Scan to end of current line; require non-whitespace content.
            let mut has_content = false;
            while p < self.input.len() && self.input[p] != '\n' {
                if !self.input[p].is_whitespace() {
                    has_content = true;
                }
                p += 1;
            }
            if !has_content {
                return None;
            }
            lines_seen += 1;
            if p >= self.input.len() {
                return None;
            }
            // Past `\n`.
            p += 1;
            // Lookahead leading spaces (up to 3) on the next line.
            let next_line_start = p;
            let mut leading = 0usize;
            while p < self.input.len() && self.input[p] == ' ' && leading < 3 {
                p += 1;
                leading += 1;
            }
            // Is this a setext underline?
            let underline_char = match self.input.get(p) {
                Some(&'=') => Some('='),
                Some(&'-') => Some('-'),
                _ => None,
            };
            if let Some(ch) = underline_char {
                let mut count = 0usize;
                let mut q = p;
                while q < self.input.len() && self.input[q] == ch {
                    count += 1;
                    q += 1;
                }
                if count > 0 {
                    let mut r = q;
                    while r < self.input.len()
                        && (self.input[r] == ' ' || self.input[r] == '\t')
                    {
                        r += 1;
                    }
                    if r >= self.input.len() || self.input[r] == '\n' {
                        return Some(if ch == '=' { 1 } else { 2 });
                    }
                }
            }
            // Not an underline. The next line must look like paragraph
            // continuation — not a list marker, blockquote, ATX heading,
            // thematic break, or fenced code — or we bail.
            if p >= self.input.len() {
                return None;
            }
            let c = self.input[p];
            // List marker
            if matches!(c, '-' | '+' | '*') {
                if let Some(&n) = self.input.get(p + 1) {
                    if n == ' ' || n == '\t' {
                        return None;
                    }
                }
            }
            if c.is_ascii_digit() {
                let mut q = p;
                while q < self.input.len() && self.input[q].is_ascii_digit() {
                    q += 1;
                }
                if q < self.input.len()
                    && (self.input[q] == '.' || self.input[q] == ')')
                {
                    if let Some(&n) = self.input.get(q + 1) {
                        if n == ' ' || n == '\t' {
                            return None;
                        }
                    }
                }
            }
            if c == '>' || c == '#' || c == '`' || c == '~' {
                return None;
            }
            // OK to continue scanning. Reset p to the start of this line so
            // the next loop iteration walks it.
            p = next_line_start;
            // Cap how many lines we'll join — guard against runaway scan on
            // very long inputs.
            if lines_seen > 100 {
                return None;
            }
        }
    }

    /// Consumes a setext heading: the current line is the heading content,
    /// then `\n`, then the underline line. The text is re-lexed as inline.
    fn consume_setext_heading(&mut self, level: usize) -> Result<Token, LexerError> {
        // Setext content can span multiple lines (peek_setext_level already
        // verified the trailing line is a valid underline). Read lines until
        // we find an underline that matches `level`'s char.
        let underline_char = if level == 1 { '=' } else { '-' };
        let mut content_lines: Vec<String> = Vec::new();
        loop {
            let line_start = self.position;
            while self.position < self.input.len() && self.current_char() != '\n' {
                self.advance();
            }
            let line: String =
                self.input[line_start..self.position].iter().collect();
            // Detect underline: 0-3 leading spaces, then a run of
            // `underline_char`, then optional whitespace.
            let trimmed = line.trim_start_matches(' ');
            let after_leading = line.len() - trimmed.len();
            let is_underline = after_leading <= 3
                && !trimmed.is_empty()
                && trimmed.chars().next() == Some(underline_char)
                && trimmed
                    .chars()
                    .take_while(|c| *c == underline_char)
                    .count()
                    > 0
                && trimmed
                    .chars()
                    .skip_while(|c| *c == underline_char)
                    .all(|c| c == ' ' || c == '\t');
            if is_underline {
                // Consume the underline's newline and stop.
                if self.position < self.input.len() && self.current_char() == '\n' {
                    self.advance();
                }
                break;
            }
            content_lines.push(line);
            // Consume the line's newline.
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            } else {
                // EOF without underline — shouldn't happen if peek_setext_level
                // accepted us. Bail out as an empty heading.
                break;
            }
        }
        let joined = content_lines.join("\n");
        let mut sub = Lexer::new(joined.trim().to_string());
        sub.in_heading = true;
        sub.definitions = self.definitions.clone();
        let content = sub.parse_with_context(ParseContext::Inline)?;
        Ok(Token::Heading(content, level))
    }

    /// Checks if current position starts an ordered list marker (e.g.
    /// `1.` or `1)`). both `.` and `)` are valid
    /// ordered-list marker terminators.
    fn check_ordered_list_marker(&mut self) -> Option<usize> {
        let start_pos = self.position;
        let mut pos = start_pos;
        let mut number_str = String::new();

        while pos < self.input.len() && self.input[pos].is_ascii_digit() {
            number_str.push(self.input[pos]);
            pos += 1;
        }

        // CommonMark caps ordered-list markers at 9 digits — `1234567890.` is
        // treated as paragraph text, not an ordered list.
        if number_str.is_empty() || number_str.len() > 9 {
            return None;
        }

        if pos < self.input.len()
            && (self.input[pos] == '.' || self.input[pos] == ')')
        {
            // Marker terminator must be followed by space/tab/end-of-line,
            // OR (for an empty-item sibling within an open list) only end-
            // of-line — never directly by a paragraph word like `1.two`.
            let after = pos + 1;
            let after_ch = self.input.get(after).copied();
            let trailing_ok = match after_ch {
                None => self.last_emitted_list_item || self.previous_line_is_blank_or_bof(),
                Some(' ') | Some('\t') => true,
                Some('\n') | Some('\r') => {
                    self.last_emitted_list_item || self.previous_line_is_blank_or_bof()
                }
                _ => false,
            };
            if !trailing_ok {
                return None;
            }
            if let Ok(number) = number_str.parse::<usize>() {
                return Some(number);
            }
        }

        None
    }

    /// Parses a list item, handling both ordered and unordered types
    fn parse_list_item(
        &mut self,
        ordered: bool,
        parent_ctx: ParseContext,
    ) -> Result<Token, LexerError> {
        // Walk back to start-of-line to compute the marker's actual column
        // (tab-expanded). The dispatcher already skipped leading spaces, so
        // self.position is at the marker.
        let marker_col = {
            let mut p = self.position;
            while p > 0 && self.input[p - 1] != '\n' {
                p -= 1;
            }
            let mut col = 0usize;
            for &c in &self.input[p..self.position] {
                match c {
                    ' ' => col += 1,
                    '\t' => col += 4 - (col % 4),
                    _ => col += 1,
                }
            }
            col
        };

        let mut number = None;
        let marker_char: char;

        if !ordered {
            marker_char = self.current_char();
            self.advance();
        } else {
            number = self.check_ordered_list_marker();
            while self.position < self.input.len() && self.current_char().is_ascii_digit() {
                self.advance();
            }
            marker_char = if self.position < self.input.len()
                && (self.current_char() == '.' || self.current_char() == ')')
            {
                let m = self.current_char();
                self.advance();
                m
            } else {
                '.'
            };
        }

        // Width of the marker we just consumed.
        let marker_width = if ordered {
            let n = number.unwrap_or(1);
            let mut digits = 1usize;
            let mut tmp = n;
            while tmp >= 10 {
                tmp /= 10;
                digits += 1;
            }
            digits + 1 // digits + terminator (`.`/`)`)
        } else {
            1 // `-` / `+` / `*`
        };

        // Count whitespace after marker, before content. If the gap is 5+
        // columns (or 0, meaning content starts on the next line), the rule
        // is to use exactly 1 column of separation — anything beyond is
        // first-line indented-code content.
        let mut probe = self.position;
        let mut spaces_after = 0usize;
        while probe < self.input.len()
            && (self.input[probe] == ' ' || self.input[probe] == '\t')
        {
            if self.input[probe] == ' ' {
                spaces_after += 1;
            } else {
                spaces_after += 4 - (spaces_after % 4);
            }
            probe += 1;
        }
        let following_is_eol = probe >= self.input.len() || self.input[probe] == '\n';
        let separator = if following_is_eol {
            1 // empty item — content_offset still uses 1
        } else if spaces_after >= 1 && spaces_after <= 4 {
            spaces_after
        } else {
            1
        };
        let content_offset = marker_col + marker_width + separator;

        // If the gap between marker and content is 5+ columns AND there's
        // non-blank content following on the same line, the first line is
        // an indented code block. Strip exactly content_offset cols
        // (marker + 1-col separator) and feed the remainder into a
        // sub-lexer as Root-context content so the indented-code path fires.
        let first_line_is_indented_code = spaces_after >= 5 && !following_is_eol;

        if !first_line_is_indented_code {
            self.skip_whitespace();
        }

        // GFM task list: detect `[ ]`, `[x]`, `[X]` immediately after the
        // list marker. Must be followed by a space (or EOL) to count.
        let mut checked: Option<bool> = None;
        if self.position + 2 < self.input.len()
            && self.input[self.position] == '['
            && self.input[self.position + 2] == ']'
            && (self.position + 3 >= self.input.len()
                || self.input[self.position + 3] == ' '
                || self.input[self.position + 3] == '\t'
                || self.input[self.position + 3] == '\n')
        {
            match self.input[self.position + 1] {
                ' ' => {
                    checked = Some(false);
                    self.position += 3;
                    self.skip_whitespace();
                }
                'x' | 'X' => {
                    checked = Some(true);
                    self.position += 3;
                    self.skip_whitespace();
                }
                _ => {}
            }
        }

        let mut content = Vec::new();
        if first_line_is_indented_code {
            // Expand leading tabs to spaces using the ORIGINAL column
            // (self.position currently corresponds to col marker_col +
            // marker_width), strip 1 col for the marker's separator slot,
            // and feed the rest to a sub-lexer. The ≥4 cols of remaining
            // leading whitespace trigger the sub-lexer's indented-code path.
            let line_end = (self.position..self.input.len())
                .find(|&i| self.input[i] == '\n')
                .unwrap_or(self.input.len());
            let mut col = marker_col + marker_width;
            let mut expanded = String::new();
            let mut i = self.position;
            while i < line_end {
                match self.input[i] {
                    '\t' => {
                        let span = 4 - (col % 4);
                        for _ in 0..span {
                            expanded.push(' ');
                        }
                        col += span;
                        i += 1;
                    }
                    ' ' => {
                        expanded.push(' ');
                        col += 1;
                        i += 1;
                    }
                    _ => break,
                }
            }
            while i < line_end {
                expanded.push(self.input[i]);
                i += 1;
            }
            let stripped: String = expanded.chars().skip(separator).collect();
            self.position = line_end;
            let mut sub = Lexer::new(stripped);
            let sub_tokens = sub.parse_with_context(ParseContext::Root)?;
            content.extend(sub_tokens);
        }
        // First-line block constructs that the regular inline dispatcher won't
        // recognize past the list marker (block_marker_start fails because
        // walking back hits the `-`/digit, not `\n`). A thematic break,
        // ATX heading, fenced code opener, or nested list marker can
        // occupy the entire content of a list item — handle them up front
        // before the inline-token loop.
        let mut first_line_handled = first_line_is_indented_code;
        if !first_line_handled
            && self.position < self.input.len()
            && self.current_char() != '\n'
        {
            let ch = self.current_char();
            if self.is_thematic_break_line() {
                self.consume_current_line();
                content.push(Token::HorizontalRule);
                first_line_handled = true;
            } else if ch == '#' && self.is_atx_heading_start() {
                content.push(self.parse_heading()?);
                first_line_handled = true;
            } else if (ch == '`' || ch == '~') && self.count_consecutive(ch) >= 3 {
                // Fenced code starting on the item's first line. Capture
                // the rest of the first line plus subsequent indent-
                // qualifying lines (stripped by content_offset) and feed
                // the result to a sub-lexer so the fence parser sees it
                // exactly as if the lines stood at the document root.
                let line_end = (self.position..self.input.len())
                    .find(|&i| self.input[i] == '\n')
                    .unwrap_or(self.input.len());
                let first_line: String = self.input[self.position..line_end]
                    .iter()
                    .collect();
                self.position = if line_end < self.input.len() {
                    line_end + 1
                } else {
                    line_end
                };
                let rest = self.collect_list_item_block_content(content_offset);
                let full = if rest.is_empty() {
                    first_line
                } else {
                    format!("{}\n{}", first_line, rest)
                };
                let mut sub = Lexer::new(full);
                let sub_tokens = sub.parse_with_context(ParseContext::Root)?;
                content.extend(sub_tokens);
                first_line_handled = true;
            } else if ch == '>' {
                // Blockquote starting on the item's first line. Capture
                // first-line + content-offset-indented continuation, then
                // also pull in any subsequent shallow-indent non-block
                // lines as lazy continuation — those extend the inner
                // blockquote's open paragraph and would otherwise become
                // siblings of the blockquote inside the item.
                let line_end = (self.position..self.input.len())
                    .find(|&i| self.input[i] == '\n')
                    .unwrap_or(self.input.len());
                let first_line: String = self.input[self.position..line_end]
                    .iter()
                    .collect();
                self.position = if line_end < self.input.len() {
                    line_end + 1
                } else {
                    line_end
                };
                let rest = self.collect_list_item_block_content(content_offset);
                let mut full = if rest.is_empty() {
                    first_line
                } else {
                    format!("{}\n{}", first_line, rest)
                };
                loop {
                    if self.position >= self.input.len() {
                        break;
                    }
                    let lz_start = self.position;
                    let lz_end = (lz_start..self.input.len())
                        .find(|&i| self.input[i] == '\n')
                        .unwrap_or(self.input.len());
                    if self.input[lz_start..lz_end]
                        .iter()
                        .all(|&c| c == ' ' || c == '\t')
                    {
                        break;
                    }
                    let mut cols = 0usize;
                    let mut q = lz_start;
                    while q < lz_end
                        && (self.input[q] == ' ' || self.input[q] == '\t')
                    {
                        if self.input[q] == ' ' {
                            cols += 1;
                        } else {
                            cols += 4 - (cols % 4);
                        }
                        q += 1;
                    }
                    if cols >= content_offset {
                        break;
                    }
                    if self.line_starts_new_block_at(q) {
                        break;
                    }
                    if !full.is_empty() {
                        full.push('\n');
                    }
                    for c in &self.input[lz_start..lz_end] {
                        full.push(*c);
                    }
                    self.position = if lz_end < self.input.len() {
                        lz_end + 1
                    } else {
                        lz_end
                    };
                }
                let mut sub = Lexer::new(full);
                let sub_tokens = sub.parse_with_context(ParseContext::Root)?;
                content.extend(sub_tokens);
                first_line_handled = true;
            } else if (ch == '-' || ch == '+') && self.is_list_marker(ch) {
                content.push(self.parse_list_item(false, parent_ctx)?);
                first_line_handled = true;
            } else if ch == '*' && self.is_list_marker('*') {
                content.push(self.parse_list_item(false, parent_ctx)?);
                first_line_handled = true;
            } else if ch.is_ascii_digit() && self.check_ordered_list_marker().is_some() {
                content.push(self.parse_list_item(true, parent_ctx)?);
                first_line_handled = true;
            }
        }
        if !first_line_handled {
            while self.position < self.input.len() && self.current_char() != '\n' {
                if let Some(token) = self.next_token(ParseContext::ListItem)? {
                    content.push(token);
                }
            }
        }

        // Move past the line-terminating newline if there is one.
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        // Setext heading inside the item: if first-line content was a
        // paragraph and the next line is an `=`/`-` underline at the
        // item's content offset, retroactively wrap the first-line tokens
        // in a Heading. Without this the underline becomes a thematic
        // break (or text) and the heading semantics are lost.
        if !first_line_handled
            && !content.is_empty()
            && content
                .iter()
                .all(|t| !matches!(t, Token::HorizontalRule | Token::Heading(_, _)))
        {
            let next_line_start = self.position;
            let mut p = next_line_start;
            let mut indent_cols = 0usize;
            while p < self.input.len() && self.input[p] == ' ' {
                p += 1;
                indent_cols += 1;
            }
            if indent_cols >= content_offset
                && p < self.input.len()
                && (self.input[p] == '=' || self.input[p] == '-')
            {
                let underline_char = self.input[p];
                let underline_start = p;
                while p < self.input.len() && self.input[p] == underline_char {
                    p += 1;
                }
                let run_len = p - underline_start;
                let mut tail = p;
                while tail < self.input.len()
                    && (self.input[tail] == ' ' || self.input[tail] == '\t')
                {
                    tail += 1;
                }
                let ends_line =
                    tail >= self.input.len() || self.input[tail] == '\n';
                if run_len >= 1 && ends_line {
                    let level = if underline_char == '=' { 1 } else { 2 };
                    let inner = std::mem::take(&mut content);
                    content.push(Token::Heading(inner, level));
                    self.position = if tail < self.input.len() {
                        tail + 1
                    } else {
                        tail
                    };
                }
            }
        }

        // Continuation loop: handles both deeper-indented sub-items / nested
        // markers AND lazy paragraph continuation (lines at
        // any indent that don't start a new block belong to this item).
        loop {
            if self.position >= self.input.len() {
                break;
            }
            if !self.is_at_line_start() {
                break;
            }

            let line_start = self.position;
            let cur_indent = self.get_current_indent();
            // `cur_indent` is a COLUMN count (tabs expand). We need the BYTE
            // offset past the leading whitespace to index into self.input
            // correctly — adding cur_indent directly is wrong when tabs are
            // present (one tab byte = up to 4 columns) and was causing the
            // continuation loop to infinite-loop on inputs like `\tbar`.
            let mut after_indent = line_start;
            while after_indent < self.input.len()
                && (self.input[after_indent] == ' ' || self.input[after_indent] == '\t')
            {
                after_indent += 1;
            }

            // Blank line: peek ahead to decide whether this is the end of the
            // item or a paragraph break within it. If the next non-blank line
            // is indented to at least the item's content offset, the item
            // continues with a new paragraph; otherwise it ends here.
            if after_indent >= self.input.len() || self.input[after_indent] == '\n' {
                // An empty list item (no content on the first line) cannot
                // grow lazy continuation across a blank line — the item
                // terminates and any indented content below becomes a
                // separate block.
                let item_has_content = content.iter().any(|t| !matches!(t, Token::Newline));
                if !item_has_content {
                    break;
                }
                let mut p = line_start;
                while p < self.input.len() {
                    let line_end = (p..self.input.len())
                        .find(|&i| self.input[i] == '\n')
                        .unwrap_or(self.input.len());
                    let only_ws = self.input[p..line_end]
                        .iter()
                        .all(|c| *c == ' ' || *c == '\t');
                    if !only_ws {
                        break;
                    }
                    if line_end >= self.input.len() {
                        p = line_end;
                        break;
                    }
                    p = line_end + 1;
                }
                if p >= self.input.len() {
                    break;
                }
                // Measure the next non-blank line's indent in columns.
                let mut next_indent = 0usize;
                let mut q = p;
                while q < self.input.len() {
                    match self.input[q] {
                        ' ' => {
                            next_indent += 1;
                            q += 1;
                        }
                        '\t' => {
                            next_indent += 4 - (next_indent % 4);
                            q += 1;
                        }
                        _ => break,
                    }
                }
                if next_indent < content_offset {
                    break;
                }
                // Continuation belongs to this item. Skip the blank gap and
                // emit Newlines so the loose-detection pass sees the
                // blank-line break.
                self.position = p;
                content.push(Token::Newline);
                content.push(Token::Newline);
                // After a blank gap, the continuation may be a code block,
                // blockquote, fence, or another structure — not just inline
                // paragraph text. Collect the remaining indent-qualifying
                // lines, strip content_offset cols, and sub-lex them as
                // block content. The sub-lexer's tokens get appended to
                // the item's content.
                let raw = self.collect_list_item_block_content(content_offset);
                if !raw.is_empty() {
                    let mut sub = Lexer::new(raw);
                    // Use Root context so indented-code, fenced-code,
                    // blockquote, and nested list parsing fire correctly.
                    // The sub-lexer's output will be a mix of block + inline
                    // tokens; the renderer's split_item_content separates
                    // inline run from nested blocks.
                    let sub_tokens =
                        sub.parse_with_context(ParseContext::Root)?;
                    content.extend(sub_tokens);
                }
                continue;
            }

            // Decide if this line starts a new block (which terminates the
            // item) or is continuation content.
            let is_marker_line = self.line_starts_with_list_marker(after_indent);
            let next_ch = self.input[after_indent];

            if cur_indent >= content_offset {
                // Indent reaches the item's content offset: continuation
                // or nested-marker territory. Below this threshold, even a
                // line whose indent exceeds the item's leading column is a
                // SIBLING (e.g. ` - bar` after a `- foo` opener cannot
                // nest because content_offset is 2 cols but the next
                // marker sits at col 1).
                if is_marker_line {
                    self.position = after_indent;
                    match next_ch {
                        '-' | '+' => {
                            if !self.check_horizontal_rule()? {
                                content.push(self.parse_list_item(false, parent_ctx)?);
                                continue;
                            }
                            // It was a thematic break — stop.
                            self.position = line_start;
                            break;
                        }
                        '*' => {
                            if self.is_list_marker('*') {
                                content.push(self.parse_list_item(false, parent_ctx)?);
                                continue;
                            }
                            self.position = line_start;
                            break;
                        }
                        '0'..='9' => {
                            if self.check_ordered_list_marker().is_some() {
                                content.push(self.parse_list_item(true, parent_ctx)?);
                                continue;
                            }
                            self.position = line_start;
                        }
                        _ => {}
                    }
                }
                // Deep-indent non-marker line that starts a block construct
                // (blockquote, fenced code) belongs inside the item as a
                // block, not as paragraph-continuation text. Sub-lex the
                // content-offset-stripped lines so the inner block parser
                // sees them at the document root and doesn't reach past
                // the item's boundary. Same path applies when the item is
                // still empty: the first deep-indent line is the item's
                // first block, not lazy continuation of nothing.
                let item_has_content =
                    content.iter().any(|t| !matches!(t, Token::Newline));
                let starts_block = next_ch == '>'
                    || ((next_ch == '`' || next_ch == '~') && {
                        let mut p = after_indent;
                        while p < self.input.len() && self.input[p] == next_ch {
                            p += 1;
                        }
                        p - after_indent >= 3
                    })
                    || !item_has_content;
                if !is_marker_line && starts_block {
                    let rest = self.collect_list_item_block_content(content_offset);
                    if !rest.is_empty() {
                        let mut sub = Lexer::new(rest);
                        let sub_tokens =
                            sub.parse_with_context(ParseContext::Root)?;
                        content.extend(sub_tokens);
                    }
                    continue;
                }
                // Fall through to lazy continuation for non-marker deeper
                // content (e.g. an indented paragraph continuation line).
            } else {
                // Indent <= parent. Sibling/outer marker terminates this
                // item — but a marker at column ≥ 4 sits in indented-code
                // territory and can't interrupt the open paragraph. It
                // falls through to lazy continuation as literal text
                // (CommonMark example 312: `   - d\n    - e` keeps `- e`
                // joined to `d`'s paragraph instead of opening a new item).
                if is_marker_line && cur_indent < 4 {
                    break;
                }
                // ATX heading, blockquote, thematic break also terminate.
                if next_ch == '#' {
                    let savepos = self.position;
                    self.position = after_indent;
                    let is_atx = self.is_atx_heading_start();
                    self.position = savepos;
                    if is_atx {
                        break;
                    }
                }
                if next_ch == '>' {
                    break;
                }
                let savepos = self.position;
                self.position = after_indent;
                let is_hr = self.is_thematic_break_line();
                self.position = savepos;
                if is_hr {
                    break;
                }
            }

            // Lazy continuation: append a Newline plus this line's inline
            // tokens to the current item's content. Use Inline context so
            // the dispatcher doesn't fire block-level handlers (which
            // would consume across the next line boundary and break the
            // paragraph-continuation semantics).
            self.position = after_indent;
            content.push(Token::Newline);
            while self.position < self.input.len() && self.current_char() != '\n' {
                if let Some(tok) = self.next_token(ParseContext::Inline)? {
                    content.push(tok);
                }
            }
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            }
        }

        // The first-line inline loop and the lazy-continuation inner loop
        // emit `DelimRun` tokens for `*`/`_` runs that haven't been matched
        // by the emphasis algorithm yet. Sub-Lex paths already resolve
        // their content, but the raw inline-loop output does not — run
        // `resolve_emphasis` here so no internal token escapes the lexer.
        let mut content = content;
        resolve_emphasis(&mut content);
        Ok(Token::ListItem {
            content,
            ordered,
            number,
            marker: marker_char,
            checked,
            loose: false,
        })
    }

    /// Returns true if the chars at `pos` (line start, post-indent) form a
    /// list-marker opener: `-`, `+`, `*` followed by space/tab/EOL, or
    /// digits + `.`/`)` followed by space/tab/EOL.
    fn line_starts_with_list_marker(&self, pos: usize) -> bool {
        if pos >= self.input.len() {
            return false;
        }
        let trailing_ok = |idx: usize| -> bool {
            match self.input.get(idx) {
                None => true,
                Some(&c) => c == ' ' || c == '\t' || c == '\n',
            }
        };
        match self.input[pos] {
            '-' | '+' | '*' => trailing_ok(pos + 1),
            c if c.is_ascii_digit() => {
                let mut p = pos;
                while p < self.input.len() && self.input[p].is_ascii_digit() {
                    p += 1;
                }
                if p >= self.input.len() {
                    return false;
                }
                let term = self.input[p];
                (term == '.' || term == ')') && trailing_ok(p + 1)
            }
            _ => false,
        }
    }

    /// Checks if the current posisiton is the start of a table
    fn is_table_start(&self) -> bool {
        let rest: String = self.input[self.position..].iter().collect();
        // Next line with --- or :---
        if let Some(pos) = rest.find('\n') {
            let next_line = rest[pos + 1..].lines().next().unwrap_or("");
            next_line.contains('-')
        } else {
            false
        }
    }

    /// Parses a table, handling column alignment
    fn parse_table(&mut self) -> Result<Token, LexerError> {
        // Parse header row
        let header_line = self.read_until_newline();
        let header_cells: Vec<String> = header_line
            .trim_matches('|')
            .split('|')
            .map(|s| s.trim().to_string())
            .collect();

        if self.current_char() == '\n' {
            self.advance();
        }

        // Parse alignment row
        let align_line = self.read_until_newline();
        let aligns: Vec<Alignment> = align_line
            .trim_matches('|')
            .split('|')
            .map(|s| {
                let s = s.trim();
                match (s.starts_with(':'), s.ends_with(':')) {
                    (true, true) => Alignment::Center,
                    (true, false) => Alignment::Left,
                    (false, true) => Alignment::Right,
                    _ => Alignment::Left,
                }
            })
            .collect();

        if self.current_char() == '\n' {
            self.advance();
        }

        // Convert header strings to token vectors
        let mut headers = Vec::new();
        for cell in header_cells {
            let mut cell_lexer = Lexer::new(cell);
            let parsed = cell_lexer.parse_with_context(ParseContext::TableCell)?;
            headers.push(parsed);
        }

        // Parse rows until blank or non-table start
        let mut rows = Vec::new();
        while self.position < self.input.len() {
            let line = self.read_until_newline();
            if line.trim().is_empty() {
                break;
            }

            let cell_texts: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|s| s.trim().to_string())
                .collect();

            let mut row_tokens = Vec::new();
            for cell in cell_texts {
                // FIX: large unbreakable words don't fit in cells
                let mut cell_lexer = Lexer::new(cell);
                let parsed = cell_lexer.parse_with_context(ParseContext::TableCell)?;
                row_tokens.push(parsed);
            }
            rows.push(row_tokens);

            if self.current_char() == '\n' {
                self.advance();
            }
        }

        // Per GFM, header column count drives the table shape. Truncate
        // or pad `aligns` to match so downstream consumers (renderers,
        // invariant checks, accessibility tools) can rely on equal-length
        // vectors. Rows are kept as-is — extra cells are dropped by
        // renderers and missing cells render as empty.
        let mut aligns = aligns;
        match aligns.len().cmp(&headers.len()) {
            std::cmp::Ordering::Less => {
                aligns.resize(headers.len(), Alignment::Left);
            }
            std::cmp::Ordering::Greater => {
                aligns.truncate(headers.len());
            }
            std::cmp::Ordering::Equal => {}
        }
        Ok(Token::Table {
            headers,
            aligns,
            rows,
        })
    }

    /// True if `self.position` is at the start of a document, or at the start
    /// of a line whose preceding line contains only whitespace. Used to gate
    /// indented-code-block detection so we don't lift list-item continuations
    /// or post-paragraph indented lines into code blocks.
    fn previous_line_is_blank_or_bof(&self) -> bool {
        // Walk back through any leading whitespace on the current line
        // (the dispatcher's skip_whitespace may have advanced us past
        // it) to land on either `\n` (preceded by a previous line) or
        // BOF. Either of those plus a blank previous line counts.
        let mut p = self.position;
        while p > 0 && (self.input[p - 1] == ' ' || self.input[p - 1] == '\t') {
            p -= 1;
        }
        if p == 0 {
            return true;
        }
        if self.input.get(p - 1) != Some(&'\n') {
            return false;
        }
        // Find the start of the line BEFORE this one.
        let mut prev_line_start = p - 1; // points at the \n
        while prev_line_start > 0 && self.input[prev_line_start - 1] != '\n' {
            prev_line_start -= 1;
        }
        let prev_line_end = p - 1;
        self.input[prev_line_start..prev_line_end]
            .iter()
            .all(|c| *c == ' ' || *c == '\t')
    }

    /// Per CommonMark §4.4 indented code may not interrupt an *open
    /// paragraph*, but is fine after a heading, thematic break, fenced
    /// code, or list item. Returns true when the spot is eligible: at BOF,
    /// after a blank line, or — if neither — when the last emission was
    /// not a paragraph text token.
    fn can_start_indented_code(&self) -> bool {
        if self.previous_line_is_blank_or_bof() {
            return true;
        }
        !self.last_emitted_was_paragraph_text
    }

    /// Collects all subsequent lines that belong to the current list item's
    /// content (post-blank-gap), stripping `content_offset` columns from
    /// each line. Stops at the first line whose indent falls below
    /// `content_offset` AND can't be lazy-continuation paragraph text, or
    /// at a line that starts a new sibling list marker or outer block.
    /// Returns the joined stripped text, suitable for feeding into a
    /// sub-Lexer.
    fn collect_list_item_block_content(&mut self, content_offset: usize) -> String {
        let mut lines: Vec<String> = Vec::new();
        loop {
            if self.position >= self.input.len() {
                break;
            }
            let line_start = self.position;
            // Measure indent.
            let mut col = 0usize;
            let mut q = line_start;
            while q < self.input.len()
                && (self.input[q] == ' ' || self.input[q] == '\t')
            {
                if self.input[q] == ' ' {
                    col += 1;
                } else {
                    col += 4 - (col % 4);
                }
                q += 1;
            }
            let line_is_blank = q >= self.input.len() || self.input[q] == '\n';
            // A blank line is included only if a subsequent line continues
            // the item — otherwise it terminates.
            if line_is_blank {
                // Look ahead past blanks for a continuing line.
                let mut scan = q;
                if scan < self.input.len() && self.input[scan] == '\n' {
                    scan += 1;
                }
                let mut still_continues = false;
                while scan < self.input.len() {
                    let mut c = 0usize;
                    let mut r = scan;
                    while r < self.input.len()
                        && (self.input[r] == ' ' || self.input[r] == '\t')
                    {
                        if self.input[r] == ' ' {
                            c += 1;
                        } else {
                            c += 4 - (c % 4);
                        }
                        r += 1;
                    }
                    if r >= self.input.len() || self.input[r] == '\n' {
                        if r >= self.input.len() {
                            break;
                        }
                        scan = r + 1;
                        continue;
                    }
                    still_continues = c >= content_offset;
                    break;
                }
                if !still_continues {
                    break;
                }
                // Include the blank line as empty content.
                lines.push(String::new());
                self.position = if q < self.input.len() { q + 1 } else { q };
                continue;
            }
            if col < content_offset {
                break;
            }
            // Strip up to content_offset cols (partial-tab → spaces).
            let mut p = line_start;
            while p < self.input.len() && self.input[p] != '\n' {
                p += 1;
            }
            lines.push(strip_leading_cols(
                &self.input,
                line_start,
                p,
                content_offset,
            ));
            if p < self.input.len() {
                self.position = p + 1;
            } else {
                self.position = p;
                break;
            }
        }
        lines.join("\n")
    }

    /// Indented code block. Strips up to 4 columns of leading whitespace
    /// from each line, includes blank lines if subsequent lines resume the
    /// 4-column indent, and stops at the first non-blank line with less
    /// than 4 columns of indent.
    fn parse_indented_code_block(&mut self) -> Token {
        let mut content = String::new();
        loop {
            if !self.is_at_line_start() {
                break;
            }
            let indent = self.get_current_indent();
            if indent < 4 {
                // Blank line: include if SOME subsequent line is 4-indented
                // (potentially with more blank lines between). Walk forward
                // past all consecutive blank lines and check.
                let line_start = self.position;
                let mut p = self.position;
                while p < self.input.len() && (self.input[p] == ' ' || self.input[p] == '\t') {
                    p += 1;
                }
                if p < self.input.len() && self.input[p] == '\n' {
                    // Skip blank lines, look for a 4-indented one.
                    let mut q = p + 1;
                    let mut found_code = false;
                    loop {
                        let mut next_indent = 0usize;
                        let mut r = q;
                        while r < self.input.len() {
                            match self.input[r] {
                                ' ' => next_indent += 1,
                                '\t' => next_indent += 4 - (next_indent % 4),
                                _ => break,
                            }
                            r += 1;
                        }
                        if r >= self.input.len() {
                            break;
                        }
                        if self.input[r] == '\n' {
                            // Another blank — keep scanning.
                            q = r + 1;
                            continue;
                        }
                        if next_indent >= 4 {
                            found_code = true;
                        }
                        break;
                    }
                    if found_code {
                        content.push('\n');
                        self.position = p + 1;
                        continue;
                    }
                }
                self.position = line_start;
                break;
            }

            // Strip 4 columns of leading whitespace.
            let mut consumed_cols = 0usize;
            while consumed_cols < 4 && self.position < self.input.len() {
                match self.current_char() {
                    ' ' => {
                        consumed_cols += 1;
                        self.advance();
                    }
                    '\t' => {
                        let span = 4 - (consumed_cols % 4);
                        if consumed_cols + span <= 4 {
                            consumed_cols += span;
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    _ => break,
                }
            }

            // Read the rest of the line.
            while self.position < self.input.len() && self.current_char() != '\n' {
                content.push(self.current_char());
                self.advance();
            }
            if self.position < self.input.len() && self.current_char() == '\n' {
                content.push('\n');
                self.advance();
            }
        }
        Token::Code {
            language: String::new(),
            content: content.trim_matches('\n').to_string(),
            block: true,
        }
    }

    /// Gets the current line's indentation level in columns. A tab advances
    /// to the next multiple of 4, so `  \t` is 4 columns total (not 6 as a
    /// flat 4-per-tab rule would give).
    pub fn get_current_indent(&self) -> usize {
        let mut count = 0usize;
        let mut pos = self.position;
        while pos < self.input.len() {
            match self.input[pos] {
                ' ' => count += 1,
                '\t' => count += 4 - (count % 4),
                _ => break,
            }
            pos += 1;
        }
        count
    }

    /// Checks if the given character at the current position is a list marker
    /// A list marker is followed by whitespace (space or tab)
    fn is_list_marker(&self, marker: char) -> bool {
        if self.current_char() != marker {
            return false;
        }

        if self.position + 1 < self.input.len() {
            let next_char = self.input[self.position + 1];
            if next_char == ' ' || next_char == '\t' {
                return true;
            }
            // Empty list marker (`-\n` / `*\n` / EOF after marker) is valid
            // only when (a) it's a sibling of an already-open list, OR (b)
            // the previous source line was blank / start-of-document — in
            // either case there's no paragraph in progress to interrupt.
            if next_char == '\n' || next_char == '\r' {
                return self.last_emitted_list_item
                    || self.previous_line_is_blank_or_bof();
            }
            false
        } else {
            // EOF right after marker: same rule.
            self.last_emitted_list_item || self.previous_line_is_blank_or_bof()
        }
    }
}
