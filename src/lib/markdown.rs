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
    /// Line break
    Newline,
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
            Token::Unknown(text) => result.push_str(text),
            Token::Newline | Token::HorizontalRule => {
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
}

impl Lexer {
    /// Creates a new lexer instance from input string
    pub fn new(input: String) -> Self {
        Lexer {
            input: input.chars().collect(),
            position: 0,
        }
    }

    /// Parses the entire input string into a sequence of tokens.
    /// Returns a Result containing either a Vec of parsed tokens or a LexerError.
    pub fn parse(&mut self) -> Result<Vec<Token>, LexerError> {
        self.parse_with_context(ParseContext::Root)
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

            if is_delimiter(ch) {
                // `_` runs flanked by alphanumerics on both sides are intra-word
                // and per CommonMark must not close emphasis (e.g. `_foo_bar_`).
                if ch == '_' && self.is_intra_word_underscore_run(self.position) {
                    // fall through and consume as regular content
                } else {
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
        // H2 instead of being consumed as Text + HorizontalRule.
        if is_line_start && matches!(ctx, ParseContext::Root) {
            if let Some(level) = self.peek_setext_level() {
                return Ok(Some(self.consume_setext_heading(level)?));
            }
        }

        let token = match current_char {
            '#' if is_line_start && allow_block_tokens(ctx) => self.parse_heading()?,
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
            '*' => self.parse_emphasis()?,
            '_' if !self.is_intra_word_underscore_run(self.position) => self.parse_emphasis()?,
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

    /// Parses a heading token, counting '#' characters for level and collecting nested content
    fn parse_heading(&mut self) -> Result<Token, LexerError> {
        let mut level = 0;
        while self.current_char() == '#' {
            level += 1;
            self.advance();
        }
        self.skip_whitespace();
        let content = self.parse_nested_content(|c| c == '\n', ParseContext::Inline)?;
        Ok(Token::Heading(content, level))
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

        let mut content = self.parse_nested_content(|c| c == delimiter, ParseContext::Inline)?;
        content.push(Token::Text(String::from(" ")));

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
        let start_backticks = self.count_backticks();

        // Single backtick case
        if start_backticks == 1 {
            let mut content = String::new();

            // Read until either a closing backtick or end of input
            while self.position < self.input.len() {
                let ch = self.current_char();
                if ch == '`' {
                    self.advance(); // skip closing backtick
                    break;
                }
                content.push(ch);
                self.advance();
            }

            return Ok(Token::Code(String::new(), content));
        }

        // Multi-line code block case
        self.skip_whitespace();
        let language = self.read_until_newline();
        let mut content = String::new();

        while self.position < self.input.len() {
            let current_backticks = self.count_backticks();
            if current_backticks == start_backticks {
                break;
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

    /// Parses a GFM strikethrough run (`~~text~~`). Falls back to literal text
    /// if the closer isn't found, mirroring the emphasis fallback (Fix #2).
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
    /// §5.1). A blank line ends the quote; lazy continuation is not yet
    /// supported.
    fn parse_blockquote(&mut self) -> Result<Token, LexerError> {
        let mut body_lines: Vec<String> = Vec::new();

        loop {
            // Consume the `>` marker.
            if self.position >= self.input.len() || self.current_char() != '>' {
                break;
            }
            self.advance();
            // Optional single space after `>`.
            if self.position < self.input.len() && self.current_char() == ' ' {
                self.advance();
            }
            // Take the rest of the line.
            let line = self.read_until_newline();
            body_lines.push(line);
            if self.position < self.input.len() && self.current_char() == '\n' {
                self.advance();
            }
            // Continue if next line also starts with `>` (no blank line in between).
            if self.position >= self.input.len() {
                break;
            }
            if !self.is_at_line_start() || self.current_char() != '>' {
                break;
            }
        }

        let body_text = body_lines.join("\n");
        let mut sub = Lexer::new(body_text);
        let body = sub.parse_with_context(ParseContext::BlockQuote)?;
        Ok(Token::BlockQuote(body))
    }

    /// Parses a link token, extracting display text and URL
    fn parse_link(&mut self) -> Result<Token, LexerError> {
        self.advance(); // skip '['
        let text = self.read_until_char(']');
        self.advance(); // skip ']'
        if self.current_char() == '(' {
            self.advance(); // skip '('
            let url = self.read_url_with_balanced_parens();
            if self.position < self.input.len() && self.current_char() == ')' {
                self.advance(); // skip ')'
            }
            return Ok(Token::Link(text, url));
        }
        Ok(Token::Link(text, String::new()))
    }

    /// Reads a URL inside `(...)` allowing nested balanced parens. Stops at
    /// the first unmatched `)` or at `\n`. Used by both link and image
    /// parsing so that URLs like `Foo_(bar)` survive intact.
    fn read_url_with_balanced_parens(&mut self) -> String {
        let start = self.position;
        let mut depth: i32 = 0;
        while self.position < self.input.len() {
            let c = self.current_char();
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
            }
            self.advance();
        }
        self.input[start..self.position].iter().collect()
    }

    /// Parses an image token, extracting alt text and URL
    fn parse_image(&mut self) -> Result<Token, LexerError> {
        let start_pos = self.position;
        self.advance();

        if self.position < self.input.len() && self.current_char() == '[' {
            self.advance();
            let alt_text = self.read_until_char(']');
            self.advance(); // skip ']'
            if self.current_char() == '(' {
                self.advance(); // skip '('
                let url = self.read_url_with_balanced_parens();
                if self.position < self.input.len() && self.current_char() == ')' {
                    self.advance(); // skip ')'
                }
                Ok(Token::Image(alt_text, url))
            } else {
                let (line, col) = self.pos_to_line_col(start_pos);
                Err(LexerError::UnknownToken(format!(
                    "Malformed image (missing URL) at line {}, column {}",
                    line, col
                )))
            }
        } else {
            // If '!' is not followed by '[', treat it as regular text
            self.position = start_pos;
            self.parse_text(ParseContext::Inline)
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
                    continue;
                }
            }

            if ch == '\n' || self.is_start_of_special_token(ctx) {
                break;
            }

            content.push(ch);
            self.advance();
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

    /// Reads characters until a specific delimiter is encountered
    fn read_until_char(&mut self, delimiter: char) -> String {
        let start = self.position;
        while self.position < self.input.len() && self.current_char() != delimiter {
            self.advance();
        }
        self.input[start..self.position].iter().collect()
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
                self.looks_like_autolink_start()
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

    /// Checks if we're immediately after a special token that should preserve following spaces
    fn is_after_special_token(&self) -> bool {
        if self.position == 0 {
            return false;
        }

        let prev_char = self.input[self.position - 1];
        match prev_char {
            '`' | ')' => true,
            _ => false,
        }
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

    /// Checks if current position starts an ordered list marker (e.g., "1.")
    fn check_ordered_list_marker(&mut self) -> Option<usize> {
        let start_pos = self.position;
        let mut pos = start_pos;
        let mut number_str = String::new();

        while pos < self.input.len() && self.input[pos].is_ascii_digit() {
            number_str.push(self.input[pos]);
            pos += 1;
        }

        if pos < self.input.len() && self.input[pos] == '.' {
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
            // Skip past number and period
            while self.position < self.input.len()
                && (self.current_char().is_ascii_digit() || self.current_char() == '.')
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

        // Move to next line if exists
        if self.position < self.input.len() && self.current_char() == '\n' {
            self.advance();
        }

        while self.position < self.input.len() {
            let current_indent = self.get_current_indent();
            if current_indent <= indent_level {
                // Back to same or lower indentation level, exit nested parsing
                break;
            }

            self.position += current_indent;
            match self.current_char() {
                '-' | '+' => {
                    if !self.check_horizontal_rule()? {
                        content.push(self.parse_list_item(false, current_indent, parent_ctx)?);
                    }
                }
                '*' => {
                    if self.is_list_marker('*') {
                        content.push(self.parse_list_item(false, current_indent, parent_ctx)?);
                    } else {
                        break;
                    }
                }
                '0'..='9' => {
                    if self.check_ordered_list_marker().is_some() {
                        content.push(self.parse_list_item(true, current_indent, parent_ctx)?);
                    }
                }
                _ => break,
            }
        }

        Ok(Token::ListItem {
            content,
            ordered,
            number,
            checked,
        })
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

    /// Gets the current line's indentation level
    fn get_current_indent(&self) -> usize {
        let mut count = 0;
        let mut pos = self.position;

        while pos < self.input.len() {
            match self.input[pos] {
                ' ' => count += 1,
                '\t' => count += 4, // Convert tabs to spaces (common convention)
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
        let tests = vec![
            (
                "*italic*",
                vec![Token::Emphasis {
                    level: 1,
                    content: vec![
                        Token::Text("italic".to_string()),
                        Token::Text(" ".to_string()),
                    ],
                }],
            ),
            (
                "**bold**",
                vec![Token::Emphasis {
                    level: 2,
                    content: vec![
                        Token::Text("bold".to_string()),
                        Token::Text(" ".to_string()),
                    ],
                }],
            ),
            (
                "_also italic_",
                vec![Token::Emphasis {
                    level: 1,
                    content: vec![
                        Token::Text("also italic".to_string()),
                        Token::Text(" ".to_string()),
                    ],
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
        let mut lexer = Lexer::new("![Invalid".to_string());
        assert!(matches!(lexer.parse(), Err(LexerError::UnknownToken(_))));
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
        let tests = vec![(
            "*emphasis with space after*  ",
            vec![Token::Emphasis {
                level: 1,
                content: vec![
                    Token::Text("emphasis with space after".to_string()),
                    Token::Text(" ".to_string()),
                ],
            }],
        )];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
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
        let mut lexer = Lexer::new("![Invalid".to_string());
        let err = lexer.parse().unwrap_err();
        match err {
            LexerError::UnknownToken(msg) => {
                assert!(
                    msg.contains("line"),
                    "error message should contain 'line', got: {}",
                    msg
                );
                assert!(
                    msg.contains("column"),
                    "error message should contain 'column', got: {}",
                    msg
                );
            }
            other => panic!("expected UnknownToken, got {:?}", other),
        }
    }

    #[test]
    fn error_reports_correct_line() {
        // Force an error on line 3 with a malformed image (no URL after `![alt]`).
        // (Unmatched emphasis no longer errors — it falls back to text.)
        let input = "first line\nsecond line\n![alt-no-url";
        let mut lexer = Lexer::new(input.to_string());
        let err = lexer.parse().unwrap_err();
        if let LexerError::UnknownToken(msg) = err {
            assert!(
                msg.contains("line 3"),
                "expected 'line 3' in message, got: {}",
                msg
            );
        } else {
            panic!("expected UnknownToken");
        }
    }

    #[test]
    fn error_variant_still_matches() {
        // Regression: existing test_error_cases pattern still works
        let mut lexer = Lexer::new("![Invalid".to_string());
        assert!(matches!(lexer.parse(), Err(LexerError::UnknownToken(_))));
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
                    content: vec![
                        Token::Text("foo".to_string()),
                        Token::Text(" ".to_string()),
                    ],
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
        // \ followed by \n is not an escape — \n is not punctuation.
        // The backslash stays literal and the newline still terminates the line.
        let tokens = parse("foo\\\nbar");
        // We expect: Text("foo\\") + Newline + Text("bar")
        assert!(matches!(tokens[0], Token::Text(ref s) if s.contains("\\")));
        assert!(tokens.iter().any(|t| matches!(t, Token::Newline)));
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
        // From earlier fix — must remain Text, not trigger fallback weirdness.
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
        // After Fix #2, an unmatched ~~ should not abort.
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
