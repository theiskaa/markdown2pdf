use crate::markdown;
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

            Token::Code {
                language,
                content,
                block,
            } => {
                format!(
                    "{}{{\n{}\"type\": \"Code\",\n{}\"block\": {},\n{}\"language\": \"{}\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    block,
                    inner_indent,
                    language.replace("\"", "\\\""),
                    inner_indent,
                    content.replace("\"", "\\\"").replace("\n", "\\n"),
                    indent
                )
            }

            Token::BlockQuote(body) => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"BlockQuote\",\n", inner_indent));
                result.push_str(&format!("{}\"content\": [\n", inner_indent));
                for (i, token) in body.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < body.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::Admonition {
                kind,
                raw_label,
                title,
                body,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Admonition\",\n", inner_indent));
                result.push_str(&format!("{}\"kind\": \"{}\",\n", inner_indent, kind));
                result.push_str(&format!(
                    "{}\"raw_label\": \"{}\",\n",
                    inner_indent, raw_label
                ));
                if let Some(t) = title {
                    result.push_str(&format!("{}\"title\": [\n", inner_indent));
                    for (i, token) in t.iter().enumerate() {
                        result.push_str(&token.to_readable_json(indent_level + 2));
                        if i < t.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&format!("{}],\n", inner_indent));
                }
                result.push_str(&format!("{}\"body\": [\n", inner_indent));
                for (i, token) in body.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < body.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }

            Token::ListItem {
                content,
                ordered,
                number,
                marker: _,
                checked,
                loose,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"ListItem\",\n", inner_indent));
                result.push_str(&format!("{}\"ordered\": {},\n", inner_indent, ordered));
                result.push_str(&format!("{}\"loose\": {},\n", inner_indent, loose));

                if let Some(num) = number {
                    result.push_str(&format!("{}\"number\": {},\n", inner_indent, num));
                } else {
                    result.push_str(&format!("{}\"number\": null,\n", inner_indent));
                }

                match checked {
                    Some(true) => result.push_str(&format!("{}\"checked\": true,\n", inner_indent)),
                    Some(false) => {
                        result.push_str(&format!("{}\"checked\": false,\n", inner_indent))
                    }
                    None => result.push_str(&format!("{}\"checked\": null,\n", inner_indent)),
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

            Token::Link {
                content,
                url,
                title,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Link\",\n", inner_indent));
                result.push_str(&format!(
                    "{}\"url\": \"{}\",\n",
                    inner_indent,
                    url.replace("\"", "\\\"")
                ));
                if let Some(t) = title {
                    result.push_str(&format!(
                        "{}\"title\": \"{}\",\n",
                        inner_indent,
                        t.replace("\"", "\\\"")
                    ));
                } else {
                    result.push_str(&format!("{}\"title\": null,\n", inner_indent));
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

            Token::Image { alt, url, title } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Image\",\n", inner_indent));
                result.push_str(&format!(
                    "{}\"url\": \"{}\",\n",
                    inner_indent,
                    url.replace("\"", "\\\"")
                ));
                if let Some(t) = title {
                    result.push_str(&format!(
                        "{}\"title\": \"{}\",\n",
                        inner_indent,
                        t.replace("\"", "\\\"")
                    ));
                } else {
                    result.push_str(&format!("{}\"title\": null,\n", inner_indent));
                }
                result.push_str(&format!("{}\"alt\": [\n", inner_indent));
                for (i, token) in alt.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < alt.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
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

            Token::DelimRun { ch, count } => {
                let s = ch.to_string().repeat(*count);
                format!(
                    "{}{{\n{}\"type\": \"DelimRun\",\n{}\"content\": \"{}\"\n{}}}",
                    indent, inner_indent, inner_indent, s, indent
                )
            }

            Token::Table {
                headers,
                aligns,
                rows,
            } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"Table\",\n", inner_indent));

                result.push_str(&format!("{}\"headers\": [\n", inner_indent));
                for (i, header_cell) in headers.iter().enumerate() {
                    result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 2)));
                    for (j, token) in header_cell.content.iter().enumerate() {
                        result.push_str(&token.to_readable_json(indent_level + 3));
                        if j < header_cell.content.len() - 1 {
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

                result.push_str(&format!("{}\"aligns\": [\n", inner_indent));
                for (i, align) in aligns.iter().enumerate() {
                    let align_str = match align {
                        markdown::TableAlignment::Left => "Left",
                        markdown::TableAlignment::Center => "Center",
                        markdown::TableAlignment::Right => "Right",
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

                result.push_str(&format!("{}\"rows\": [\n", inner_indent));
                for (i, row) in rows.iter().enumerate() {
                    result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 2)));
                    for (j, cell) in row.iter().enumerate() {
                        result.push_str(&format!("{}[\n", "  ".repeat(indent_level + 3)));
                        for (k, token) in cell.content.iter().enumerate() {
                            result.push_str(&token.to_readable_json(indent_level + 4));
                            if k < cell.content.len() - 1 {
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
                    markdown::TableAlignment::Left => "Left",
                    markdown::TableAlignment::Center => "Center",
                    markdown::TableAlignment::Right => "Right",
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

            Token::HtmlInline(html) => {
                format!(
                    "{}{{\n{}\"type\": \"HtmlInline\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    html.replace("\"", "\\\""),
                    indent
                )
            }

            Token::HtmlBlock(html) => {
                format!(
                    "{}{{\n{}\"type\": \"HtmlBlock\",\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    html.replace("\"", "\\\""),
                    indent
                )
            }

            Token::HardBreak => format!(
                "{}{{\n{}\"type\": \"HardBreak\"\n{}}}",
                indent, inner_indent, indent
            ),

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
            Token::Strikethrough(body) | Token::Highlight(body) => {
                let type_name = if matches!(self, Token::Highlight(_)) {
                    "Highlight"
                } else {
                    "Strikethrough"
                };
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"{}\",\n", inner_indent, type_name));
                result.push_str(&format!("{}\"content\": [\n", inner_indent));
                for (i, token) in body.iter().enumerate() {
                    result.push_str(&token.to_readable_json(indent_level + 2));
                    if i < body.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
            }
            Token::FootnoteReference(label) => format!(
                "{}{{\n{}\"type\": \"FootnoteReference\",\n{}\"label\": \"{}\"\n{}}}",
                indent,
                inner_indent,
                inner_indent,
                label.replace("\"", "\\\""),
                indent
            ),
            Token::FootnoteDefinition { label, content }
            | Token::InlineFootnote { label, content } => {
                let type_name = if matches!(self, Token::InlineFootnote { .. }) {
                    "InlineFootnote"
                } else {
                    "FootnoteDefinition"
                };
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"{}\",\n", inner_indent, type_name));
                result.push_str(&format!(
                    "{}\"label\": \"{}\",\n",
                    inner_indent,
                    label.replace("\"", "\\\"")
                ));
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
            Token::DefinitionList { entries } => {
                let mut result = format!("{}{{\n", indent);
                result.push_str(&format!("{}\"type\": \"DefinitionList\",\n", inner_indent));
                result.push_str(&format!("{}\"entries\": [\n", inner_indent));
                for (i, entry) in entries.iter().enumerate() {
                    result.push_str(&format!("{}  {{\n", inner_indent));
                    result.push_str(&format!("{}    \"terms\": [\n", inner_indent));
                    for (j, term) in entry.terms.iter().enumerate() {
                        result.push_str(&format!("{}      [\n", inner_indent));
                        for (k, token) in term.iter().enumerate() {
                            result.push_str(&token.to_readable_json(indent_level + 4));
                            if k < term.len() - 1 {
                                result.push(',');
                            }
                            result.push('\n');
                        }
                        result.push_str(&format!("{}      ]", inner_indent));
                        if j < entry.terms.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&format!("{}    ],\n", inner_indent));
                    result.push_str(&format!("{}    \"definitions\": [\n", inner_indent));
                    for (j, def) in entry.definitions.iter().enumerate() {
                        result.push_str(&format!("{}      [\n", inner_indent));
                        for (k, token) in def.iter().enumerate() {
                            result.push_str(&token.to_readable_json(indent_level + 4));
                            if k < def.len() - 1 {
                                result.push(',');
                            }
                            result.push('\n');
                        }
                        result.push_str(&format!("{}      ]", inner_indent));
                        if j < entry.definitions.len() - 1 {
                            result.push(',');
                        }
                        result.push('\n');
                    }
                    result.push_str(&format!("{}    ]\n", inner_indent));
                    result.push_str(&format!("{}  }}", inner_indent));
                    if i < entries.len() - 1 {
                        result.push(',');
                    }
                    result.push('\n');
                }
                result.push_str(&format!("{}]\n", inner_indent));
                result.push_str(&format!("{}}}", indent));
                result
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
            Token::Math { inline, content } => {
                format!(
                    "{}{{\n{}\"type\": \"Math\",\n{}\"inline\": {},\n{}\"content\": \"{}\"\n{}}}",
                    indent,
                    inner_indent,
                    inner_indent,
                    inline,
                    inner_indent,
                    content
                        .replace("\\", "\\\\")
                        .replace("\"", "\\\"")
                        .replace("\n", "\\n"),
                    indent
                )
            }
        }
    }

    /// Renders a single token as a one-line s-expression for compact
    /// diagnostic dumps. Use in test assertions / debug prints where
    /// the multi-line JSON form is too noisy. Example:
    /// `Heading(1, [Text("Hi"), Emphasis(1, [Text("x")])])`
    pub fn to_compact(&self) -> String {
        fn quote(s: &str) -> String {
            let mut out = String::with_capacity(s.len() + 2);
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\t' => out.push_str("\\t"),
                    '\r' => out.push_str("\\r"),
                    c if (c as u32) < 0x20 => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
                    c => out.push(c),
                }
            }
            out.push('"');
            out
        }
        fn list(tokens: &[Token]) -> String {
            let inner: Vec<String> = tokens.iter().map(|t| t.to_compact()).collect();
            format!("[{}]", inner.join(", "))
        }
        match self {
            Token::Heading(content, level) => {
                format!("Heading({}, {})", level, list(content))
            }
            Token::Emphasis { level, content } => {
                format!("Emphasis({}, {})", level, list(content))
            }
            Token::StrongEmphasis(content) => format!("StrongEmphasis({})", list(content)),
            Token::Code {
                language,
                content,
                block,
            } => {
                let kind = if *block { "CodeBlock" } else { "CodeSpan" };
                format!("{}({}, {})", kind, quote(language), quote(content))
            }
            Token::BlockQuote(body) => format!("BlockQuote({})", list(body)),
            Token::Admonition {
                kind,
                raw_label,
                title,
                body,
            } => {
                let title_s = match title {
                    Some(t) => list(t),
                    None => "_".to_string(),
                };
                format!(
                    "Admonition(kind={}, raw={}, title={}, body={})",
                    quote(kind),
                    quote(raw_label),
                    title_s,
                    list(body)
                )
            }
            Token::ListItem {
                content,
                ordered,
                number,
                marker: _,
                checked,
                loose,
            } => {
                let num = match number {
                    Some(n) => n.to_string(),
                    None => "_".to_string(),
                };
                let chk = match checked {
                    Some(true) => "x",
                    Some(false) => " ",
                    None => "_",
                };
                format!(
                    "ListItem(ordered={}, n={}, checked={}, loose={}, {})",
                    ordered,
                    num,
                    chk,
                    loose,
                    list(content)
                )
            }
            Token::Link {
                content,
                url,
                title,
            } => {
                let t = match title {
                    Some(s) => quote(s),
                    None => "_".to_string(),
                };
                format!("Link({}, {}, title={})", list(content), quote(url), t)
            }
            Token::Image { alt, url, title } => {
                let t = match title {
                    Some(s) => quote(s),
                    None => "_".to_string(),
                };
                format!("Image({}, {}, title={})", list(alt), quote(url), t)
            }
            Token::FootnoteReference(label) => format!("FootnoteRef({})", quote(label)),
            Token::FootnoteDefinition { label, content } => {
                format!("FootnoteDef({}, {})", quote(label), list(content))
            }
            Token::InlineFootnote { label, content } => {
                format!("InlineFootnote({}, {})", quote(label), list(content))
            }
            Token::Text(s) => format!("Text({})", quote(s)),
            Token::DelimRun { ch, count } => {
                format!("DelimRun({}{})", ch, count)
            }
            Token::Table {
                headers,
                aligns,
                rows,
            } => {
                let hs: Vec<String> = headers.iter().map(|c| list(&c.content)).collect();
                let aligns_s: Vec<&str> = aligns
                    .iter()
                    .map(|a| match a {
                        markdown::TableAlignment::Left => "L",
                        markdown::TableAlignment::Center => "C",
                        markdown::TableAlignment::Right => "R",
                    })
                    .collect();
                let rs: Vec<String> = rows
                    .iter()
                    .map(|row| {
                        let cells: Vec<String> = row.iter().map(|c| list(&c.content)).collect();
                        format!("[{}]", cells.join(", "))
                    })
                    .collect();
                format!(
                    "Table(headers=[{}], aligns=[{}], rows=[{}])",
                    hs.join(", "),
                    aligns_s.join(", "),
                    rs.join(", ")
                )
            }
            Token::TableAlignment(a) => {
                let s = match a {
                    markdown::TableAlignment::Left => "L",
                    markdown::TableAlignment::Center => "C",
                    markdown::TableAlignment::Right => "R",
                };
                format!("TableAlignment({})", s)
            }
            Token::HtmlComment(content) => format!("HtmlComment({})", quote(content)),
            Token::HtmlInline(html) => format!("HtmlInline({})", quote(html)),
            Token::HtmlBlock(html) => format!("HtmlBlock({})", quote(html)),
            Token::Newline => "Newline".to_string(),
            Token::HardBreak => "HardBreak".to_string(),
            Token::HorizontalRule => "HorizontalRule".to_string(),
            Token::Strikethrough(body) => format!("Strikethrough({})", list(body)),
            Token::Highlight(body) => format!("Highlight({})", list(body)),
            Token::DefinitionList { entries } => {
                let es: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        let defs: Vec<String> = e.definitions.iter().map(|d| list(d)).collect();
                        let terms: Vec<String> = e.terms.iter().map(|t| list(t)).collect();
                        format!("([{}] -> [{}])", terms.join(", "), defs.join(", "))
                    })
                    .collect();
                format!("DefinitionList([{}])", es.join(", "))
            }
            Token::Unknown(s) => format!("Unknown({})", quote(s)),
            Token::Math { inline, content } => {
                let kind = if *inline { "inline" } else { "display" };
                format!("Math({}, {})", kind, quote(content))
            }
        }
    }

    /// Renders a slice of tokens as a compact bracketed list — useful for
    /// dumping a full parse result on a single line.
    pub fn slice_to_compact(tokens: &[Token]) -> String {
        let inner: Vec<String> = tokens.iter().map(|t| t.to_compact()).collect();
        format!("[{}]", inner.join(", "))
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

#[cfg(test)]
mod compact_tests {
    use crate::markdown::Token;

    #[test]
    fn compact_text_quotes_special_chars() {
        let t = Token::Text("a \"b\" \n c \\ d".to_string());
        assert_eq!(t.to_compact(), r#"Text("a \"b\" \n c \\ d")"#);
    }

    #[test]
    fn compact_heading_nested() {
        let t = Token::Heading(
            vec![
                Token::Text("Hi ".to_string()),
                Token::Emphasis {
                    level: 1,
                    content: vec![Token::Text("x".to_string())],
                },
            ],
            2,
        );
        assert_eq!(
            t.to_compact(),
            r#"Heading(2, [Text("Hi "), Emphasis(1, [Text("x")])])"#
        );
    }

    #[test]
    fn compact_list_item_includes_checked() {
        let unchecked = Token::ListItem {
            content: vec![Token::Text("a".into())],
            ordered: false,
            number: None,
            marker: '-',
            checked: Some(false),
            loose: false,
        };
        let checked = Token::ListItem {
            content: vec![Token::Text("a".into())],
            ordered: false,
            number: None,
            marker: '-',
            checked: Some(true),
            loose: false,
        };
        let regular = Token::ListItem {
            content: vec![Token::Text("a".into())],
            ordered: true,
            number: Some(3),
            marker: '.',
            checked: None,
            loose: true,
        };
        assert_eq!(
            unchecked.to_compact(),
            r#"ListItem(ordered=false, n=_, checked= , loose=false, [Text("a")])"#
        );
        assert_eq!(
            checked.to_compact(),
            r#"ListItem(ordered=false, n=_, checked=x, loose=false, [Text("a")])"#
        );
        assert_eq!(
            regular.to_compact(),
            r#"ListItem(ordered=true, n=3, checked=_, loose=true, [Text("a")])"#
        );
    }

    #[test]
    fn compact_simple_atoms() {
        assert_eq!(Token::Newline.to_compact(), "Newline");
        assert_eq!(Token::HardBreak.to_compact(), "HardBreak");
        assert_eq!(Token::HorizontalRule.to_compact(), "HorizontalRule");
    }

    #[test]
    fn compact_slice_helper() {
        let tokens = vec![
            Token::Text("a".to_string()),
            Token::HardBreak,
            Token::Text("b".to_string()),
        ];
        assert_eq!(
            Token::slice_to_compact(&tokens),
            r#"[Text("a"), HardBreak, Text("b")]"#
        );
    }
}
