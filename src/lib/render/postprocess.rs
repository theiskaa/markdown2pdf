//! lopdf post-processing for features printpdf 0.9 doesn't expose:
//! - Inline link tooltips (`/Contents` on Link annotations)
//! - PDF/A-1b conformance metadata (XMP, OutputIntent, document ID)
//!
//! The post-passes parse the bytes printpdf produced, mutate the
//! relevant objects, and re-serialize. Failures degrade silently
//! (return the original bytes unchanged) — no PDF feature regression,
//! the user just doesn't get the polish.

use crate::markdown::Token;
use lopdf::{Dictionary, Document, Object, SaveOptions};
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
            | Token::Highlight(inner)
            | Token::BlockQuote(inner)
            | Token::ListItem { content: inner, .. }
            | Token::FootnoteDefinition { content: inner, .. }
            | Token::InlineFootnote { content: inner, .. } => walk(inner, map),
            Token::Image { alt, .. } => walk(alt, map),
            Token::Table { headers, rows, .. } => {
                for h in headers {
                    walk(&h.content, map);
                }
                for r in rows {
                    for c in r {
                        walk(&c.content, map);
                    }
                }
            }
            Token::DefinitionList { entries } => {
                for e in entries {
                    for t in &e.terms {
                        walk(t, map);
                    }
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
        let Some(tip) = tooltips.get(&uri) else {
            continue;
        };
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
                if let Some(uri) = link_uri(d)
                    && let Some(tip) = tooltips.get(&uri)
                {
                    d.set("Contents", Object::string_literal(tip.clone()));
                    changed = true;
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

/// Set the document Catalog's `/Lang` entry to `lang` (a BCP-47 tag
/// like `"en-US"`). printpdf 0.9 doesn't expose this. Screen readers
/// and `Tagged PDF`-aware tools use it to pick a pronunciation
/// dictionary. Degrades silently to the input bytes on any parse /
/// serialize failure. No-op when `lang` is empty.
pub fn inject_lang(bytes: Vec<u8>, lang: &str) -> Vec<u8> {
    if lang.trim().is_empty() {
        return bytes;
    }
    let Ok(mut doc) = Document::load_mem(&bytes) else {
        return bytes;
    };
    let Ok(root_ref) = doc.trailer.get(b"Root") else {
        return bytes;
    };
    let Ok(root_id) = root_ref.as_reference() else {
        return bytes;
    };
    let Some(Object::Dictionary(catalog)) = doc.objects.get_mut(&root_id) else {
        return bytes;
    };
    catalog.set("Lang", Object::string_literal(lang.to_string()));
    let mut out = Vec::new();
    if doc.save_to(&mut out).is_ok() {
        out
    } else {
        bytes
    }
}

/// Shrink the PDF as much as is lossless. Two independent passes:
///
/// 1. `doc.compress()` — Flate-deflate every content / object
///    *stream*. printpdf 0.9's `optimize` flag is a no-op (its
///    `doc.compress()` call is commented out), so it ships raw,
///    uncompressed page streams; math drawn as vector outlines makes
///    those huge.
/// 2. `save_with_options(use_object_streams + use_xref_streams)` —
///    pack the non-stream indirect objects (page dicts, annotations,
///    destinations, metadata) into a Flate-compressed object stream
///    and replace the verbose ASCII xref table with a compact binary
///    cross-reference stream (PDF 1.5+). Once the content streams are
///    deflated this structural ASCII is the *majority* of the file,
///    so this is the larger remaining win — and it is purely how
///    objects are *stored*, never how anything renders.
///
/// Both are standard, viewer-universal mechanisms. The result is kept
/// only if it is actually smaller; any parse / serialize failure
/// degrades silently to the input bytes, so no document is ever lost.
pub fn compress(bytes: Vec<u8>) -> Vec<u8> {
    let Ok(mut doc) = Document::load_mem(&bytes) else {
        return bytes;
    };
    fix_form_xobjects(&mut doc);
    doc.compress();
    let opts = SaveOptions {
        use_object_streams: true,
        use_xref_streams: true,
        ..Default::default()
    };
    let mut out = Vec::new();
    if doc.save_with_options(&mut out, opts).is_ok() && out.len() < bytes.len() {
        out
    } else {
        bytes
    }
}

/// printpdf 0.9's `FormXObject` serializer omits the spec-required
/// `/BBox` and writes `/FormType` as a name instead of the integer
/// `1`. The math engine emits one Form XObject per glyph (its outline
/// in raw font units, scaled by a `1/upem` `/Matrix`), so patch every
/// `/Subtype /Form` stream: add a generous font-unit bounding box
/// (well beyond any glyph; `/BBox` only clips) and a numeric
/// `/FormType`. Without `/BBox`, conformant viewers drop the form.
fn fix_form_xobjects(doc: &mut Document) {
    for obj in doc.objects.values_mut() {
        let Object::Stream(stream) = obj else {
            continue;
        };
        let is_form = matches!(
            stream.dict.get(b"Subtype"),
            Ok(Object::Name(n)) if n == b"Form"
        );
        if !is_form {
            continue;
        }
        stream.dict.set("FormType", Object::Integer(1));
        if stream.dict.get(b"BBox").is_err() {
            stream.dict.set(
                "BBox",
                Object::Array(vec![
                    Object::Integer(-2000),
                    Object::Integer(-2000),
                    Object::Integer(4000),
                    Object::Integer(4000),
                ]),
            );
        }
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

    d.get(b"Type")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| n == b"Annot")
        .unwrap_or(true)
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
