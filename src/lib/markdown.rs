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
                                content.push(self.parse_list_item(false, current_indent, ctx)?);
                                continue;
                            }
                        }
                        '*' => {
                            if self.is_list_marker('*') {
                                content.push(self.parse_list_item(false, current_indent, ctx)?);
                                continue;
                            }
                        }
                        '0'..='9' => {
                            if self.check_ordered_list_marker().is_some() {
                                content.push(self.parse_list_item(true, current_indent, ctx)?);
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
                self.parse_list_item(false, 0, ctx)?
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
                    self.parse_list_item(false, 0, ctx)?
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
                        self.parse_list_item(true, 0, ctx)?
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
                        // Unquoted value: chars until whitespace/`>`.
                        if "\"'=<>`".contains(chars[p]) {
                            return None;
                        }
                        while p < chars.len()
                            && !chars[p].is_whitespace()
                            && chars[p] != '>'
                            && chars[p] != '/'
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
        self.input[self.position..]
            .iter()
            .collect::<String>()
            .starts_with("<!--")
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
                self.try_match_html_tag_len().is_some()
            }

            _ => false,
        }
    }

    /// Converts a flat character offset into a 1-based (line, column) pair so
    /// error messages point users at a specific spot in the input.
    fn pos_to_line_col(&self, pos: usize) -> (usize, usize) {
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
        indent_level: usize,
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
                content.push(self.parse_list_item(false, content_offset, parent_ctx)?);
                first_line_handled = true;
            } else if ch == '*' && self.is_list_marker('*') {
                content.push(self.parse_list_item(false, content_offset, parent_ctx)?);
                first_line_handled = true;
            } else if ch.is_ascii_digit() && self.check_ordered_list_marker().is_some() {
                content.push(self.parse_list_item(true, content_offset, parent_ctx)?);
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
                    first_line_handled = true;
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
                                content.push(self.parse_list_item(
                                    false,
                                    cur_indent,
                                    parent_ctx,
                                )?);
                                continue;
                            }
                            // It was a thematic break — stop.
                            self.position = line_start;
                            break;
                        }
                        '*' => {
                            if self.is_list_marker('*') {
                                content.push(self.parse_list_item(
                                    false,
                                    cur_indent,
                                    parent_ctx,
                                )?);
                                continue;
                            }
                            self.position = line_start;
                            break;
                        }
                        '0'..='9' => {
                            if self.check_ordered_list_marker().is_some() {
                                content.push(self.parse_list_item(
                                    true,
                                    cur_indent,
                                    parent_ctx,
                                )?);
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
    fn get_current_indent(&self) -> usize {
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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper function to create a lexer and parse input
    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn test_basic_text() {
        let tokens = parse("Hello world");
        assert_eq!(tokens, vec![Token::Text("Hello world".to_string())]);
    }

    #[test]
    fn test_headings() {
        let tests = vec![
            (
                "# H1",
                vec![Token::Heading(vec![Token::Text("H1".to_string())], 1)],
            ),
            (
                "## H2",
                vec![Token::Heading(vec![Token::Text("H2".to_string())], 2)],
            ),
            (
                "### H3",
                vec![Token::Heading(vec![Token::Text("H3".to_string())], 3)],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_emphasis() {
        // After removing the spurious trailing-space push in parse_emphasis,
        // emphasis content is exactly the inner text — no extra " " token.
        let tests = vec![
            (
                "*italic*",
                vec![Token::Emphasis {
                    level: 1,
                    content: vec![Token::Text("italic".to_string())],
                }],
            ),
            (
                "**bold**",
                vec![Token::Emphasis {
                    level: 2,
                    content: vec![Token::Text("bold".to_string())],
                }],
            ),
            (
                "_also italic_",
                vec![Token::Emphasis {
                    level: 1,
                    content: vec![Token::Text("also italic".to_string())],
                }],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_code_blocks() {
        let tests = vec![
            (
                "`inline code`",
                vec![Token::Code { language: "".to_string(), content: "inline code".to_string(), block: false }],
            ),
            (
                "```rust\nfn main() {}\n```",
                vec![Token::Code { language: "rust".to_string(), content: "fn main() {}".to_string(), block: true }],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_blockquotes() {
        let tokens = parse("> This is a quote");
        assert_eq!(tokens.len(), 1);
        if let Token::BlockQuote(body) = &tokens[0] {
            let text = Token::collect_all_text(body);
            assert_eq!(text, "This is a quote");
        } else {
            panic!("expected BlockQuote, got {:?}", tokens);
        }
    }

    #[test]
    fn test_lists() {
        let tests = vec![
            (
                "- Item 1\n- Item 2",
                vec![
                    Token::ListItem {
                        content: vec![Token::Text("Item 1".to_string())],
                        ordered: false,
                        number: None,
                        marker: '-',
                        checked: None,
                loose: false,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Item 2".to_string())],
                        ordered: false,
                        number: None,
                        marker: '-',
                        checked: None,
                loose: false,
                    },
                ],
            ),
            (
                "1. First\n2. Second",
                vec![
                    Token::ListItem {
                        content: vec![Token::Text("First".to_string())],
                        ordered: true,
                        number: Some(1),
                        marker: '.',
                        checked: None,
                loose: false,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Second".to_string())],
                        ordered: true,
                        number: Some(2),
                        marker: '.',
                        checked: None,
                loose: false,
                    },
                ],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_nested_lists() {
        let input = "- Item 1\n  - Nested 1\n  - Nested 2\n- Item 2";
        let expected = vec![
            Token::ListItem {
                content: vec![
                    Token::Text("Item 1".to_string()),
                    Token::ListItem {
                        content: vec![Token::Text("Nested 1".to_string())],
                        ordered: false,
                        number: None,
                        marker: '-',
                        checked: None,
                        loose: false,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Nested 2".to_string())],
                        ordered: false,
                        number: None,
                        marker: '-',
                        checked: None,
                        loose: false,
                    },
                ],
                ordered: false,
                number: None,
                marker: '-',
                checked: None,
                loose: false,
            },
            Token::ListItem {
                content: vec![Token::Text("Item 2".to_string())],
                ordered: false,
                number: None,
                marker: '-',
                checked: None,
                loose: false,
            },
        ];
        assert_eq!(parse(input), expected);
    }

    #[test]
    fn test_links() {
        let tests = vec![
            (
                "[Link](https://example.com)",
                vec![Token::Link { content: vec![Token::Text("Link".to_string())], url: "https://example.com".to_string(), title: None }],
            ),
            (
                "![Image](image.jpg)",
                vec![Token::Image { alt: vec![Token::Text("Image".to_string())], url: "image.jpg".to_string(), title: None }],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_horizontal_rule() {
        let tests = vec!["---", "----", "-----"];
        for input in tests {
            assert_eq!(parse(input), vec![Token::HorizontalRule]);
        }
    }
    #[test]
    fn test_complex_document() {
        let input = r#"# Main Title

This is a paragraph with *italic* and **bold** text.

## Subsection

- List item 1
  - Nested item with `code`
- List item 2

> A blockquote

---

[Link](https://example.com)"#;

        let tokens = parse(input);
        assert!(tokens.len() > 0);
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
        // Add more specific assertions as needed
    }

    #[test]
    fn test_error_cases() {
        // Unclosed HTML comment falls back to literal text (the lexer
        // emits the partial `<!--…` chars as `Text` rather than bubbling
        // an error up). The robustness contract is: lexer.parse() returns
        // Ok for any input that doesn't hit a hard panic.
        let mut lexer = Lexer::new("<!--never closes".to_string());
        let tokens = lexer.parse().expect("partial HTML comment should not error");
        let dbg = format!("{:?}", tokens);
        assert!(
            dbg.contains("Text") && dbg.contains("<!--"),
            "expected literal `<!--…` text, got {}",
            dbg
        );
    }

    #[test]
    fn test_code_block_edge_cases() {
        let tests = vec![
            (
                "```\nempty language\n```",
                vec![Token::Code {
                    language: "".to_string(),
                    content: "empty language".to_string(),
                    block: true,
                }],
            ),
            (
                "`code with *asterisk*`",
                vec![Token::Code {
                    language: "".to_string(),
                    content: "code with *asterisk*".to_string(),
                    block: false,
                }],
            ),
            (
                "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```",
                vec![Token::Code { language: "rust".to_string(), content: "fn main() {\n    println!(\"Hello\");\n}".to_string(), block: true }],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_nested_list_combinations() {
        let input = r#"1. First level
   - Nested unordered
   - Another unordered
2. Second level
   1. Nested ordered
   2. Another ordered
   - Mixed with unordered"#;

        let tokens = parse(input);
        assert_eq!(tokens.len(), 2); // Two top-level items
        assert!(matches!(
            tokens[0],
            Token::ListItem {
                ordered: true,
                number: Some(1),
                ..
            }
        ));
        assert!(matches!(
            tokens[1],
            Token::ListItem {
                ordered: true,
                number: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn test_blockquote_variations() {
        // After the blockquote shape change, the body is a Vec<Token> and
        // inline formatting inside a quote is parsed (so *emphasis* becomes
        // an Emphasis token, [link](url) becomes a Link, etc.).
        let cases: &[(&str, &dyn Fn(&[Token])) ] = &[
            (
                "> Simple quote",
                &|body| {
                    assert_eq!(Token::collect_all_text(body), "Simple quote");
                },
            ),
            (
                "> Quote with *emphasis*",
                &|body| {
                    assert!(body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
                },
            ),
            (
                "> Quote with [link](url)",
                &|body| {
                    assert!(body.iter().any(|t| matches!(t, Token::Link { .. })));
                },
            ),
        ];

        for (input, check) in cases {
            let tokens = parse(input);
            assert_eq!(tokens.len(), 1, "input was {:?}", input);
            if let Token::BlockQuote(body) = &tokens[0] {
                check(body);
            } else {
                panic!("expected BlockQuote for {:?}, got {:?}", input, tokens);
            }
        }
    }

    #[test]
    fn test_link_and_image_edge_cases() {
        let tests = vec![
            (
                // Plain URLs may not contain spaces — the URL ends at the
                // first whitespace and the rest is text.
                "[Link with spaces](<https://example.com/path with spaces>)",
                vec![Token::Link {
                    content: vec![Token::Text("Link with spaces".to_string())],
                    url: "https://example.com/path with spaces".to_string(),
                    title: None,
                }],
            ),
            (
                "![Image with *emphasis* in alt](image.jpg)",
                vec![Token::Image {
                    alt: vec![
                        Token::Text("Image with ".to_string()),
                        Token::Emphasis {
                            level: 1,
                            content: vec![Token::Text("emphasis".to_string())],
                        },
                        Token::Text(" in alt".to_string()),
                    ],
                    url: "image.jpg".to_string(),
                    title: None,
                }],
            ),
            (
                "[Empty]()",
                vec![Token::Link { content: vec![Token::Text("Empty".to_string())], url: "".to_string(), title: None }],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_whitespace_handling() {
        // Trailing whitespace after a closing emphasis delimiter is preserved
        // as a separate Text token rather than swallowed. Validate that the
        // emphasis itself parses cleanly; trailing whitespace tokens are OK.
        let tokens = parse("*emphasis with space after*  ");
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
        if let Token::Emphasis { content, .. } = &tokens[0] {
            let inner = Token::collect_all_text(content);
            assert!(
                inner.contains("emphasis with space after"),
                "got {:?}",
                inner
            );
        }
    }

    #[test]
    fn test_mixed_content() {
        let input = r#"# Title with *emphasis*

A paragraph with `code` and [link](url).

- List with **bold**
  1. Nested with *italic*
  2. And `code`

> Quote with [link](url)"#;

        let tokens = parse(input);
        assert!(tokens.len() > 0);

        // Verify first token is a heading with emphasis
        if let Token::Heading(content, 1) = &tokens[0] {
            assert!(content
                .iter()
                .any(|token| matches!(token, Token::Emphasis { .. })));
        } else {
            panic!("Expected heading with emphasis");
        }
    }

    #[test]
    fn test_html_comment_variations() {
        let tests = vec![
            (
                "<!-- Simple -->",
                vec![Token::HtmlComment(" Simple ".to_string())],
            ),
            (
                "<!--Multi\nline\ncomment-->",
                vec![Token::HtmlComment("Multi\nline\ncomment".to_string())],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
        }
    }

    #[test]
    fn test_standalone_exclamation() {
        let tokens = parse("Hello! World");
        assert_eq!(tokens, vec![Token::Text("Hello! World".to_string())]);

        let tokens = parse("This is exciting!");
        assert_eq!(tokens, vec![Token::Text("This is exciting!".to_string())]);

        let tokens = parse("Multiple marks!!");
        assert_eq!(tokens, vec![Token::Text("Multiple marks!!".to_string())]);

        let tokens = parse("![Alt text](image.png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("Alt text".to_string())], url: "image.png".to_string(), title: None }]
        );
    }

    #[test]
    fn test_tables() {
        let input = r#"| Name | Age | City |
|:-----|:---:|----:|
| Alice | 30 | Paris |
| Bob | 25 | Lyon |"#;

        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Table {
                headers: vec![
                    vec![Token::Text("Name".to_string())],
                    vec![Token::Text("Age".to_string())],
                    vec![Token::Text("City".to_string())],
                ],
                aligns: vec![Alignment::Left, Alignment::Center, Alignment::Right],
                rows: vec![
                    vec![
                        vec![Token::Text("Alice".to_string())],
                        vec![Token::Text("30".to_string())],
                        vec![Token::Text("Paris".to_string())],
                    ],
                    vec![
                        vec![Token::Text("Bob".to_string())],
                        vec![Token::Text("25".to_string())],
                        vec![Token::Text("Lyon".to_string())],
                    ],
                ],
            }]
        );
    }
}

#[cfg(test)]
mod heading_hash_in_paragraph_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn csharp_in_paragraph_is_text() {
        let tokens = parse("This uses C# heavily");
        assert_eq!(tokens, vec![Token::Text("This uses C# heavily".to_string())]);
    }

    #[test]
    fn multiple_hashes_in_paragraph() {
        let tokens = parse("Compare C# and F# please");
        assert_eq!(
            tokens,
            vec![Token::Text("Compare C# and F# please".to_string())]
        );
    }

    #[test]
    fn trailing_hash_in_paragraph() {
        let tokens = parse("ends with C#");
        assert_eq!(tokens, vec![Token::Text("ends with C#".to_string())]);
    }

    #[test]
    fn line_start_heading_still_works() {
        let tokens = parse("# Real heading");
        assert_eq!(
            tokens,
            vec![Token::Heading(
                vec![Token::Text("Real heading".to_string())],
                1
            )]
        );
    }

    #[test]
    fn heading_with_hash_in_content() {
        let tokens = parse("## Summary about C#");
        assert_eq!(
            tokens,
            vec![Token::Heading(
                vec![Token::Text("Summary about C#".to_string())],
                2
            )]
        );
    }

    #[test]
    fn paragraph_then_heading() {
        let tokens = parse("first uses C#\n# heading");
        assert_eq!(
            tokens,
            vec![
                Token::Text("first uses C#".to_string()),
                Token::Newline,
                Token::Heading(vec![Token::Text("heading".to_string())], 1),
            ]
        );
    }

    #[test]
    fn heading_then_paragraph_with_hash() {
        let tokens = parse("# Title\n\nbody mentions C# here");
        assert_eq!(
            tokens,
            vec![
                Token::Heading(vec![Token::Text("Title".to_string())], 1),
                Token::Newline,
                Token::Newline,
                Token::Text("body mentions C# here".to_string()),
            ]
        );
    }

    #[test]
    fn full_csharp_issue_repro() {
        // Exact reproducer from issues/csharp.md
        let input = "## Summary\n\nThis monorepo is a coordination layer over four independent implementations of the same problem set. Clojure defines the Clojure algorithmic source, and C#, Rust, and Elixir mirror that source in their own idioms. The container repo keeps the system organized through ZSH-based orchestration, documentation, and repo-wide conventions.";
        let mut lexer = Lexer::new(input.to_string());
        let tokens = lexer.parse().expect("must not error on C# in paragraph");

        assert!(matches!(tokens[0], Token::Heading(_, 2)));
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("C#"));
        assert!(body.contains("Rust"));
        assert!(body.contains("Elixir"));
    }
}

#[cfg(test)]
mod intra_word_underscore_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn single_intra_word_underscore() {
        let tokens = parse("foo_bar");
        assert_eq!(tokens, vec![Token::Text("foo_bar".to_string())]);
    }

    #[test]
    fn double_intra_word_underscore() {
        let tokens = parse("foo__bar");
        assert_eq!(tokens, vec![Token::Text("foo__bar".to_string())]);
    }

    #[test]
    fn triple_intra_word_underscore() {
        let tokens = parse("foo___bar");
        assert_eq!(tokens, vec![Token::Text("foo___bar".to_string())]);
    }

    #[test]
    fn multiple_intra_word_underscores() {
        let tokens = parse("foo_bar_baz_qux");
        assert_eq!(tokens, vec![Token::Text("foo_bar_baz_qux".to_string())]);
    }

    #[test]
    fn snake_case_identifier() {
        let tokens = parse("snake_case_variable");
        assert_eq!(tokens, vec![Token::Text("snake_case_variable".to_string())]);
    }

    #[test]
    fn upper_snake_case() {
        let tokens = parse("UPPER_CASE_CONSTANT");
        assert_eq!(tokens, vec![Token::Text("UPPER_CASE_CONSTANT".to_string())]);
    }

    #[test]
    fn path_with_underscore() {
        let tokens = parse("phpmyadmin/localized_docs");
        assert_eq!(
            tokens,
            vec![Token::Text("phpmyadmin/localized_docs".to_string())]
        );
    }

    #[test]
    fn underscore_path_in_sentence() {
        let tokens = parse("blabla phpmyadmin/localized_docs blabla");
        assert_eq!(
            tokens,
            vec![Token::Text(
                "blabla phpmyadmin/localized_docs blabla".to_string()
            )]
        );
    }

    #[test]
    fn heading_with_intra_word_underscore() {
        let tokens = parse("## phpmyadmin/localized_docs (GitHub)");
        assert_eq!(
            tokens,
            vec![Token::Heading(
                vec![Token::Text("phpmyadmin/localized_docs (GitHub)".to_string())],
                2
            )]
        );
    }

    #[test]
    fn heading_with_code_containing_underscore() {
        let tokens = parse("## `phpmyadmin/localized_docs` (GitHub)");
        if let Token::Heading(content, 2) = &tokens[0] {
            assert!(matches!(content[0], Token::Code { .. }));
            if let Token::Code { content: code, .. } = &content[0] {
                assert_eq!(code, "phpmyadmin/localized_docs");
            }
        } else {
            panic!("expected H2 heading, got {:?}", tokens);
        }
    }

    // Emphasis still works (regression)

    #[test]
    fn single_underscore_emphasis_still_works() {
        let tokens = parse("_italic_");
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    }

    #[test]
    fn double_underscore_strong_still_works() {
        let tokens = parse("__bold__");
        assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
    }

    #[test]
    fn underscore_emphasis_with_space_flank() {
        let tokens = parse("foo _bar_ baz");
        // foo_<space> Text, then _bar_ Emphasis, then baz Text
        // (existing whitespace handling collapses the space after closing `_`)
        assert!(matches!(tokens[0], Token::Text(ref s) if s.starts_with("foo")));
        assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
        assert!(matches!(tokens[2], Token::Text(ref s) if s.contains("baz")));
        if let Token::Emphasis { content, .. } = &tokens[1] {
            let inner = Token::collect_all_text(content);
            assert!(inner.contains("bar"));
        }
    }

    #[test]
    fn underscore_emphasis_in_parens() {
        let tokens = parse("(_foo_)");
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "("));
        assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
        assert!(matches!(tokens[2], Token::Text(ref s) if s == ")"));
    }

    // CommonMark-tricky: outer _ open/close, inner _ is intra-word
    #[test]
    fn outer_emphasis_with_inner_intra_word_underscore() {
        let tokens = parse("_foo_bar_");
        // Should be one emphasis with text "foo_bar"
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
        let inner_text = Token::collect_all_text(&[tokens[0].clone()]);
        assert!(
            inner_text.contains("foo_bar"),
            "expected emphasis to contain 'foo_bar', got {:?}",
            tokens
        );
    }

    // Star emphasis must remain unchanged

    #[test]
    fn star_emphasis_intra_word_still_emphasis() {
        // * is allowed intra-word
        let tokens = parse("a*b*c");
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "a"));
        assert!(matches!(tokens[1], Token::Emphasis { level: 1, .. }));
        assert!(matches!(tokens[2], Token::Text(ref s) if s == "c"));
    }

    #[test]
    fn star_emphasis_basic() {
        let tokens = parse("*italic*");
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    }

    #[test]
    fn star_strong() {
        let tokens = parse("**bold**");
        assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
    }

    // Cross-context

    #[test]
    fn list_item_with_intra_word_underscore() {
        let tokens = parse("- foo_bar item");
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo_bar"));
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }

    #[test]
    fn blockquote_with_intra_word_underscore() {
        let tokens = parse("> Quote with foo_bar inside");
        assert_eq!(tokens.len(), 1);
        if let Token::BlockQuote(body) = &tokens[0] {
            assert_eq!(
                Token::collect_all_text(body),
                "Quote with foo_bar inside"
            );
            // intra-word `_` must not produce emphasis here either
            assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        } else {
            panic!("expected BlockQuote, got {:?}", tokens);
        }
    }

    #[test]
    fn link_with_intra_word_underscore() {
        let tokens = parse("[link_text](https://example.com)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("link_text".to_string())], url: "https://example.com".to_string(), title: None }]
        );
    }

    #[test]
    fn code_with_underscore() {
        let tokens = parse("`foo_bar`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "foo_bar".to_string(), block: false }]
        );
    }

    #[test]
    fn image_alt_with_underscore() {
        let tokens = parse("![alt_text](img.png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("alt_text".to_string())], url: "img.png".to_string(), title: None }]
        );
    }

    // Real-world reproducer from issues/unmatching.md
    #[test]
    fn full_unmatching_issue_repro() {
        let input = "## `phpmyadmin/localized_docs` (GitHub)\n## phpmyadmin/localized_docs (GitHub)";
        let mut lexer = Lexer::new(input.to_string());
        let tokens = lexer.parse().expect("must not error on intra-word _");

        // Two headings, separated by Newline
        assert!(matches!(tokens[0], Token::Heading(_, 2)));
        let last_heading = tokens
            .iter()
            .rev()
            .find(|t| matches!(t, Token::Heading(_, 2)))
            .unwrap();
        if let Token::Heading(content, _) = last_heading {
            let text = Token::collect_all_text(content);
            assert!(text.contains("phpmyadmin/localized_docs"));
        }
    }
}

#[cfg(test)]
mod error_position_tests {
    use super::*;

    #[test]
    fn error_message_uses_line_and_column() {
        let lexer = Lexer::new("a\nb\nc".to_string());
        let (line, col) = lexer.pos_to_line_col(4);
        assert_eq!(line, 3);
        assert_eq!(col, 1);
    }

    #[test]
    fn error_reports_correct_line() {
        let lexer = Lexer::new("first\nsecond\nthird".to_string());
        let pos = "first\nsecond\n".len();
        let (line, col) = lexer.pos_to_line_col(pos);
        assert_eq!(line, 3);
        assert_eq!(col, 1);
    }

    #[test]
    fn lexer_error_variants_exist() {
        // Smoke-test that the LexerError enum still has its variants —
        // future code may surface `UnexpectedEndOfInput` for other inputs
        // even though the unclosed HTML comment now falls back to text.
        let _ = LexerError::UnexpectedEndOfInput;
        let _ = LexerError::UnknownToken("x".to_string());
    }
}

#[cfg(test)]
mod backslash_escape_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }


    #[test]
    fn escape_asterisk_blocks_emphasis() {
        let tokens = parse(r"\*not emphasis\*");
        assert_eq!(tokens, vec![Token::Text("*not emphasis*".to_string())]);
    }

    #[test]
    fn escape_underscore_blocks_emphasis() {
        let tokens = parse(r"\_not emphasis\_");
        assert_eq!(tokens, vec![Token::Text("_not emphasis_".to_string())]);
    }

    #[test]
    fn escape_hash_blocks_heading() {
        // \# at line start should NOT start a heading.
        let tokens = parse(r"\# not a heading");
        assert_eq!(tokens, vec![Token::Text("# not a heading".to_string())]);
    }

    #[test]
    fn escape_left_bracket_blocks_link() {
        let tokens = parse(r"\[not a link]");
        assert_eq!(tokens, vec![Token::Text("[not a link]".to_string())]);
    }

    #[test]
    fn escape_backtick_blocks_code() {
        let tokens = parse(r"\`not code\`");
        assert_eq!(tokens, vec![Token::Text("`not code`".to_string())]);
    }

    #[test]
    fn escape_bang_blocks_image() {
        let tokens = parse(r"\![not an image](x)");
        // \! becomes literal !, then the [ ... ](x) gets parsed as a regular link.
        // Important: this must NOT crash with "Malformed image".
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "!"));
        assert!(matches!(tokens[1], Token::Link { .. }));
    }

    #[test]
    fn escape_double_backslash_yields_single_backslash() {
        let tokens = parse(r"\\");
        assert_eq!(tokens, vec![Token::Text("\\".to_string())]);
    }

    #[test]
    fn escape_then_unescaped_emphasis() {
        // Spec: \\ -> literal \; then _foo_ opens emphasis normally.
        let tokens = parse(r"\\_foo_");
        assert_eq!(
            tokens,
            vec![
                Token::Text("\\".to_string()),
                Token::Emphasis {
                    level: 1,
                    content: vec![Token::Text("foo".to_string())],
                },
            ]
        );
    }

    #[test]
    fn escape_all_punctuation_chars() {
        // Sweep every CommonMark-recognized punctuation char.
        // Each escape pair must collapse to the punctuation char alone.
        let punct = [
            '!', '"', '#', '$', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', ':', ';',
            '<', '=', '>', '?', '@', '[', '\\', ']', '^', '_', '`', '{', '|', '}', '~',
        ];
        for c in punct {
            let input = format!("a\\{}b", c);
            let tokens = parse(&input);
            let collected = Token::collect_all_text(&tokens);
            assert!(
                collected.contains(&format!("a{}b", c)) || collected.contains(c),
                "punctuation {:?}: expected escaped literal in {:?}, got {:?}",
                c,
                input,
                tokens
            );
        }
    }


    #[test]
    fn backslash_before_letter_is_literal() {
        // \a is not an escape — both chars survive.
        let tokens = parse(r"\a");
        assert_eq!(tokens, vec![Token::Text("\\a".to_string())]);
    }

    #[test]
    fn backslash_before_digit_is_literal() {
        let tokens = parse(r"\7");
        assert_eq!(tokens, vec![Token::Text("\\7".to_string())]);
    }

    #[test]
    fn trailing_backslash_at_eof_is_literal() {
        let tokens = parse(r"foo\");
        assert_eq!(tokens, vec![Token::Text("foo\\".to_string())]);
    }


    #[test]
    fn escape_inside_emphasis_run() {
        // *\*foo* opens emphasis, escape produces literal *, foo* closes.
        let tokens = parse(r"*\*foo*");
        assert!(
            matches!(tokens[0], Token::Emphasis { level: 1, .. }),
            "expected emphasis, got {:?}",
            tokens
        );
        if let Token::Emphasis { content, .. } = &tokens[0] {
            let inner = Token::collect_all_text(content);
            assert!(inner.contains("*foo"), "inner was {:?}", inner);
        }
    }

    #[test]
    fn escape_underscore_inside_emphasis() {
        // _foo\_bar_ -> emphasis with literal foo_bar
        let tokens = parse(r"_foo\_bar_");
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
        if let Token::Emphasis { content, .. } = &tokens[0] {
            let inner = Token::collect_all_text(content);
            assert!(inner.contains("foo_bar"), "inner was {:?}", inner);
        }
    }


    #[test]
    fn escape_inside_heading() {
        let tokens = parse(r"# Header with \*literal asterisks\*");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
        if let Token::Heading(content, _) = &tokens[0] {
            let inner = Token::collect_all_text(content);
            assert!(inner.contains("*literal asterisks*"), "got {:?}", inner);
        }
    }


    #[test]
    fn escape_not_active_in_inline_code() {
        // Inside code, \\ and \* are literal — \ stays \.
        let tokens = parse(r"`\*literal\*`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: r"\*literal\*".to_string(), block: false }]
        );
    }

    #[test]
    fn escape_not_active_in_fenced_code() {
        let input = "```\n\\*kept literal\\*\n```";
        let tokens = parse(input);
        if let Token::Code { content: body, .. } = &tokens[0] {
            assert!(body.contains(r"\*kept literal\*"), "body was {:?}", body);
        } else {
            panic!("expected code block, got {:?}", tokens);
        }
    }


    #[test]
    fn escape_blocks_thematic_rule() {
        let tokens = parse(r"\---");
        // \- becomes literal -; remaining -- is plain text.
        assert_eq!(tokens, vec![Token::Text("---".to_string())]);
    }

    #[test]
    fn escape_blocks_blockquote() {
        let tokens = parse(r"\> not a quote");
        assert_eq!(tokens, vec![Token::Text("> not a quote".to_string())]);
    }

    #[test]
    fn escape_blocks_list_marker() {
        // \- at line start should not start a list.
        let tokens = parse(r"\- not a list item");
        assert_eq!(tokens, vec![Token::Text("- not a list item".to_string())]);
    }


    #[test]
    fn mixed_paragraph_with_multiple_escapes() {
        let tokens = parse(r"Use \*asterisks\* or \_underscores\_ for emphasis.");
        assert_eq!(
            tokens,
            vec![Token::Text(
                "Use *asterisks* or _underscores_ for emphasis.".to_string()
            )]
        );
    }

    #[test]
    fn escape_mixed_with_real_emphasis() {
        // Both asterisks around "literal" are escaped (so it stays plain),
        // followed by a genuine *real* emphasis pair.
        let tokens = parse(r"\*literal\* and *real*");
        // -> Text("*literal* and ") + Emphasis(real)
        assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("*literal*")));
        let last = tokens.last().unwrap();
        assert!(matches!(last, Token::Emphasis { .. }));
    }

    #[test]
    fn escape_does_not_consume_newline() {
        // a lone trailing backslash before a newline
        // is a hard line break — produces Text("foo") + HardBreak + Text("bar").
        let tokens = parse("foo\\\nbar");
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "foo"));
        assert!(tokens.iter().any(|t| matches!(t, Token::HardBreak)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Text(ref s) if s == "bar")));
    }

    #[test]
    fn escape_inside_inline_code_span_is_literal() {
        // backslash escapes do NOT apply inside code spans.
        // Body must contain the literal backslash and the asterisk verbatim.
        let tokens = parse(r"`\*not emphasis\*`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: r"\*not emphasis\*".to_string(), block: false }]
        );
    }

    #[test]
    fn escape_inside_multi_backtick_code_span_is_literal() {
        let tokens = parse(r"``a \` b``");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: r"a \` b".to_string(), block: false }]
        );
    }

    #[test]
    fn escape_inside_fenced_code_block_is_literal() {
        let tokens = parse("```\n\\*not emphasis\\*\n```");
        let code = tokens
            .iter()
            .find_map(|t| if let Token::Code { content: body, .. } = t { Some(body) } else { None })
            .expect("expected Code token");
        assert!(
            code.contains(r"\*not emphasis\*"),
            "fenced code body should preserve backslashes literally, got {:?}",
            code
        );
    }

    #[test]
    fn escape_inside_tilde_fenced_code_block_is_literal() {
        let tokens = parse("~~~\n\\*not emphasis\\*\n~~~");
        let code = tokens
            .iter()
            .find_map(|t| if let Token::Code { content: body, .. } = t { Some(body) } else { None })
            .expect("expected Code token");
        assert!(
            code.contains(r"\*not emphasis\*"),
            "tilde fence body should preserve backslashes literally, got {:?}",
            code
        );
    }

    #[test]
    fn escape_inside_autolink_url_is_literal() {
        // escapes don't apply in autolinks. `<http://x/\bar>` keeps
        // the backslash verbatim as part of the URL.
        let tokens = parse(r"<http://example.com/\bar>");
        let link = tokens
            .iter()
            .find_map(|t| if let Token::Link { url, .. } = t { Some(url) } else { None })
            .expect("expected autolink Link token");
        assert!(
            link.contains(r"/\bar"),
            "autolink URL should preserve backslash literally, got {:?}",
            link
        );
    }

    #[test]
    fn escape_in_link_url_inside_parens() {
        // escapes DO apply inside parenthesized link destinations,
        // so `\(` produces a literal `(` in the URL.
        let tokens = parse(r"[t](http://x\)y)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("t".to_string())], url: "http://x)y".to_string(), title: None }]
        );
    }

    #[test]
    fn escape_in_link_text() {
        // escapes apply in link text — `\]` is literal `]`.
        let tokens = parse(r"[a\]b](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("a]b".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn escape_propagates_through_heading() {
        // Heading inline content reuses parse_text, so escapes should also
        // apply inside an ATX heading.
        let tokens = parse(r"# foo \* bar");
        if let Token::Heading(content, 1) = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo * bar"), "got {:?}", text);
            // And no Emphasis should have formed inside.
            assert!(!content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        } else {
            panic!("expected Heading, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn escape_propagates_through_blockquote() {
        let tokens = parse(r"> foo \* bar");
        if let Token::BlockQuote(body) = &tokens[0] {
            let text = Token::collect_all_text(body);
            assert!(text.contains("foo * bar"), "got {:?}", text);
            assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        } else {
            panic!("expected BlockQuote, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn escape_propagates_through_list_item() {
        let tokens = parse(r"- foo \* bar");
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo * bar"), "got {:?}", text);
            assert!(!content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        } else {
            panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn escape_inside_emphasis_run_keeps_punctuation_literal() {
        // *\*foo* — outer * opens emphasis, \\* produces literal *, foo* closes.
        let tokens = parse(r"*\*foo*");
        if let Token::Emphasis { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.starts_with('*'), "got {:?}", text);
            assert!(text.contains("foo"), "got {:?}", text);
        } else {
            panic!("expected Emphasis, got {}", Token::slice_to_compact(&tokens));
        }
    }
}

#[cfg(test)]
mod unmatched_emphasis_fallback_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }


    #[test]
    fn lone_asterisk_in_paragraph_is_text() {
        let tokens = parse("Use * for bullets.");
        let text = Token::collect_all_text(&tokens);
        assert_eq!(text, "Use * for bullets.");
    }

    #[test]
    fn lone_underscore_in_paragraph_is_text() {
        // Note: trailing _ after a space is left-flanking and tries to open;
        // with no closer, it must fall back to literal text.
        let tokens = parse("Lone _underscore here");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("_underscore here"), "got {:?}", text);
    }

    #[test]
    fn unmatched_double_asterisk() {
        let tokens = parse("This **bold start has no end");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("**bold start"), "got {:?}", text);
    }

    #[test]
    fn stray_asterisk_at_eof() {
        let tokens = parse("trailing *");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("*"), "got {:?}", text);
    }

    #[test]
    fn stray_underscore_at_eof() {
        let tokens = parse("trailing _");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("_"), "got {:?}", text);
    }


    #[test]
    fn stray_then_valid_emphasis() {
        // The first * is unmatched -> literal; the *real* pair is emphasis.
        let tokens = parse("stray * then *real* pair");
        // Must contain at least one Emphasis somewhere
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "expected emphasis somewhere in {:?}",
            tokens
        );
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("real"), "got {:?}", text);
    }

    #[test]
    fn valid_then_stray_emphasis() {
        let tokens = parse("*good* then a stray *");
        // Token 0 should be a real emphasis, last token is plain text containing *.
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("*"), "got {:?}", text);
    }


    #[test]
    fn stray_in_heading() {
        let tokens = parse("# heading with * stray");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("*"), "got {:?}", text);
    }

    #[test]
    fn stray_in_list_item() {
        let tokens = parse("- item with * stray");
        assert!(matches!(tokens[0], Token::ListItem { .. }));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("*"), "got {:?}", text);
    }


    #[test]
    fn triple_asterisk_no_close() {
        let tokens = parse("***boldital with no closer");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("***"), "got {:?}", text);
        assert!(text.contains("boldital"), "got {:?}", text);
    }


    #[test]
    fn regression_basic_italic() {
        let tokens = parse("*italic*");
        assert!(matches!(tokens[0], Token::Emphasis { level: 1, .. }));
    }

    #[test]
    fn regression_basic_bold() {
        let tokens = parse("**bold**");
        assert!(matches!(tokens[0], Token::Emphasis { level: 2, .. }));
    }

    #[test]
    fn regression_underscore_emphasis() {
        let tokens = parse("_italic_ and __bold__");
        let count = tokens
            .iter()
            .filter(|t| matches!(t, Token::Emphasis { .. }))
            .count();
        assert_eq!(count, 2, "expected two emphasis tokens, got {:?}", tokens);
    }

    #[test]
    fn regression_intra_word_underscore_still_text() {
        let tokens = parse("phpmyadmin/localized_docs");
        assert_eq!(
            tokens,
            vec![Token::Text("phpmyadmin/localized_docs".to_string())]
        );
    }


    #[test]
    fn document_with_stray_does_not_lose_other_tokens() {
        let input = "# Title\n\nBody has * stray and `code` and [link](url).";
        let tokens = parse(input);
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
        // Code span and link must still parse despite the stray *.
        assert!(tokens.iter().any(|t| matches!(t, Token::Code { .. })));
        assert!(tokens.iter().any(|t| matches!(t, Token::Link { .. })));
    }
}

#[cfg(test)]
mod blockquote_inline_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn block_body(t: &Token) -> &Vec<Token> {
        if let Token::BlockQuote(body) = t {
            body
        } else {
            panic!("expected BlockQuote, got {:?}", t);
        }
    }


    #[test]
    fn inline_emphasis_inside_quote() {
        let tokens = parse("> use **bold** here");
        assert_eq!(tokens.len(), 1);
        let body = block_body(&tokens[0]);
        // Body must contain a real emphasis token, not raw "**bold**" text.
        assert!(
            body.iter().any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
            "expected emphasis inside quote, got body {:?}",
            body
        );
    }

    #[test]
    fn inline_code_inside_quote() {
        let tokens = parse("> see `the_code` for details");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Code { .. })),
            "expected code span, got body {:?}",
            body
        );
    }

    #[test]
    fn inline_link_inside_quote() {
        let tokens = parse("> visit [example](https://example.com)");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Link { .. })),
            "expected link inside quote, got body {:?}",
            body
        );
    }

    #[test]
    fn intra_word_underscore_inside_quote() {
        let tokens = parse("> Quote with foo_bar inside");
        let body = block_body(&tokens[0]);
        let text = Token::collect_all_text(body);
        assert!(text.contains("foo_bar"), "got {:?}", text);
        // Should NOT have produced an emphasis token.
        assert!(!body.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    }


    #[test]
    fn two_line_quote_merges_into_one() {
        let tokens = parse("> first\n> second");
        // One BlockQuote with both lines as content (text/newline structure
        // is fine, but we should NOT have two BlockQuote tokens).
        let count = tokens
            .iter()
            .filter(|t| matches!(t, Token::BlockQuote(_)))
            .count();
        assert_eq!(count, 1, "expected one merged blockquote, got {:?}", tokens);
        let body = block_body(&tokens[0]);
        let text = Token::collect_all_text(body);
        assert!(text.contains("first"), "got {:?}", text);
        assert!(text.contains("second"), "got {:?}", text);
    }

    #[test]
    fn multi_line_with_emphasis_spanning_lines() {
        let tokens = parse("> _start\n> end_");
        let body = block_body(&tokens[0]);
        // Emphasis wraps "start\nend" (across the line break)
        assert!(
            body.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "expected emphasis spanning lines, got {:?}",
            body
        );
    }

    #[test]
    fn blank_line_breaks_blockquote() {
        let tokens = parse("> first\n\n> second");
        let count = tokens
            .iter()
            .filter(|t| matches!(t, Token::BlockQuote(_)))
            .count();
        assert_eq!(
            count, 2,
            "blank line should separate quotes, got {:?}",
            tokens
        );
    }


    #[test]
    fn empty_quote_marker() {
        // A bare `>` followed by EOL is valid CommonMark — empty quote.
        let tokens = parse(">");
        assert!(matches!(tokens[0], Token::BlockQuote(_)));
    }

    #[test]
    fn quote_with_no_space_after_marker() {
        // `>foo` is also a blockquote (the space is optional).
        let tokens = parse(">foo");
        assert!(matches!(tokens[0], Token::BlockQuote(_)));
        let body = block_body(&tokens[0]);
        let text = Token::collect_all_text(body);
        assert!(text.contains("foo"), "got {:?}", text);
    }


    #[test]
    fn regression_simple_quote_text_still_present() {
        let tokens = parse("> This is a quote");
        let body = block_body(&tokens[0]);
        let text = Token::collect_all_text(body);
        assert!(text.contains("This is a quote"), "got {:?}", text);
    }


    #[test]
    fn paragraph_then_quote_then_paragraph() {
        let input = "first\n> middle\nlast";
        let tokens = parse(input);
        let bq_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::BlockQuote(_)))
            .count();
        assert_eq!(bq_count, 1, "expected exactly one quote, got {:?}", tokens);
    }
}

#[cfg(test)]
mod setext_and_thematic_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }


    #[test]
    fn setext_h1_basic() {
        let tokens = parse("Title\n===");
        assert!(
            matches!(tokens[0], Token::Heading(_, 1)),
            "expected H1, got {:?}",
            tokens
        );
        if let Token::Heading(content, 1) = &tokens[0] {
            assert_eq!(Token::collect_all_text(content), "Title");
        }
    }

    #[test]
    fn setext_h1_long_underline() {
        let tokens = parse("Title\n=======");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
    }

    #[test]
    fn setext_h1_with_inline_emphasis() {
        let tokens = parse("Title with *emphasis*\n===");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
        if let Token::Heading(content, 1) = &tokens[0] {
            assert!(content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        }
    }


    #[test]
    fn setext_h2_basic() {
        let tokens = parse("Title\n---");
        assert!(
            matches!(tokens[0], Token::Heading(_, 2)),
            "expected H2 (NOT a HorizontalRule), got {:?}",
            tokens
        );
        if let Token::Heading(content, 2) = &tokens[0] {
            assert_eq!(Token::collect_all_text(content), "Title");
        }
    }

    #[test]
    fn setext_h2_long_underline() {
        let tokens = parse("Title\n----------");
        assert!(matches!(tokens[0], Token::Heading(_, 2)));
    }


    #[test]
    fn thematic_break_dashes() {
        let tokens = parse("---");
        assert_eq!(tokens, vec![Token::HorizontalRule]);
    }

    #[test]
    fn thematic_break_asterisks() {
        let tokens = parse("***");
        assert_eq!(tokens, vec![Token::HorizontalRule]);
    }

    #[test]
    fn thematic_break_underscores() {
        let tokens = parse("___");
        assert_eq!(tokens, vec![Token::HorizontalRule]);
    }

    #[test]
    fn thematic_break_long_runs() {
        for input in ["-------", "*******", "_______"] {
            assert_eq!(parse(input), vec![Token::HorizontalRule], "input {:?}", input);
        }
    }


    #[test]
    fn paragraph_followed_by_dashes_is_setext_h2_not_hr() {
        let tokens = parse("Some content\n---");
        // Must be Heading, not Text + HorizontalRule
        let has_hr = tokens.iter().any(|t| matches!(t, Token::HorizontalRule));
        assert!(!has_hr, "should not have produced an HR, got {:?}", tokens);
        assert!(matches!(tokens[0], Token::Heading(_, 2)));
    }

    #[test]
    fn lone_dashes_after_blank_line_is_hr() {
        let tokens = parse("Some content\n\n---");
        // Blank line means dashes are a true HR, not a setext underline.
        assert!(tokens.iter().any(|t| matches!(t, Token::HorizontalRule)));
    }


    #[test]
    fn regression_atx_h1_still_works() {
        let tokens = parse("# H1");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
    }

    #[test]
    fn regression_atx_h2_still_works() {
        let tokens = parse("## H2");
        assert!(matches!(tokens[0], Token::Heading(_, 2)));
    }

    #[test]
    fn regression_list_item_after_paragraph() {
        // Make sure setext detection doesn't eat list markers.
        let tokens = parse("paragraph\n- item");
        let has_li = tokens.iter().any(|t| matches!(t, Token::ListItem { .. }));
        assert!(has_li, "expected list item, got {:?}", tokens);
    }
}

#[cfg(test)]
mod gfm_trio_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }


    #[test]
    fn unchecked_task_list_item() {
        let tokens = parse("- [ ] Pending task");
        if let Token::ListItem {
            content, checked, ..
        } = &tokens[0]
        {
            assert_eq!(*checked, Some(false), "expected unchecked");
            let text = Token::collect_all_text(content);
            assert!(text.contains("Pending task"), "got {:?}", text);
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }

    #[test]
    fn checked_task_list_item() {
        let tokens = parse("- [x] Done task");
        if let Token::ListItem {
            content, checked, ..
        } = &tokens[0]
        {
            assert_eq!(*checked, Some(true), "expected checked");
            let text = Token::collect_all_text(content);
            assert!(text.contains("Done task"), "got {:?}", text);
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }

    #[test]
    fn task_list_capital_x() {
        let tokens = parse("- [X] also done");
        if let Token::ListItem { checked, .. } = &tokens[0] {
            assert_eq!(*checked, Some(true));
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }

    #[test]
    fn regular_list_item_has_no_checkbox() {
        let tokens = parse("- regular item");
        if let Token::ListItem { checked, .. } = &tokens[0] {
            assert_eq!(*checked, None);
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }

    #[test]
    fn ordered_task_list_item() {
        // GFM allows task markers on ordered lists too.
        let tokens = parse("1. [ ] First task");
        if let Token::ListItem {
            content,
            checked,
            ordered,
            number,
            marker: _,
            loose: _,
        } = &tokens[0]
        {
            assert!(ordered);
            assert_eq!(*number, Some(1));
            assert_eq!(*checked, Some(false));
            assert!(Token::collect_all_text(content).contains("First task"));
        } else {
            panic!("expected list item, got {:?}", tokens);
        }
    }


    #[test]
    fn tilde_fenced_code_block_basic() {
        let input = "~~~\nfn main() {}\n~~~";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code {
                language: "".to_string(),
                content: "fn main() {}".to_string(),
                block: true,
            }]
        );
    }

    #[test]
    fn tilde_fenced_code_block_with_language() {
        let input = "~~~rust\nlet x = 5;\n~~~";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code { language: "rust".to_string(), content: "let x = 5;".to_string(), block: true }]
        );
    }

    #[test]
    fn tilde_fence_can_contain_backticks() {
        // The whole point of `~~~` is letting code contain literal backticks.
        let input = "~~~\nlet s = `template`;\n~~~";
        let tokens = parse(input);
        if let Token::Code { content: body, .. } = &tokens[0] {
            assert!(body.contains("`template`"), "got {:?}", body);
        } else {
            panic!("expected code, got {:?}", tokens);
        }
    }


    #[test]
    fn strikethrough_basic() {
        let tokens = parse("~~deleted~~");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))),
            "expected Strikethrough, got {:?}",
            tokens
        );
        if let Token::Strikethrough(content) = &tokens[0] {
            assert_eq!(Token::collect_all_text(content), "deleted");
        }
    }

    #[test]
    fn strikethrough_inside_paragraph() {
        let tokens = parse("This is ~~old~~ news.");
        assert!(tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("old"), "got {:?}", text);
        assert!(text.contains("news"), "got {:?}", text);
    }

    #[test]
    fn strikethrough_unmatched_falls_back() {
        // An unmatched ~~ must not abort — it falls back to literal text.
        let tokens = parse("starts ~~ but never closes");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("~~"), "got {:?}", text);
    }

    #[test]
    fn single_tilde_is_not_strikethrough() {
        // Only ~~ (two or more) opens strikethrough; lone ~ is plain text.
        let tokens = parse("a ~ b");
        assert!(!tokens.iter().any(|t| matches!(t, Token::Strikethrough(_))));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("~"), "got {:?}", text);
    }

    #[test]
    fn strikethrough_with_emphasis_inside() {
        let tokens = parse("~~deleted *and italic*~~");
        if let Token::Strikethrough(content) = &tokens[0] {
            assert!(content.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        } else {
            panic!("expected Strikethrough, got {:?}", tokens);
        }
    }


    #[test]
    fn tilde_in_inline_code_stays_literal() {
        let tokens = parse("`~~not strikethrough~~`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "~~not strikethrough~~".to_string(), block: false }]
        );
    }
}

#[cfg(test)]
mod link_url_paren_and_autolink_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }


    #[test]
    fn url_with_single_balanced_paren_pair() {
        let tokens = parse("[Wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("Wiki".to_string())], url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(), title: None }]
        );
    }

    #[test]
    fn url_with_nested_balanced_parens() {
        let tokens = parse("[X](http://a.b/((c)d))");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("X".to_string())], url: "http://a.b/((c)d)".to_string(), title: None }]
        );
    }

    #[test]
    fn image_url_with_paren_pair() {
        let tokens = parse("![alt](pic_(small).png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("alt".to_string())], url: "pic_(small).png".to_string(), title: None }]
        );
    }

    #[test]
    fn url_with_unbalanced_close_paren_truncates() {
        let tokens = parse("[X](https://example.com/path)trailing");
        if let Token::Link { content, url, .. } = &tokens[0] {
            assert_eq!(Token::collect_all_text(content), "X");
            assert_eq!(url, "https://example.com/path");
        } else {
            panic!("expected link, got {:?}", tokens);
        }
    }


    #[test]
    fn autolink_https() {
        let tokens = parse("<https://example.com>");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("https://example.com".to_string())], url: "https://example.com".to_string(), title: None }]
        );
    }

    #[test]
    fn autolink_http() {
        let tokens = parse("<http://example.org/path>");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("http://example.org/path".to_string())], url: "http://example.org/path".to_string(), title: None }]
        );
    }

    #[test]
    fn autolink_email() {
        let tokens = parse("<user@example.com>");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("user@example.com".to_string())], url: "mailto:user@example.com".to_string(), title: None }]
        );
    }

    #[test]
    fn autolink_in_paragraph() {
        let tokens = parse("see <https://example.com> for more");
        assert!(
            tokens
                .iter()
                .any(|t| matches!(t, Token::Link { url, .. } if url == "https://example.com")),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn invalid_autolink_falls_through_as_text() {
        let tokens = parse("<not an autolink>");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("<not an autolink>"), "got {:?}", text);
    }


    #[test]
    fn html_comment_still_parsed() {
        let tokens = parse("<!-- comment -->");
        assert!(matches!(tokens[0], Token::HtmlComment(_)));
    }

    #[test]
    fn regression_simple_link() {
        let tokens = parse("[example](https://example.com)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("example".to_string())], url: "https://example.com".to_string(), title: None }]
        );
    }

    #[test]
    fn regression_simple_image() {
        let tokens = parse("![alt](image.png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("alt".to_string())], url: "image.png".to_string(), title: None }]
        );
    }
}

#[cfg(test)]
mod line_ending_normalization_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn crlf_paragraph_then_heading() {
        let lf = parse("first line\n# Heading");
        let crlf = parse("first line\r\n# Heading");
        assert_eq!(lf, crlf);
    }

    #[test]
    fn crlf_blockquote_continuation() {
        let lf = parse("> first\n> second");
        let crlf = parse("> first\r\n> second");
        assert_eq!(lf, crlf);
    }

    #[test]
    fn crlf_setext_heading() {
        let lf = parse("Title\n===");
        let crlf = parse("Title\r\n===");
        assert_eq!(lf, crlf);
    }

    #[test]
    fn crlf_thematic_break() {
        let lf = parse("Para\n\n---\n\nBody");
        let crlf = parse("Para\r\n\r\n---\r\n\r\nBody");
        assert_eq!(lf, crlf);
    }

    #[test]
    fn bare_cr_old_mac_normalized() {
        let lf = parse("first\nsecond");
        let cr = parse("first\rsecond");
        assert_eq!(lf, cr);
    }

    #[test]
    fn mixed_line_endings_in_one_doc() {
        let mixed = parse("# A\r\nbody one\nbody two\rbody three");
        let lf = parse("# A\nbody one\nbody two\nbody three");
        assert_eq!(mixed, lf);
    }
}

#[cfg(test)]
mod hard_line_break_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn two_trailing_spaces_produce_hard_break() {
        let tokens = parse("first  \nsecond");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HardBreak)),
            "expected HardBreak, got {:?}",
            tokens
        );
        // Trailing spaces should be stripped from the preceding Text.
        if let Token::Text(s) = &tokens[0] {
            assert!(!s.ends_with(' '), "trailing spaces not stripped: {:?}", s);
        }
    }

    #[test]
    fn three_trailing_spaces_also_hard_break() {
        let tokens = parse("first   \nsecond");
        assert!(tokens.iter().any(|t| matches!(t, Token::HardBreak)));
    }

    #[test]
    fn one_trailing_space_is_soft_break() {
        let tokens = parse("first \nsecond");
        assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Newline)));
    }

    #[test]
    fn no_trailing_space_is_soft_break() {
        let tokens = parse("first\nsecond");
        assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Newline)));
    }

    #[test]
    fn trailing_backslash_is_hard_break() {
        let tokens = parse("first\\\nsecond");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HardBreak)),
            "expected HardBreak from trailing \\, got {:?}",
            tokens
        );
        // The backslash itself must be stripped from the preceding Text.
        if let Token::Text(s) = &tokens[0] {
            assert!(!s.ends_with('\\'), "backslash not stripped: {:?}", s);
        }
    }

    #[test]
    fn escaped_backslash_then_newline_is_soft_break() {
        // `\\\n` is an escaped backslash (literal `\`) plus a soft break,
        // NOT a hard break (the trailing char isn't a "lone" backslash).
        let tokens = parse("first\\\\\nsecond");
        assert!(!tokens.iter().any(|t| matches!(t, Token::HardBreak)));
        // The literal backslash must remain in the Text.
        if let Token::Text(s) = &tokens[0] {
            assert!(s.contains('\\'), "literal backslash dropped: {:?}", s);
        }
    }

    #[test]
    fn hard_break_inside_blockquote() {
        let tokens = parse("> line one  \n> line two");
        if let Token::BlockQuote(body) = &tokens[0] {
            assert!(body.iter().any(|t| matches!(t, Token::HardBreak)));
        } else {
            panic!("expected BlockQuote, got {:?}", tokens);
        }
    }

    #[test]
    fn hard_break_in_list_item() {
        let tokens = parse("- item one  \n  continuation");
        // Just ensure no error and the HardBreak appears somewhere.
        let any_hb = tokens.iter().any(|t| matches!(t, Token::HardBreak))
            || matches!(&tokens[0], Token::ListItem { content, .. }
                if content.iter().any(|t| matches!(t, Token::HardBreak)));
        assert!(any_hb, "expected HardBreak somewhere, got {:?}", tokens);
    }

    #[test]
    fn no_hard_break_in_atx_heading() {
        // Headings are single-line; trailing spaces are not a hard break.
        let tokens = parse("# Heading  \nbody");
        // Heading content shouldn't contain HardBreak.
        if let Token::Heading(content, _) = &tokens[0] {
            assert!(!content.iter().any(|t| matches!(t, Token::HardBreak)));
        }
    }
}

#[cfg(test)]
mod entity_reference_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn collected(input: &str) -> String {
        Token::collect_all_text(&parse(input))
    }

    #[test]
    fn xml_safe_entities() {
        assert_eq!(collected("a &amp; b"), "a & b");
        assert_eq!(collected("&lt;tag&gt;"), "<tag>");
        assert_eq!(collected("she said &quot;hi&quot;"), "she said \"hi\"");
        assert_eq!(collected("it&apos;s"), "it's");
    }

    #[test]
    fn common_html_named_entities() {
        assert_eq!(collected("&copy; 2025"), "© 2025");
        assert_eq!(collected("&reg; mark"), "® mark");
        assert_eq!(collected("&trade;"), "™");
        assert_eq!(collected("&mdash;"), "—");
        assert_eq!(collected("&ndash;"), "–");
        assert_eq!(collected("&hellip;"), "…");
    }

    #[test]
    fn numeric_decimal_reference() {
        assert_eq!(collected("&#35;"), "#");
        assert_eq!(collected("&#65;"), "A");
        assert_eq!(collected("&#8212;"), "—"); // em dash
    }

    #[test]
    fn numeric_hex_reference() {
        assert_eq!(collected("&#x23;"), "#");
        assert_eq!(collected("&#x41;"), "A");
        assert_eq!(collected("&#X41;"), "A"); // capital X also valid
        assert_eq!(collected("&#x2014;"), "—");
    }

    #[test]
    fn unknown_entity_passes_through() {
        assert_eq!(collected("&zzznotreal;"), "&zzznotreal;");
    }

    #[test]
    fn missing_semicolon_passes_through() {
        // CommonMark requires terminating semicolon; without one, no decoding.
        assert_eq!(collected("&amp foo"), "&amp foo");
    }

    #[test]
    fn lone_ampersand_is_literal() {
        assert_eq!(collected("a & b"), "a & b");
    }

    #[test]
    fn entity_inside_emphasis() {
        let tokens = parse("*alpha &amp; beta*");
        if let Token::Emphasis { content, .. } = &tokens[0] {
            let inner = Token::collect_all_text(content);
            assert!(inner.contains("alpha & beta"), "got {:?}", inner);
        } else {
            panic!("expected emphasis, got {:?}", tokens);
        }
    }

    #[test]
    fn entity_not_decoded_inside_code_span() {
        // Code spans are literal — entity stays as-is.
        let tokens = parse("`&amp;`");
        assert_eq!(tokens, vec![Token::Code { language: "".to_string(), content: "&amp;".to_string(), block: false }]);
    }

    #[test]
    fn invalid_numeric_passes_through() {
        // Out-of-range / malformed numerics pass through unchanged.
        assert_eq!(collected("&#xZZZ;"), "&#xZZZ;");
        assert_eq!(collected("&#abc;"), "&#abc;");
    }

    #[test]
    fn extended_named_entities_decode() {
        // Sample entries spanning the alphabet / character planes.
        assert_eq!(collected("&alpha;"), "\u{03B1}");
        assert_eq!(collected("&beta;"), "\u{03B2}");
        assert_eq!(collected("&Pi;"), "\u{03A0}");
        assert_eq!(collected("&infin;"), "\u{221E}");
        assert_eq!(collected("&euro;"), "\u{20AC}");
        assert_eq!(collected("&para;"), "\u{00B6}");
        assert_eq!(collected("&shy;"), "\u{00AD}"); // soft hyphen
    }

    #[test]
    fn longest_named_entity_decodes() {
        // 31-char body; verifies the lookahead is wide enough.
        assert_eq!(
            collected("&CounterClockwiseContourIntegral;"),
            "\u{2233}"
        );
    }

    #[test]
    fn multi_codepoint_named_entities_decode() {
        // Some entries map to two code points.
        assert_eq!(collected("&fjlig;"), "fj");
        assert_eq!(collected("&ThickSpace;"), "\u{205F}\u{200A}");
    }

    #[test]
    fn entity_names_are_case_sensitive() {
        // Per HTML5: `Aacute` and `aacute` are distinct entries.
        assert_eq!(collected("&Aacute;"), "\u{00C1}");
        assert_eq!(collected("&aacute;"), "\u{00E1}");
    }

    #[test]
    fn numeric_null_becomes_replacement_char() {
        // code point 0 → U+FFFD.
        assert_eq!(collected("&#0;"), "\u{FFFD}");
        assert_eq!(collected("&#x0;"), "\u{FFFD}");
    }

    #[test]
    fn numeric_surrogate_becomes_replacement_char() {
        // Surrogates D800..=DFFF → U+FFFD.
        assert_eq!(collected("&#xD800;"), "\u{FFFD}");
        assert_eq!(collected("&#xDFFF;"), "\u{FFFD}");
        assert_eq!(collected("&#55296;"), "\u{FFFD}"); // 0xD800 decimal
    }

    #[test]
    fn numeric_out_of_range_becomes_replacement_char() {
        // > U+10FFFF → U+FFFD.
        assert_eq!(collected("&#x110000;"), "\u{FFFD}");
        assert_eq!(collected("&#1114112;"), "\u{FFFD}");
    }

    #[test]
    fn numeric_overflow_passes_through_literal() {
        // A digit string that overflows u32 isn't a valid numeric reference;
        // it should appear verbatim (not silently decode to FFFD).
        assert_eq!(collected("&#999999999999;"), "&#999999999999;");
    }

    #[test]
    fn empty_numeric_digits_passes_through() {
        // `&#;` and `&#x;` are malformed — no decoding.
        assert_eq!(collected("&#;"), "&#;");
        assert_eq!(collected("&#x;"), "&#x;");
    }

    #[test]
    fn legacy_non_semicolon_entity_passes_through() {
        // only semicolon-terminated entries decode, even
        // though browsers accept some legacy forms like `&amp` or `&AElig`.
        assert_eq!(collected("&AElig hello"), "&AElig hello");
    }

    #[test]
    fn many_entities_in_one_paragraph() {
        // Stress: a fistful of decodings in a single token stream.
        let text = collected("&alpha; &beta; &gamma; &delta; &epsilon;");
        assert_eq!(text, "\u{03B1} \u{03B2} \u{03B3} \u{03B4} \u{03B5}");
    }

    #[test]
    fn unknown_long_entity_does_not_runaway() {
        // A bogus `&` with no `;` for many chars must NOT consume the rest
        // of the document — emits literal `&` and the rest stays text.
        let text = collected("a &thisnameisreallylongandnotrealatall but continues here.");
        assert!(text.starts_with("a &thisname"), "got: {:?}", text);
        assert!(text.contains("continues here"), "got: {:?}", text);
    }
}

#[cfg(test)]
mod link_title_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn link_with_double_quote_title_strips_title_from_url() {
        let tokens = parse(r#"[text](url "title here")"#);
        assert_eq!(
            tokens,
            vec![Token::Link {
                content: vec![Token::Text("text".to_string())],
                url: "url".to_string(),
                title: Some("title here".to_string()),
            }]
        );
    }

    #[test]
    fn link_with_single_quote_title() {
        let tokens = parse("[text](url 'title here')");
        assert_eq!(
            tokens,
            vec![Token::Link {
                content: vec![Token::Text("text".to_string())],
                url: "url".to_string(),
                title: Some("title here".to_string()),
            }]
        );
    }

    #[test]
    fn link_with_paren_title() {
        let tokens = parse("[text](url (title here))");
        assert_eq!(
            tokens,
            vec![Token::Link {
                content: vec![Token::Text("text".to_string())],
                url: "url".to_string(),
                title: Some("title here".to_string()),
            }]
        );
    }

    #[test]
    fn image_with_title() {
        let tokens = parse(r#"![alt](pic.png "Photo of cat")"#);
        assert_eq!(
            tokens,
            vec![Token::Image {
                alt: vec![Token::Text("alt".to_string())],
                url: "pic.png".to_string(),
                title: Some("Photo of cat".to_string()),
            }]
        );
    }

    #[test]
    fn link_no_title_unchanged() {
        let tokens = parse("[text](url)");
        assert_eq!(
            tokens,
            vec![Token::Link {
                content: vec![Token::Text("text".to_string())],
                url: "url".to_string(),
                title: None,
            }]
        );
    }

    #[test]
    fn link_url_paren_pair_with_title() {
        // URL contains balanced parens AND a title at the end.
        let tokens = parse(r#"[Wiki](https://en.wikipedia.org/wiki/Foo_(bar) "Wikipedia entry")"#);
        assert_eq!(
            tokens,
            vec![Token::Link {
                content: vec![Token::Text("Wiki".to_string())],
                url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(),
                title: Some("Wikipedia entry".to_string()),
            }]
        );
    }

    #[test]
    fn link_with_only_whitespace_after_url_no_title() {
        // Trailing whitespace before `)` without a title is fine.
        let tokens = parse("[text](url   )");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("text".to_string())], url: "url".to_string(), title: None }]
        );
    }

    #[test]
    fn link_url_with_no_space_then_quote_is_url_only() {
        // `(url"foo")` with no whitespace between url and quote — not a title.
        // The whole `url"foo"` is the URL.
        let tokens = parse("[text](url\"foo\")");
        if let Token::Link { url, .. } = &tokens[0] {
            assert!(url.contains("\""), "expected url to contain quote, got {:?}", url);
        } else {
            panic!("expected link, got {:?}", tokens);
        }
    }
}

#[cfg(test)]
mod reference_link_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn full_reference_link() {
        let input = "[CommonMark][cm]\n\n[cm]: https://commonmark.org";
        let tokens = parse(input);
        assert!(
            tokens.iter().any(|t| matches!(
                t,
                Token::Link { content, url, .. }
                if Token::collect_all_text(content) == "CommonMark"
                    && url == "https://commonmark.org"
            )),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn collapsed_reference_link() {
        let input = "[CommonMark][]\n\n[CommonMark]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn shortcut_reference_link() {
        let input = "[CommonMark]\n\n[CommonMark]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn label_matching_is_case_insensitive() {
        let input = "[CommonMark][CM]\n\n[cm]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link { url, .. } if url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn definition_line_is_not_emitted_as_text() {
        let input = "para\n\n[cm]: https://commonmark.org";
        let tokens = parse(input);
        // No token should contain the literal text "https://commonmark.org"
        // outside of a Link, since the definition line is consumed.
        let stray = tokens
            .iter()
            .any(|t| matches!(t, Token::Text(s) if s.contains("https://commonmark.org")));
        assert!(!stray, "definition line bled into output: {:?}", tokens);
    }

    #[test]
    fn unresolved_shortcut_falls_back_to_text() {
        // `[Word]` with no matching definition should NOT become a Link
        // (today it does — empty URL — which is the bug).
        let tokens = parse("Just [Word] in text.");
        let has_link = tokens.iter().any(|t| matches!(t, Token::Link { .. }));
        assert!(
            !has_link,
            "unresolved shortcut must NOT become a link, got {:?}",
            tokens
        );
    }

    #[test]
    fn reference_image() {
        let input = "![alt][img]\n\n[img]: pic.png";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Image { url, .. } if url == "pic.png")
        ), "got {:?}", tokens);
    }

    #[test]
    fn definition_with_title_is_parsed_url_clean() {
        let input = "[a][r]\n\n[r]: https://example.com \"Example\"";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link { url, .. } if url == "https://example.com")
        ), "URL should be clean (no title baked in), got {:?}", tokens);
    }

    #[test]
    fn inline_link_still_takes_priority_over_reference() {
        // [text](url) is inline — must NOT be confused with a reference.
        let tokens = parse("[text](https://example.com)\n\n[text]: should-not-apply");
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link { url, .. } if url == "https://example.com")
        ));
    }

    #[test]
    fn whitespace_in_label_normalized() {
        let input = "[Multi  Word  Label][m]\n\n[M  Word  Label]: https://example.com";
        let tokens = parse(input);
        let _ = tokens;
    }

    #[test]
    fn space_after_reference_link_preserved() {
        // Text following a [t][r] reference must keep its leading space —
        // `]` should be treated like `)` by
        // is_after_special_token so skip_whitespace doesn't swallow it.
        let input = "See [the spec][cm] for details.\n\n[cm]: https://x";
        let tokens = parse(input);
        let body = Token::collect_all_text(&tokens);
        assert!(
            body.contains(" for details"),
            "expected leading space before 'for', got {:?}",
            body
        );
    }

    #[test]
    fn space_after_shortcut_link_preserved() {
        let input = "A bare [Rust] is also a link.\n\n[Rust]: https://rust-lang.org";
        let tokens = parse(input);
        let body = Token::collect_all_text(&tokens);
        assert!(
            body.contains(" is also"),
            "expected leading space before 'is', got {:?}",
            body
        );
    }

    #[test]
    fn space_after_collapsed_reference_preserved() {
        let input = "The [Wikipedia][] entry.\n\n[Wikipedia]: https://x";
        let tokens = parse(input);
        let body = Token::collect_all_text(&tokens);
        assert!(
            body.contains(" entry"),
            "expected leading space before 'entry', got {:?}",
            body
        );
    }

    #[test]
    fn space_after_unresolved_shortcut_preserved() {
        let input = "Phrase [No Such Label] stays literal.";
        let tokens = parse(input);
        let body = Token::collect_all_text(&tokens);
        assert!(
            body.contains(" stays"),
            "expected leading space before 'stays', got {:?}",
            body
        );
    }

    #[test]
    fn space_after_autolink_preserved() {
        let tokens = parse("see <https://example.com> for more");
        let body = Token::collect_all_text(&tokens);
        assert!(
            body.contains(" for "),
            "expected leading space before 'for', got {:?}",
            body
        );
    }
}

#[cfg(test)]
mod multi_backtick_inline_code_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn double_backtick_inline_with_single_backtick_inside() {
        let tokens = parse("``code with ` inside``");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "code with ` inside".to_string(), block: false }]
        );
    }

    #[test]
    fn triple_backtick_inline_when_not_at_line_start() {
        let tokens = parse("inline ```code with `` inside``` here");
        // First Text("inline "), then Code, then Text(" here").
        assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("inline")));
        assert!(matches!(tokens[1], Token::Code { ref content, .. } if content.contains("``")));
    }

    #[test]
    fn double_backtick_with_count_mismatch_inside() {
        // ``a`b``  -> code containing "a`b". A single ` inside doesn't close.
        let tokens = parse("``a`b``");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "a`b".to_string(), block: false }]
        );
    }

    #[test]
    fn fenced_block_still_works() {
        let input = "```rust\nfn main() {}\n```";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code { language: "rust".to_string(), content: "fn main() {}".to_string(), block: true }]
        );
    }

    #[test]
    fn fenced_block_preserves_inner_backticks() {
        // A single ` (or any run shorter than the opener) inside the body
        // must remain in the output. Pre-existing bug: count_backticks
        // advanced past the inner ticks but never pushed them to content.
        let input = "```rust\nlet s = `template`;\n```";
        let tokens = parse(input);
        if let Token::Code { content: body, .. } = &tokens[0] {
            assert!(
                body.contains("`template`"),
                "fenced block stripped inner backticks: {:?}",
                body
            );
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn fenced_block_preserves_double_backtick_run_inside() {
        // Triple-fence; body contains `` (count 2) which must survive.
        let input = "```\nfoo `` bar\n```";
        let tokens = parse(input);
        if let Token::Code { content: body, .. } = &tokens[0] {
            assert!(
                body.contains("``"),
                "double-backtick run lost in fence body: {:?}",
                body
            );
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn double_backtick_at_line_start_with_content_is_inline() {
        // ``code`` at line start is still inline if there's content on the
        // same line beyond the closing run.
        let tokens = parse("``inline`` plus text");
        assert!(matches!(tokens[0], Token::Code { ref content, .. } if content == "inline"));
        assert!(tokens.iter().any(|t| matches!(t, Token::Text(s) if s.contains("plus text"))));
    }

    #[test]
    fn unclosed_inline_code_falls_back_to_text() {
        // No matching closer (EOF reached) — the opener run
        // becomes literal text so the body chars still render normally.
        let tokens = parse("``never closes");
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "``"));
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("never closes"), "got {:?}", body);
    }

    #[test]
    fn unclosed_inline_code_does_not_gobble_across_blank_line() {
        // An unclosed `` ` `` inside a paragraph must NOT pull the next
        // paragraph's text into a code block. The literal-text fallback
        // prevents the gobble.
        let input = "first paragraph with `unclosed.\n\nSecond paragraph.";
        let tokens = parse(input);
        // No multi-line Code should be produced.
        let multi_line_code = tokens
            .iter()
            .any(|t| matches!(t, Token::Code { content: c, .. } if c.contains('\n')));
        assert!(
            !multi_line_code,
            "code span gobbled across paragraphs: {:?}",
            tokens
        );
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("Second paragraph"), "got {:?}", body);
    }

    #[test]
    fn single_backtick_unchanged() {
        let tokens = parse("`simple`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "simple".to_string(), block: false }]
        );
    }
}

#[cfg(test)]
mod tab_expansion_tests {
    use super::*;

    #[test]
    fn tab_at_column_one_is_four_spaces() {
        let lexer = Lexer::new("\tx".to_string());
        assert_eq!(lexer.get_current_indent(), 4);
    }

    #[test]
    fn two_spaces_then_tab_is_four_columns() {
        // 2 spaces + \t → tab fills to next column-4 boundary, total = 4.
        let lexer = Lexer::new("  \tx".to_string());
        assert_eq!(lexer.get_current_indent(), 4);
    }

    #[test]
    fn three_spaces_then_tab_is_four_columns() {
        // 3 spaces + \t → tab fills col 4 only, total = 4.
        let lexer = Lexer::new("   \tx".to_string());
        assert_eq!(lexer.get_current_indent(), 4);
    }

    #[test]
    fn one_space_then_tab_is_four_columns() {
        let lexer = Lexer::new(" \tx".to_string());
        assert_eq!(lexer.get_current_indent(), 4);
    }

    #[test]
    fn two_tabs_is_eight_columns() {
        let lexer = Lexer::new("\t\tx".to_string());
        assert_eq!(lexer.get_current_indent(), 8);
    }

    #[test]
    fn tab_then_spaces() {
        // \t + 2 spaces → 4 + 2 = 6
        let lexer = Lexer::new("\t  x".to_string());
        assert_eq!(lexer.get_current_indent(), 6);
    }
}

#[cfg(test)]
mod indented_code_block_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn four_space_indented_line_is_code() {
        let tokens = parse("    let x = 5;");
        assert_eq!(
            tokens,
            vec![Token::Code {
                language: "".to_string(),
                content: "let x = 5;".to_string(),
                block: true,
            }]
        );
    }

    #[test]
    fn tab_indent_is_code() {
        let tokens = parse("\tlet x = 5;");
        assert_eq!(
            tokens,
            vec![Token::Code {
                language: "".to_string(),
                content: "let x = 5;".to_string(),
                block: true,
            }]
        );
    }

    #[test]
    fn three_spaces_is_not_code() {
        // 3 spaces is not enough; should be regular paragraph text.
        let tokens = parse("   not code");
        let body = Token::collect_all_text(&tokens);
        assert_eq!(body, "not code");
        assert!(!tokens.iter().any(|t| matches!(t, Token::Code { .. })));
    }

    #[test]
    fn multi_line_indented_code() {
        let input = "    fn main() {\n        println!(\"hi\");\n    }";
        let tokens = parse(input);
        if let Token::Code { content: body, .. } = &tokens[0] {
            assert!(body.contains("fn main()"), "got {:?}", body);
            assert!(body.contains("println!"), "got {:?}", body);
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn indented_code_inside_paragraph_does_not_apply() {
        // Indented line directly after a paragraph is treated as paragraph
        // continuation not code. We're more permissive: it
        // becomes code if separated by a blank line. Test the blank-line case.
        let input = "Some paragraph\n\n    code line";
        let tokens = parse(input);
        assert!(tokens.iter().any(|t| matches!(t, Token::Code { .. })));
    }

    #[test]
    fn fenced_code_block_unaffected() {
        let input = "```\nfn main() {}\n```";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code {
                language: "".to_string(),
                content: "fn main() {}".to_string(),
                block: true,
            }]
        );
    }

    #[test]
    fn list_item_four_space_indent_is_nesting_not_code() {
        // 4 spaces under a list bullet is list-item continuation/nesting,
        // NOT an indented code block.
        let input = "- item one\n    nested\n- item two";
        let tokens = parse(input);
        let li_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count();
        assert!(li_count >= 2, "expected at least 2 list items, got {:?}", tokens);
        assert!(!tokens.iter().any(|t| matches!(t, Token::Code { .. })));
    }
}

#[cfg(test)]
mod raw_inline_html_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn open_tag_inline() {
        let tokens = parse("text <span> more");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HtmlInline(s) if s == "<span>")),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn closing_tag_inline() {
        let tokens = parse("text </span> more");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HtmlInline(s) if s == "</span>")),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn open_tag_with_attribute() {
        let tokens = parse(r#"<a href="https://example.com">"#);
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HtmlInline(_))),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn open_tag_self_closing() {
        let tokens = parse("<br/>");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HtmlInline(s) if s.contains("br"))),
            "got {:?}",
            tokens
        );
    }

    #[test]
    fn html_comment_still_works() {
        let tokens = parse("<!-- comment -->");
        assert!(matches!(tokens[0], Token::HtmlComment(_)));
    }

    #[test]
    fn autolink_still_works() {
        let tokens = parse("<https://example.com>");
        assert!(matches!(tokens[0], Token::Link { .. }));
    }

    #[test]
    fn invalid_tag_falls_through_as_text() {
        let tokens = parse("<not a real tag>");
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("<not a real tag>"), "got {:?}", body);
    }

    #[test]
    fn lt_alone_stays_text() {
        let tokens = parse("a < b is true");
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("<"), "got {:?}", body);
    }

    #[test]
    fn surrounding_text_preserved() {
        let tokens = parse("before <em> middle </em> after");
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("before"), "got {:?}", body);
        assert!(body.contains("after"), "got {:?}", body);
        let html_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::HtmlInline(_)))
            .count();
        assert_eq!(html_count, 2, "expected 2 HtmlInline tokens, got {:?}", tokens);
    }
}

#[cfg(test)]
mod emphasis_flanking_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn emphasis_with_inner_spaces_does_not_open() {
        // `* foo *` — the opening `*` is followed by a space, so it can't
        // open emphasis (not left-flanking). Should be plain text.
        let tokens = parse("a * foo * b");
        assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("* foo *"), "got {:?}", body);
    }

    #[test]
    fn opener_followed_by_space_no_emphasis() {
        let tokens = parse("a* foo*");
        // Opener is `*` followed by space → not left-flanking → no emphasis.
        assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    }

    #[test]
    fn closer_preceded_by_space_no_emphasis() {
        let tokens = parse("a *foo *");
        // Closing `*` is preceded by space → not right-flanking → no close.
        assert!(!tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    }

    #[test]
    fn valid_emphasis_with_no_inner_space() {
        let tokens = parse("a *foo* b");
        assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { level: 1, .. })));
    }

    #[test]
    fn valid_strong_with_no_inner_space() {
        let tokens = parse("a **bold** b");
        assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { level: 2, .. })));
    }

    #[test]
    fn underscore_emphasis_works_at_word_boundary() {
        let tokens = parse("a _foo_ b");
        assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { level: 1, .. })));
    }

    #[test]
    fn intra_word_underscore_still_text() {
        // `_` flanked by alphanumerics on both sides is treated as literal text.
        let tokens = parse("foo_bar_baz");
        assert_eq!(tokens, vec![Token::Text("foo_bar_baz".to_string())]);
    }

    #[test]
    fn star_can_open_intra_word() {
        // `*` is more permissive than `_` per spec: it can open intra-word.
        let tokens = parse("foo*bar*baz");
        assert!(tokens.iter().any(|t| matches!(t, Token::Emphasis { .. })));
    }

    #[test]
    fn unmatched_lone_asterisk_still_text() {
        // A stray asterisk must not abort — it falls back to literal text.
        let tokens = parse("Use * for bullets.");
        let body = Token::collect_all_text(&tokens);
        assert_eq!(body, "Use * for bullets.");
    }

    #[test]
    fn emphasis_does_not_cross_blank_line() {
        // An opener that can't find a valid same-paragraph closer must NOT
        // gobble the next paragraph's content. The blank line acts as a
        // paragraph boundary, forcing literal-text fallback.
        let input = "para with *unclosed opener\n\n## Heading after blank";
        let tokens = parse(input);
        // The `## Heading…` must parse as a real heading token.
        let has_heading = tokens
            .iter()
            .any(|t| matches!(t, Token::Heading(_, 2)));
        assert!(
            has_heading,
            "expected H2 after blank line, got {:?}",
            tokens
        );
        // Body must still contain the `*` literally.
        let body = Token::collect_all_text(&tokens);
        assert!(body.contains("*unclosed opener"), "got {:?}", body);
    }

    #[test]
    fn star_with_inner_space_does_not_eat_following_paragraph() {
        // `*foo *` cannot close (closer preceded by space) and must not
        // gobble the next heading.
        let input = "Closer preceded: a *foo * — text.\n\n## Next heading";
        let tokens = parse(input);
        let has_heading = tokens
            .iter()
            .any(|t| matches!(t, Token::Heading(_, 2)));
        assert!(
            has_heading,
            "expected H2 after the paragraph, got {:?}",
            tokens
        );
    }
}

#[cfg(test)]
mod heading_strictness_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn atx_without_space_is_text() {
        let tokens = parse("#hello");
        assert_eq!(tokens, vec![Token::Text("#hello".to_string())]);
    }

    #[test]
    fn atx_with_space_is_heading() {
        let tokens = parse("# hello");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
    }

    #[test]
    fn atx_with_tab_after_hash_is_heading() {
        let tokens = parse("#\thello");
        assert!(matches!(tokens[0], Token::Heading(_, 1)));
    }

    #[test]
    fn atx_seven_hashes_falls_back_to_text() {
        let tokens = parse("####### too deep");
        assert!(!matches!(tokens[0], Token::Heading(_, _)));
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("####### too deep"), "got {:?}", text);
    }

    #[test]
    fn atx_six_hashes_is_h6() {
        let tokens = parse("###### six");
        assert!(matches!(tokens[0], Token::Heading(_, 6)));
    }

    #[test]
    fn atx_trailing_hashes_stripped() {
        let tokens = parse("## Title ##");
        if let Token::Heading(content, 2) = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert_eq!(text, "Title");
        } else {
            panic!("expected H2, got {:?}", tokens);
        }
    }

    #[test]
    fn atx_trailing_hashes_with_trailing_space_stripped() {
        let tokens = parse("## Title ## ");
        if let Token::Heading(content, 2) = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert_eq!(text, "Title");
        } else {
            panic!("expected H2, got {:?}", tokens);
        }
    }

    #[test]
    fn atx_trailing_hash_without_preceding_space_kept() {
        // Regression — `## C#` must keep the `#` as content (no preceding space).
        let tokens = parse("## C#");
        if let Token::Heading(content, 2) = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert_eq!(text, "C#");
        } else {
            panic!("expected H2, got {:?}", tokens);
        }
    }

    #[test]
    fn empty_atx_just_hashes() {
        let tokens = parse("##");
        assert!(matches!(tokens[0], Token::Heading(_, 2)));
        if let Token::Heading(content, _) = &tokens[0] {
            assert!(content.is_empty());
        }
    }
}

#[cfg(test)]
mod ordered_list_marker_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn paren_marker_creates_ordered_list_item() {
        let tokens = parse("1) one\n2) two");
        let count = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { ordered: true, .. }))
            .count();
        assert_eq!(count, 2, "got {:?}", tokens);
    }

    #[test]
    fn paren_marker_preserves_number() {
        let tokens = parse("5) five");
        if let Token::ListItem { number, ordered, .. } = &tokens[0] {
            assert!(*ordered);
            assert_eq!(*number, Some(5));
        } else {
            panic!("expected ordered list item, got {:?}", tokens);
        }
    }

    #[test]
    fn dot_marker_still_works() {
        let tokens = parse("1. one");
        if let Token::ListItem { ordered, number, .. } = &tokens[0] {
            assert!(*ordered);
            assert_eq!(*number, Some(1));
        } else {
            panic!("expected ordered list item, got {:?}", tokens);
        }
    }
}

#[cfg(test)]
mod code_span_space_strip_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn single_surrounding_space_stripped() {
        let tokens = parse("a ` foo ` b");
        let codes: Vec<_> = tokens
            .iter()
            .filter_map(|t| {
                if let Token::Code { content: body, .. } = t {
                    Some(body.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(codes, vec!["foo"]);
    }

    #[test]
    fn double_surrounding_space_only_one_stripped() {
        let tokens = parse("a `  foo  ` b");
        if let Some(Token::Code { content: body, .. }) =
            tokens.iter().find(|t| matches!(t, Token::Code { .. }))
        {
            assert_eq!(body, " foo ");
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn all_spaces_not_stripped() {
        let tokens = parse("a `   ` b");
        if let Some(Token::Code { content: body, .. }) =
            tokens.iter().find(|t| matches!(t, Token::Code { .. }))
        {
            assert_eq!(body, "   ");
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn no_surrounding_space_unchanged() {
        let tokens = parse("`foo`");
        assert_eq!(
            tokens,
            vec![Token::Code { language: "".to_string(), content: "foo".to_string(), block: false }]
        );
    }

    #[test]
    fn one_sided_space_unchanged() {
        // Only strip when BOTH sides have a space.
        let tokens = parse("a ` foo` b");
        if let Some(Token::Code { content: body, .. }) =
            tokens.iter().find(|t| matches!(t, Token::Code { .. }))
        {
            assert_eq!(body, " foo");
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }
}

#[cfg(test)]
mod blockquote_block_constructs_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn block_body(t: &Token) -> &Vec<Token> {
        if let Token::BlockQuote(body) = t {
            body
        } else {
            panic!("expected BlockQuote, got {:?}", t);
        }
    }

    #[test]
    fn setext_h2_inside_blockquote() {
        let tokens = parse("> Title\n> ---");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Heading(_, 2))),
            "expected H2 inside quote, got {:?}",
            body
        );
    }

    #[test]
    fn setext_h1_inside_blockquote() {
        let tokens = parse("> Big\n> ===");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Heading(_, 1))),
            "expected H1 inside quote, got {:?}",
            body
        );
    }

    #[test]
    fn indented_code_inside_blockquote() {
        let tokens = parse(">     code line in quote");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Code { .. })),
            "expected Code inside quote, got {:?}",
            body
        );
    }

    #[test]
    fn regular_text_inside_blockquote_unaffected() {
        let tokens = parse("> Just a sentence with three spaces:    not code.");
        let body = block_body(&tokens[0]);
        assert!(!body.iter().any(|t| matches!(t, Token::Code { .. })));
    }
}

#[cfg(test)]
mod blockquote_lazy_continuation_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn body(t: &Token) -> &Vec<Token> {
        if let Token::BlockQuote(body) = t {
            body
        } else {
            panic!("expected BlockQuote, got {:?}", t);
        }
    }

    // a non-prefixed line that doesn't start a new block
    // joins the open paragraph inside the quote.
    #[test]
    fn single_lazy_line_joins_paragraph() {
        let tokens = parse("> foo\nbar");
        assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
        let text = Token::collect_all_text(body(&tokens[0]));
        assert!(
            text.contains("foo") && text.contains("bar"),
            "got {:?}",
            text
        );
    }

    #[test]
    fn multiple_lazy_lines_all_join() {
        let tokens = parse("> foo\nbar\nbaz");
        assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
        let text = Token::collect_all_text(body(&tokens[0]));
        for needle in &["foo", "bar", "baz"] {
            assert!(text.contains(needle), "{:?} missing from {:?}", needle, text);
        }
    }

    #[test]
    fn lazy_mixed_with_marker_lines() {
        // Spec lazy lines can be interleaved with `>` lines.
        let tokens = parse("> foo\nbar\n> baz");
        assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
        let text = Token::collect_all_text(body(&tokens[0]));
        for needle in &["foo", "bar", "baz"] {
            assert!(text.contains(needle), "{:?} missing from {:?}", needle, text);
        }
    }

    #[test]
    fn blank_line_terminates_lazy() {
        let tokens = parse("> foo\nbar\n\nbaz");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo") && q_text.contains("bar"));
        assert!(!q_text.contains("baz"), "blank line didn't stop quote: {:?}", q_text);
        // baz should appear as a separate top-level token.
        let after = Token::collect_all_text(&tokens[1..]);
        assert!(after.contains("baz"), "baz missing from rest {:?}", after);
    }

    #[test]
    fn thematic_break_interrupts_lazy() {
        let tokens = parse("> foo\n---");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo"));
        assert!(!q_text.contains("---"), "thematic leaked in: {:?}", q_text);
        assert!(
            tokens[1..].iter().any(|t| matches!(t, Token::HorizontalRule)),
            "expected HR after quote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn list_marker_interrupts_lazy() {
        let tokens = parse("> foo\n- bar");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo"));
        assert!(!q_text.contains("bar"), "marker leaked in: {:?}", q_text);
        assert!(
            tokens[1..].iter().any(|t| matches!(t, Token::ListItem { .. })),
            "expected ListItem after quote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn atx_heading_interrupts_lazy() {
        let tokens = parse("> foo\n# heading");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo"));
        assert!(!q_text.contains("heading"));
        assert!(
            tokens[1..].iter().any(|t| matches!(t, Token::Heading(_, 1))),
            "expected H1 after quote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn fenced_code_interrupts_lazy() {
        let tokens = parse("> foo\n```\ncode\n```");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo"));
        assert!(!q_text.contains("code"), "fence leaked in: {:?}", q_text);
        assert!(
            tokens[1..].iter().any(|t| matches!(t, Token::Code { .. })),
            "expected Code after quote, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn lazy_in_nested_blockquote_attaches_innermost() {
        // Per spec example 234: `>> foo\nbar` → bar joins the inner quote's
        // paragraph. We rely on the sub-lexer running the same lazy logic
        // recursively.
        let tokens = parse(">> foo\nbar");
        assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
        let outer = body(&tokens[0]);
        // The outer body must contain exactly one nested BlockQuote, whose
        // body contains both `foo` and `bar`.
        let inner = outer
            .iter()
            .find_map(|t| if let Token::BlockQuote(b) = t { Some(b) } else { None })
            .expect("expected nested BlockQuote");
        let inner_text = Token::collect_all_text(inner);
        assert!(
            inner_text.contains("foo") && inner_text.contains("bar"),
            "inner text: {:?}",
            inner_text
        );
    }

    #[test]
    fn empty_quote_line_closes_paragraph_no_lazy() {
        // `> foo\n>\nbar` — the empty `>` line closes the open paragraph;
        // `bar` should NOT lazy-continue into the quote.
        let tokens = parse("> foo\n>\nbar");
        let q_text = Token::collect_all_text(body(&tokens[0]));
        assert!(q_text.contains("foo"));
        assert!(
            !q_text.contains("bar"),
            "bar must not be lazy after empty `>` line: {:?}",
            q_text
        );
    }

    #[test]
    fn lazy_line_inline_formatting_is_parsed() {
        // The lazy line goes through the same sub-lexer pass — emphasis,
        // links, etc. must still be recognized inside it.
        let tokens = parse("> normal\n*lazy emphasis*");
        let quote = body(&tokens[0]);
        assert!(
            quote.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "expected emphasis in quote body, got {}",
            Token::slice_to_compact(quote)
        );
    }

    #[test]
    fn nested_blockquote_marker_continues_quote() {
        // `> foo\n>> bar` — second line starts another quote inside the
        // first; should not be lazy. Both should be inside the outer quote.
        let tokens = parse("> foo\n>> bar");
        assert_eq!(tokens.len(), 1, "got {}", Token::slice_to_compact(&tokens));
        let outer = body(&tokens[0]);
        assert!(
            outer.iter().any(|t| matches!(t, Token::BlockQuote(_))),
            "expected nested BlockQuote inside outer, got {}",
            Token::slice_to_compact(outer)
        );
    }
}

#[cfg(test)]
mod link_escape_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn escape_close_bracket_in_link_text() {
        let tokens = parse(r"[a\]b](http://x)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("a]b".to_string())], url: "http://x".to_string(), title: None }]
        );
    }

    #[test]
    fn escape_close_paren_in_link_url() {
        let tokens = parse(r"[t](http://x\)y)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("t".to_string())], url: "http://x)y".to_string(), title: None }]
        );
    }

    #[test]
    fn escape_backslash_in_link_text() {
        let tokens = parse(r"[a\\b](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("a\\b".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn escape_close_bracket_in_image_alt() {
        let tokens = parse(r"![alt\]more](pic.png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("alt]more".to_string())], url: "pic.png".to_string(), title: None }]
        );
    }

    #[test]
    fn unescaped_link_still_works() {
        let tokens = parse("[foo](http://example.com)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("foo".to_string())], url: "http://example.com".to_string(), title: None }]
        );
    }

    #[test]
    fn balanced_parens_still_work() {
        // Pre-existing balanced-paren handling shouldn't regress.
        let tokens = parse("[Wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("Wiki".to_string())], url: "https://en.wikipedia.org/wiki/Foo_(bar)".to_string(), title: None }]
        );
    }
}

#[cfg(test)]
mod link_entity_decoding_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn entity_in_link_text_decodes() {
        let tokens = parse("[a &amp; b](http://x.com)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("a & b".to_string())], url: "http://x.com".to_string(), title: None }]
        );
    }

    #[test]
    fn numeric_entity_in_link_text_decodes() {
        let tokens = parse("[em &#8212; dash](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("em — dash".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn entity_in_link_url_decodes() {
        let tokens = parse("[link](http://example.com/?a=1&amp;b=2)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("link".to_string())], url: "http://example.com/?a=1&b=2".to_string(), title: None }]
        );
    }

    #[test]
    fn numeric_entity_in_link_url_decodes() {
        let tokens = parse("[t](http://x/&#35;frag)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("t".to_string())], url: "http://x/#frag".to_string(), title: None }]
        );
    }

    #[test]
    fn entity_in_image_alt_decodes() {
        let tokens = parse("![an &amp; alt](pic.png)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("an & alt".to_string())], url: "pic.png".to_string(), title: None }]
        );
    }

    #[test]
    fn entity_in_image_url_decodes() {
        let tokens = parse("![alt](http://x/?q=1&amp;y=2)");
        assert_eq!(
            tokens,
            vec![Token::Image { alt: vec![Token::Text("alt".to_string())], url: "http://x/?q=1&y=2".to_string(), title: None }]
        );
    }

    #[test]
    fn unknown_entity_in_link_text_passes_through() {
        let tokens = parse("[&zzz;](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("&zzz;".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn lone_ampersand_in_link_text_stays_literal() {
        let tokens = parse("[a & b](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("a & b".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn entity_inside_escape_in_link_text() {
        // Escape applies first, entity decoding still works for unescaped chars.
        let tokens = parse(r"[\[ &amp; \]](u)");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("[ & ]".to_string())], url: "u".to_string(), title: None }]
        );
    }

    #[test]
    fn autolink_url_does_not_decode_entities() {
        // autolink URLs are literal — entities preserved verbatim.
        let tokens = parse("<http://x.com/?a=&amp;b>");
        assert_eq!(
            tokens,
            vec![Token::Link { content: vec![Token::Text("http://x.com/?a=&amp;b".to_string())], url: "http://x.com/?a=&amp;b".to_string(), title: None }]
        );
    }

    #[test]
    fn reference_label_with_entity_does_not_resolve() {
        // Per CommonMark, link-label comparison is on RAW source chars
        // (case-folded, whitespace-collapsed). Entity and backslash escapes
        // are NOT decoded before matching — `caf&eacute;` doesn't match a
        // `[café]` definition.
        let tokens = parse("[link][caf&eacute;]\n\n[café]: /u");
        let resolved = tokens.iter().any(|t| {
            matches!(
                t,
                Token::Link { content, url, .. }
                if Token::collect_all_text(content) == "link" && url == "/u"
            )
        });
        assert!(
            !resolved,
            "reference label with entity must not resolve to a literal-char def; got {}",
            Token::slice_to_compact(&tokens)
        );
    }
}

#[cfg(test)]
mod list_lazy_continuation_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn indented_continuation_belongs_to_item() {
        let input = "- item one\n  continues here\n- item two";
        let tokens = parse(input);
        let li_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count();
        assert_eq!(li_count, 2, "got {:?}", tokens);
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("item one"), "got {:?}", text);
            assert!(text.contains("continues here"), "got {:?}", text);
        }
    }

    #[test]
    fn zero_indent_lazy_continuation() {
        // a non-blank, non-marker line at indent 0 still
        // continues the previous item's paragraph.
        let input = "- item one\nlazy line\n- item two";
        let tokens = parse(input);
        let li_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count();
        assert_eq!(li_count, 2, "got {:?}", tokens);
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("lazy line"), "got {:?}", text);
        }
    }

    #[test]
    fn blank_line_ends_list_item() {
        let input = "- item one\n\n- item two";
        let tokens = parse(input);
        let li_count = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count();
        // Two items either way; ensure first item didn't gobble blank.
        assert_eq!(li_count, 2);
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(!text.contains("item two"), "first item should not include second");
        }
    }

    #[test]
    fn heading_line_terminates_item() {
        let input = "- item one\n# heading";
        let tokens = parse(input);
        assert!(tokens.iter().any(|t| matches!(t, Token::Heading(_, 1))));
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(!text.contains("heading"), "heading shouldn't be inside item");
        }
    }

    #[test]
    fn thematic_break_terminates_item() {
        let input = "- item one\n---";
        let tokens = parse(input);
        assert!(
            tokens.iter().any(|t| matches!(t, Token::HorizontalRule)),
            "expected HR, got {:?}",
            tokens
        );
    }

    #[test]
    fn nested_list_still_works() {
        let input = "- Item 1\n  - Nested 1\n  - Nested 2\n- Item 2";
        let tokens = parse(input);
        let top_li = tokens
            .iter()
            .filter(|t| matches!(t, Token::ListItem { .. }))
            .count();
        assert_eq!(top_li, 2, "got {:?}", tokens);
    }

    #[test]
    fn simple_two_items_unchanged() {
        let input = "- a\n- b";
        let tokens = parse(input);
        assert_eq!(
            tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
            2
        );
    }
}

#[cfg(test)]
mod fenced_code_info_string_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn fence(input: &str) -> (String, String) {
        let tokens = parse(input);
        for t in &tokens {
            if let Token::Code { language: lang, content: body, .. } = t {
                return (lang.clone(), body.clone());
            }
        }
        panic!("expected Code token, got {}", Token::slice_to_compact(&tokens));
    }

    #[test]
    fn backtick_fence_simple_language() {
        let (lang, _) = fence("```rust\nfn x() {}\n```");
        assert_eq!(lang, "rust");
    }

    #[test]
    fn backtick_fence_language_with_trailing_metadata() {
        let (lang, _) = fence("```rust title=\"example\" linenos\nfn x() {}\n```");
        assert_eq!(lang, "rust", "info-string metadata must not be in language");
    }

    #[test]
    fn backtick_fence_empty_info_string() {
        let (lang, _) = fence("```\nplain\n```");
        assert_eq!(lang, "");
    }

    #[test]
    fn backtick_fence_whitespace_only_info_string() {
        let (lang, _) = fence("```   \nplain\n```");
        assert_eq!(lang, "");
    }

    #[test]
    fn backtick_fence_language_trimmed() {
        let (lang, _) = fence("```   rust   \ncode\n```");
        assert_eq!(lang, "rust");
    }

    #[test]
    fn tilde_fence_simple_language() {
        let (lang, _) = fence("~~~python\nprint('hi')\n~~~");
        assert_eq!(lang, "python");
    }

    #[test]
    fn tilde_fence_language_with_metadata() {
        let (lang, _) = fence("~~~ts strict=true\ntype A = number;\n~~~");
        assert_eq!(lang, "ts");
    }

    #[test]
    fn tilde_fence_allows_backticks_in_info_string() {
        let (lang, _) = fence("~~~`backticks` allowed here\ncontent\n~~~");
        assert_eq!(lang, "`backticks`");
    }

    #[test]
    fn tilde_fence_empty_info_string() {
        let (lang, _) = fence("~~~\nplain\n~~~");
        assert_eq!(lang, "");
    }

    #[test]
    fn backtick_fence_with_backticks_in_info_string_is_inline_span() {
        // a backtick fence's info string may not contain any
        // backticks — so this opens an inline span instead. Discriminator:
        // a real fence would put `body` in content with the info string
        // dropped; the inline-span fallback includes the info-string text
        // (`bad` literal) inside the span body.
        let tokens = parse("``` `bad` info\nbody\n```");
        let inline_span_with_info_text = tokens.iter().any(|t| {
            matches!(t, Token::Code { content: body, .. } if body.contains("bad"))
        });
        assert!(
            inline_span_with_info_text,
            "expected inline span carrying info-string text, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn fence_body_unchanged_by_info_string_split() {
        let (lang, body) = fence("```rust meta1 meta2\nlet x = 1;\nlet y = 2;\n```");
        assert_eq!(lang, "rust");
        assert!(body.contains("let x = 1;"));
        assert!(body.contains("let y = 2;"));
    }
}

#[cfg(test)]
mod loose_tight_list_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    fn items(tokens: &[Token]) -> Vec<bool> {
        tokens
            .iter()
            .filter_map(|t| {
                if let Token::ListItem { loose, .. } = t {
                    Some(*loose)
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn tight_bullet_list_marks_no_items_loose() {
        let tokens = parse("- a\n- b\n- c");
        assert_eq!(items(&tokens), vec![false, false, false]);
    }

    #[test]
    fn blank_line_between_items_marks_list_loose() {
        let tokens = parse("- a\n\n- b\n\n- c");
        assert_eq!(items(&tokens), vec![true, true, true]);
    }

    #[test]
    fn single_blank_anywhere_makes_whole_list_loose() {
        // Spec: even one blank-separated pair makes ALL items loose.
        let tokens = parse("- a\n- b\n\n- c");
        assert_eq!(items(&tokens), vec![true, true, true]);
    }

    #[test]
    fn tight_ordered_list() {
        let tokens = parse("1. one\n2. two\n3. three");
        assert_eq!(items(&tokens), vec![false, false, false]);
    }

    #[test]
    fn loose_ordered_list() {
        let tokens = parse("1. one\n\n2. two");
        assert_eq!(items(&tokens), vec![true, true]);
    }

    #[test]
    fn single_item_list_is_tight() {
        let tokens = parse("- solo");
        assert_eq!(items(&tokens), vec![false]);
    }

    #[test]
    fn tight_nested_list_keeps_inner_tight() {
        let input = "- outer1\n  - in1\n  - in2\n- outer2";
        let tokens = parse(input);
        assert_eq!(items(&tokens), vec![false, false]);
        if let Token::ListItem { content, .. } = &tokens[0] {
            assert_eq!(items(content), vec![false, false], "inner: {:?}", content);
        } else {
            panic!("expected ListItem");
        }
    }

    #[test]
    fn nested_list_blank_makes_both_levels_loose() {
        // Spec: a list is loose if any item directly contains two block-level
        // elements with a blank line between them. The two inner ListItems
        // inside outer1 are separated by a blank, so the inner list AND the
        // outer list are both loose.
        let input = "- outer1\n  - inner1\n\n  - inner2\n- outer2";
        let tokens = parse(input);
        assert_eq!(
            items(&tokens),
            vec![true, true],
            "outer: {}",
            Token::slice_to_compact(&tokens)
        );
        if let Token::ListItem { content, .. } = &tokens[0] {
            let inner = items(content);
            assert_eq!(
                inner,
                vec![true, true],
                "inner items: {}",
                Token::slice_to_compact(content)
            );
        } else {
            panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn outer_loose_inner_tight() {
        let input = "- outer1\n  - in1\n  - in2\n\n- outer2";
        let tokens = parse(input);
        assert_eq!(items(&tokens), vec![true, true]);
        if let Token::ListItem { content, .. } = &tokens[0] {
            let inner = items(content);
            assert_eq!(inner, vec![false, false], "inner items: {:?}", content);
        } else {
            panic!("expected ListItem");
        }
    }

    #[test]
    fn list_in_blockquote_loose_detected() {
        let input = "> - a\n>\n> - b";
        let tokens = parse(input);
        if let Token::BlockQuote(body) = &tokens[0] {
            assert_eq!(
                items(body),
                vec![true, true],
                "quote body: {}",
                Token::slice_to_compact(body)
            );
        } else {
            panic!("expected BlockQuote, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn two_separate_lists_each_have_own_loose_flag() {
        // A blank line followed by content that isn't another item ends the
        // list. The next list starts fresh.
        let input = "- a\n- b\n\nparagraph\n\n- c\n\n- d";
        let tokens = parse(input);
        let item_states: Vec<bool> = tokens
            .iter()
            .filter_map(|t| {
                if let Token::ListItem { loose, .. } = t {
                    Some(*loose)
                } else {
                    None
                }
            })
            .collect();
        // First list (a, b): tight. Second list (c, d): loose.
        assert_eq!(item_states, vec![false, false, true, true]);
    }

    #[test]
    fn task_list_loose_detection() {
        let input = "- [ ] task1\n\n- [x] task2";
        let tokens = parse(input);
        assert_eq!(items(&tokens), vec![true, true]);
    }
}

#[cfg(test)]
mod tab_indentation_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn leading_tab_is_indented_code_block() {
        // A leading tab expands to 4 columns → indented code block.
        let tokens = parse("\tfoo");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
            "expected indented code block, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn two_spaces_plus_tab_is_indented_code_block() {
        // 2 spaces, then tab → tab expands to next multiple of 4 = col 4
        // total = 4 columns of indent → indented code block.
        let tokens = parse("  \tfoo");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
            "expected indented code block, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn three_spaces_plus_tab_is_indented_code_block() {
        // 3 spaces + tab → tab fills cols 3-4 → 4 columns → indented code.
        let tokens = parse("   \tfoo");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
            "expected indented code block, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn three_leading_spaces_no_tab_keeps_heading() {
        // 3 spaces of indent before `#` is still a heading.
        let tokens = parse("   # heading");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Heading(_, 1))),
            "expected Heading, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn one_space_plus_tab_before_hash_is_indented_code() {
        // 1 space + tab → 4 columns → indented code, NOT heading.
        let tokens = parse(" \t# not a heading");
        assert!(
            !tokens.iter().any(|t| matches!(t, Token::Heading(_, _))),
            "unexpected Heading, got {}",
            Token::slice_to_compact(&tokens)
        );
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Code { .. })),
            "expected indented code, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn tab_after_blockquote_marker_is_content_padding() {
        // `>\tfoo` — tab after `>` is content-side padding, so the body is
        // paragraph "foo", not indented code.
        let tokens = parse(">\tfoo");
        if let Token::BlockQuote(body) = &tokens[0] {
            let text = Token::collect_all_text(body);
            assert!(text.contains("foo"), "got {:?}", text);
            // The quote body should NOT contain a code block.
            assert!(
                !body.iter().any(|t| matches!(t, Token::Code { .. })),
                "unexpected code in quote body: {}",
                Token::slice_to_compact(body)
            );
        } else {
            panic!("expected BlockQuote, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn tab_after_list_marker_is_content_padding() {
        // `-\tfoo` — tab after the bullet is item-content padding, content="foo".
        let tokens = parse("-\tfoo");
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo"), "got {:?}", text);
        } else {
            panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn four_spaces_is_indented_code() {
        let tokens = parse("    foo");
        assert!(
            tokens.iter().any(|t| matches!(t, Token::Code { content: body, .. } if body.contains("foo"))),
            "expected indented code, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn three_spaces_no_tab_is_paragraph() {
        // 3 spaces, no tab → only 3 columns of indent → still a paragraph.
        let tokens = parse("   foo");
        assert!(
            !tokens.iter().any(|t| matches!(t, Token::Code { .. })),
            "unexpected code, got {}",
            Token::slice_to_compact(&tokens)
        );
    }

    #[test]
    fn tab_inside_paragraph_preserved() {
        // A tab not at line start is just literal text content.
        let tokens = parse("a\tb");
        let text = Token::collect_all_text(&tokens);
        assert!(text.contains("a"), "got {:?}", text);
        assert!(text.contains("b"), "got {:?}", text);
    }
}

#[cfg(test)]
mod multi_paragraph_list_item_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn bullet_item_with_two_paragraphs() {
        let tokens = parse("- foo\n\n  bar");
        assert_eq!(
            tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
            1,
            "expected exactly one list item, got {}",
            Token::slice_to_compact(&tokens)
        );
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo"), "got {:?}", text);
            assert!(
                text.contains("bar"),
                "second paragraph must be inside the item: {:?}",
                text
            );
        } else {
            panic!("expected ListItem, got {}", Token::slice_to_compact(&tokens));
        }
    }

    #[test]
    fn under_indented_continuation_starts_top_level_paragraph() {
        // `bar` is at column 0 (no indent) — must NOT be inside the item.
        let tokens = parse("- foo\n\nbar");
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("foo"));
            assert!(!text.contains("bar"), "bar leaked into item: {:?}", text);
        }
        // bar must appear as a separate top-level token.
        let after = Token::collect_all_text(&tokens[1..]);
        assert!(after.contains("bar"), "bar missing from rest");
    }

    #[test]
    fn bullet_item_with_blank_makes_list_loose() {
        // The blank-line-between-paragraphs inside an item makes the list
        // loose per spec ("any item directly contains two block-level
        // elements with a blank line between them").
        let tokens = parse("- foo\n\n  bar\n- second");
        let loose_flags: Vec<bool> = tokens
            .iter()
            .filter_map(|t| {
                if let Token::ListItem { loose, .. } = t {
                    Some(*loose)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(loose_flags, vec![true, true], "got {}", Token::slice_to_compact(&tokens));
    }

    #[test]
    fn ordered_item_with_two_paragraphs() {
        // For ordered `1. ` the content offset is col 3.
        let tokens = parse("1. first\n\n   second");
        assert_eq!(
            tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
            1,
            "got {}",
            Token::slice_to_compact(&tokens)
        );
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert!(text.contains("first") && text.contains("second"), "got {:?}", text);
        }
    }

    #[test]
    fn item_with_only_one_paragraph_unchanged() {
        // Regression: single-paragraph items must not change shape.
        let tokens = parse("- only");
        assert_eq!(
            tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
            1
        );
        if let Token::ListItem { content, loose, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            assert_eq!(text, "only");
            assert!(!loose);
        }
    }

    #[test]
    fn three_paragraphs_in_one_item() {
        let tokens = parse("- a\n\n  b\n\n  c");
        assert_eq!(
            tokens.iter().filter(|t| matches!(t, Token::ListItem { .. })).count(),
            1,
            "got {}",
            Token::slice_to_compact(&tokens)
        );
        if let Token::ListItem { content, .. } = &tokens[0] {
            let text = Token::collect_all_text(content);
            for needle in &["a", "b", "c"] {
                assert!(text.contains(needle), "{:?} missing from {:?}", needle, text);
            }
        }
    }
}

#[cfg(test)]
mod link_inline_content_tests {
    use super::*;

    fn parse(input: &str) -> Vec<Token> {
        let mut lexer = Lexer::new(input.to_string());
        lexer.parse().unwrap()
    }

    #[test]
    fn link_text_parses_emphasis() {
        let tokens = parse("[*emph* text](u)");
        let Token::Link { content, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert!(
            content.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "expected Emphasis inside link text, got {}",
            Token::slice_to_compact(content)
        );
    }

    #[test]
    fn link_text_parses_strong_emphasis() {
        // `**bold**` produces Emphasis with level 2 in this lexer (not a
        // separate StrongEmphasis token).
        let tokens = parse("[**bold** link](u)");
        let Token::Link { content, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert!(
            content
                .iter()
                .any(|t| matches!(t, Token::Emphasis { level: 2, .. })),
            "expected Emphasis level=2, got {}",
            Token::slice_to_compact(content)
        );
    }

    #[test]
    fn link_text_parses_code_span() {
        let tokens = parse("[`code` snippet](u)");
        let Token::Link { content, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert!(
            content
                .iter()
                .any(|t| matches!(t, Token::Code { content: body, .. } if body == "code")),
            "expected Code span, got {}",
            Token::slice_to_compact(content)
        );
    }

    #[test]
    fn link_text_decodes_entities() {
        let tokens = parse("[a &amp; b](u)");
        let Token::Link { content, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        let text = Token::collect_all_text(content);
        assert_eq!(text, "a & b");
    }

    #[test]
    fn link_text_honors_backslash_escape() {
        let tokens = parse(r"[a\*not emph\*](u)");
        let Token::Link { content, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        // No Emphasis should have been produced — escapes blocked it.
        assert!(
            !content.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "escape should have blocked emphasis, got {}",
            Token::slice_to_compact(content)
        );
        let text = Token::collect_all_text(content);
        assert_eq!(text, "a*not emph*");
    }

    #[test]
    fn image_alt_parses_inline_formatting() {
        let tokens = parse("![*alt* text](pic.png)");
        let Token::Image { alt, .. } = &tokens[0] else {
            panic!("expected Image, got {}", Token::slice_to_compact(&tokens));
        };
        assert!(
            alt.iter().any(|t| matches!(t, Token::Emphasis { .. })),
            "expected Emphasis in alt, got {}",
            Token::slice_to_compact(alt)
        );
    }

    #[test]
    fn link_title_escape_double_quote() {
        // `\"` inside a double-quoted title produces a literal `"`.
        let tokens = parse(r#"[t](u "a\"b")"#);
        let Token::Link { title, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert_eq!(title.as_deref(), Some("a\"b"));
    }

    #[test]
    fn link_title_entity_decoded() {
        let tokens = parse(r#"[t](u "a &amp; b")"#);
        let Token::Link { title, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert_eq!(title.as_deref(), Some("a & b"));
    }

    #[test]
    fn link_title_with_paren_delimiter_escaped_close() {
        let tokens = parse(r"[t](u (in\) title))");
        let Token::Link { title, .. } = &tokens[0] else {
            panic!("expected Link, got {}", Token::slice_to_compact(&tokens));
        };
        assert_eq!(title.as_deref(), Some("in) title"));
    }

    #[test]
    fn autolink_keeps_url_as_link_text() {
        let tokens = parse("<https://example.com>");
        let Token::Link { content, url, title } = &tokens[0] else {
            panic!("expected autolink, got {}", Token::slice_to_compact(&tokens));
        };
        assert_eq!(Token::collect_all_text(content), "https://example.com");
        assert_eq!(url, "https://example.com");
        assert!(title.is_none());
    }
}
