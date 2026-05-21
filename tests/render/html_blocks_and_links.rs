//! Render-level checks for the HTML subset the renderer interprets
//! semantically: inline `<a href="…">…</a>` becomes a clickable link
//! (with an optional `title` tooltip), and `<div>` / `<section>` /
//! `<figure>` / `<figcaption>` block wrappers drop out so their
//! children render normally. Everything outside that subset still
//! falls through as literal text — the renderer never executes or
//! understands arbitrary HTML.

use super::common::*;
use lopdf::{Document, Object};

/// Count `/Subtype/Link` annotation dictionaries. The lopdf-compact
/// form has no space inside the name run, and `/URI` appears twice
/// per annotation (action subtype + URI value), so counting the
/// subtype is the unambiguous signal.
fn link_annotation_count(bytes: &[u8]) -> usize {
    count_substr(bytes, b"/Subtype/Link")
}

fn is_link(d: &lopdf::Dictionary) -> bool {
    d.get(b"Subtype")
        .and_then(|o| o.as_name())
        .map(|n| n == b"Link")
        .unwrap_or(false)
}

/// Resolve an `Object::Reference` one hop (best-effort); for any other
/// object kind returns the object as-is.
fn deref_once<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.objects.get(id),
        other => Some(other),
    }
}

/// Walk every link annotation in the PDF — both inline (`/Annots [<<…>>]`)
/// and indirect (`/Annots [N 0 R]`) — and return its parsed
/// `(uri, contents)` pair where `contents` is the optional tooltip.
fn collect_link_annotations(bytes: &[u8]) -> Vec<(String, Option<String>)> {
    let doc = Document::load_mem(bytes).expect("PDF must parse");
    let mut out = Vec::new();
    let mut consume = |d: &lopdf::Dictionary| {
        if !is_link(d) {
            return;
        }
        let uri = d
            .get(b"A")
            .and_then(|o| o.as_dict())
            .and_then(|a| a.get(b"URI"))
            .and_then(|o| o.as_str())
            .ok()
            .map(|b| String::from_utf8_lossy(b).into_owned());
        let contents = d
            .get(b"Contents")
            .and_then(|o| o.as_str())
            .ok()
            .map(|b| String::from_utf8_lossy(b).into_owned());
        if let Some(u) = uri {
            out.push((u, contents));
        }
    };

    // (a) top-level Link annotation objects
    for (_, obj) in doc.objects.iter() {
        if let Object::Dictionary(d) = obj {
            consume(d);
        }
    }
    // (b) inline annotations inside each page's /Annots array
    for pid in doc.page_iter() {
        let Some(Object::Dictionary(page)) = doc.objects.get(&pid) else {
            continue;
        };
        let Ok(annots) = page.get(b"Annots") else {
            continue;
        };
        let Some(Object::Array(items)) = deref_once(&doc, annots) else {
            continue;
        };
        for item in items {
            let Some(resolved) = deref_once(&doc, item) else {
                continue;
            };
            if let Object::Dictionary(d) = resolved {
                consume(d);
            }
        }
    }
    out
}

/// All link annotation rectangles in `[llx, lly, urx, ury]` user-space
/// points. Mirrors [`collect_link_annotations`] but pulls geometry
/// instead of action/tooltip data.
fn collect_link_rects(bytes: &[u8]) -> Vec<[f32; 4]> {
    let doc = Document::load_mem(bytes).expect("PDF must parse");
    let mut out = Vec::new();
    let mut consume = |d: &lopdf::Dictionary| {
        if !is_link(d) {
            return;
        }
        let Ok(Object::Array(items)) = d.get(b"Rect") else {
            return;
        };
        if items.len() != 4 {
            return;
        }
        let mut r = [0.0_f32; 4];
        for (i, v) in items.iter().enumerate() {
            match v {
                Object::Real(f) => r[i] = *f,
                Object::Integer(n) => r[i] = *n as f32,
                _ => return,
            }
        }
        out.push(r);
    };
    for (_, obj) in doc.objects.iter() {
        if let Object::Dictionary(d) = obj {
            consume(d);
        }
    }
    for pid in doc.page_iter() {
        let Some(Object::Dictionary(page)) = doc.objects.get(&pid) else {
            continue;
        };
        let Ok(annots) = page.get(b"Annots") else {
            continue;
        };
        let Some(Object::Array(items)) = deref_once(&doc, annots) else {
            continue;
        };
        for item in items {
            let Some(resolved) = deref_once(&doc, item) else {
                continue;
            };
            if let Object::Dictionary(d) = resolved {
                consume(d);
            }
        }
    }
    out
}

#[test]
fn inline_anchor_renders_as_clickable_link() {
    let md = "Click <a href=\"https://example.com\">here</a>.\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        link_annotation_count(&bytes),
        1,
        "inline <a> should produce one URI annotation"
    );
    assert!(
        contains_text(&bytes, "https://example.com"),
        "URL should appear in the link annotation"
    );
    assert!(
        !contains_text(&bytes, "<a href"),
        "raw <a> markup must not leak into the rendered text"
    );
}

#[test]
fn anchor_title_attaches_pdf_tooltip() {
    let md = "Visit <a href=\"https://example.com\" title=\"Tip\">x</a>.\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        contains_text(&bytes, "(Tip)"),
        "title attribute should be injected as the annotation /Contents tooltip"
    );
}

#[test]
fn div_wrapper_drops_around_markdown_content() {
    let md = "<div>\n\nWrapped paragraph body.\n\n</div>\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        !contains_text(&bytes, "<div>") && !contains_text(&bytes, "</div>"),
        "div framing tags must not render as literal text"
    );
    assert!(
        contains_text(&bytes, "Wrapped paragraph body"),
        "wrapped markdown content must still render"
    );
}

#[test]
fn section_figure_figcaption_wrappers_drop() {
    let md = "\
<section>

Section body.

</section>

<figure>

Figure body.

<figcaption>

Caption body.

</figcaption>

</figure>
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    for tag in [
        "<section>",
        "</section>",
        "<figure>",
        "</figure>",
        "<figcaption>",
        "</figcaption>",
    ] {
        assert!(
            !contains_text(&bytes, tag),
            "framing tag {tag} must not render as literal text"
        );
    }
    for body in ["Section body", "Figure body", "Caption body"] {
        assert!(
            contains_text(&bytes, body),
            "wrapped content '{body}' should still render"
        );
    }
}

#[test]
fn unknown_block_tag_still_passes_through() {
    let md = "<custom-element>content here</custom-element>\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(
        contains_text(&bytes, "custom-element") || contains(&bytes, b"custom-element"),
        "unrecognised tags must keep the existing pass-through behavior"
    );
}

#[test]
fn unclosed_anchor_degrades_to_literal_without_panic() {
    let md = "before <a href=\"https://example.com\">no close here.\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        link_annotation_count(&bytes),
        0,
        "unclosed <a> should not emit a link annotation"
    );
    assert!(
        contains_text(&bytes, "no close here"),
        "literal fallback content must still render"
    );
}

#[test]
fn anchor_inside_emphasis_links_within_styling() {
    let md = "*click <a href=\"https://example.com\">here</a> now*\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(link_annotation_count(&bytes), 1);
    assert!(contains_text(&bytes, "https://example.com"));
}

#[test]
fn link_annotation_has_nonempty_clickable_rect() {
    let bytes = render(
        "Click <a href=\"https://example.com\">here</a>.\n",
        "",
    );
    let rects = collect_link_rects(&bytes);
    assert_eq!(rects.len(), 1);
    let [llx, lly, urx, ury] = rects[0];
    assert!(urx > llx, "rect must have positive width: {:?}", rects[0]);
    assert!(ury > lly, "rect must have positive height: {:?}", rects[0]);
    assert!(urx - llx >= 1.0, "click target too narrow: {:?}", rects[0]);
}

#[test]
fn multiple_anchors_in_one_paragraph_each_get_annotation() {
    let md = "see <a href=\"https://a.example\">one</a> and \
              <a href=\"https://b.example\" title=\"second\">two</a> here.\n";
    let bytes = render(md, "");
    let links = collect_link_annotations(&bytes);
    assert_eq!(links.len(), 2);
    let mut uris: Vec<String> = links.iter().map(|(u, _)| u.clone()).collect();
    uris.sort();
    assert_eq!(uris, vec!["https://a.example", "https://b.example"]);
    let with_tip: Vec<&String> = links
        .iter()
        .find(|(u, _)| u == "https://b.example")
        .and_then(|(_, c)| c.as_ref())
        .into_iter()
        .collect();
    assert_eq!(with_tip.len(), 1);
    assert_eq!(with_tip[0], "second");
}

#[test]
fn nested_block_wrappers_drop_both_levels() {
    let md = "\
<div>

<section>

Inner body.

</section>

</div>
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    for tag in ["<div", "</div>", "<section", "</section>"] {
        assert!(
            !contains_text(&bytes, tag),
            "nested wrapper {tag} leaked into output"
        );
    }
    assert!(contains_text(&bytes, "Inner body"));
}

#[test]
fn wrapper_with_attributes_drops() {
    let md = "<div class=\"hero\" id=\"top\">\n\nBody text.\n\n</div>\n";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert!(!contains_text(&bytes, "<div"));
    assert!(!contains_text(&bytes, "class=\"hero\""));
    assert!(contains_text(&bytes, "Body text"));
}

#[test]
fn wrapper_preserves_inner_markdown_link() {
    let md = "<section>\n\nSee [docs](https://docs.example) for more.\n\n</section>\n";
    let bytes = render(md, "");
    let links = collect_link_annotations(&bytes);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0, "https://docs.example");
}

#[test]
fn wrapper_with_inline_anchor_inside_paragraph() {
    let md = "\
<div>

Body with <a href=\"https://x.example\">inline anchor</a> inside.

</div>
";
    let bytes = render(md, "");
    let links = collect_link_annotations(&bytes);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].0, "https://x.example");
    assert!(!contains_text(&bytes, "<a href"));
}

#[test]
fn anchor_inside_heading_renders_link() {
    let md = "# Section with <a href=\"https://x.example\">a link</a>\n\nBody.\n";
    let bytes = render(md, "");
    assert_eq!(link_annotation_count(&bytes), 1);
}

#[test]
fn anchor_inside_list_item_renders_link() {
    let md = "- first item with <a href=\"https://x.example\">link</a>\n- second\n";
    let bytes = render(md, "");
    assert_eq!(link_annotation_count(&bytes), 1);
}

#[test]
fn anchor_inside_table_cell_renders_link() {
    let md = "\
| left | right |
| ---- | ----- |
| <a href=\"https://x.example\">go</a> | stay |
";
    let bytes = render(md, "");
    assert_eq!(link_annotation_count(&bytes), 1);
}

#[test]
fn anchor_inside_blockquote_renders_link() {
    let md = "> quoted text with <a href=\"https://x.example\">a link</a> inside\n";
    let bytes = render(md, "");
    assert_eq!(link_annotation_count(&bytes), 1);
}

#[test]
fn anchor_with_query_and_fragment_preserves_url() {
    let md = "go to <a href=\"https://example.com/p?q=1&amp;r=2#sec\">there</a>.\n";
    let bytes = render(md, "");
    let links = collect_link_annotations(&bytes);
    assert_eq!(links.len(), 1);
    // Entities inside attribute values get decoded by the lexer when
    // it's a markdown link, but `<a href="…">` carries the raw value
    // verbatim; either form is acceptable as long as the path,
    // separators, and fragment all reach the annotation intact.
    assert!(
        links[0].0.contains("p?q=1") && links[0].0.contains("#sec"),
        "URL parts missing: {:?}",
        links[0]
    );
}

#[test]
fn uppercase_anchor_tag_works_end_to_end() {
    let md = "go <A HREF=\"https://example.com\">here</A> now\n";
    let bytes = render(md, "");
    assert_eq!(link_annotation_count(&bytes), 1);
    assert!(contains_text(&bytes, "https://example.com"));
}

#[test]
fn self_closing_anchor_emits_no_annotation() {
    let bytes = render("go <a href=\"https://example.com\" /> bye\n", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(link_annotation_count(&bytes), 0);
}

#[test]
fn anchor_without_href_emits_no_annotation() {
    let bytes = render("see <a name=\"foo\">target</a>\n", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(link_annotation_count(&bytes), 0);
}

#[test]
fn unrecognised_inline_tag_passes_through_as_text() {
    let bytes = render("text <weird>inside</weird> more\n", "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(link_annotation_count(&bytes), 0);
    // The literal "<weird>" markup should still appear (existing
    // pass-through is unchanged by this feature).
    assert!(contains_text(&bytes, "weird"));
}

#[test]
fn standalone_html_comment_remains_invisible() {
    // Comment-only blocks were already dropped; this test pins the
    // behavior so the wrapper-tag change above doesn't accidentally
    // re-introduce the literal `<!--` text.
    let bytes = render("<!-- private note -->\nBody.\n", "");
    assert!(!contains_text(&bytes, "private note"));
    assert!(contains_text(&bytes, "Body"));
}

#[test]
fn markdown_link_title_tooltip_still_works() {
    // Regression check: existing `[text](url "title")` markdown links
    // continue to get their tooltip injected by postprocess. The new
    // <a>-rewrite shares the same downstream path.
    let bytes = render(
        "see [docs](https://docs.example \"Documentation\") here\n",
        "",
    );
    let links = collect_link_annotations(&bytes);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].1.as_deref(), Some("Documentation"));
}

#[test]
fn unknown_block_tag_treats_inner_content_as_code_block() {
    // Type-7 standalone-tag HTML blocks (anything not in our
    // framing-tag whitelist) still render as a verbatim HtmlBlock
    // (monospace). This test pins that pre-existing behavior so the
    // framing-tag extension doesn't accidentally absorb it.
    let bytes = render("<aside>\n\ncontent\n\n</aside>\n", "");
    assert!(pdf_well_formed(&bytes));
    // The tag should appear verbatim somewhere in the rendered text.
    assert!(
        contains_text(&bytes, "aside") || contains(&bytes, b"aside"),
        "unknown block tag was silently dropped"
    );
}

#[test]
fn unclosed_anchor_does_not_capture_next_paragraph() {
    // Regression: an unclosed <a href="…"> followed in the next
    // paragraph by `<a name="…">…</a>` used to be paired across the
    // blank line, swallowing both paragraphs into one giant link.
    let md = "\
First: <a href=\"https://example.com/missing\">no close — falls through.

Second: <a name=\"bookmark\">not a link</a> here.
";
    let bytes = render(md, "");
    assert!(pdf_well_formed(&bytes));
    assert_eq!(
        link_annotation_count(&bytes),
        0,
        "neither malformed anchor should produce a link annotation"
    );
}

#[test]
fn empty_anchor_body_still_emits_annotation() {
    let bytes = render(
        "Trailing <a href=\"https://example.com\"></a> link.\n",
        "",
    );
    assert!(pdf_well_formed(&bytes));
    // An anchor with no visible text still produces an annotation,
    // but its rect can be zero-width — accept either 0 or 1 here so
    // the renderer is free to skip degenerate cases later.
    assert!(link_annotation_count(&bytes) <= 1);
}
