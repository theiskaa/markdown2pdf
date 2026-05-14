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

use super::ir::{Block, InlineRun, ListBullet, ListEntry, RunFlags};

/// Lower a slice of top-level tokens into the block IR.
pub fn lower(tokens: &[Token]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut buffered_inline: Vec<InlineRun> = Vec::new();

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
                let runs = flatten_inline(content, RunFlags::default(), None);
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
                } else if !is_only_html_comments(content) {
                    // CommonMark §4.6: HTML comments are invisible.
                    // Only emit a real HtmlBlock when the payload has
                    // something beyond comments.
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
                        *ordered, *number, *checked, *loose, content,
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
                    .map(|cell| flatten_inline(cell, RunFlags::default(), None))
                    .collect();
                let row_runs: Vec<Vec<Vec<InlineRun>>> = rows
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|cell| flatten_inline(cell, RunFlags::default(), None))
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
            Token::Image { alt, url, .. }
                if buffered_inline.is_empty() && image_is_standalone(tokens, i) =>
            {
                let path = std::path::PathBuf::from(url);
                if path.exists() {
                    let alt_text = crate::markdown::Token::collect_all_text(alt);
                    out.push(Block::Image {
                        path,
                        alt: alt_text,
                    });
                    i += 1;
                    continue;
                }
                // Fall through to inline rendering if the file isn't
                // there — we'd rather show the alt text than nothing.
                flatten_one(&tokens[i], RunFlags::default(), None, &mut buffered_inline);
                i += 1;
            }
            // Inline-level tokens at the root accumulate into the
            // current paragraph buffer.
            _ => {
                flatten_one(&tokens[i], RunFlags::default(), None, &mut buffered_inline);
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

    let runs = flatten_inline(head, RunFlags::default(), None);
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
fn flatten_inline(tokens: &[Token], flags: RunFlags, link: Option<&str>) -> Vec<InlineRun> {
    let mut out = Vec::new();
    for tok in tokens {
        flatten_one(tok, flags, link, &mut out);
    }
    out
}

fn flatten_one(tok: &Token, flags: RunFlags, link: Option<&str>, out: &mut Vec<InlineRun>) {
    match tok {
        Token::Text(s) => push_text(out, s, flags, link),
        Token::Emphasis { level, content } => {
            let nested = match level {
                1 => flags.with_italic(),
                2 => flags.with_bold(),
                _ => flags.with_bold().with_italic(),
            };
            for t in content {
                flatten_one(t, nested, link, out);
            }
        }
        Token::StrongEmphasis(content) => {
            let nested = flags.with_bold();
            for t in content {
                flatten_one(t, nested, link, out);
            }
        }
        Token::Strikethrough(content) => {
            let nested = flags.with_strikethrough();
            for t in content {
                flatten_one(t, nested, link, out);
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
                flatten_one(t, nested, Some(url_str), out);
            }
        }
        Token::Image { alt, .. } => {
            // Inline images render only their alt text. Block-level
            // standalone images are promoted to `Block::Image` in the
            // top-level lower loop and get the full embedded image.
            for t in alt {
                flatten_one(t, flags, link, out);
            }
        }
        Token::HtmlInline(tag) => {
            // Recognized inline whitelist (b/i/u/s/del/strike/br plus
            // em/strong) is handled by the lexer via the regular
            // emphasis tokens, so by the time we see HtmlInline here
            // the tag is *outside* that whitelist. Emit the literal
            // text so the user sees what's there.
            let lower = tag.to_ascii_lowercase();
            // Hide standalone whitelist tags that the lexer doesn't
            // convert (e.g. `<br/>`).
            if lower.starts_with("<br")
                || lower.starts_with("</br")
                || lower == "<br>"
                || lower == "<br/>"
                || lower == "<br />"
            {
                push_text(out, " ", flags, link);
            } else if lower.starts_with("<!--") {
                // Inline HTML comment payload — drop silently.
            } else {
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
                flatten_one(t, flags, link, out);
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
}
