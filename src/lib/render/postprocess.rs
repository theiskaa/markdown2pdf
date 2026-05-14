//! lopdf post-processing for features printpdf 0.9 doesn't expose:
//! - Inline link tooltips (`/Contents` on Link annotations)
//! - PDF/A-1b conformance metadata (XMP, OutputIntent, document ID)
//!
//! The post-passes parse the bytes printpdf produced, mutate the
//! relevant objects, and re-serialize. Failures degrade silently
//! (return the original bytes unchanged) — no PDF feature regression,
//! the user just doesn't get the polish.

use crate::markdown::Token;
use lopdf::{Dictionary, Document, Object};
use std::collections::HashMap;

/// Walk the token tree and collect a URL → tooltip map from
/// `Token::Link { title, url, .. }`. Multiple links pointing at the
/// same URL with different titles collapse to the last one seen — a
/// minor edge case the lopdf post-pass can't disambiguate by URL
/// alone.
pub fn collect_link_tooltips(tokens: &[Token]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    walk(tokens, &mut map);
    map
}

fn walk(tokens: &[Token], map: &mut HashMap<String, String>) {
    for tok in tokens {
        match tok {
            Token::Link {
                content,
                url,
                title: Some(t),
            } => {
                map.insert(url.clone(), t.clone());
                walk(content, map);
            }
            Token::Link { content, .. } => walk(content, map),
            Token::Heading(inner, _)
            | Token::Emphasis { content: inner, .. }
            | Token::StrongEmphasis(inner)
            | Token::Strikethrough(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::FootnoteDefinition { content: inner, .. } => walk(inner, map),
            Token::Image { alt, .. } => walk(alt, map),
            Token::Table { headers, rows, .. } => {
                for h in headers {
                    walk(h, map);
                }
                for r in rows {
                    for c in r {
                        walk(c, map);
                    }
                }
            }
            Token::DefinitionList { entries } => {
                for e in entries {
                    walk(&e.term, map);
                    for d in &e.definitions {
                        walk(d, map);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Add `/Contents` (tooltip) entries to every `/Subtype /Link`
/// annotation whose URI action matches a key in `tooltips`. Returns
/// the modified PDF bytes; on any parse / serialize failure returns
/// the input unchanged so the rest of the document is never lost.
pub fn inject_link_tooltips(bytes: Vec<u8>, tooltips: &HashMap<String, String>) -> Vec<u8> {
    if tooltips.is_empty() {
        return bytes;
    }
    let Ok(mut doc) = Document::load_mem(&bytes) else {
        return bytes;
    };
    let mut changed = false;

    // Link annotations live inside page dicts as either:
    //   (a) inline dictionaries in `/Annots [<<...>>]`, or
    //   (b) indirect references in `/Annots [N 0 R]` pointing at
    //       top-level objects.
    // We handle both. First pass: rewrite any top-level Link-annotation
    // objects in `doc.objects`.
    let ids: Vec<lopdf::ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Some(Object::Dictionary(d)) = doc.objects.get_mut(&id) else {
            continue;
        };
        if !is_link_annotation(d) {
            continue;
        }
        let Some(uri) = link_uri(d) else { continue };
        let Some(tip) = tooltips.get(&uri) else { continue };
        d.set("Contents", Object::string_literal(tip.clone()));
        changed = true;
    }

    // Second pass: walk each page object and rewrite any inline Link
    // annotation dicts in its `/Annots` array.
    let page_ids: Vec<lopdf::ObjectId> = doc.page_iter().collect();
    for pid in page_ids {
        let Some(Object::Dictionary(page)) = doc.objects.get_mut(&pid) else {
            continue;
        };
        let Ok(annots) = page.get_mut(b"Annots") else {
            continue;
        };
        let Object::Array(items) = annots else {
            continue;
        };
        for item in items.iter_mut() {
            if let Object::Dictionary(d) = item {
                if !is_link_annotation(d) {
                    continue;
                }
                if let Some(uri) = link_uri(d) {
                    if let Some(tip) = tooltips.get(&uri) {
                        d.set("Contents", Object::string_literal(tip.clone()));
                        changed = true;
                    }
                }
            }
        }
    }

    if !changed {
        return bytes;
    }
    let mut out = Vec::new();
    if doc.save_to(&mut out).is_ok() {
        out
    } else {
        bytes
    }
}

fn is_link_annotation(d: &Dictionary) -> bool {
    let subtype_link = d
        .get(b"Subtype")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n == b"Link")
        .unwrap_or(false);
    if !subtype_link {
        return false;
    }
    // `Type` is optional on annotations per spec but printpdf emits it.
    let type_annot = d
        .get(b"Type")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n == b"Annot")
        .unwrap_or(true);
    type_annot
}

fn link_uri(d: &Dictionary) -> Option<String> {
    let action = d.get(b"A").ok()?;
    let action_dict = action.as_dict().ok()?;
    let s_uri = action_dict
        .get(b"S")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n == b"URI")
        .unwrap_or(false);
    if !s_uri {
        return None;
    }
    let uri_obj = action_dict.get(b"URI").ok()?;
    let bytes = uri_obj.as_str().ok()?;
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}
