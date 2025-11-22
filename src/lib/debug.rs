use crate::markdown::Token;

impl Token {
    /// Saves tokens to a JSON file for visualization.
    /// Recursively formats nested tokens with proper indentation.
    ///
    /// # Arguments
    /// * `tokens` - The tokens to save
    /// * `file_path` - Path to the output JSON file (e.g., "tokens.json")
    ///
    /// # Returns
    /// Result indicating success or IO error
    ///
    /// # Example
    /// ```no_run
    /// use markdown2pdf::markdown::Lexer;
    ///
    /// let mut lexer = Lexer::new("# Title".to_string());
    /// let tokens = lexer.parse().unwrap();
    /// markdown2pdf::markdown::Token::save_to_json_file(tokens, "tokens.json").unwrap();
    /// ```
    pub fn save_to_json_file(tokens: Vec<Token>, file_path: &str) -> std::io::Result<()> {
        let json_content = Self::tokens_to_readable_json(tokens);
        std::fs::write(file_path, json_content)?;
        Ok(())
    }

    /// Converts a token into a readable JSON representation for visualization.
    /// Recursively formats nested tokens with proper indentation.
    fn to_readable_json(&self, indent_level: usize) -> String {
        let indent = "  ".repeat(indent_level);
        let inner_indent = "  ".repeat(indent_level + 1);

        match self {
            Token::Heading(content, level) => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Heading\",\n", inner_indent));
                result.push_str(&format!("{}\"level\": {},\n", inner_indent, level));
                result.push_str(&format!("{}\"content\": [\n", inner_indent));

                for (i, token) in content.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < content.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }

                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::Emphasis { level, content } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Emphasis\",\n", inner_indent));
                result.push_str(&format!("{}\"level\": {},\n", inner_indent, level));
                result.push_str(&format!("{}\"content\": [\n", inner_indent));

                for (i, token) in content.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < content.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }

                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::StrongEmphasis(content) => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"StrongEmphasis\",\n", inner_indent));
                result.push_str(&format!("{}\"content\": [\n", inner_indent));

                for (i, token) in content.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < content.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }

                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::Code(language, content) => {
                format!("{}{{\n{}\"type\": \"Code\",\n{}\"language\": \"{}\",\n{}\"content\": \"{}\"\n{}}}",
                    indent, inner_indent, inner_indent,
                    language.replace("\"", "\\\""), inner_indent,
                    content.replace("\"", "\\\"").replace("\n", "\\n"), indent)
            }

            Token::BlockQuote(content) => {
                format!(
                    "{}{{\n{}\"type\": \"BlockQuote\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    content.replace("\"", "\\\""),
                    indent
                )
            }

            Token::ListItem {
                content,
                ordered,
                number,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"ListItem\",\n", inner_indent));
                result.push_str(&format!("{}\"ordered\": {},\n", inner_indent, ordered));

                if let Some(num) = number {
                    result.push_str(&format!("{}\"number\": {},\n", inner_indent, num));
                } else {
                    result.push_str(&format!("{}\"number\": null,\n", inner_indent));
                }

                result.push_str(&format!("{}\"content\": [\n", inner_indent));

                for (i, token) in content.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < content.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }

                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::Link(text, url) => {
                format!(
                    "{}{{\n{}\"type\": \"Link\",\n{}\"text\": \"{}\",\n{}\"url\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    text.replace("\"", "\\\""),
                    inner_indent,
                    url.replace("\"", "\\\""),
                    indent
                )
            }

            Token::Image(alt_text, url) => {
                format!("{}{{\n{}\"type\": \"Image\",\n{}\"alt_text\": \"{}\",\n{}\"url\": \"{}\"\n{}}}",
                    indent, inner_indent, inner_indent,
                    alt_text.replace("\"", "\\\""), inner_indent,
                    url.replace("\"", "\\\""), indent)
            }

            Token::Text(content) => {
                format!(
                    "{}{{\n{}\"type\": \"Text\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    content.replace("\"", "\\\"").replace("\n", "\\n"),
                    indent
                )
            }

            Token::Table {
                headers,
                aligns,
                rows,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Table\",\n", inner_indent));

                // Headers
                result.push_str(&format!("{}\"headers\": [\n", inner_indent));
                for (i, header_cell) in headers.iter().enumerate() {
                    result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 2)));
                    for (j, token) in header_cell.iter().enumerate() {
                        result.push_str(&token.to_readable_json(indent_level + 3));
                        if j < header_cell.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&format!("{}]", "  ".repeat(indent_level + 2)));
                    if i < headers.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}],\n", inner_indent));

                // Alignments
                result.push_str(&format!("{}\"aligns\": [\n", inner_indent));
                for (i, align) in aligns.iter().enumerate() {
                    let align_str = match align {
                        genpdfi::Alignment::Left => "Left",
                        genpdfi::Alignment::Center => "Center",
                        genpdfi::Alignment::Right => "Right",
                    };
                    result.push_str(&format!(
                        "{}\"{}\"",
                        "  ".repeat(indent_level + 2),
                        align_str
                    ));
                    if i < aligns.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}],\n", inner_indent));

                // Rows
                result.push_str(&format!("{}\"rows\": [\n", inner_indent));
                for (i, row) in rows.iter().enumerate() {
                    result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 2)));
                    for (j, cell) in row.iter().enumerate() {
                        result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 3)));
                        for (k, token) in cell.iter().enumerate() {
                            result.push_str(&token.to_readable_json(indent_level + 4));
                            if k < cell.len() - 1 {
                                result.push(',');
                            }
                            result.push('\n');
                        }
                        result.push_str(&format!("{}]", "  ".repeat(indent_level + 3)));
                        if j < row.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&format!("{}]", "  ".repeat(indent_level + 2)));
                    if i < rows.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::TableAlignment(align) => {
                let align_str = match align {
                    genpdfi::Alignment::Left => "Left",
                    genpdfi::Alignment::Center => "Center",
                    genpdfi::Alignment::Right => "Right",
                };
                format!(
                    "{}{{\n{}\"type\": \"TableAlignment\",\n{}\"alignment\": \"{}\"\n{}}}",
                    indent, inner_indent, inner_indent, align_str, indent
                )
            }

            Token::HtmlComment(content) => {
                format!(
                    "{}{{\n{}\"type\": \"HtmlComment\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    content.replace("\"", "\\\""),
                    indent
                )
            }

            Token::Newline => {
                format!(
                    "{}{{\n{}\"type\": \"Newline\"\n{}}}",
                    indent, inner_indent, indent
                )
            }

            Token::HorizontalRule => {
                format!(
                    "{}{{\n{}\"type\": \"HorizontalRule\"\n{}}}",
                    indent, inner_indent, indent
                )
            }
            Token::Unknown(content) => {
                format!(
                    "{}{{\n{}\"type\": \"Unknown\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    content.replace("\"", "\\\""),
                    indent
                )
            }
        }
    }

    /// Convenience method to convert a vector of tokens into a readable JSON array.
    fn tokens_to_readable_json(tokens: Vec<Token>) -> String {
        let mut result = String::from("[\n");

        for (i, token) in tokens.iter().enumerate() {
            result.push_str(&token.to_readable_json(1));
            if i < tokens.len() - 1 {
                result.push(',');
            }
            result.push('\n');
        }

        result.push(']');
        result
    }
}
