//! Test-only AST → HTML renderer used by the CommonMark spec runner.
//!
//! This renderer exists *only* to compare lexer output against the canonical
//! spec HTML. It is not the PDF renderer and not part of the production
//! library. Keep it intentionally narrow: spec-compliant output for every
//! Token variant we currently emit, no formatting beyond what the spec
//! requires, no styling.

use markdown2pdf::markdown::Token;

pub fn render(tokens: &[Token]) -> String {
    let mut out = String::new();
    render_blocks(tokens, &mut out, false);
    out
}

fn render_blocks(tokens: &[Token], out: &mut String, in_loose_list_item: bool) {
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Newline => {
                i += 1;
            }
            Token::Heading(content, level) => {
                out.push_str(&format!("<h{}>", level));
                render_inlines(content, out);
                out.push_str(&format!("</h{}>\n", level));
                i += 1;
            }
            Token::BlockQuote(body) => {
                out.push_str("<blockquote>\n");
                render_blocks(body, out, false);
                out.push_str("</blockquote>\n");
                i += 1;
            }
            Token::Admonition { kind, body, .. } => {
                out.push_str(&format!(
                    "<aside class=\"admonition admonition-{}\">\n",
                    escape_attr(kind)
                ));
                render_blocks(body, out, false);
                out.push_str("</aside>\n");
                i += 1;
            }
            Token::HorizontalRule => {
                out.push_str("<hr />\n");
                i += 1;
            }
            Token::Code { language, content, block: true } => {
                out.push_str("<pre><code");
                let lang_first = language.split_whitespace().next().unwrap_or("");
                if !lang_first.is_empty() {
                    out.push_str(" class=\"language-");
                    out.push_str(&escape_attr(lang_first));
                    out.push('"');
                }
                out.push('>');
                out.push_str(&escape_text(content));
                if !content.is_empty() {
                    out.push('\n');
                }
                out.push_str("</code></pre>\n");
                i += 1;
            }
            Token::ListItem { ordered, .. } => {
                let group_end = scan_list_end(tokens, i);
                let loose = group_has_loose_item(&tokens[i..group_end]);
                let start_num = list_start_number(&tokens[i]);
                render_list(&tokens[i..group_end], *ordered, start_num, loose, out);
                i = group_end;
            }
            Token::Table { headers, aligns, rows } => {
                render_table(headers, aligns, rows, out);
                i += 1;
            }
            Token::HtmlComment(content) => {
                // Short comment forms from CommonMark §6.6 round-trip
                // to themselves rather than the wrapped `<!--body-->`
                // shape — empty body is `<!-->`, single-hyphen body
                // is `<!--->`.
                match content.as_str() {
                    "" => out.push_str("<!-->\n"),
                    "-" => out.push_str("<!--->\n"),
                    _ => {
                        out.push_str("<!--");
                        out.push_str(content);
                        out.push_str("-->\n");
                    }
                }
                i += 1;
            }
            Token::HtmlBlock(content) => {
                // HTML block content is verbatim per CommonMark §4.6.
                // The trailing newline mirrors the spec runner's expected
                // output for block-level constructs.
                out.push_str(content);
                if !content.ends_with('\n') {
                    out.push('\n');
                }
                i += 1;
            }
            // Inline tokens at this level form a paragraph (or, inside a
            // tight-list item, a bare inline run).
            _ => {
                let para_end = find_inline_run_end(tokens, i);
                if in_loose_list_item {
                    // Caller controls whether to wrap; we never wrap when
                    // already inside loose-list-item content.
                    render_inlines(&tokens[i..para_end], out);
                } else {
                    out.push_str("<p>");
                    render_inlines(&tokens[i..para_end], out);
                    out.push_str("</p>\n");
                }
                i = para_end;
            }
        }
    }
}

fn is_code_block_tok(tok: &Token) -> bool {
    matches!(tok, Token::Code { block: true, .. })
}

fn scan_list_end(tokens: &[Token], start: usize) -> usize {
    // A list run is consecutive ListItem tokens, possibly interspersed with
    // Newline tokens. It ends at the first non-(ListItem/Newline) token or
    // end of slice. All items in the run must share the same `ordered`
    // AND `marker` — a marker switch (`- foo\n+ bar`, `1. a\n1) b`) breaks
    // the run into two separate lists.
    let (first_ordered, first_marker) = match &tokens[start] {
        Token::ListItem { ordered, marker, .. } => (*ordered, *marker),
        _ => return start + 1,
    };
    let mut i = start + 1;
    let mut last_item = start;
    while i < tokens.len() {
        match &tokens[i] {
            Token::ListItem { ordered, marker, .. }
                if *ordered == first_ordered && *marker == first_marker =>
            {
                last_item = i;
                i += 1;
            }
            Token::Newline => i += 1,
            _ => break,
        }
    }
    last_item + 1
}

fn group_has_loose_item(group: &[Token]) -> bool {
    group.iter().any(|t| matches!(t, Token::ListItem { loose: true, .. }))
}

fn list_start_number(tok: &Token) -> Option<usize> {
    if let Token::ListItem { ordered: true, number, .. } = tok {
        *number
    } else {
        None
    }
}

fn render_list(
    items: &[Token],
    ordered: bool,
    start_num: Option<usize>,
    loose: bool,
    out: &mut String,
) {
    if ordered {
        match start_num {
            Some(1) | None => out.push_str("<ol>\n"),
            Some(n) => out.push_str(&format!("<ol start=\"{}\">\n", n)),
        }
    } else {
        out.push_str("<ul>\n");
    }
    for tok in items {
        if let Token::ListItem { content, checked, .. } = tok {
            render_list_item(content, *checked, loose, out);
        }
    }
    out.push_str(if ordered { "</ol>\n" } else { "</ul>\n" });
}

fn render_list_item(
    content: &[Token],
    checked: Option<bool>,
    loose: bool,
    out: &mut String,
) {
    out.push_str("<li>");
    if let Some(c) = checked {
        let mark = if c { "checked=\"\" " } else { "" };
        out.push_str(&format!(
            "<input {mark}disabled=\"\" type=\"checkbox\" /> "
        ));
    }
    // Split content into: leading inline run + nested block content (nested
    // lists, etc.). Nested ListItems are passed back to render_blocks.
    let (inline_run, nested) = split_item_content(content);
    if loose {
        out.push('\n');
        // Loose-list item content may contain blank-line-separated
        // paragraphs (encoded as ≥2 consecutive Newline tokens). Walk the
        // inline_run, emit a `<p>...</p>` for each paragraph chunk.
        for chunk in split_paragraphs(&inline_run) {
            out.push_str("<p>");
            render_inlines(&chunk, out);
            out.push_str("</p>\n");
        }
        if !nested.is_empty() {
            render_blocks(&nested, out, false);
        }
    } else {
        render_inlines(&inline_run, out);
        if !nested.is_empty() {
            out.push('\n');
            render_tight_blocks(&nested, out);
        }
    }
    out.push_str("</li>\n");
}

/// Block renderer specialized for tight list items: same as `render_blocks`
/// except inline runs are emitted without `<p>` wrapping. A tight item's
/// inline paragraphs render bare; block children (headings, fenced code,
/// nested lists) still get their normal markup.
fn render_tight_blocks(tokens: &[Token], out: &mut String) {
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Newline => {
                i += 1;
            }
            Token::Heading(content, level) => {
                out.push_str(&format!("<h{}>", level));
                render_inlines(content, out);
                out.push_str(&format!("</h{}>\n", level));
                i += 1;
            }
            Token::BlockQuote(body) => {
                out.push_str("<blockquote>\n");
                render_blocks(body, out, false);
                out.push_str("</blockquote>\n");
                i += 1;
            }
            Token::Admonition { kind, body, .. } => {
                out.push_str(&format!(
                    "<aside class=\"admonition admonition-{}\">\n",
                    escape_attr(kind)
                ));
                render_blocks(body, out, false);
                out.push_str("</aside>\n");
                i += 1;
            }
            Token::HorizontalRule => {
                out.push_str("<hr />\n");
                i += 1;
            }
            Token::Code { language, content, block: true } => {
                out.push_str("<pre><code");
                let lang_first = language.split_whitespace().next().unwrap_or("");
                if !lang_first.is_empty() {
                    out.push_str(" class=\"language-");
                    out.push_str(&escape_attr(lang_first));
                    out.push('"');
                }
                out.push('>');
                out.push_str(&escape_text(content));
                if !content.is_empty() {
                    out.push('\n');
                }
                out.push_str("</code></pre>\n");
                i += 1;
            }
            Token::ListItem { ordered, .. } => {
                let group_end = scan_list_end(tokens, i);
                let loose = group_has_loose_item(&tokens[i..group_end]);
                let start_num = list_start_number(&tokens[i]);
                render_list(&tokens[i..group_end], *ordered, start_num, loose, out);
                i = group_end;
            }
            _ => {
                let para_end = find_inline_run_end(tokens, i);
                render_inlines(&tokens[i..para_end], out);
                i = para_end;
            }
        }
    }
}

fn split_paragraphs(tokens: &[Token]) -> Vec<Vec<Token>> {
    let mut out = Vec::new();
    let mut buf: Vec<Token> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if matches!(tokens[i], Token::Newline)
            && i + 1 < tokens.len()
            && matches!(tokens[i + 1], Token::Newline)
        {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
            }
            while i < tokens.len() && matches!(tokens[i], Token::Newline) {
                i += 1;
            }
            continue;
        }
        buf.push(tokens[i].clone());
        i += 1;
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn split_item_content(content: &[Token]) -> (Vec<Token>, Vec<Token>) {
    // Walk content. Tokens before the first block-level child go in the
    // leading inline run. From the first block-level child onward, everything
    // goes in the nested vec.
    let mut inline = Vec::new();
    let mut nested: Vec<Token> = Vec::new();
    let mut switched = false;
    for t in content {
        if !switched {
            if is_block_level(t) {
                switched = true;
                nested.push(t.clone());
            } else {
                inline.push(t.clone());
            }
        } else {
            nested.push(t.clone());
        }
    }
    (inline, nested)
}

fn is_block_level(tok: &Token) -> bool {
    matches!(
        tok,
        Token::Heading(_, _)
            | Token::BlockQuote(_)
            | Token::ListItem { .. }
            | Token::HorizontalRule
            | Token::Table { .. }
            | Token::HtmlBlock(_)
    ) || is_code_block_tok(tok)
}

fn find_inline_run_end(tokens: &[Token], start: usize) -> usize {
    // An inline run ends at: (a) a block-level token, or (b) two consecutive
    // Newlines (blank line / paragraph break), or (c) end of slice.
    let mut i = start;
    while i < tokens.len() {
        if is_block_level(&tokens[i]) {
            break;
        }
        if matches!(&tokens[i], Token::Newline)
            && i + 1 < tokens.len()
            && matches!(&tokens[i + 1], Token::Newline)
        {
            break;
        }
        i += 1;
    }
    i
}

fn render_inlines(tokens: &[Token], out: &mut String) {
    // Strip leading/trailing Newlines from the inline run — they're paragraph
    // boundaries, not content.
    let start = tokens.iter().position(|t| !matches!(t, Token::Newline)).unwrap_or(tokens.len());
    let end = tokens
        .iter()
        .rposition(|t| !matches!(t, Token::Newline))
        .map(|p| p + 1)
        .unwrap_or(start);
    for t in &tokens[start..end] {
        render_inline_token(t, out);
    }
}

fn render_inline_token(t: &Token, out: &mut String) {
    match t {
        Token::Text(s) => out.push_str(&escape_text(s)),
        Token::DelimRun { ch, count } => {
            for _ in 0..*count {
                out.push(*ch);
            }
        }
        Token::Emphasis { level, content } => match level {
            1 => {
                out.push_str("<em>");
                render_inlines(content, out);
                out.push_str("</em>");
            }
            2 => {
                out.push_str("<strong>");
                render_inlines(content, out);
                out.push_str("</strong>");
            }
            _ => {
                out.push_str("<em><strong>");
                render_inlines(content, out);
                out.push_str("</strong></em>");
            }
        },
        Token::StrongEmphasis(content) => {
            out.push_str("<strong>");
            render_inlines(content, out);
            out.push_str("</strong>");
        }
        Token::Strikethrough(content) => {
            out.push_str("<del>");
            render_inlines(content, out);
            out.push_str("</del>");
        }
        Token::Highlight(content) => {
            out.push_str("<mark>");
            render_inlines(content, out);
            out.push_str("</mark>");
        }
        Token::Code { content: body, .. } => {
            out.push_str("<code>");
            out.push_str(&escape_text(body));
            out.push_str("</code>");
        }
        Token::Link { content, url, title } => {
            out.push_str("<a href=\"");
            out.push_str(&escape_url(url));
            out.push('"');
            if let Some(t) = title {
                out.push_str(" title=\"");
                out.push_str(&escape_attr(t));
                out.push('"');
            }
            out.push('>');
            render_inlines(content, out);
            out.push_str("</a>");
        }
        Token::Image { alt, url, title } => {
            out.push_str("<img src=\"");
            out.push_str(&escape_url(url));
            out.push_str("\" alt=\"");
            out.push_str(&escape_attr(&Token::collect_all_text(alt)));
            out.push('"');
            if let Some(t) = title {
                out.push_str(" title=\"");
                out.push_str(&escape_attr(t));
                out.push('"');
            }
            out.push_str(" />");
        }
        Token::HtmlInline(html) => out.push_str(html),
        Token::HtmlComment(content) => match content.as_str() {
            "" => out.push_str("<!-->"),
            "-" => out.push_str("<!--->"),
            _ => {
                out.push_str("<!--");
                out.push_str(content);
                out.push_str("-->");
            }
        },
        Token::HardBreak => out.push_str("<br />\n"),
        Token::Newline => out.push('\n'),
        Token::HorizontalRule => out.push_str("<hr />\n"),
        Token::Heading(_, _)
        | Token::BlockQuote(_)
        | Token::ListItem { .. }
        | Token::Table { .. }
        | Token::TableAlignment(_)
        | Token::HtmlBlock(_) => {
            // Block tokens shouldn't appear at inline position. If they do,
            // emit them as a block escape hatch.
            render_blocks(std::slice::from_ref(t), out, false);
        }
        Token::Unknown(s) => out.push_str(&escape_text(s)),
        Token::FootnoteReference(label) => {
            out.push_str("<sup class=\"footnote-ref\">");
            out.push_str(&escape_text(label));
            out.push_str("</sup>");
        }
        Token::FootnoteDefinition { label, content } => {
            // CommonMark HTML render emits these as a `<div class="footnote">`
            // — for spec coverage we just escape the contents.
            out.push_str("<div class=\"footnote\" id=\"");
            out.push_str(&escape_text(label));
            out.push_str("\">");
            render_inlines(content, out);
            out.push_str("</div>");
        }
        Token::InlineFootnote { label, content } => {
            // Pandoc inline footnote. Not a CommonMark construct; this
            // helper just keeps the marker + body visible for coverage.
            out.push_str("<sup class=\"footnote-ref\">");
            out.push_str(&escape_text(label));
            out.push_str("</sup><span class=\"footnote-inline\">");
            render_inlines(content, out);
            out.push_str("</span>");
        }
        Token::DefinitionList { entries } => {
            out.push_str("<dl>");
            for entry in entries {
                out.push_str("<dt>");
                render_inlines(&entry.term, out);
                out.push_str("</dt>");
                for def in &entry.definitions {
                    out.push_str("<dd>");
                    render_inlines(def, out);
                    out.push_str("</dd>");
                }
            }
            out.push_str("</dl>");
        }
        Token::Math { inline, content } => {
            // Pandoc / MathJax-compatible delimiters. No CommonMark
            // spec example produces math, so this only needs to be
            // sensible and compile.
            let (open, close) = if *inline {
                ("\\(", "\\)")
            } else {
                ("\\[", "\\]")
            };
            out.push_str(open);
            out.push_str(&escape_text(content));
            out.push_str(close);
        }
        // Block-level token; never reached from the inline renderer
        // but the match needs to stay exhaustive.
        Token::Admonition { .. } => {}
    }
}

fn render_table(
    headers: &[markdown2pdf::markdown::TableCell<Token>],
    aligns: &[markdown2pdf::markdown::TableAlignment],
    rows: &[Vec<markdown2pdf::markdown::TableCell<Token>>],
    out: &mut String,
) {
    out.push_str("<table>\n<thead>\n<tr>\n");
    for (i, cell) in headers.iter().enumerate() {
        if cell.covered {
            continue;
        }
        let align = aligns.get(i).copied().unwrap_or(markdown2pdf::markdown::TableAlignment::Left);
        let style = match align {
            markdown2pdf::markdown::TableAlignment::Left => "",
            markdown2pdf::markdown::TableAlignment::Center => " style=\"text-align: center\"",
            markdown2pdf::markdown::TableAlignment::Right => " style=\"text-align: right\"",
        };
        let span = if cell.colspan > 1 {
            format!(" colspan=\"{}\"", cell.colspan)
        } else {
            String::new()
        };
        out.push_str(&format!("<th{}{}>", style, span));
        render_inlines(&cell.content, out);
        out.push_str("</th>\n");
    }
    out.push_str("</tr>\n</thead>\n");
    if !rows.is_empty() {
        out.push_str("<tbody>\n");
        for row in rows {
            out.push_str("<tr>\n");
            for (i, cell) in row.iter().enumerate() {
                if cell.covered {
                    continue;
                }
                let align = aligns.get(i).copied().unwrap_or(markdown2pdf::markdown::TableAlignment::Left);
                let style = match align {
                    markdown2pdf::markdown::TableAlignment::Left => "",
                    markdown2pdf::markdown::TableAlignment::Center => " style=\"text-align: center\"",
                    markdown2pdf::markdown::TableAlignment::Right => " style=\"text-align: right\"",
                };
                let mut span = String::new();
                if cell.colspan > 1 {
                    span.push_str(&format!(" colspan=\"{}\"", cell.colspan));
                }
                if cell.rowspan > 1 {
                    span.push_str(&format!(" rowspan=\"{}\"", cell.rowspan));
                }
                out.push_str(&format!("<td{}{}>", style, span));
                render_inlines(&cell.content, out);
                out.push_str("</td>\n");
            }
            out.push_str("</tr>\n");
        }
        out.push_str("</tbody>\n");
    }
    out.push_str("</table>\n");
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    escape_text(s)
}

fn escape_url(s: &str) -> String {
    // CommonMark reference output percent-encodes URL bytes outside a
    // specific "safe" ASCII set, then HTML-escapes `&` `<` `>` `"`. The safe
    // set comes from the cmark reference impl: alphanumerics + a fixed
    // punctuation list. Anything else (non-ASCII, spaces, backslash, etc.)
    // is %-encoded byte-by-byte. Then the result is HTML-escaped.
    fn is_url_safe(b: u8) -> bool {
        matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9')
            || matches!(
                b,
                b'-' | b'_' | b'.' | b'~' | b'!' | b'*' | b'\'' | b'('
                | b')' | b';' | b':' | b'@' | b'&' | b'=' | b'+'
                | b'$' | b',' | b'/' | b'?' | b'#' | b'%'
            )
    }
    let mut percent = String::with_capacity(s.len());
    for byte in s.bytes() {
        if is_url_safe(byte) {
            percent.push(byte as char);
        } else {
            percent.push_str(&format!("%{:02X}", byte));
        }
    }
    let mut out = String::with_capacity(percent.len());
    for c in percent.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}
