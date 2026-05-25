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

use crate::markdown::{TableCell, Token};

use super::ir::{Block, DefinitionEntry, FootnoteEntry, InlineRun, ListBullet, ListEntry, RunFlags};
use std::collections::HashMap;

/// Lower a slice of top-level tokens into the block IR.
pub fn lower(tokens: &[Token]) -> Vec<Block> {
    // First-reference-order numbering for footnotes — built once over
    // the entire token tree, then threaded into every recursive
    // sub-lowering so nested contexts (blockquote, admonition, list
    // item children) resolve refs against the document-wide map
    // instead of re-numbering local labels from 1.
    let footnote_numbers = collect_footnote_numbering(tokens);
    let mut footnote_definitions: HashMap<String, Vec<InlineRun>> = HashMap::new();
    collect_inline_footnote_defs(tokens, &footnote_numbers, &mut footnote_definitions);

    let mut out = lower_blocks(tokens, &footnote_numbers, &mut footnote_definitions);

    // Tail Footnotes section, ordered by first-reference number.
    // Definitions defined but never referenced trail in label-sort
    // order so they don't disappear.
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

fn lower_blocks(
    tokens: &[Token],
    footnote_numbers: &HashMap<String, usize>,
    footnote_definitions: &mut HashMap<String, Vec<InlineRun>>,
) -> Vec<Block> {
    let mut out = Vec::new();
    let mut buffered_inline: Vec<InlineRun> = Vec::new();

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
                } else if let Some(inner) = strip_framing_wrapper(content) {
                    // Runs before is_framing_only_html so wrappers with
                    // attributes (`<div class="…">body</div>`) get
                    // unwrapped instead of dropped as a standalone tag.
                    if let Ok(inner_tokens) = crate::markdown::Lexer::new(inner).parse() {
                        let inner_blocks = lower_blocks(
                            &inner_tokens,
                            footnote_numbers,
                            footnote_definitions,
                        );
                        out.extend(inner_blocks);
                    } else if !is_only_html_comments(content) {
                        out.push(Block::HtmlBlock {
                            content: content.clone(),
                        });
                    }
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
                let nested = lower_blocks(body, footnote_numbers, footnote_definitions);
                out.push(Block::BlockQuote { body: nested });
                i += 1;
            }
            Token::Admonition {
                kind,
                raw_label,
                title,
                body,
            } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                let title_runs = title.as_ref().map(|t| {
                    flatten_inline(t, RunFlags::default(), None, footnote_numbers)
                });
                let nested = lower_blocks(body, footnote_numbers, footnote_definitions);
                out.push(Block::Admonition {
                    kind: kind.clone(),
                    raw_label: raw_label.clone(),
                    title: title_runs,
                    body: nested,
                });
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
            Token::Math {
                inline: false,
                content,
            } => {
                flush_paragraph(&mut out, &mut buffered_inline);
                out.push(Block::MathBlock {
                    content: content.clone(),
                });
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
                        footnote_numbers,
                        footnote_definitions,
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
                let to_runs = |cell: &TableCell<Token>| {
                    cell.map_content(|c| {
                        flatten_inline(c, RunFlags::default(), None, &footnote_numbers)
                    })
                };
                let head_runs: Vec<TableCell<InlineRun>> =
                    headers.iter().map(to_runs).collect();
                let row_runs: Vec<Vec<TableCell<InlineRun>>> = rows
                    .iter()
                    .map(|row| row.iter().map(to_runs).collect())
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
            // another block boundary. Both successful loads (URL or
            // existing local file) and failures (missing local file,
            // unreachable URL) route through `Block::Image`; the
            // layout pass decodes and falls back to
            // `render_image_fallback` on failure so every "image not
            // shown" path produces the same italic `[image: ALT]`
            // placeholder.
            Token::Image { alt, url, title }
                if buffered_inline.is_empty() && image_is_standalone(tokens, i) =>
            {
                let path = std::path::PathBuf::from(url);
                let alt_text = crate::markdown::Token::collect_all_text(alt);
                out.push(Block::Image {
                    path,
                    alt: alt_text,
                    caption: title.clone(),
                });
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
                    // `<br/>` at paragraph level: flush the buffer and
                    // start a new paragraph so the break is visible.
                    if is_void_br(tag) {
                        flush_paragraph(&mut out, &mut buffered_inline);
                        i += 1;
                        continue;
                    }
                    // `<hr/>` at paragraph level: flush + emit HR.
                    if is_void_hr(tag) {
                        flush_paragraph(&mut out, &mut buffered_inline);
                        out.push(Block::HorizontalRule);
                        i += 1;
                        continue;
                    }
                }
                let effective = root_html_depth.apply(RunFlags::default());
                flatten_one(&tokens[i], effective, None, &mut buffered_inline, footnote_numbers);
                i += 1;
            }
        }
    }

    flush_paragraph(&mut out, &mut buffered_inline);
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
pub(super) fn parse_html_attrs(s: &str) -> Vec<(String, String)> {
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

/// True for any spelling of `<br>` / `<br/>` / `<br />` / `</br>`.
fn is_void_br(raw: &str) -> bool {
    let s = raw.trim().to_ascii_lowercase();
    matches!(s.as_str(), "<br>" | "<br/>" | "<br />" | "</br>")
}

/// True for any spelling of `<hr>` / `<hr/>` / `<hr />` / `</hr>`.
fn is_void_hr(raw: &str) -> bool {
    let s = raw.trim().to_ascii_lowercase();
    matches!(s.as_str(), "<hr>" | "<hr/>" | "<hr />" | "</hr>")
}

/// If the HTML block is a framing wrapper around real content (e.g.
/// `<div>…markdown…</div>`, `<section>…</section>`,
/// `<p>text</p>`), return the inner with the outer open + close tags
/// removed so it can be re-lexed as markdown. Returns `None` if the
/// content isn't a single matching wrapper pair, so the caller can
/// fall back to rendering it as a verbatim HTML block.
fn strip_framing_wrapper(s: &str) -> Option<String> {
    const WRAPPERS: &[&str] = &["div", "section", "figure", "figcaption", "center", "p"];
    let trimmed = s.trim();
    if !trimmed.starts_with('<') {
        return None;
    }
    let open_end = trimmed.find('>')?;
    let open_inner = &trimmed[1..open_end];
    let open_tag_end = open_inner
        .find(|c: char| c.is_ascii_whitespace())
        .unwrap_or(open_inner.len());
    let open_tag = open_inner[..open_tag_end].to_ascii_lowercase();
    if !WRAPPERS.contains(&open_tag.as_str()) {
        return None;
    }
    // Must end with the matching closing tag.
    let close_lower = format!("</{}>", open_tag);
    let trimmed_lower = trimmed.to_ascii_lowercase();
    if !trimmed_lower.ends_with(&close_lower) {
        return None;
    }
    let close_start = trimmed.len() - close_lower.len();
    let inner = trimmed[open_end + 1..close_start].trim().to_string();
    if inner.is_empty() {
        return None;
    }
    Some(inner)
}

/// True if the HTML block is just a structural wrapper tag with no
/// real content — `<p>`, `<div>`, `<section>`, `<figure>`,
/// `<figcaption>`, `<center>` (plus their close tags), optionally
/// with attributes. These wrap content in GitHub-flavored markdown
/// for layout/grouping; we'd rather drop them than render them as
/// literal monospace.
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
    matches!(
        tag.as_str(),
        "p" | "div" | "section" | "figure" | "figcaption" | "center"
    )
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
            Token::Admonition { title, body, .. } => {
                if let Some(t) = title {
                    for c in t {
                        walk(c, map);
                    }
                }
                for c in body {
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
                    for c in &header.content {
                        walk(c, map);
                    }
                }
                for row in rows {
                    for cell in row {
                        for c in &cell.content {
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
            Token::Admonition { title, body, .. } => {
                if let Some(t) = title {
                    for c in t {
                        walk(c, footnotes, out);
                    }
                }
                for c in body {
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
                    for c in &header.content {
                        walk(c, footnotes, out);
                    }
                }
                for row in rows {
                    for cell in row {
                        for c in &cell.content {
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
    footnote_definitions: &mut HashMap<String, Vec<InlineRun>>,
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
                | Token::Admonition { .. }
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
        lower_blocks(tail, footnotes, footnote_definitions)
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
    BoldOpen,
    BoldClose,
    ItalicOpen,
    ItalicClose,
    CodeOpen,
    CodeClose,
    SpanOpen,
    SpanClose,
}

fn classify_inline_html_tag(raw: &str) -> Option<InlineHtmlTag> {
    let s = raw.trim();
    let rest = s.strip_prefix('<')?.strip_suffix('>')?;
    let (rest, is_close) = match rest.strip_prefix('/') {
        Some(r) => (r, true),
        None => (rest, false),
    };
    let rest = rest.trim_start();
    let name_end = rest
        .find(|c: char| c.is_ascii_whitespace() || c == '/')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_ascii_lowercase();
    let opener = match name.as_str() {
        "sup" => InlineHtmlTag::SupOpen,
        "sub" => InlineHtmlTag::SubOpen,
        "u" => InlineHtmlTag::UnderlineOpen,
        "s" | "del" | "strike" => InlineHtmlTag::StrikeOpen,
        "small" => InlineHtmlTag::SmallOpen,
        "kbd" => InlineHtmlTag::KbdOpen,
        "strong" | "b" => InlineHtmlTag::BoldOpen,
        "em" | "i" => InlineHtmlTag::ItalicOpen,
        "code" => InlineHtmlTag::CodeOpen,
        "span" => InlineHtmlTag::SpanOpen,
        _ => return None,
    };
    Some(if is_close {
        match opener {
            InlineHtmlTag::SupOpen => InlineHtmlTag::SupClose,
            InlineHtmlTag::SubOpen => InlineHtmlTag::SubClose,
            InlineHtmlTag::UnderlineOpen => InlineHtmlTag::UnderlineClose,
            InlineHtmlTag::StrikeOpen => InlineHtmlTag::StrikeClose,
            InlineHtmlTag::SmallOpen => InlineHtmlTag::SmallClose,
            InlineHtmlTag::KbdOpen => InlineHtmlTag::KbdClose,
            InlineHtmlTag::BoldOpen => InlineHtmlTag::BoldClose,
            InlineHtmlTag::ItalicOpen => InlineHtmlTag::ItalicClose,
            InlineHtmlTag::CodeOpen => InlineHtmlTag::CodeClose,
            InlineHtmlTag::SpanOpen => InlineHtmlTag::SpanClose,
            _ => unreachable!(),
        }
    } else {
        opener
    })
}

#[derive(Default, Clone, Copy)]
struct InlineHtmlDepth {
    sup: u32,
    sub: u32,
    underline: u32,
    strike: u32,
    small: u32,
    kbd: u32,
    bold: u32,
    italic: u32,
    code: u32,
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
            InlineHtmlTag::BoldOpen => self.bold += 1,
            InlineHtmlTag::BoldClose => self.bold = self.bold.saturating_sub(1),
            InlineHtmlTag::ItalicOpen => self.italic += 1,
            InlineHtmlTag::ItalicClose => self.italic = self.italic.saturating_sub(1),
            InlineHtmlTag::CodeOpen => self.code += 1,
            InlineHtmlTag::CodeClose => self.code = self.code.saturating_sub(1),
            // <span> is a transparent wrapper: tracked so a stray
            // </span> is also consumed, but contributes no flag.
            InlineHtmlTag::SpanOpen | InlineHtmlTag::SpanClose => {}
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
            flags = flags.with_inline_code();
        }
        if self.bold > 0 {
            flags = flags.with_bold();
        }
        if self.italic > 0 {
            flags = flags.with_italic();
        }
        if self.code > 0 {
            flags = flags.with_inline_code();
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
            let mono = flags.with_inline_code();
            push_text(out, content, mono, link);
        }
        Token::Math { content, .. } => {
            // Inline math is one indivisible typeset box on the text
            // baseline. A display-math token only reaches here when it
            // isn't at the top level (e.g. inside a list item / table
            // cell); the top-level lower loop promotes standalone
            // display math to a centered `Block::MathBlock`.
            out.push(InlineRun::math(
                content.clone(),
                flags,
                link.map(|s| s.to_string()),
            ));
        }
        Token::Link { content, url, .. } => {
            // Link styling (underline + colour) is applied at the
            // layout pass from the `[link]` config — the run only
            // carries the URL. The underline flag is *not* forced
            // here, so `[link].underline = false` is honoured.
            let url_str = url.as_str();
            for t in content {
                flatten_one(t, flags, Some(url_str), out, footnotes);
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
            out.push(InlineRun { math: None,
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
                out.push(InlineRun { math: None,
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
            // Inline images render their alt text wrapped in an
            // italic `[image: …]` placeholder so readers can tell at
            // a glance which inline glyphs stood in for an image,
            // regardless of context (paragraph, list item, table
            // cell, admonition, blockquote). Block-level standalone
            // images that successfully load are promoted to
            // `Block::Image` in the top-level lower loop and get the
            // full embedded image; a failed Block::Image goes through
            // `render_image_fallback`, which produces the same italic
            // wrapper as a paragraph — so every "image not shown"
            // path renders identically.
            //
            // Empty-alt images stay invisible: `[image: ]` is uglier
            // than skipping, and the author signaled the image was
            // decorative (or didn't bother with alt) so dropping it
            // matches `render_image_fallback`'s same-case behavior.
            let alt_text = crate::markdown::Token::collect_all_text(alt);
            if alt_text.trim().is_empty() {
                return;
            }
            let italic = flags.with_italic();
            push_text(out, "[image: ", italic, link);
            for t in alt {
                flatten_one(t, italic, link, out, footnotes);
            }
            push_text(out, "]", italic, link);
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
        // If an admonition ever reaches the inline flattener (it
        // shouldn't — the top-level lower arm promotes it to
        // Block::Admonition) we degrade gracefully by spilling its
        // header label and body text into the surrounding run.
        Token::Admonition { raw_label, title, body, .. } => {
            if let Some(t) = title {
                for tok in t {
                    flatten_one(tok, flags, link, out, footnotes);
                }
            } else {
                push_text(out, raw_label, flags, link);
            }
            for t in body {
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
        if last.math.is_none() && last.flags == flags && last.link == link_owned {
            last.text.push_str(text);
            return;
        }
    }
    out.push(InlineRun { math: None,
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
    fn inline_math_becomes_a_math_run() {
        let blocks = lower(&[
            Token::Text("when ".into()),
            Token::Math {
                inline: true,
                content: "x^2".into(),
            },
        ]);
        let Block::Paragraph { runs } = &blocks[0] else {
            panic!("expected paragraph");
        };
        // The math run carries the raw TeX and no flowing text — the
        // layout pass typesets + draws it as outlines.
        assert!(runs
            .iter()
            .any(|r| r.math.as_deref() == Some("x^2") && r.text.is_empty()));
    }

    #[test]
    fn display_math_becomes_centered_block_and_flushes_paragraphs() {
        let blocks = lower(&[
            Token::Text("intro".into()),
            Token::Math {
                inline: false,
                content: "E = mc^2".into(),
            },
            Token::Text("outro".into()),
        ]);
        // Paragraph("intro"), MathBlock, Paragraph("outro").
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
        let Block::MathBlock { content } = &blocks[1] else {
            panic!("expected a MathBlock, got {:?}", blocks[1]);
        };
        assert_eq!(content, "E = mc^2");
        assert!(matches!(blocks[2], Block::Paragraph { .. }));
    }

    #[test]
    fn display_math_in_list_item_falls_back_to_inline_run() {
        // A display token that isn't at the top level (here, inside a
        // list item) must still render — as an inline math box —
        // rather than vanish.
        let blocks = lower(&lex("- see $$a+b$$ here"));
        let Block::List { entries } = &blocks[0] else {
            panic!("expected list");
        };
        assert!(entries[0]
            .runs
            .iter()
            .any(|r| r.math.as_deref() == Some("a+b")));
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

    fn walk_superscript_markers(blocks: &[Block], out: &mut Vec<String>) {
        for b in blocks {
            match b {
                Block::Paragraph { runs }
                | Block::Heading { runs, .. } => {
                    for r in runs {
                        if r.flags.superscript {
                            out.push(r.text.clone());
                        }
                    }
                }
                Block::BlockQuote { body } | Block::Admonition { body, .. } => {
                    walk_superscript_markers(body, out);
                }
                Block::List { entries } => {
                    for e in entries {
                        for r in &e.runs {
                            if r.flags.superscript {
                                out.push(r.text.clone());
                            }
                        }
                        walk_superscript_markers(&e.children, out);
                    }
                }
                Block::Table { headers, rows, .. } => {
                    for h in headers {
                        for r in &h.content {
                            if r.flags.superscript {
                                out.push(r.text.clone());
                            }
                        }
                    }
                    for row in rows {
                        for cell in row {
                            for r in &cell.content {
                                if r.flags.superscript {
                                    out.push(r.text.clone());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Footnote refs nested inside blockquote / admonition / list-item
    /// bodies must resolve against the document-wide first-reference
    /// numbering, not a fresh per-body 1.
    #[test]
    fn nested_contexts_share_document_wide_footnote_numbering() {
        let src = "Top[^a].\n\n\
> Quote[^b].\n\n\
> [!NOTE]\n\
> Admo[^c].\n\n\
- list[^d]\n\n\
[^a]: A.\n[^b]: B.\n[^c]: C.\n[^d]: D.\n";
        let blocks = lower(&lex(src));
        let mut markers = Vec::new();
        walk_superscript_markers(&blocks, &mut markers);
        // First-reference order is a, b, c, d -> 1, 2, 3, 4. Each
        // ref must show its assigned number, NOT 1.
        assert_eq!(
            markers,
            vec!["1".to_string(), "2".to_string(), "3".to_string(), "4".to_string()],
            "footnote markers in nested contexts must use document-wide numbering"
        );
    }

    /// Footnote definitions remain emitted as a single tail block, in
    /// first-reference order, even when refs are scattered across
    /// blockquote / admonition / list-item bodies.
    #[test]
    fn nested_footnote_refs_keep_single_tail_section() {
        let src = "Top[^a].\n\n\
> Quote[^b].\n\n\
> [!NOTE]\n\
> Admo[^c].\n\n\
[^a]: A.\n[^b]: B.\n[^c]: C.\n";
        let blocks = lower(&lex(src));
        let tail_blocks: Vec<&FootnoteEntry> = blocks
            .iter()
            .filter_map(|b| match b {
                Block::FootnoteDefinitions { entries } => Some(entries.iter()),
                _ => None,
            })
            .flatten()
            .collect();
        // Three definitions, one tail block, ordered a,b,c by first ref.
        assert_eq!(tail_blocks.len(), 3);
        assert_eq!(tail_blocks[0].number, 1);
        assert_eq!(tail_blocks[1].number, 2);
        assert_eq!(tail_blocks[2].number, 3);
        // And only one tail block — no per-body duplicates leaking into
        // the document body.
        let tail_count = blocks
            .iter()
            .filter(|b| matches!(b, Block::FootnoteDefinitions { .. }))
            .count();
        assert_eq!(tail_count, 1);
    }
}
