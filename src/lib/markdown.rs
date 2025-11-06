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
    /// Block quote containing quoted text
    BlockQuote(String),
    /// List item with nested content and type information
    ListItem {
        content: Vec<Token>,
        ordered: bool,
        number: Option<usize>, // For ordered lists (e.g., "1.", "2.")
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
    /// Unknown or malformed token
    Unknown(String),
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
        let mut tokens = Vec::new();

        while self.position < self.input.len() {
            if let Some(token) = self.next_token()? {
                tokens.push(token);
            }
        }

        Ok(tokens)
    }

    /// Helper function to parse nested content until a delimiter is encountered.
    /// Used for parsing content within emphasis, headings, and list items.
    fn parse_nested_content<F>(&mut self, is_delimiter: F) -> Result<Vec<Token>, LexerError>
    where
        F: Fn(char) -> bool,
    {
        let mut content = Vec::new();
        let initial_indent = self.get_current_indent();

        while self.position < self.input.len() {
            let ch = self.current_char();

            if is_delimiter(ch) {
                break;
            }

            // Handle nested content
            if self.is_at_line_start() {
                let current_indent = self.get_current_indent();

                // If more indented than parent, treat as nested content
                if current_indent > initial_indent {
                    self.position += current_indent;

                    match self.current_char() {
                        '-' | '+' => {
                            if !self.check_horizontal_rule()? {
                                content.push(self.parse_list_item(false, current_indent)?);
                                continue;
                            }
                        }
                        '*' => {
                            if self.is_list_marker('*') {
                                content.push(self.parse_list_item(false, current_indent)?);
                                continue;
                            }
                        }
                        '0'..='9' => {
                            if self.check_ordered_list_marker().is_some() {
                                content.push(self.parse_list_item(true, current_indent)?);
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Parse regular content
            if let Some(token) = self.next_token()? {
                content.push(token);
            }
        }

        Ok(content)
    }

    /// Determines the next token in the input stream based on the current character
    /// and context. Handles special cases like line starts differently.
    fn next_token(&mut self) -> Result<Option<Token>, LexerError> {
        // Only skip whitespace if we're not immediately after a special token
        if !self.is_after_special_token() {
            self.skip_whitespace();
        }

        if self.position >= self.input.len() {
            return Ok(None);
        }

        let current_char = self.current_char();
        let is_line_start = self.is_at_line_start();

        let token = match current_char {
            '#' if is_line_start => self.parse_heading()?,
            '*' if is_line_start && self.is_list_marker('*') => self.parse_list_item(false, 0)?,
            '*' | '_' => self.parse_emphasis()?,
            '`' => self.parse_code()?,
            '>' if is_line_start => self.parse_blockquote()?,
            '-' | '+' if is_line_start => {
                if self.check_horizontal_rule()? {
                    Token::HorizontalRule
                } else {
                    self.parse_list_item(false, 0)?
                }
            }
            '0'..='9' if is_line_start => {
                if let Some(_) = self.check_ordered_list_marker() {
                    self.parse_list_item(true, 0)?
                } else {
                    self.parse_text()?
                }
            }
            '[' => self.parse_link()?,
            '!' => {
                // Check if this is a valid image start (! followed by [)
                if self.position + 1 < self.input.len() && self.input[self.position + 1] == '[' {
                    self.parse_image()?
                } else {
                    self.parse_text()?
                }
            }
            '<' if self.is_html_comment_start() => self.parse_html_comment()?,
            '\n' => self.parse_newline()?,
            '|' if is_line_start => {
                if self.is_table_start() {
                    self.parse_table()?
                } else {
                    self.parse_text()?
                }
            }
            _ => self.parse_text()?,
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
        let content = self.parse_nested_content(|c| c == '\n')?;
        Ok(Token::Heading(content, level))
    }

    /// Parses emphasis tokens (* or _) with support for multiple levels (1-3).
    /// Ensures proper matching of opening and closing delimiters.
    fn parse_emphasis(&mut self) -> Result<Token, LexerError> {
        let start_pos = self.position;
        let delimiter = self.current_char();
        let mut level = 0;

        // Count the number of delimiters
        while self.current_char() == delimiter {
            level += 1;
            self.advance();
        }

        let mut content = self.parse_nested_content(|c| c == delimiter)?;
        content.push(Token::Text(String::from(" ")));

        // Ensure proper closing
        for _ in 0..level {
            if self.current_char() != delimiter {
                return Err(LexerError::UnknownToken(format!(
                    "Unmatched emphasis at position {}",
                    start_pos
                )));
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

    /// Helper method to count consecutive backticks
    fn count_backticks(&mut self) -> usize {
        let mut count = 0;
        while self.position < self.input.len() && self.current_char() == '`' {
            count += 1;
            self.advance();
        }
        count
    }

    /// Parses a blockquote, collecting text until newline
    fn parse_blockquote(&mut self) -> Result<Token, LexerError> {
        self.advance();
        self.skip_whitespace();
        let content = self.read_until_newline();
        Ok(Token::BlockQuote(content))
    }

    /// Parses a link token, extracting display text and URL
    fn parse_link(&mut self) -> Result<Token, LexerError> {
        self.advance(); // skip '['
        let text = self.read_until_char(']');
        self.advance(); // skip ']'
        if self.current_char() == '(' {
            self.advance(); // skip '('
            let url = self.read_until_char(')');
            self.advance(); // skip ')'
            return Ok(Token::Link(text, url));
        }
        Ok(Token::Link(text, String::new()))
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
                let url = self.read_until_char(')');
                self.advance(); // skip ')'
                Ok(Token::Image(alt_text, url))
            } else {
                Err(LexerError::UnknownToken(alt_text))
            }
        } else {
            // If '!' is not followed by '[', treat it as regular text
            self.position = start_pos;
            self.parse_text()
        }
    }

    /// Parses a newline token
    fn parse_newline(&mut self) -> Result<Token, LexerError> {
        self.advance();
        Ok(Token::Newline)
    }

    /// Parses regular text until a special token start or newline is encountered.
    /// Returns an error if no text could be parsed.
    fn parse_text(&mut self) -> Result<Token, LexerError> {
        let mut content = String::new();
        let start_pos = self.position;

        // If we're starting with a space after a special token, include it
        if self.position > 0 && self.current_char() == ' ' {
            content.push(' ');
            self.advance();
        }

        while self.position < self.input.len() {
            let ch = self.current_char();

            if ch == '\n' || self.is_start_of_special_token() {
                break;
            }

            content.push(ch);
            self.advance();
        }

        if content.is_empty() {
            Err(LexerError::UnknownToken(format!(
                "Unexpected character at position {}",
                start_pos
            )))
        } else {
            Ok(Token::Text(content))
        }
    }

    /// Parses an HTML comment, extracting the comment content
    fn parse_html_comment(&mut self) -> Result<Token, LexerError> {
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

    /// Checks if current character could start a special token
    fn is_start_of_special_token(&self) -> bool {
        let ch = self.current_char();
        match ch {
            '#' | '*' | '_' | '`' | '[' => true,
            '!' => {
                if self.position + 1 < self.input.len() {
                    self.input[self.position + 1] == '['
                } else {
                    false
                }
            }
            '<' => self.is_html_comment_start(),
            _ => false,
        }
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
    fn parse_list_item(&mut self, ordered: bool, indent_level: usize) -> Result<Token, LexerError> {
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

        let mut content = Vec::new();
        while self.position < self.input.len() && self.current_char() != '\n' {
            if let Some(token) = self.next_token()? {
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
                        content.push(self.parse_list_item(false, current_indent)?);
                    }
                }
                '*' => {
                    if self.is_list_marker('*') {
                        content.push(self.parse_list_item(false, current_indent)?);
                    } else {
                        break;
                    }
                }
                '0'..='9' => {
                    if self.check_ordered_list_marker().is_some() {
                        content.push(self.parse_list_item(true, current_indent)?);
                    }
                }
                _ => break,
            }
        }

        Ok(Token::ListItem {
            content,
            ordered,
            number,
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
            headers.push(cell_lexer.parse()?);
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
                let mut cell_lexer = Lexer::new(cell);
                row_tokens.push(cell_lexer.parse()?);
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
        assert_eq!(
            tokens,
            vec![Token::BlockQuote("This is a quote".to_string())]
        );
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
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Item 2".to_string())],
                        ordered: false,
                        number: None,
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
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Second".to_string())],
                        ordered: true,
                        number: Some(2),
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
                    },
                    Token::ListItem {
                        content: vec![Token::Text("Nested 2".to_string())],
                        ordered: false,
                        number: None,
                    },
                ],
                ordered: false,
                number: None,
            },
            Token::ListItem {
                content: vec![Token::Text("Item 2".to_string())],
                ordered: false,
                number: None,
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
        let tests = vec![
            (
                "> Simple quote",
                vec![Token::BlockQuote("Simple quote".to_string())],
            ),
            (
                "> Quote with *emphasis*",
                vec![Token::BlockQuote("Quote with *emphasis*".to_string())],
            ),
            (
                "> Quote with [link](url)",
                vec![Token::BlockQuote("Quote with [link](url)".to_string())],
            ),
        ];

        for (input, expected) in tests {
            assert_eq!(parse(input), expected);
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
