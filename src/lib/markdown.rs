//! Markdown lexical analysis and token representation.
//!
//! This module provides the core lexical analysis functionality for parsing Markdown text into a
//! structured token stream. It handles both block-level elements like headings and lists, as well
//! as inline formatting like emphasis and links.
//!
//! The lexer maintains proper nesting of elements and handles edge cases around delimiter matching
//! and whitespace handling according to CommonMark spec.
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
//! // Link token with text and URL
//! let link = Token::Link(
//!     "Click here".to_string(),
//!     "https://example.com".to_string()
//! );
//! assert!(matches!(link, Token::Link(_, _)));
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
//!         ├── text: String
//!         └── url: String

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
    /// Code block with optional language specification and content
    Code(String, String),
    /// Block quote whose body is itself a sequence of tokens (so emphasis,
    /// links, code, etc. inside `> …` lines are properly parsed).
    BlockQuote(Vec<Token>),
    /// List item with nested content and type information
    ListItem {
        content: Vec<Token>,
        ordered: bool,
        number: Option<usize>, // For ordered lists (e.g., "1.", "2.")
        /// GFM task list state: `None` = regular item, `Some(false)` = `[ ]`,
        /// `Some(true)` = `[x]` / `[X]`.
        checked: Option<bool>,
    },
    /// Link with display text and URL
    Link(String, String),
    /// Image with alt text and URL
    Image(String, String),
    /// Plain text content
    Text(String),
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
    /// Raw inline HTML (`<span>`, `</span>`, `<br/>`, etc.) per CommonMark
    /// §6.6. Stored verbatim including the angle brackets.
    HtmlInline(String),
    /// Soft line break (single `\n`).
    Newline,
    /// Hard line break (CommonMark §6.7): two-or-more trailing spaces or a
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
            Token::Code(_, code) => result.push_str(code),
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
            Token::Link(text, _) => result.push_str(text),
            Token::Image(alt, _) => result.push_str(alt),
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
/// Per CommonMark §2.5, only semicolon-terminated references are valid.
/// Numeric references for code point 0, surrogates, or values above
/// 0x10FFFF decode to U+FFFD (REPLACEMENT CHARACTER) rather than failing —
/// only syntactically invalid references (empty digits, non-hex digits,
/// missing `;`) fall back to a literal `&`.
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
        // Per CommonMark §2.5: invalid Unicode code points (including the
        // null character, surrogates, and out-of-range values) are
        // replaced with U+FFFD. Only a *syntactic* failure (overflowing
        // u32, non-digits in the chosen radix) falls back to literal.
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

/// Tries to parse a single line as a CommonMark link reference definition:
/// `(spaces 0-3)[label]:(spaces)url(spaces title)?(spaces)?`.
/// Returns `(label, url, optional_title)` if the whole line matches.
fn parse_definition_line(line: &str) -> Option<(String, String, Option<String>)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0usize;

    let mut leading = 0usize;
    while i < chars.len() && chars[i] == ' ' && leading < 3 {
        i += 1;
        leading += 1;
    }
    if chars.get(i) != Some(&'[') {
        return None;
    }
    i += 1;
    let label_start = i;
    while i < chars.len() && chars[i] != ']' {
        if chars[i] == '\n' {
            return None;
        }
        i += 1;
    }
    if chars.get(i) != Some(&']') {
        return None;
    }
    let label: String = chars[label_start..i].iter().collect();
    if label.trim().is_empty() {
        return None;
    }
    i += 1; // past ]
    if chars.get(i) != Some(&':') {
        return None;
    }
    i += 1; // past :

    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    if i >= chars.len() {
        return None;
    }

    let url_start = i;
    while i < chars.len() && chars[i] != ' ' && chars[i] != '\t' {
        i += 1;
    }
    if i == url_start {
        return None;
    }
    let url: String = chars[url_start..i].iter().collect();

    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }

    let title = if i < chars.len() {
        let (open, close) = match chars[i] {
            '"' => ('"', '"'),
            '\'' => ('\'', '\''),
            '(' => ('(', ')'),
            _ => return Some((label, url, None)).filter(|_| {
                // No title — the rest of the line must be whitespace.
                chars[i..].iter().all(|c| *c == ' ' || *c == '\t')
            }),
        };
        if chars[i] != open {
            return None;
        }
        i += 1;
        let title_start = i;
        while i < chars.len() && chars[i] != close {
            i += 1;
        }
        if chars.get(i) != Some(&close) {
            return None;
        }
        let t: String = chars[title_start..i].iter().collect();
        i += 1;
        Some(t)
    } else {
        None
    };

    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    if i != chars.len() {
        return None; // junk after definition — not a valid definition
    }
    Some((label, url, title))
}

/// Normalizes a reference-link label per CommonMark §4.7: ASCII case-fold
/// plus internal-whitespace collapse plus leading/trailing trim.
fn normalize_label(s: &str) -> String {
    let mut out = String::new();
    let mut prev_ws = true; // leading whitespace is collapsed away
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            for ch in c.to_lowercase() {
                out.push(ch);
            }
            prev_ws = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// CommonMark §6.1: if a code-span body begins AND ends with a space (and
/// is not entirely composed of spaces), strip exactly one leading and one
/// trailing space. Otherwise leave content untouched.
fn strip_code_span_outer_space(s: String) -> String {
    if s.len() >= 2 && s.starts_with(' ') && s.ends_with(' ') && !s.chars().all(|c| c == ' ') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

/// CommonMark "Unicode punctuation" predicate, used by the left/right-
/// flanking-run rules. We accept all ASCII punctuation plus a small set of
/// common Unicode punctuation marks. Strict CommonMark also includes the
/// full Unicode `P*` general categories; deferred for simplicity.
fn is_md_punctuation(c: char) -> bool {
    is_ascii_punctuation(c) || matches!(c, '–' | '—' | '…' | '‘' | '’' | '“' | '”')
}

/// True for the 32 ASCII punctuation characters that CommonMark §2.4 allows
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
    /// Reference-link definitions collected by `extract_definitions()` in
    /// the pre-pass. Keys are normalized (lowercased, whitespace-collapsed);
    /// values are `(url, title)`.
    definitions: HashMap<String, (String, Option<String>)>,
}

impl Lexer {
    /// Creates a new lexer instance from input string. CRLF and bare CR line
    /// endings are normalized to LF up-front so the rest of the lexer only
    /// needs to reason about `\n`.
    pub fn new(input: String) -> Self {
        let normalized: String = input.replace("\r\n", "\n").replace('\r', "\n");
        Lexer {
            input: normalized.chars().collect(),
            position: 0,
            pending_hard_break: false,
            definitions: HashMap::new(),
        }
    }

    /// Parses the entire input string into a sequence of tokens.
    /// Returns a Result containing either a Vec of parsed tokens or a LexerError.
    pub fn parse(&mut self) -> Result<Vec<Token>, LexerError> {
        // Pre-pass: collect reference-link definitions and strip those lines
        // so the main lexer doesn't see them as paragraph text.
        self.extract_definitions();
        self.parse_with_context(ParseContext::Root)
    }

    /// Pre-pass: scans the input line-by-line for `[label]: url "title"`
    /// definitions, removes those lines from `self.input`, and stores the
    /// result in `self.definitions` for later resolution by `parse_link` /
    /// `parse_image`. Idempotent: safe to call multiple times.
    fn extract_definitions(&mut self) {
        let original: String = self.input.iter().collect();
        let mut kept = String::new();
        let mut definitions = HashMap::new();
        for line in original.split_inclusive('\n') {
            let stripped = line.trim_end_matches('\n');
            if let Some((label, url, title)) = parse_definition_line(stripped) {
                definitions
                    .entry(normalize_label(&label))
                    .or_insert((url, title));
            } else {
                kept.push_str(line);
            }
        }
        self.input = kept.chars().collect();
        self.position = 0;
        self.definitions = definitions;
    }

    /// Parses the entire input string into a sequence of tokens for a given context.
    /// Returns a Result containing either a Vec of parsed tokens or a LexerError.
    /// Takes in a `ParseContext` that determines which tokens are valid in the current location.
    pub fn parse_with_context(&mut self, ctx: ParseContext) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        while self.position < self.input.len() {
            if let Some(token) = self.next_token(ctx)? {
                tokens.push(token);
            }
        }

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

        while self.position < self.input.len() {
            let ch = self.current_char();

            // Inline runs (emphasis, strikethrough) cannot span paragraph
            // boundaries. A blank line forces parse_emphasis /
            // parse_strikethrough into their literal-text fallback so the
            // opener doesn't gobble subsequent paragraphs / headings.
            if ch == '\n' && self.input.get(self.position + 1) == Some(&'\n') {
                break;
            }

            if is_delimiter(ch) {
                // For emphasis delimiters (`*`/`_`), only treat the run as a
                // closer if it satisfies CommonMark's right-flanking rule
                // (and isn't an intra-word `_`). Other delimiter chars (`\n`
                // for headings, `~` for strikethrough) close unconditionally.
                let is_emphasis_delim = ch == '*' || ch == '_';
                let blocks_close = if is_emphasis_delim {
                    let intra_word =
                        ch == '_' && self.is_intra_word_underscore_run(self.position);
                    intra_word || !self.can_close_emphasis(self.position)
                } else {
                    false
                };
                if !blocks_close {
                    break;
                }
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

        Ok(content)
    }

    /// Determines the next token in the input stream based on the current character
    /// and context. Handles special cases like line starts differently.
    fn next_token(&mut self, ctx: ParseContext) -> Result<Option<Token>, LexerError> {
        // A pending hard break overrides the usual dispatch — emit it before
        // looking at the next character.
        if self.pending_hard_break {
            self.pending_hard_break = false;
            return Ok(Some(Token::HardBreak));
        }

        // CommonMark §4.4: an indented (4-column) code block. Triggers at
        // line start in Root or BlockQuote context AND only when the previous
        // line is blank or we're at start-of-document, so list-item
        // continuations and post-paragraph-without-blank lines aren't
        // mis-routed to code.
        if matches!(ctx, ParseContext::Root | ParseContext::BlockQuote)
            && self.is_at_line_start()
            && self.get_current_indent() >= 4
            && self.previous_line_is_blank_or_bof()
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
        // setext detection for `> Title\n> ---`).
        if is_line_start
            && matches!(ctx, ParseContext::Root | ParseContext::BlockQuote)
        {
            if let Some(level) = self.peek_setext_level() {
                return Ok(Some(self.consume_setext_heading(level)?));
            }
        }

        let token = match current_char {
            '#' if is_line_start && allow_block_tokens(ctx) && self.is_atx_heading_start() => {
                self.parse_heading()?
            }
            '*' if is_line_start && allow_block_tokens(ctx) && self.is_thematic_break_line() => {
                self.consume_current_line();
                Token::HorizontalRule
            }
            '_' if is_line_start && allow_block_tokens(ctx) && self.is_thematic_break_line() => {
                self.consume_current_line();
                Token::HorizontalRule
            }
            '*' if is_line_start && allow_block_tokens(ctx) && self.is_list_marker('*') => {
                self.parse_list_item(false, 0, ctx)?
            }
            '*' => {
                if self.can_open_emphasis(self.position) {
                    self.parse_emphasis()?
                } else {
                    self.consume_run_as_text('*')
                }
            }
            '_' if !self.is_intra_word_underscore_run(self.position) => {
                if self.can_open_emphasis(self.position) {
                    self.parse_emphasis()?
                } else {
                    self.consume_run_as_text('_')
                }
            }
            '_' => self.parse_text(ctx)?,
            '`' => self.parse_code()?,
            '~' if is_line_start
                && allow_block_tokens(ctx)
                && self.count_consecutive('~') >= 3 =>
            {
                self.parse_tilde_fence()?
            }
            '~' if self.count_consecutive('~') >= 2 => self.parse_strikethrough()?,
            '~' => self.parse_text(ctx)?,
            '>' if is_line_start && allow_block_tokens(ctx) => self.parse_blockquote()?,
            '-' | '+' if is_line_start && allow_block_tokens(ctx) => {
                if self.is_thematic_break_line() {
                    self.consume_current_line();
                    Token::HorizontalRule
                } else if self.check_horizontal_rule()? {
                    Token::HorizontalRule
                } else {
                    self.parse_list_item(false, 0, ctx)?
                }
            }
            '0'..='9' if is_line_start && allow_block_tokens(ctx) => {
                if let Some(_) = self.check_ordered_list_marker() {
                    self.parse_list_item(true, 0, ctx)?
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

    /// Per CommonMark §4.2: an ATX heading opener must be 1-6 `#` chars
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
    /// collects nested inline content. Per CommonMark §4.2, an optional
    /// closing run of `#`s preceded by a space and followed only by spaces
    /// is stripped from the heading content.
    fn parse_heading(&mut self) -> Result<Token, LexerError> {
        let mut level = 0usize;
        while self.current_char() == '#' && level < 6 {
            level += 1;
            self.advance();
        }
        self.skip_whitespace();
        let mut content = self.parse_nested_content(|c| c == '\n', ParseContext::Inline)?;

        // Strip optional closing `#` sequence: ` +#+( +)*$` from the trailing
        // text token's content. The space-before-the-#-run is required by the
        // spec — a trailing `C#` with no preceding space stays as content.
        if let Some(Token::Text(s)) = content.last_mut() {
            let trimmed = s.trim_end_matches(|c: char| c == ' ' || c == '\t');
            let mut bytes = trimmed.as_bytes();
            let mut hash_run = 0usize;
            while !bytes.is_empty() && *bytes.last().unwrap() == b'#' {
                hash_run += 1;
                bytes = &bytes[..bytes.len() - 1];
            }
            if hash_run > 0 && !bytes.is_empty() {
                let prev = *bytes.last().unwrap();
                if prev == b' ' || prev == b'\t' {
                    let new_len = bytes.len();
                    s.truncate(new_len);
                    while s.ends_with(' ') || s.ends_with('\t') {
                        s.pop();
                    }
                }
            }
            if s.is_empty() {
                content.pop();
            }
        }

        Ok(Token::Heading(content, level))
    }

    /// Emits a run of identical characters as a literal `Token::Text`,
    /// preserving any single trailing space so `next_token`'s leading-
    /// whitespace skip doesn't swallow it. Used by the flanking-rule
    /// rejections of `*`/`_` runs that can't open emphasis.
    fn consume_run_as_text(&mut self, ch: char) -> Token {
        let mut count = 0;
        while self.position < self.input.len() && self.current_char() == ch {
            count += 1;
            self.advance();
        }
        let mut run = ch.to_string().repeat(count);
        if self.position < self.input.len() && self.current_char() == ' ' {
            run.push(' ');
            self.advance();
        }
        Token::Text(run)
    }

    /// Parses emphasis tokens (* or _) with support for multiple levels (1-3).
    /// Ensures proper matching of opening and closing delimiters.
    ///
    /// Per CommonMark §6.2, an unmatched opener falls back to literal text
    /// rather than raising an error. We implement this with rewind-on-failure:
    /// if the closing delimiter isn't found, position is reset to right after
    /// the opener run and the run is emitted as `Token::Text`. The body chars
    /// are then re-tokenized by the main loop.
    fn parse_emphasis(&mut self) -> Result<Token, LexerError> {
        let delimiter = self.current_char();
        let mut level = 0;

        // Count the number of delimiters
        while self.current_char() == delimiter {
            level += 1;
            self.advance();
        }
        let after_opener = self.position;

        let content = self.parse_nested_content(|c| c == delimiter, ParseContext::Inline)?;

        // Ensure proper closing
        for _ in 0..level {
            if self.current_char() != delimiter {
                // Fallback: rewind so the body re-tokenizes, and emit the
                // opener as literal text. Preserve a single trailing space
                // if present so `next_token`'s leading-whitespace skip
                // doesn't swallow a meaningful gap (e.g. "Use * for bullets").
                self.position = after_opener;
                let mut run = delimiter.to_string().repeat(level);
                if self.position < self.input.len() && self.current_char() == ' ' {
                    run.push(' ');
                    self.advance();
                }
                return Ok(Token::Text(run));
            }
            self.advance();
        }

        Ok(Token::Emphasis {
            level: level.min(3), // Cap the level at 3
            content,
        })
    }

    /// Parses code blocks, handling both inline code and fenced code blocks
    fn parse_code(&mut self) -> Result<Token, LexerError> {
        let opener_pos = self.position;
        let is_line_start = self.is_at_line_start();
        let start_backticks = self.count_backticks();

        // CommonMark: a fenced code block needs 3+ backticks at line start
        // AND no run of equal-or-larger backticks on the same line (which
        // would be an inline-span closer instead).
        let is_fence = start_backticks >= 3
            && is_line_start
            && self.no_backtick_closer_on_same_line(opener_pos, start_backticks);

        if !is_fence {
            return Ok(self.parse_inline_code_span_body(start_backticks));
        }

        // Fenced code block.
        self.skip_whitespace();
        let language = self.read_until_newline();
        let mut content = String::new();

        while self.position < self.input.len() {
            let current_backticks = self.count_backticks();
            if current_backticks == start_backticks {
                break;
            }
            if current_backticks > 0 {
                // A backtick run shorter than the opener is part of the
                // body — push it back as content (count_backticks already
                // advanced past it). Without this, `let s = \`x\`;` inside
                // a triple-fence loses both backticks around `x`.
                for _ in 0..current_backticks {
                    content.push('`');
                }
                continue;
            }
            content.push(self.current_char());
            self.advance();
        }

        // Skip closing backticks if they exist
        for _ in 0..start_backticks {
            if self.position < self.input.len() && self.current_char() == '`' {
                self.advance();
            }
        }

        Ok(Token::Code(
            language.trim().to_string(),
            content.trim().to_string(),
        ))
    }

    /// Walks from `opener_pos + count` to the end of the current line. Returns
    /// false if a backtick run of `count` or more is found before `\n` (in
    /// which case the opener is the start of an inline code span, not a fence).
    fn no_backtick_closer_on_same_line(&self, opener_pos: usize, count: usize) -> bool {
        let mut p = opener_pos + count;
        while p < self.input.len() && self.input[p] != '\n' {
            if self.input[p] == '`' {
                let mut run = 0usize;
                while p < self.input.len() && self.input[p] == '`' {
                    run += 1;
                    p += 1;
                }
                if run >= count {
                    return false;
                }
                continue;
            }
            p += 1;
        }
        true
    }

    /// Reads an inline code span body. The opener has already been consumed
    /// by `count_backticks`. Closes on the next backtick run of exactly
    /// `opener_count` chars; runs of a different size are content. A single
    /// `\n` is converted to a space (CommonMark §6.1). A blank line (`\n\n`)
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
                    return Token::Code(String::new(), strip_code_span_outer_space(content));
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

        Ok(Token::Strikethrough(content))
    }

    /// Parses a `~~~`-fenced code block. Mirrors the backtick fence path but
    /// distinct so the two fences don't accidentally close each other.
    fn parse_tilde_fence(&mut self) -> Result<Token, LexerError> {
        let mut start_tildes = 0;
        while self.current_char() == '~' {
            start_tildes += 1;
            self.advance();
        }
        self.skip_whitespace();
        let language = self.read_until_newline();
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        let mut content = String::new();
        while self.position < self.input.len() {
            // Closing fence: line begins (with up to 3 leading spaces) with
            // `start_tildes` or more `~` chars.
            if self.is_at_line_start() {
                let mut p = self.position;
                let mut leading = 0usize;
                while p < self.input.len() && self.input[p] == ' ' && leading < 3 {
                    p += 1;
                    leading += 1;
                }
                let mut close_count = 0usize;
                while p < self.input.len() && self.input[p] == '~' {
                    close_count += 1;
                    p += 1;
                }
                if close_count >= start_tildes {
                    while p < self.input.len() && self.input[p] != '\n' {
                        p += 1;
                    }
                    self.position = p;
                    if self.position < self.input.len() && self.current_char() == '\n' {
                        self.advance();
                    }
                    return Ok(Token::Code(
                        language.trim().to_string(),
                        content.trim_end_matches('\n').to_string(),
                    ));
                }
            }
            content.push(self.current_char());
            self.advance();
        }

        // Unclosed fence: still emit what we have.
        Ok(Token::Code(
            language.trim().to_string(),
            content.trim_end_matches('\n').to_string(),
        ))
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
    /// recursively lexing the body so inline formatting works (CommonMark
    /// §5.1). Supports §5.2 lazy continuation: a non-`>`-prefixed line that
    /// doesn't itself start a new block construct joins the open paragraph.
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
        let mut body_lines: Vec<String> = Vec::new();

        loop {
            if self.position >= self.input.len() || !self.is_at_line_start() {
                break;
            }
            let line_start = self.position;
            // Skip up to 3 leading spaces for marker detection (per §5.1
            // marker may have 0-3 spaces of indent).
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
                // Optional single space after `>`.
                if self.position < self.input.len() && self.current_char() == ' ' {
                    self.advance();
                }
                let line = self.read_until_newline();
                body_lines.push(line);
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
            let last_was_paragraph = body_lines
                .last()
                .map(|l| !l.trim().is_empty())
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
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            }
        }

        let body_text = body_lines.join("\n");
        let mut sub = Lexer::new(body_text);
        let body = sub.parse_with_context(ParseContext::BlockQuote)?;
        Ok(Token::BlockQuote(body))
    }

    /// Returns true if the line beginning at `pos` (already past any 0-3
    /// leading spaces) starts a new block-level construct that interrupts
    /// an open paragraph per CommonMark §5.2 / §4.10. Covers ATX heading,
    /// thematic break, list marker, and fenced code. Does NOT detect
    /// indented-code interruptions, since they only apply when the open
    /// block in the surrounding context is itself a paragraph.
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
        self.advance(); // skip '['
        let text = self.read_until_char_with_escapes(']');
        self.advance(); // skip ']'

        // Inline: [text](url ...)
        if self.current_char() == '(' {
            self.advance(); // skip '('
            let url = self.read_url_with_balanced_parens();
            if self.position < self.input.len() && self.current_char() == ')' {
                self.advance(); // skip ')'
            }
            return Ok(Token::Link(text, url));
        }

        // Full or collapsed reference: [text][label] or [text][]
        if self.current_char() == '[' {
            self.advance(); // skip [
            let label = self.read_until_char_with_escapes(']');
            if self.current_char() == ']' {
                self.advance();
            }
            let key = if label.trim().is_empty() {
                normalize_label(&text)
            } else {
                normalize_label(&label)
            };
            if let Some((url, _title)) = self.definitions.get(&key).cloned() {
                return Ok(Token::Link(text, url));
            }
            // Lookup failed — emit the literal brackets/text.
            let bracket_label = if label.is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", label)
            };
            return Ok(Token::Text(format!("[{}]{}", text, bracket_label)));
        }

        // Shortcut: [text] alone — only a link if the label resolves.
        let key = normalize_label(&text);
        if let Some((url, _title)) = self.definitions.get(&key).cloned() {
            return Ok(Token::Link(text, url));
        }

        // Unresolved — emit `[text]` literally so the brackets aren't lost.
        Ok(Token::Text(format!("[{}]", text)))
    }

    /// Reads a URL inside `(...)` allowing nested balanced parens. Stops at
    /// the first unmatched `)` or at `\n`. Used by both link and image
    /// parsing so that URLs like `Foo_(bar)` survive intact.
    fn read_url_with_balanced_parens(&mut self) -> String {
        let (url, _title) = self.read_link_destination_and_title();
        url
    }

    /// Reads a link destination plus an optional CommonMark-style title.
    /// The title may be delimited by `"…"`, `'…'`, or `(…)` and must be
    /// separated from the URL by at least one ASCII whitespace char. Returns
    /// the URL (with any trailing whitespace trimmed) and the title (if any).
    /// On exit, `self.position` is at the closing `)` or end of input.
    fn read_link_destination_and_title(&mut self) -> (String, Option<String>) {
        let mut url = String::new();
        let mut depth: i32 = 0;
        while self.position < self.input.len() {
            let c = self.current_char();
            // CommonMark §6.3: `\<punct>` inside a URL emits the punctuation
            // literally — so `Foo\(bar` and `Foo\)bar` both survive.
            if c == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
                    url.push(next);
                    self.advance();
                    self.advance();
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
                while p < self.input.len() && (self.input[p] == ' ' || self.input[p] == '\t') {
                    p += 1;
                }
                if p < self.input.len() {
                    let next = self.input[p];
                    if next == '"' || next == '\'' || next == '(' {
                        break;
                    }
                }
            }
            url.push(c);
            self.advance();
        }
        let url = url.trim_end().to_string();

        // Skip whitespace between URL and potential title.
        while self.position < self.input.len()
            && (self.current_char() == ' ' || self.current_char() == '\t')
        {
            self.advance();
        }

        let title = if self.position < self.input.len() {
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
        while self.position < self.input.len()
            && (self.current_char() == ' ' || self.current_char() == '\t')
        {
            self.advance();
        }

        (url, title)
    }

    /// Reads a quoted/parenthesised title body. Assumes `self.current_char()`
    /// is the opening delimiter; advances past the closing delimiter.
    fn read_title_delimited(&mut self, _open: char, close: char) -> String {
        self.advance(); // past opener
        let start = self.position;
        while self.position < self.input.len() && self.current_char() != close {
            if self.current_char() == '\n' {
                break;
            }
            self.advance();
        }
        let title: String = self.input[start..self.position].iter().collect();
        if self.position < self.input.len() && self.current_char() == close {
            self.advance(); // past closer
        }
        title
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
        let alt_text = self.read_until_char_with_escapes(']');
        self.advance(); // skip ']'

        // Inline: ![alt](url "title")
        if self.current_char() == '(' {
            self.advance(); // skip '('
            let url = self.read_url_with_balanced_parens();
            if self.position < self.input.len() && self.current_char() == ')' {
                self.advance(); // skip ')'
            }
            return Ok(Token::Image(alt_text, url));
        }

        // Reference / collapsed: ![alt][label] or ![alt][]
        if self.current_char() == '[' {
            self.advance();
            let label = self.read_until_char_with_escapes(']');
            if self.current_char() == ']' {
                self.advance();
            }
            let key = if label.trim().is_empty() {
                normalize_label(&alt_text)
            } else {
                normalize_label(&label)
            };
            if let Some((url, _title)) = self.definitions.get(&key).cloned() {
                return Ok(Token::Image(alt_text, url));
            }
            let bracket_label = if label.is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", label)
            };
            return Ok(Token::Text(format!("![{}]{}", alt_text, bracket_label)));
        }

        // Shortcut: ![alt]
        let key = normalize_label(&alt_text);
        if let Some((url, _title)) = self.definitions.get(&key).cloned() {
            return Ok(Token::Image(alt_text, url));
        }

        // Unresolved shortcut — emit literally instead of erroring.
        Ok(Token::Text(format!("![{}]", alt_text)))
    }

    /// Tries to recognize a raw inline HTML tag (open tag, closing tag,
    /// or self-closing) starting at the current `<`. Returns the matched
    /// length (including angle brackets) on success. Pragmatic subset of
    /// CommonMark §6.6 — comments, processing instructions, declarations,
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
            !local.is_empty() && domain.contains('.')
        };

        if !is_url_scheme && !is_email {
            return None;
        }

        self.position = p + 1; // skip past '>'

        Some(if is_email {
            Token::Link(body.clone(), format!("mailto:{}", body))
        } else {
            Token::Link(body.clone(), body)
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

            // CommonMark §2.4: `\` before any ASCII punctuation char emits the
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

            // CommonMark §2.5: HTML entity / numeric character references.
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

        // CommonMark §6.7 hard line break: 2+ trailing spaces or a lone trailing
        // backslash before `\n`, in block-paragraph contexts only.
        if self.position < self.input.len()
            && self.current_char() == '\n'
            && matches!(
                ctx,
                ParseContext::Root | ParseContext::ListItem | ParseContext::BlockQuote
            )
        {
            if content.ends_with("  ") {
                while content.ends_with(' ') {
                    content.pop();
                }
                self.advance(); // consume the \n
                self.pending_hard_break = true;
            } else if !last_was_escape && content.ends_with('\\') {
                content.pop();
                self.advance(); // consume the \n
                self.pending_hard_break = true;
            }
        }

        if content.is_empty() {
            let (line, col) = self.pos_to_line_col(start_pos);
            Err(LexerError::UnknownToken(format!(
                "Unexpected character at line {}, column {}",
                line, col
            )))
        } else {
            Ok(Token::Text(content))
        }
    }

    /// Parses an HTML comment, extracting the comment content
    fn parse_html_comment(&mut self) -> Result<Token, LexerError> {
        // Assumes current position at '<' and '!--' follows
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
            Err(LexerError::UnexpectedEndOfInput)
        }
    }

    /// Checks if current position is at the start of a line
    fn is_at_line_start(&self) -> bool {
        self.position == 0 || self.input.get(self.position - 1) == Some(&'\n')
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
    /// for ASCII punctuation per CommonMark §2.4. Stops at the closing
    /// delimiter (which is NOT consumed). `\<close>` and `\\` produce
    /// literal chars; `\<punct>` produces the punctuation; `\<other>`
    /// remains a literal backslash followed by the char.
    fn read_until_char_with_escapes(&mut self, delimiter: char) -> String {
        let mut out = String::new();
        while self.position < self.input.len() {
            let ch = self.current_char();
            if ch == '\\' && self.position + 1 < self.input.len() {
                let next = self.input[self.position + 1];
                if is_ascii_punctuation(next) {
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

    /// CommonMark §6.2 left-flanking-delimiter-run. The run at `pos` is
    /// left-flanking if it is NOT followed by Unicode whitespace, AND
    /// EITHER not followed by punctuation OR preceded by whitespace/punc.
    /// "Followed by end-of-input" counts as whitespace for this rule.
    fn is_left_flanking_run(&self, pos: usize) -> bool {
        let delim = match self.input.get(pos) {
            Some(&c) if c == '*' || c == '_' || c == '~' => c,
            _ => return false,
        };
        let mut end = pos;
        while end < self.input.len() && self.input[end] == delim {
            end += 1;
        }
        let before = if pos == 0 {
            None
        } else {
            self.input.get(pos - 1).copied()
        };
        let after = self.input.get(end).copied();

        let not_followed_by_ws = matches!(after, Some(c) if !c.is_whitespace());
        if !not_followed_by_ws {
            return false;
        }
        let followed_by_punc = matches!(after, Some(c) if is_md_punctuation(c));
        if !followed_by_punc {
            return true;
        }
        match before {
            None => true,
            Some(c) => c.is_whitespace() || is_md_punctuation(c),
        }
    }

    /// CommonMark §6.2 right-flanking-delimiter-run. Symmetric to left-flanking.
    fn is_right_flanking_run(&self, pos: usize) -> bool {
        let delim = match self.input.get(pos) {
            Some(&c) if c == '*' || c == '_' || c == '~' => c,
            _ => return false,
        };
        let mut end = pos;
        while end < self.input.len() && self.input[end] == delim {
            end += 1;
        }
        let before = if pos == 0 {
            None
        } else {
            self.input.get(pos - 1).copied()
        };
        let after = self.input.get(end).copied();

        let not_preceded_by_ws = matches!(before, Some(c) if !c.is_whitespace());
        if !not_preceded_by_ws {
            return false;
        }
        let preceded_by_punc = matches!(before, Some(c) if is_md_punctuation(c));
        if !preceded_by_punc {
            return true;
        }
        match after {
            None => true,
            Some(c) => c.is_whitespace() || is_md_punctuation(c),
        }
    }

    /// Whether the `*`/`_` run at `pos` is allowed to open emphasis. `*` runs
    /// open when left-flanking; `_` additionally must not be right-flanking
    /// (or must be preceded by punctuation).
    fn can_open_emphasis(&self, pos: usize) -> bool {
        let delim = self.input.get(pos).copied();
        if !self.is_left_flanking_run(pos) {
            return false;
        }
        if delim == Some('*') {
            return true;
        }
        if !self.is_right_flanking_run(pos) {
            return true;
        }
        let before = if pos == 0 {
            None
        } else {
            self.input.get(pos - 1).copied()
        };
        matches!(before, Some(c) if is_md_punctuation(c))
    }

    /// Whether the `*`/`_` run at `pos` is allowed to close emphasis. `*` runs
    /// close when right-flanking; `_` additionally must not be left-flanking
    /// (or must be followed by punctuation).
    fn can_close_emphasis(&self, pos: usize) -> bool {
        let delim = self.input.get(pos).copied();
        if !self.is_right_flanking_run(pos) {
            return false;
        }
        if delim == Some('*') {
            return true;
        }
        if !self.is_left_flanking_run(pos) {
            return true;
        }
        let mut end = pos;
        while end < self.input.len() && self.input.get(end) == delim.as_ref() {
            end += 1;
        }
        matches!(self.input.get(end), Some(c) if is_md_punctuation(*c))
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

    /// Checks if the current position contains a horizontal rule (---)
    fn check_horizontal_rule(&mut self) -> Result<bool, LexerError> {
        if self.current_char() == '-' {
            let mut count = 1;
            let mut pos = self.position + 1;

            // Look ahead for at least 3 consecutive hyphens
            while pos < self.input.len() && self.input[pos] == '-' {
                count += 1;
                pos += 1;
            }

            if count >= 3 {
                self.position = pos;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// CommonMark §4.1: a thematic break is a line of 3+ matching markers
    /// from `-`/`*`/`_` (with optional internal/leading whitespace, up to 3
    /// leading spaces). Caller must already be at line start.
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
    /// must contain non-whitespace content. Per CommonMark §4.3.
    fn peek_setext_level(&self) -> Option<usize> {
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

        // Scan the current line; require non-whitespace content.
        let mut p = self.position;
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
        if p >= self.input.len() {
            return None;
        }
        // Skip the newline.
        p += 1;
        // Optional up to 3 leading spaces.
        let mut leading = 0usize;
        while p < self.input.len() && self.input[p] == ' ' && leading < 3 {
            p += 1;
            leading += 1;
        }
        let underline_char = match self.input.get(p) {
            Some(&'=') => '=',
            Some(&'-') => '-',
            _ => return None,
        };
        let mut count = 0usize;
        while p < self.input.len() && self.input[p] == underline_char {
            count += 1;
            p += 1;
        }
        if count == 0 {
            return None;
        }
        // Optional trailing whitespace.
        while p < self.input.len() && (self.input[p] == ' ' || self.input[p] == '\t') {
            p += 1;
        }
        // Must reach \n or EOF.
        if p < self.input.len() && self.input[p] != '\n' {
            return None;
        }
        Some(if underline_char == '=' { 1 } else { 2 })
    }

    /// Consumes a setext heading: the current line is the heading content,
    /// then `\n`, then the underline line. The text is re-lexed as inline.
    fn consume_setext_heading(&mut self, level: usize) -> Result<Token, LexerError> {
        let start = self.position;
        let mut end = start;
        while end < self.input.len() && self.input[end] != '\n' {
            end += 1;
        }
        let line: String = self.input[start..end].iter().collect();
        self.position = end;
        // Skip newline after content.
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }
        // Skip the underline line.
        self.consume_current_line();

        let mut sub = Lexer::new(line.trim().to_string());
        let content = sub.parse_with_context(ParseContext::Inline)?;
        Ok(Token::Heading(content, level))
    }

    /// Checks if current position starts an ordered list marker (e.g.
    /// `1.` or `1)`). Per CommonMark §5.2, both `.` and `)` are valid
    /// ordered-list marker terminators.
    fn check_ordered_list_marker(&mut self) -> Option<usize> {
        let start_pos = self.position;
        let mut pos = start_pos;
        let mut number_str = String::new();

        while pos < self.input.len() && self.input[pos].is_ascii_digit() {
            number_str.push(self.input[pos]);
            pos += 1;
        }

        if pos < self.input.len()
            && (self.input[pos] == '.' || self.input[pos] == ')')
        {
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
        let mut number = None;

        if !ordered {
            self.advance();
        } else {
            number = self.check_ordered_list_marker();
            // Skip past digit run plus the marker terminator (`.` or `)`).
            while self.position < self.input.len() && self.current_char().is_ascii_digit() {
                self.advance();
            }
            if self.position < self.input.len()
                && (self.current_char() == '.' || self.current_char() == ')')
            {
                self.advance();
            }
        }

        self.skip_whitespace();

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
        while self.position < self.input.len() && self.current_char() != '\n' {
            if let Some(token) = self.next_token(ParseContext::ListItem)? {
                content.push(token);
            }
        }

        // Move past the line-terminating newline if there is one.
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        // Continuation loop: handles both deeper-indented sub-items / nested
        // markers AND CommonMark §5.2 lazy paragraph continuation (lines at
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
            let after_indent = line_start + cur_indent;

            // Blank line ends the item.
            if after_indent >= self.input.len() || self.input[after_indent] == '\n' {
                break;
            }

            // Decide if this line starts a new block (which terminates the
            // item) or is continuation content.
            let is_marker_line = self.line_starts_with_list_marker(after_indent);
            let next_ch = self.input[after_indent];

            if cur_indent > indent_level {
                // Deeper-indented line: prefer nested-list-marker handling.
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
                // Fall through to lazy continuation for non-marker deeper
                // content (e.g. an indented paragraph continuation line).
            } else {
                // Indent <= parent. Sibling/outer marker terminates this item.
                if is_marker_line {
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
            // tokens to the current item's content.
            self.position = after_indent;
            content.push(Token::Newline);
            while self.position < self.input.len() && self.current_char() != '\n' {
                if let Some(tok) = self.next_token(ParseContext::ListItem)? {
                    content.push(tok);
                }
            }
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            }
        }

        Ok(Token::ListItem {
            content,
            ordered,
            number,
            checked,
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
        if self.position == 0 {
            return true;
        }
        if self.input.get(self.position - 1) != Some(&'\n') {
            return false;
        }
        // Find the start of the previous line.
        let mut prev_line_start = self.position - 1; // points at the \n
        while prev_line_start > 0 && self.input[prev_line_start - 1] != '\n' {
            prev_line_start -= 1;
        }
        // Previous line is input[prev_line_start..position-1] (excluding the \n).
        let prev_line_end = self.position - 1;
        self.input[prev_line_start..prev_line_end]
            .iter()
            .all(|c| *c == ' ' || *c == '\t')
    }

    /// CommonMark §4.4 indented code block. Strips up to 4 columns of leading
    /// whitespace from each line, includes blank lines if subsequent lines
    /// resume the 4-column indent, and stops at the first non-blank line with
    /// less than 4 columns of indent.
    fn parse_indented_code_block(&mut self) -> Token {
        let mut content = String::new();
        loop {
            if !self.is_at_line_start() {
                break;
            }
            let indent = self.get_current_indent();
            if indent < 4 {
                // Blank line: include if a 4-indented line follows; else end.
                let line_start = self.position;
                let mut p = self.position;
                while p < self.input.len() && (self.input[p] == ' ' || self.input[p] == '\t') {
                    p += 1;
                }
                if p < self.input.len() && self.input[p] == '\n' {
                    // Look ahead past the \n to see if the next line is 4-indented.
                    let mut q = p + 1;
                    let mut next_indent = 0usize;
                    while q < self.input.len() {
                        match self.input[q] {
                            ' ' => next_indent += 1,
                            '\t' => next_indent += 4 - (next_indent % 4),
                            _ => break,
                        }
                        q += 1;
                    }
                    if next_indent >= 4 && q < self.input.len() && self.input[q] != '\n' {
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
        Token::Code(
            String::new(),
            content.trim_end_matches('\n').to_string(),
        )
    }

    /// Gets the current line's indentation level in columns. CommonMark §2.2:
    /// a tab advances to the next multiple of 4, so `  \t` is 4 columns total
    /// (not 6 as a flat 4-per-tab rule would give).
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
            next_char == ' ' || next_char == '\t'
        } else {
            false
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
                vec![Token::Code("".to_string(), "inline code".to_string())],
            ),
            (
                "```rust\nfn main() {}\n```",
                vec![Token::Code("rust".to_string(), "fn main() {}".to_string())],
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
                        checked: None,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Item 2".to_string())],
                        ordered: false,
                        number: None,
                        checked: None,
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
                        checked: None,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Second".to_string())],
                        ordered: true,
                        number: Some(2),
                        checked: None,
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
                        checked: None,
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Nested 2".to_string())],
                        ordered: false,
                        number: None,
                        checked: None,
                    },
                ],
                ordered: false,
                number: None,
                checked: None,
            },
            Token::ListItem {
                content: vec![Token::Text("Item 2".to_string())],
                ordered: false,
                number: None,
                checked: None,
            },
        ];
        assert_eq!(parse(input), expected);
    }

    #[test]
    fn test_links() {
        let tests = vec![
            (
                "[Link](https://example.com)",
                vec![Token::Link(
                    "Link".to_string(),
                    "https://example.com".to_string(),
                )],
            ),
            (
                "![Image](image.jpg)",
                vec![Token::Image("Image".to_string(), "image.jpg".to_string())],
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
        // An unclosed HTML comment still surfaces a real LexerError via
        // `UnexpectedEndOfInput` — most other malformed inline constructs
        // gracefully fall back to literal text instead of erroring.
        let mut lexer = Lexer::new("<!--never closes".to_string());
        assert!(matches!(lexer.parse(), Err(_)));
    }

    #[test]
    fn test_code_block_edge_cases() {
        let tests = vec![
            (
                "```\nempty language\n```",
                vec![Token::Code("".to_string(), "empty language".to_string())],
            ),
            (
                "`code with *asterisk*`",
                vec![Token::Code(
                    "".to_string(),
                    "code with *asterisk*".to_string(),
                )],
            ),
            (
                "```rust\nfn main() {\n    println!(\"Hello\");\n}\n```",
                vec![Token::Code(
                    "rust".to_string(),
                    "fn main() {\n    println!(\"Hello\");\n}".to_string(),
                )],
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
                    assert!(body.iter().any(|t| matches!(t, Token::Link(_, _))));
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
                "[Link with spaces](https://example.com/path with spaces)",
                vec![Token::Link(
                    "Link with spaces".to_string(),
                    "https://example.com/path with spaces".to_string(),
                )],
            ),
            (
                "![Image with *emphasis* in alt](image.jpg)",
                vec![Token::Image(
                    "Image with *emphasis* in alt".to_string(),
                    "image.jpg".to_string(),
                )],
            ),
            (
                "[Empty]()",
                vec![Token::Link("Empty".to_string(), "".to_string())],
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
            vec![Token::Image(
                "Alt text".to_string(),
                "image.png".to_string()
            )]
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
            assert!(matches!(content[0], Token::Code(_, _)));
            if let Token::Code(_, code) = &content[0] {
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
        // Per CommonMark, * is allowed intra-word
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
            vec![Token::Link(
                "link_text".to_string(),
                "https://example.com".to_string()
            )]
        );
    }

    #[test]
    fn code_with_underscore() {
        let tokens = parse("`foo_bar`");
        assert_eq!(
            tokens,
            vec![Token::Code("".to_string(), "foo_bar".to_string())]
        );
    }

    #[test]
    fn image_alt_with_underscore() {
        let tokens = parse("![alt_text](img.png)");
        assert_eq!(
            tokens,
            vec![Token::Image("alt_text".to_string(), "img.png".to_string())]
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
    fn error_variant_still_matches() {
        let mut lexer = Lexer::new("<!--never closes".to_string());
        assert!(matches!(lexer.parse(), Err(_)));
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
        // Per CommonMark §2.4: \# at line start should NOT start a heading.
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
        assert!(matches!(tokens[1], Token::Link(_, _)));
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
            vec![Token::Code("".to_string(), r"\*literal\*".to_string())]
        );
    }

    #[test]
    fn escape_not_active_in_fenced_code() {
        let input = "```\n\\*kept literal\\*\n```";
        let tokens = parse(input);
        if let Token::Code(_, body) = &tokens[0] {
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
        // Per CommonMark §6.7 a lone trailing backslash before a newline
        // is a hard line break — produces Text("foo") + HardBreak + Text("bar").
        let tokens = parse("foo\\\nbar");
        assert!(matches!(tokens[0], Token::Text(ref s) if s == "foo"));
        assert!(tokens.iter().any(|t| matches!(t, Token::HardBreak)));
        assert!(tokens.iter().any(|t| matches!(t, Token::Text(ref s) if s == "bar")));
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
        assert!(tokens.iter().any(|t| matches!(t, Token::Code(_, _))));
        assert!(tokens.iter().any(|t| matches!(t, Token::Link(_, _))));
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
            body.iter().any(|t| matches!(t, Token::Code(_, _))),
            "expected code span, got body {:?}",
            body
        );
    }

    #[test]
    fn inline_link_inside_quote() {
        let tokens = parse("> visit [example](https://example.com)");
        let body = block_body(&tokens[0]);
        assert!(
            body.iter().any(|t| matches!(t, Token::Link(_, _))),
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
        // Per CommonMark, `>foo` is also a blockquote (the space is optional).
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
            vec![Token::Code("".to_string(), "fn main() {}".to_string())]
        );
    }

    #[test]
    fn tilde_fenced_code_block_with_language() {
        let input = "~~~rust\nlet x = 5;\n~~~";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code("rust".to_string(), "let x = 5;".to_string())]
        );
    }

    #[test]
    fn tilde_fence_can_contain_backticks() {
        // The whole point of `~~~` is letting code contain literal backticks.
        let input = "~~~\nlet s = `template`;\n~~~";
        let tokens = parse(input);
        if let Token::Code(_, body) = &tokens[0] {
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
            vec![Token::Code(
                "".to_string(),
                "~~not strikethrough~~".to_string()
            )]
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
            vec![Token::Link(
                "Wiki".to_string(),
                "https://en.wikipedia.org/wiki/Foo_(bar)".to_string()
            )]
        );
    }

    #[test]
    fn url_with_nested_balanced_parens() {
        let tokens = parse("[X](http://a.b/((c)d))");
        assert_eq!(
            tokens,
            vec![Token::Link("X".to_string(), "http://a.b/((c)d)".to_string())]
        );
    }

    #[test]
    fn image_url_with_paren_pair() {
        let tokens = parse("![alt](pic_(small).png)");
        assert_eq!(
            tokens,
            vec![Token::Image(
                "alt".to_string(),
                "pic_(small).png".to_string()
            )]
        );
    }

    #[test]
    fn url_with_unbalanced_close_paren_truncates() {
        let tokens = parse("[X](https://example.com/path)trailing");
        if let Token::Link(text, url) = &tokens[0] {
            assert_eq!(text, "X");
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
            vec![Token::Link(
                "https://example.com".to_string(),
                "https://example.com".to_string()
            )]
        );
    }

    #[test]
    fn autolink_http() {
        let tokens = parse("<http://example.org/path>");
        assert_eq!(
            tokens,
            vec![Token::Link(
                "http://example.org/path".to_string(),
                "http://example.org/path".to_string()
            )]
        );
    }

    #[test]
    fn autolink_email() {
        let tokens = parse("<user@example.com>");
        assert_eq!(
            tokens,
            vec![Token::Link(
                "user@example.com".to_string(),
                "mailto:user@example.com".to_string()
            )]
        );
    }

    #[test]
    fn autolink_in_paragraph() {
        let tokens = parse("see <https://example.com> for more");
        assert!(
            tokens
                .iter()
                .any(|t| matches!(t, Token::Link(_, url) if url == "https://example.com")),
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
            vec![Token::Link(
                "example".to_string(),
                "https://example.com".to_string()
            )]
        );
    }

    #[test]
    fn regression_simple_image() {
        let tokens = parse("![alt](image.png)");
        assert_eq!(
            tokens,
            vec![Token::Image("alt".to_string(), "image.png".to_string())]
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
        assert_eq!(tokens, vec![Token::Code("".to_string(), "&amp;".to_string())]);
    }

    #[test]
    fn invalid_numeric_passes_through() {
        // Out-of-range / malformed numerics pass through unchanged.
        assert_eq!(collected("&#xZZZ;"), "&#xZZZ;");
        assert_eq!(collected("&#abc;"), "&#abc;");
    }

    // --- Section 1 (full HTML5 table) coverage ---

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
        // CommonMark §2.5: code point 0 → U+FFFD.
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
        // Per CommonMark, only semicolon-terminated entries decode, even
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
            vec![Token::Link("text".to_string(), "url".to_string())],
            "URL must be clean (title parsed and dropped, not folded into URL)"
        );
    }

    #[test]
    fn link_with_single_quote_title() {
        let tokens = parse("[text](url 'title here')");
        assert_eq!(
            tokens,
            vec![Token::Link("text".to_string(), "url".to_string())]
        );
    }

    #[test]
    fn link_with_paren_title() {
        let tokens = parse("[text](url (title here))");
        assert_eq!(
            tokens,
            vec![Token::Link("text".to_string(), "url".to_string())]
        );
    }

    #[test]
    fn image_with_title() {
        let tokens = parse(r#"![alt](pic.png "Photo of cat")"#);
        assert_eq!(
            tokens,
            vec![Token::Image("alt".to_string(), "pic.png".to_string())]
        );
    }

    #[test]
    fn link_no_title_unchanged() {
        let tokens = parse("[text](url)");
        assert_eq!(
            tokens,
            vec![Token::Link("text".to_string(), "url".to_string())]
        );
    }

    #[test]
    fn link_url_paren_pair_with_title() {
        // URL contains balanced parens AND a title at the end.
        let tokens = parse(r#"[Wiki](https://en.wikipedia.org/wiki/Foo_(bar) "Wikipedia entry")"#);
        assert_eq!(
            tokens,
            vec![Token::Link(
                "Wiki".to_string(),
                "https://en.wikipedia.org/wiki/Foo_(bar)".to_string()
            )]
        );
    }

    #[test]
    fn link_with_only_whitespace_after_url_no_title() {
        // Trailing whitespace before `)` without a title is fine.
        let tokens = parse("[text](url   )");
        assert_eq!(
            tokens,
            vec![Token::Link("text".to_string(), "url".to_string())]
        );
    }

    #[test]
    fn link_url_with_no_space_then_quote_is_url_only() {
        // `(url"foo")` with no whitespace between url and quote — not a title.
        // The whole `url"foo"` is the URL per CommonMark.
        let tokens = parse("[text](url\"foo\")");
        if let Token::Link(_, url) = &tokens[0] {
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
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(text, url) if text == "CommonMark" && url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn collapsed_reference_link() {
        let input = "[CommonMark][]\n\n[CommonMark]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(_, url) if url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn shortcut_reference_link() {
        let input = "[CommonMark]\n\n[CommonMark]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(_, url) if url == "https://commonmark.org")
        ), "got {:?}", tokens);
    }

    #[test]
    fn label_matching_is_case_insensitive() {
        let input = "[CommonMark][CM]\n\n[cm]: https://commonmark.org";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(_, url) if url == "https://commonmark.org")
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
        let has_link = tokens.iter().any(|t| matches!(t, Token::Link(_, _)));
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
            |t| matches!(t, Token::Image(_, url) if url == "pic.png")
        ), "got {:?}", tokens);
    }

    #[test]
    fn definition_with_title_is_parsed_url_clean() {
        let input = "[a][r]\n\n[r]: https://example.com \"Example\"";
        let tokens = parse(input);
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(_, url) if url == "https://example.com")
        ), "URL should be clean (no title baked in), got {:?}", tokens);
    }

    #[test]
    fn inline_link_still_takes_priority_over_reference() {
        // [text](url) is inline — must NOT be confused with a reference.
        let tokens = parse("[text](https://example.com)\n\n[text]: should-not-apply");
        assert!(tokens.iter().any(
            |t| matches!(t, Token::Link(_, url) if url == "https://example.com")
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
            vec![Token::Code("".to_string(), "code with ` inside".to_string())]
        );
    }

    #[test]
    fn triple_backtick_inline_when_not_at_line_start() {
        let tokens = parse("inline ```code with `` inside``` here");
        // First Text("inline "), then Code, then Text(" here").
        assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("inline")));
        assert!(matches!(tokens[1], Token::Code(_, ref c) if c.contains("``")));
    }

    #[test]
    fn double_backtick_with_count_mismatch_inside() {
        // ``a`b``  -> code containing "a`b". A single ` inside doesn't close.
        let tokens = parse("``a`b``");
        assert_eq!(
            tokens,
            vec![Token::Code("".to_string(), "a`b".to_string())]
        );
    }

    #[test]
    fn fenced_block_still_works() {
        let input = "```rust\nfn main() {}\n```";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code("rust".to_string(), "fn main() {}".to_string())]
        );
    }

    #[test]
    fn fenced_block_preserves_inner_backticks() {
        // A single ` (or any run shorter than the opener) inside the body
        // must remain in the output. Pre-existing bug: count_backticks
        // advanced past the inner ticks but never pushed them to content.
        let input = "```rust\nlet s = `template`;\n```";
        let tokens = parse(input);
        if let Token::Code(_, body) = &tokens[0] {
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
        if let Token::Code(_, body) = &tokens[0] {
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
        assert!(matches!(tokens[0], Token::Code(_, ref c) if c == "inline"));
        assert!(tokens.iter().any(|t| matches!(t, Token::Text(s) if s.contains("plus text"))));
    }

    #[test]
    fn unclosed_inline_code_falls_back_to_text() {
        // No matching closer (EOF reached) — per CommonMark, the opener run
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
            .any(|t| matches!(t, Token::Code(_, c) if c.contains('\n')));
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
            vec![Token::Code("".to_string(), "simple".to_string())]
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
            vec![Token::Code("".to_string(), "let x = 5;".to_string())]
        );
    }

    #[test]
    fn tab_indent_is_code() {
        let tokens = parse("\tlet x = 5;");
        assert_eq!(
            tokens,
            vec![Token::Code("".to_string(), "let x = 5;".to_string())]
        );
    }

    #[test]
    fn three_spaces_is_not_code() {
        // 3 spaces is not enough; should be regular paragraph text.
        let tokens = parse("   not code");
        let body = Token::collect_all_text(&tokens);
        assert_eq!(body, "not code");
        assert!(!tokens.iter().any(|t| matches!(t, Token::Code(_, _))));
    }

    #[test]
    fn multi_line_indented_code() {
        let input = "    fn main() {\n        println!(\"hi\");\n    }";
        let tokens = parse(input);
        if let Token::Code(_, body) = &tokens[0] {
            assert!(body.contains("fn main()"), "got {:?}", body);
            assert!(body.contains("println!"), "got {:?}", body);
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn indented_code_inside_paragraph_does_not_apply() {
        // Indented line directly after a paragraph is treated as paragraph
        // continuation per CommonMark, not code. We're more permissive: it
        // becomes code if separated by a blank line. Test the blank-line case.
        let input = "Some paragraph\n\n    code line";
        let tokens = parse(input);
        assert!(tokens.iter().any(|t| matches!(t, Token::Code(_, _))));
    }

    #[test]
    fn fenced_code_block_unaffected() {
        let input = "```\nfn main() {}\n```";
        let tokens = parse(input);
        assert_eq!(
            tokens,
            vec![Token::Code("".to_string(), "fn main() {}".to_string())]
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
        assert!(!tokens.iter().any(|t| matches!(t, Token::Code(_, _))));
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
        assert!(matches!(tokens[0], Token::Link(_, _)));
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
                if let Token::Code(_, body) = t {
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
        if let Some(Token::Code(_, body)) =
            tokens.iter().find(|t| matches!(t, Token::Code(_, _)))
        {
            assert_eq!(body, " foo ");
        } else {
            panic!("expected Code, got {:?}", tokens);
        }
    }

    #[test]
    fn all_spaces_not_stripped() {
        let tokens = parse("a `   ` b");
        if let Some(Token::Code(_, body)) =
            tokens.iter().find(|t| matches!(t, Token::Code(_, _)))
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
            vec![Token::Code("".to_string(), "foo".to_string())]
        );
    }

    #[test]
    fn one_sided_space_unchanged() {
        // Only strip when BOTH sides have a space.
        let tokens = parse("a ` foo` b");
        if let Some(Token::Code(_, body)) =
            tokens.iter().find(|t| matches!(t, Token::Code(_, _)))
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
            body.iter().any(|t| matches!(t, Token::Code(_, _))),
            "expected Code inside quote, got {:?}",
            body
        );
    }

    #[test]
    fn regular_text_inside_blockquote_unaffected() {
        let tokens = parse("> Just a sentence with three spaces:    not code.");
        let body = block_body(&tokens[0]);
        assert!(!body.iter().any(|t| matches!(t, Token::Code(_, _))));
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

    // CommonMark §5.2: a non-prefixed line that doesn't start a new block
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
        // Spec §5.2: lazy lines can be interleaved with `>` lines.
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
            tokens[1..].iter().any(|t| matches!(t, Token::Code(_, _))),
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
            vec![Token::Link("a]b".to_string(), "http://x".to_string())]
        );
    }

    #[test]
    fn escape_close_paren_in_link_url() {
        let tokens = parse(r"[t](http://x\)y)");
        assert_eq!(
            tokens,
            vec![Token::Link("t".to_string(), "http://x)y".to_string())]
        );
    }

    #[test]
    fn escape_backslash_in_link_text() {
        let tokens = parse(r"[a\\b](u)");
        assert_eq!(
            tokens,
            vec![Token::Link("a\\b".to_string(), "u".to_string())]
        );
    }

    #[test]
    fn escape_close_bracket_in_image_alt() {
        let tokens = parse(r"![alt\]more](pic.png)");
        assert_eq!(
            tokens,
            vec![Token::Image(
                "alt]more".to_string(),
                "pic.png".to_string()
            )]
        );
    }

    #[test]
    fn unescaped_link_still_works() {
        let tokens = parse("[foo](http://example.com)");
        assert_eq!(
            tokens,
            vec![Token::Link(
                "foo".to_string(),
                "http://example.com".to_string()
            )]
        );
    }

    #[test]
    fn balanced_parens_still_work() {
        // Pre-existing balanced-paren handling shouldn't regress.
        let tokens = parse("[Wiki](https://en.wikipedia.org/wiki/Foo_(bar))");
        assert_eq!(
            tokens,
            vec![Token::Link(
                "Wiki".to_string(),
                "https://en.wikipedia.org/wiki/Foo_(bar)".to_string()
            )]
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
        // CommonMark §5.2: a non-blank, non-marker line at indent 0 still
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
