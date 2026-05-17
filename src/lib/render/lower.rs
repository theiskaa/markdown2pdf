//! Token tree -> block IR.
//!
//! Walks the [`Token`] stream once and emits a flat sequence of
//! [`Block`]s. Inline runs inside a paragraph or heading are
//! flattened (bold/italic/monospace propagate through nested
//! [`Token::Emphasis`]/[`Token::StrongEmphasis`]/[`Token::Code`]).
//!
//! Tokens that don't have a dedicated `Block` variant degrade to
//! a paragraph containing their collected text — the content still
//! appears, just without distinctive layout.

use crate::markdown::Token;

use super::ir::{Block, DefinitionEntry, FootnoteEntry, InlineRun, ListBullet, ListEntry, RunFlags};
use std::collections::HashMap;

/// Lower a slice of top-level tokens into the block IR.
pub fn lower(tokens: &[Token]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut buffered_inline: Vec<InlineRun> = Vec::new();

    // First-reference-order numbering for footnotes. The map is built
    // once over the entire token tree; flatten_one consults it to
    // render references with the right superscript number, and the
    // post-pass walks definitions in numeric order.
    let footnote_numbers = collect_footnote_numbering(tokens);
    let mut footnote_definitions: HashMap<String, Vec<InlineRun>> = HashMap::new();
    // Inline footnotes (`text^[body]`) carry their definition with
    // them; pull every body out up-front so it lands in the tail
    // "Footnotes" section without disturbing the inline stream.
    collect_inline_footnote_defs(tokens, &footnote_numbers, &mut footnote_definitions);

    // Inline-HTML scope tracking at the top level (sup/sub/u/s/del/
    // small/kbd). Mirrors `flatten_inline`'s nested-context handling.
    let mut root_html_depth = InlineHtmlDepth::default();

    fn flush_paragraph(out: &mut Vec<Block>, buffered: &mut Vec<InlineRun>) {
        if !buffered.iter().all(|r| r.text.trim().is_empty()) {
            out.push(Block::Paragraph {
                runs: std::mem::take(buffered),
            });
        }
        buffered.clear();
    }

    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Newline => {
                let mut run = 0usize;
                while i + run < tokens.len() && matches!(tokens[i + run], Token::Newline) {
                    run += 1;
                }
                if run >= 2 {
                    flush_paragraph(&mut out, &mut buffered_inline);
                } else if !buffered_inline.is_empty() {
                    push_text(&mut buffered_inline, " ", RunFlags::default(), None);
                }
                i += run;
            }
            Token::HardBreak => {
                flush_paragraph(&mut out, &mut buffered_inline);
                i += 1;
            }
            Token::Heading(content, level) => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let runs = flatten_inline(content, RunFlags::default(), None, &footnote_numbers);
                out.push(Block::Heading {
                    level: (*level).clamp(1, 6) as u8,
                    runs,
                });
                i += 1;
            }
            Token::Code {
                content,
                block: true,
                ..
            } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let lines = content.split('\n').map(|s| s.to_string()).collect();
                out.push(Block::CodeBlock { lines });
                i += 1;
            }
            Token::HorizontalRule => {
                flush_paragraph(&mut out, &mut buffered_inline);
                out.push(Block::HorizontalRule);
                i += 1;
            }
            Token::HtmlBlock(content) => {
                flush_paragraph(&mut out, &mut buffered_inline);
                if is_pagebreak_marker(content) {
                    out.push(Block::PageBreak);
                } else if let Some(img) = parse_html_img_block(content) {
                    out.push(Block::Image {
                        path: std::path::PathBuf::from(&img.src),
                        alt: img.alt,
                        caption: img.title,
                    });
                } else if is_framing_only_html(content) {
                    // Standalone <p>, </p>, <div>, </div>, <center>,
                    // </center>: pure GFM wrappers around real
                    // markdown. Rendering them verbatim noisy; dropping
                    // them lets the wrapped content render normally.
                } else if !is_only_html_comments(content) {
                    out.push(Block::HtmlBlock {
                        content: content.clone(),
                    });
                }
                i += 1;
            }
            Token::BlockQuote(body) => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let nested = lower(body);
                out.push(Block::BlockQuote { body: nested });
                i += 1;
            }
            Token::DefinitionList { entries } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let ir_entries: Vec<DefinitionEntry> = entries
                    .iter()
                    .map(|e| DefinitionEntry {
                        term: flatten_inline(&e.term, RunFlags::default(), None, &footnote_numbers),
                        definitions: e
                            .definitions
                            .iter()
                            .map(|d| flatten_inline(d, RunFlags::default(), None, &footnote_numbers))
                            .collect(),
                    })
                    .collect();
                out.push(Block::DefinitionList { entries: ir_entries });
                i += 1;
            }
            Token::FootnoteDefinition { label, content } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                // Definitions don't produce a Block at their source
                // position; they're collected into a single
                // `Block::FootnoteDefinitions` appended at the end of
                // the document below. Pre-flatten the content's
                // inline runs so the post-pass doesn't have to lower
                // recursively.
                let runs = flatten_inline(content, RunFlags::default(), None, &footnote_numbers);
                footnote_definitions
                    .entry(label.clone())
                    .or_insert(runs);
                i += 1;
            }
            Token::ListItem { .. } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                // Slurp every consecutive sibling ListItem into one
                // List block. Items with different markers (`-` then
                // `*` etc.) currently merge into one list; CommonMark
                // says marker changes should start a new list, which
                // we'll fix when it actually bites.
                let mut entries = Vec::new();
                while i < tokens.len() {
                    let Token::ListItem {
                        content,
                        ordered,
                        number,
                        checked,
                        loose,
                        ..
                    } = &tokens[i]
                    else {
                        break;
                    };
                    entries.push(make_list_entry(
                        *ordered,
                        *number,
                        *checked,
                        *loose,
                        content,
                        &footnote_numbers,
                    ));
                    i += 1;
                    // Skip blank lines between list items so we don't
                    // mistake the next item for the start of a new
                    // block.
                    while i < tokens.len() && matches!(tokens[i], Token::Newline) {
                        i += 1;
                    }
                }
                out.push(Block::List { entries });
            }
            Token::Table {
                headers,
                aligns,
                rows,
            } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let head_runs: Vec<Vec<InlineRun>> = headers
                    .iter()
                    .map(|cell| flatten_inline(cell, RunFlags::default(), None, &footnote_numbers))
                    .collect();
                let row_runs: Vec<Vec<Vec<InlineRun>>> = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| flatten_inline(cell, RunFlags::default(), None, &footnote_numbers))
                            .collect()
                    })
                    .collect();
                out.push(Block::Table {
                    headers: head_runs,
                    aligns: aligns.clone(),
                    rows: row_runs,
                });
                i += 1;
            }
            // A bare Token::Image at the top level (not surrounded
            // by other inline content) gets promoted to a block-level
            // image. We require the buffered paragraph to be empty
            // and the next non-newline token to be either EOF or
            // another block boundary.
            Token::Image { alt, url, title }
                if buffered_inline.is_empty() && image_is_standalone(tokens, i) =>
            {
                let is_url = url.starts_with("http://") || url.starts_with("https://");
                let path = std::path::PathBuf::from(url);
                if is_url || path.exists() {
                    let alt_text = crate::markdown::Token::collect_all_text(alt);
                    out.push(Block::Image {
                        path,
                        alt: alt_text,
                        caption: title.clone(),
                    });
                    i += 1;
                    continue;
                }
                // Fall through to inline rendering if the file isn't
                // there — we'd rather show the alt text than nothing.
                flatten_one(&tokens[i], RunFlags::default(), None, &mut buffered_inline, &footnote_numbers);
                i += 1;
            }
            // Inline-level tokens at the root accumulate into the
            // current paragraph buffer.
            _ => {
                if let Token::HtmlInline(tag) = &tokens[i] {
                    if let Some(parsed) = classify_inline_html_tag(tag) {
                        root_html_depth.handle(parsed);
                        i += 1;
                        continue;
                    }
                }
                let effective = root_html_depth.apply(RunFlags::default());
                flatten_one(&tokens[i], effective, None, &mut buffered_inline, &footnote_numbers);
                i += 1;
            }
        }
    }

    flush_paragraph(&mut out, &mut buffered_inline);

    // Collect all footnote definitions captured during lowering into
    // a single tail-of-document block, ordered by the number assigned
    // when each label was first referenced in the body. Labels that
    // were defined but never referenced are appended at the end in
    // label-sort order so they don't disappear.
    if !footnote_definitions.is_empty() {
        let mut entries: Vec<FootnoteEntry> = Vec::with_capacity(footnote_definitions.len());
        for (label, number) in {
            let mut v: Vec<(String, usize)> = footnote_numbers
                .iter()
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            v.sort_by_key(|(_, n)| *n);
            v
        } {
            if let Some(runs) = footnote_definitions.remove(&label) {
                entries.push(FootnoteEntry {
                    label,
                    number,
                    runs,
                });
            }
        }
        // Unused definitions (never referenced) — assign trailing numbers.
        let mut next = entries.len() + 1;
        let mut unused: Vec<(String, Vec<InlineRun>)> = footnote_definitions.into_iter().collect();
        unused.sort_by(|a, b| a.0.cmp(&b.0));
        for (label, runs) in unused {
            entries.push(FootnoteEntry {
                label,
                number: next,
                runs,
            });
            next += 1;
        }
        out.push(Block::FootnoteDefinitions { entries });
    }

    out
}

/// Returns true if a Token::Image at `idx` should be lifted to a
/// block-level [`Block::Image`] — i.e. nothing comes after it on the
/// same paragraph.
/// True if `s` is exactly `<!-- pagebreak -->` (whitespace-tolerant,
/// case-insensitive). Standalone-comment convention borrowed from
/// Pandoc / mdBook / GitBook: a single HTML comment whose payload is
/// the word `pagebreak` flushes the current page.
fn is_pagebreak_marker(s: &str) -> bool {
    let trimmed = s.trim();
    let inner = trimmed
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .map(str::trim);
    matches!(inner, Some(word) if word.eq_ignore_ascii_case("pagebreak"))
}

struct HtmlImg {
    src: String,
    alt: String,
    title: Option<String>,
}

/// True if `s` (after trimming and stripping HTML comments) is a
/// single `<img ...>` tag. Returns the parsed attributes when so.
fn parse_html_img_block(s: &str) -> Option<HtmlImg> {
    let stripped = strip_html_comments(s);
    let trimmed = stripped.trim();
    if !trimmed.to_ascii_lowercase().starts_with("<img") {
        return None;
    }
    let end = trimmed.find('>')?;
    if trimmed[end + 1..].trim_end_matches('/').trim() != "" {
        return None;
    }
    let inner = &trimmed[4..end].trim_end_matches('/').trim();
    let attrs = parse_html_attrs(inner);
    let src = attrs.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("src") {
            Some(v.clone())
        } else {
            None
        }
    })?;
    let alt = attrs
        .iter()
        .find_map(|(k, v)| {
            if k.eq_ignore_ascii_case("alt") {
                Some(v.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let title = attrs.iter().find_map(|(k, v)| {
        if k.eq_ignore_ascii_case("title") {
            Some(v.clone())
        } else {
            None
        }
    });
    Some(HtmlImg { src, alt, title })
}

/// Parses HTML attributes inside an open tag (the bit between the
/// tag name and the closing `>`). Returns `(name, value)` pairs.
/// Tolerates double-quoted, single-quoted, and unquoted values, plus
/// boolean attributes.
fn parse_html_attrs(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'/'
        {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let name = s[name_start..i].to_string();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            out.push((name, String::new()));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            out.push((name, String::new()));
            break;
        }
        let quote = bytes[i];
        let value = if quote == b'"' || quote == b'\'' {
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            let v = s[start..i].to_string();
            if i < bytes.len() {
                i += 1;
            }
            v
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'>' {
                i += 1;
            }
            s[start..i].to_string()
        };
        out.push((name, value));
    }
    out
}

/// Strip every `<!-- ... -->` comment out of `s` and return the rest.
fn strip_html_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// True if the HTML block is just a framing tag with no real content
/// — `<p>`, `</p>`, `<div>`, `</div>`, `<center>`, `</center>`,
/// optionally with attributes. These wrap content in GitHub-flavored
/// markdown to apply alignment; we'd rather drop them than render
/// them as literal monospace.
fn is_framing_only_html(s: &str) -> bool {
    let trimmed = strip_html_comments(s);
    let trimmed = trimmed.trim();
    if !trimmed.starts_with('<') || !trimmed.ends_with('>') {
        return false;
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let inner = inner.trim_start_matches('/').trim_end_matches('/').trim();
    let tag_end = inner
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(inner.len());
    let tag = inner[..tag_end].to_ascii_lowercase();
    matches!(tag.as_str(), "p" | "div" | "center")
}

/// True if `s` (trimmed) consists of zero or more `<!-- ... -->`
/// blocks and nothing else. The block-level lexer rule for raw HTML
/// catches a standalone comment line as `Token::HtmlBlock(content)`;
/// we drop those at lower time so the PDF doesn't show the literal
/// `<!--`.
fn is_only_html_comments(s: &str) -> bool {
    let mut rest = s.trim();
    if rest.is_empty() {
        return false;
    }
    while !rest.is_empty() {
        if !rest.starts_with("<!--") {
            return false;
        }
        match rest.find("-->") {
            Some(end) => rest = rest[end + 3..].trim(),
            None => return false,
        }
    }
    true
}

/// Walk every token in document order; assign each unique footnote
/// label the next ordinal. The returned map is consumed by
/// `flatten_one` (for rendering inline `[^label]` references with
/// the right number) and by the post-pass that collects definitions
/// into `Block::FootnoteDefinitions` in numeric order.
fn collect_footnote_numbering(tokens: &[Token]) -> HashMap<String, usize> {
    let mut map: HashMap<String, usize> = HashMap::new();
    fn walk(t: &Token, map: &mut HashMap<String, usize>) {
        match t {
            Token::FootnoteReference(label) => {
                let next = map.len() + 1;
                map.entry(label.clone()).or_insert(next);
            }
            Token::InlineFootnote { label, content } => {
                // Same numbering sequence as `[^id]`, assigned at the
                // marker's document position so inline and regular
                // footnotes interleave correctly. The label is unique
                // per occurrence, so this always inserts.
                let next = map.len() + 1;
                map.entry(label.clone()).or_insert(next);
                for c in content {
                    walk(c, map);
                }
            }
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
            | Token::Highlight(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::Link { content: inner, .. }
            | Token::Image { alt: inner, .. } => {
                for c in inner {
                    walk(c, map);
                }
            }
            Token::FootnoteDefinition { content, .. } => {
                for c in content {
                    walk(c, map);
                }
            }
            Token::DefinitionList { entries } => {
                for entry in entries {
                    for c in &entry.term {
                        walk(c, map);
                    }
                    for def in &entry.definitions {
                        for c in def {
                            walk(c, map);
                        }
                    }
                }
            }
            Token::Table { headers, rows, .. } => {
                for header in headers {
                    for c in header {
                        walk(c, map);
                    }
                }
                for row in rows {
                    for cell in row {
                        for c in cell {
                            walk(c, map);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for t in tokens {
        walk(t, &mut map);
    }
    map
}

/// Recursively gather every inline-footnote (`text^[body]`) body,
/// keyed by its lexer-assigned label, flattening each to inline runs.
/// These feed the same `footnote_definitions` map that block `[^id]:`
/// definitions populate, so the tail "Footnotes" section and the
/// back-link anchors come out of the existing machinery unchanged.
/// The body is collected here rather than at the marker's position so
/// it never splits the paragraph the marker sits in.
fn collect_inline_footnote_defs(
    tokens: &[Token],
    footnotes: &HashMap<String, usize>,
    out: &mut HashMap<String, Vec<InlineRun>>,
) {
    fn walk(
        t: &Token,
        footnotes: &HashMap<String, usize>,
        out: &mut HashMap<String, Vec<InlineRun>>,
    ) {
        match t {
            Token::InlineFootnote { label, content } => {
                // Nested footnotes inside the body, if any, first.
                for c in content {
                    walk(c, footnotes, out);
                }
                let runs =
                    flatten_inline(content, RunFlags::default(), None, footnotes);
                out.entry(label.clone()).or_insert(runs);
            }
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
            | Token::Highlight(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::Link { content: inner, .. }
            | Token::Image { alt: inner, .. }
            | Token::FootnoteDefinition { content: inner, .. } => {
                for c in inner {
                    walk(c, footnotes, out);
                }
            }
            Token::DefinitionList { entries } => {
                for entry in entries {
                    for c in &entry.term {
                        walk(c, footnotes, out);
                    }
                    for def in &entry.definitions {
                        for c in def {
                            walk(c, footnotes, out);
                        }
                    }
                }
            }
            Token::Table { headers, rows, .. } => {
                for header in headers {
                    for c in header {
                        walk(c, footnotes, out);
                    }
                }
                for row in rows {
                    for cell in row {
                        for c in cell {
                            walk(c, footnotes, out);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for t in tokens {
        walk(t, footnotes, out);
    }
}

fn image_is_standalone(tokens: &[Token], idx: usize) -> bool {
    for tok in tokens.iter().skip(idx + 1) {
        match tok {
            Token::Newline | Token::HardBreak => return true,
            _ => return false,
        }
    }
    true
}

/// Convert one `Token::ListItem` into a [`ListEntry`], splitting its
/// content into the inline portion (text on the bullet's line) and
/// nested block-level children.
fn make_list_entry(
    ordered: bool,
    number: Option<usize>,
    checked: Option<bool>,
    loose: bool,
    content: &[Token],
    footnotes: &HashMap<String, usize>,
) -> ListEntry {
    let bullet = match checked {
        Some(true) => ListBullet::TaskChecked,
        Some(false) => ListBullet::TaskUnchecked,
        None if ordered => ListBullet::Ordered(number.unwrap_or(1)),
        None => ListBullet::Unordered('-'),
    };

    // The "header" inline content sits before any nested block-level
    // children (nested lists, paragraphs from a blank line, etc.).
    // Walk until we hit a block-level token, then lower the tail
    // recursively.
    let mut inline_end = 0;
    for (i, tok) in content.iter().enumerate() {
        if matches!(
            tok,
            Token::ListItem { .. }
                | Token::Heading(..)
                | Token::Code { block: true, .. }
                | Token::HorizontalRule
                | Token::BlockQuote(_)
                | Token::Table { .. }
        ) {
            inline_end = i;
            break;
        }
        inline_end = i + 1;
    }

    let head = &content[..inline_end];
    let tail = &content[inline_end..];

    let runs = flatten_inline(head, RunFlags::default(), None, footnotes);
    let children = if tail.is_empty() {
        Vec::new()
    } else {
        lower(tail)
    };

    ListEntry {
        bullet,
        runs,
        children,
        loose,
    }
}

/// Recursively flatten a slice of inline-level tokens into runs,
/// propagating `flags` and the current link URL through nested
/// style wrappers.
fn flatten_inline(
    tokens: &[Token],
    flags: RunFlags,
    link: Option<&str>,
    footnotes: &HashMap<String, usize>,
) -> Vec<InlineRun> {
    let mut out = Vec::new();
    // Track open inline-HTML scopes (sup/sub/u/s/del/small/kbd). The
    // lexer emits each opening / closing tag as a separate `HtmlInline`
    // token; we consume them here and toggle the relevant flag.
    let mut depth = InlineHtmlDepth::default();
    for tok in tokens {
        if let Token::HtmlInline(tag) = tok {
            if let Some(parsed) = classify_inline_html_tag(tag) {
                depth.handle(parsed);
                continue;
            }
        }
        let effective = depth.apply(flags);
        flatten_one(tok, effective, link, &mut out, footnotes);
    }
    out
}

enum InlineHtmlTag {
    SupOpen,
    SupClose,
    SubOpen,
    SubClose,
    UnderlineOpen,
    UnderlineClose,
    StrikeOpen,
    StrikeClose,
    SmallOpen,
    SmallClose,
    KbdOpen,
    KbdClose,
}

fn classify_inline_html_tag(raw: &str) -> Option<InlineHtmlTag> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "<sup>" => Some(InlineHtmlTag::SupOpen),
        "</sup>" => Some(InlineHtmlTag::SupClose),
        "<sub>" => Some(InlineHtmlTag::SubOpen),
        "</sub>" => Some(InlineHtmlTag::SubClose),
        "<u>" => Some(InlineHtmlTag::UnderlineOpen),
        "</u>" => Some(InlineHtmlTag::UnderlineClose),
        "<s>" | "<del>" | "<strike>" => Some(InlineHtmlTag::StrikeOpen),
        "</s>" | "</del>" | "</strike>" => Some(InlineHtmlTag::StrikeClose),
        "<small>" => Some(InlineHtmlTag::SmallOpen),
        "</small>" => Some(InlineHtmlTag::SmallClose),
        "<kbd>" => Some(InlineHtmlTag::KbdOpen),
        "</kbd>" => Some(InlineHtmlTag::KbdClose),
        _ => None,
    }
}

#[derive(Default, Clone, Copy)]
struct InlineHtmlDepth {
    sup: u32,
    sub: u32,
    underline: u32,
    strike: u32,
    small: u32,
    kbd: u32,
}

impl InlineHtmlDepth {
    /// Update depth counters for a recognized tag. Returns `true` if
    /// the tag was consumed (and should be skipped from output).
    fn handle(&mut self, tag: InlineHtmlTag) {
        match tag {
            InlineHtmlTag::SupOpen => self.sup += 1,
            InlineHtmlTag::SupClose => self.sup = self.sup.saturating_sub(1),
            InlineHtmlTag::SubOpen => self.sub += 1,
            InlineHtmlTag::SubClose => self.sub = self.sub.saturating_sub(1),
            InlineHtmlTag::UnderlineOpen => self.underline += 1,
            InlineHtmlTag::UnderlineClose => self.underline = self.underline.saturating_sub(1),
            InlineHtmlTag::StrikeOpen => self.strike += 1,
            InlineHtmlTag::StrikeClose => self.strike = self.strike.saturating_sub(1),
            InlineHtmlTag::SmallOpen => self.small += 1,
            InlineHtmlTag::SmallClose => self.small = self.small.saturating_sub(1),
            InlineHtmlTag::KbdOpen => self.kbd += 1,
            InlineHtmlTag::KbdClose => self.kbd = self.kbd.saturating_sub(1),
        }
    }

    fn apply(&self, mut flags: RunFlags) -> RunFlags {
        if self.sup > 0 {
            flags = flags.with_superscript();
        }
        if self.sub > 0 {
            flags = flags.with_subscript();
        }
        if self.underline > 0 {
            flags = flags.with_underline();
        }
        if self.strike > 0 {
            flags = flags.with_strikethrough();
        }
        if self.small > 0 {
            flags = flags.with_small();
        }
        if self.kbd > 0 {
            flags = flags.with_monospace();
        }
        flags
    }
}

fn flatten_one(
    tok: &Token,
    flags: RunFlags,
    link: Option<&str>,
    out: &mut Vec<InlineRun>,
    footnotes: &HashMap<String, usize>,
) {
    match tok {
        Token::Text(s) => push_text(out, s, flags, link),
        Token::Emphasis { level, content } => {
            let nested = match level {
                1 => flags.with_italic(),
                2 => flags.with_bold(),
                _ => flags.with_bold().with_italic(),
            };
            for t in content {
                flatten_one(t, nested, link, out, footnotes);
            }
        }
        Token::StrongEmphasis(content) => {
            let nested = flags.with_bold();
            for t in content {
                flatten_one(t, nested, link, out, footnotes);
            }
        }
        Token::Strikethrough(content) => {
            let nested = flags.with_strikethrough();
            for t in content {
                flatten_one(t, nested, link, out, footnotes);
            }
        }
        Token::Highlight(content) => {
            let nested = flags.with_highlight();
            for t in content {
                flatten_one(t, nested, link, out, footnotes);
            }
        }
        Token::Code {
            content,
            block: false,
            ..
        } => {
            let mono = flags.with_monospace();
            push_text(out, content, mono, link);
        }
        Token::Link { content, url, .. } => {
            // The link styling (underline + color) is applied at the
            // layout pass — here we just propagate the URL and mark
            // the run with underline so the visual decoration is
            // ready before the annotation is drawn.
            let url_str = url.as_str();
            let nested = flags.with_underline();
            for t in content {
                flatten_one(t, nested, Some(url_str), out, footnotes);
            }
        }
        Token::FootnoteReference(label) => {
            // Display number assigned by collect_footnote_numbering.
            // Missing entries can happen if numbering wasn't run for
            // this subtree (e.g. nested calls from a fresh sub-lexer
            // in `make_list_entry`); fall back to the literal label.
            let number = footnotes.get(label).copied();
            let display = number
                .map(|n| n.to_string())
                .unwrap_or_else(|| label.clone());
            let anchor_link = number.map(|n| format!("#footnote-{}", n));
            let sup_flags = flags.with_superscript();
            out.push(InlineRun {
                text: display,
                flags: sup_flags,
                link: anchor_link,
            });
        }
        Token::InlineFootnote { label, .. } => {
            // Render exactly like a `[^id]` reference: a superscript
            // number linked to its tail entry. The body itself is
            // collected separately by `collect_inline_footnote_defs`.
            // The synthetic label is never user-visible, so if
            // numbering somehow missed it we emit nothing rather than
            // leak the control-prefixed id.
            if let Some(n) = footnotes.get(label).copied() {
                out.push(InlineRun {
                    text: n.to_string(),
                    flags: flags.with_superscript(),
                    link: Some(format!("#footnote-{}", n)),
                });
            }
        }
        Token::FootnoteDefinition { .. } => {
            // Definitions handled at the top-level lower loop; this
            // arm is unreachable in practice but kept exhaustive.
        }
        Token::Image { alt, .. } => {
            // Inline images render only their alt text. Block-level
            // standalone images are promoted to `Block::Image` in the
            // top-level lower loop and get the full embedded image.
            for t in alt {
                flatten_one(t, flags, link, out, footnotes);
            }
        }
        Token::HtmlInline(tag) => {
            // Tags we semantically handle (sup/sub/u/s/del/small/kbd)
            // are consumed by the calling context's depth tracker
            // before reaching `flatten_one`; if we see one here it
            // means flatten_one was called directly (e.g. inside an
            // emphasis run) — drop it silently so the inline style
            // applies to the surrounded text without dumping `<u>`.
            if classify_inline_html_tag(tag).is_some() {
                return;
            }
            let lower = tag.to_ascii_lowercase();
            // <br>, </br>, <br/>, <br /> — soft inline line break.
            if lower.starts_with("<br") || lower.starts_with("</br") {
                push_text(out, " ", flags, link);
            } else if lower.starts_with("<!--") {
                // Inline HTML comment payload — drop silently.
            } else {
                // Unknown tag — emit verbatim so users see something
                // rather than have it silently disappear.
                push_text(out, tag, flags, link);
            }
        }
        // HTML comments are invisible by markdown spec.
        Token::HtmlComment(_) => {}
        Token::HtmlBlock(s) => {
            push_text(out, s, flags, link);
        }
        Token::Newline => push_text(out, " ", flags, link),
        Token::HardBreak => push_text(out, " ", flags, link),
        Token::Heading(content, _)
        | Token::BlockQuote(content)
        | Token::ListItem { content, .. } => {
            for t in content {
                flatten_one(t, flags, link, out, footnotes);
            }
        }
        Token::Code {
            content,
            block: true,
            ..
        } => {
            let mono = flags.with_monospace();
            push_text(out, content, mono, link);
        }
        _ => {}
    }
}

/// Append text to the run buffer, merging with the previous run if
/// the flags and link target match (keeps the IR compact).
fn push_text(out: &mut Vec<InlineRun>, text: &str, flags: RunFlags, link: Option<&str>) {
    if text.is_empty() {
        return;
    }
    let link_owned = link.map(|s| s.to_string());
    if let Some(last) = out.last_mut() {
        if last.flags == flags && last.link == link_owned {
            last.text.push_str(text);
            return;
        }
    }
    out.push(InlineRun {
        text: text.to_string(),
        flags,
        link: link_owned,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::Token;

    #[test]
    fn plain_text_to_paragraph() {
        let blocks = lower(&[Token::Text("hello world".to_string())]);
        assert_eq!(blocks.len(), 1);
        let Block::Paragraph { runs } = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "hello world");
        assert!(!runs[0].flags.bold);
    }

    #[test]
    fn heading_lifts_to_block() {
        let blocks = lower(&[Token::Heading(vec![Token::Text("Hi".into())], 2)]);
        assert_eq!(blocks.len(), 1);
        let Block::Heading { level, runs } = &blocks[0] else {
            panic!("expected heading");
        };
        assert_eq!(*level, 2);
        assert_eq!(runs[0].text, "Hi");
    }

    #[test]
    fn emphasis_propagates_flags() {
        let blocks = lower(&[
            Token::Text("a ".into()),
            Token::Emphasis {
                level: 2,
                content: vec![Token::Text("bold".into())],
            },
            Token::Text(" tail".into()),
        ]);
        let Block::Paragraph { runs } = &blocks[0] else {
            panic!("expected paragraph");
        };
        // Expect three runs: "a " (regular), "bold" (bold), " tail" (regular)
        assert_eq!(runs.len(), 3);
        assert!(!runs[0].flags.bold);
        assert!(runs[1].flags.bold);
        assert!(!runs[2].flags.bold);
    }

    #[test]
    fn double_newline_separates_paragraphs() {
        let blocks = lower(&[
            Token::Text("first".into()),
            Token::Newline,
            Token::Newline,
            Token::Text("second".into()),
        ]);
        assert_eq!(blocks.len(), 2);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
        assert!(matches!(blocks[1], Block::Paragraph { .. }));
    }

    #[test]
    fn inline_code_becomes_monospace_run() {
        let blocks = lower(&[
            Token::Text("see ".into()),
            Token::Code {
                language: String::new(),
                content: "foo".into(),
                block: false,
            },
        ]);
        let Block::Paragraph { runs } = &blocks[0] else {
            panic!();
        };
        assert!(runs.iter().any(|r| r.text == "foo" && r.flags.monospace));
    }

    #[test]
    fn code_block_becomes_codeblock() {
        let blocks = lower(&[Token::Code {
            language: "rust".into(),
            content: "fn main()\n{}".into(),
            block: true,
        }]);
        assert_eq!(blocks.len(), 1);
        let Block::CodeBlock { lines } = &blocks[0] else {
            panic!();
        };
        assert_eq!(lines, &vec!["fn main()".to_string(), "{}".to_string()]);
    }

    fn lex(src: &str) -> Vec<Token> {
        crate::markdown::Lexer::new(src.to_string()).parse().unwrap()
    }

    fn footnote_section(blocks: &[Block]) -> &[FootnoteEntry] {
        blocks
            .iter()
            .find_map(|b| match b {
                Block::FootnoteDefinitions { entries } => Some(entries.as_slice()),
                _ => None,
            })
            .expect("no Block::FootnoteDefinitions emitted")
    }

    #[test]
    fn inline_footnote_numbered_and_collected_to_tail() {
        let blocks = lower(&lex("Body^[the note]. More text."));

        // The marker and the text after it stay in one paragraph —
        // collecting the definition must not split it.
        let para = blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph { runs } => Some(runs),
                _ => None,
            })
            .expect("no paragraph");
        let joined: String = para.iter().map(|r| r.text.as_str()).collect();
        assert!(
            joined.contains("Body") && joined.contains("More text."),
            "paragraph was split: {joined:?}"
        );
        let marker = para
            .iter()
            .find(|r| r.flags.superscript)
            .expect("no superscript marker");
        assert_eq!(marker.text, "1");
        assert_eq!(marker.link.as_deref(), Some("#footnote-1"));

        let entries = footnote_section(&blocks);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].number, 1);
        let body: String = entries[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert!(body.contains("the note"), "tail body wrong: {body:?}");
    }

    #[test]
    fn inline_and_regular_footnotes_share_numbering() {
        // Inline note appears first -> #1; the `[^x]` ref -> #2.
        let blocks = lower(&lex("First^[inline note] then[^x].\n\n[^x]: ref def"));
        let entries = footnote_section(&blocks);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].number, 1);
        assert_eq!(entries[1].number, 2);
        let first: String = entries[0].runs.iter().map(|r| r.text.as_str()).collect();
        let second: String =
            entries[1].runs.iter().map(|r| r.text.as_str()).collect();
        assert!(first.contains("inline note"), "got {first:?}");
        assert!(second.contains("ref def"), "got {second:?}");
    }
}
