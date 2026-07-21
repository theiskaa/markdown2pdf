//! PDF structural validation tests. Parses the rendered bytes back
//! with `lopdf` and asserts the object graph is well-formed:
//!
//! - Trailer has a `/Root` reference
//! - Root resolves to a `/Type /Catalog` dictionary
//! - Catalog has a `/Pages` reference
//! - Pages root has `/Type /Pages`, a `/Kids` array, and `/Count`
//! - Each entry in `/Kids` resolves to a `/Type /Page` dictionary
//! - `/Count` matches the actual page count
//! - Every `Reference` in the document points at an existing object
//!   (no dangling refs)
//!
//! These tests complement the byte-level assertions in `styling.rs`
//! and `adversarial.rs` — those check content; this file checks
//! structural integrity. If printpdf ever ships a regression that
//! emits a broken object graph, these will catch it.

use super::common::*;
use lopdf::{Document, Object, ObjectId};

/// Parse PDF bytes back into a `lopdf::Document`. Panics with a
/// descriptive message on parse failure so test output is useful.
fn parse(bytes: &[u8]) -> Document {
    Document::load_mem(bytes).expect("rendered PDF should parse via lopdf")
}

/// Resolve a `Reference` chain to its terminal object id, or return
/// the input id when already a direct object.
fn resolve_ref(doc: &Document, id: ObjectId) -> ObjectId {
    let mut cur = id;
    for _ in 0..16 {
        match doc.objects.get(&cur) {
            Some(Object::Reference(next)) => cur = *next,
            _ => return cur,
        }
    }
    cur
}

/// Walk `/Root` from the trailer and return the Catalog dictionary.
fn catalog(doc: &Document) -> &lopdf::Dictionary {
    let root_ref = doc
        .trailer
        .get(b"Root")
        .expect("trailer missing /Root entry");
    let root_id = root_ref
        .as_reference()
        .expect("/Root must be an indirect reference");
    let root_id = resolve_ref(doc, root_id);
    let cat_obj = doc.objects.get(&root_id).expect("/Root target missing");
    cat_obj
        .as_dict()
        .expect("/Root target must be a dictionary")
}

/// Validate the full object graph and return the resolved page count.
/// Panics with a descriptive message on any structural issue.
fn validate(bytes: &[u8]) -> usize {
    assert!(
        pdf_well_formed(bytes),
        "PDF byte stream missing %PDF- or %%EOF marker"
    );
    let doc = parse(bytes);

    let cat = catalog(&doc);
    if let Ok(t) = cat.get(b"Type") {
        assert_eq!(
            t.as_name().ok(),
            Some(&b"Catalog"[..]),
            "Catalog /Type field must be /Catalog"
        );
    }

    let pages_ref = cat
        .get(b"Pages")
        .expect("Catalog missing /Pages")
        .as_reference()
        .expect("Catalog /Pages must be a reference");
    let pages_id = resolve_ref(&doc, pages_ref);
    let pages = doc
        .objects
        .get(&pages_id)
        .expect("Catalog /Pages target missing")
        .as_dict()
        .expect("Pages root must be a dictionary");

    assert_eq!(
        pages
            .get(b"Type")
            .ok()
            .and_then(|v| v.as_name().ok())
            .map(|n| n.to_vec()),
        Some(b"Pages".to_vec()),
        "Pages root /Type must be /Pages"
    );

    let kids = pages
        .get(b"Kids")
        .expect("Pages root missing /Kids")
        .as_array()
        .expect("/Kids must be an array");
    let declared_count = pages
        .get(b"Count")
        .expect("Pages root missing /Count")
        .as_i64()
        .expect("/Count must be an integer");

    let mut found_pages = 0usize;
    for kid in kids {
        let kid_id = kid.as_reference().expect("/Kids entry must be a reference");
        let kid_id = resolve_ref(&doc, kid_id);
        let kid_obj = doc
            .objects
            .get(&kid_id)
            .expect("/Kids entry target missing");
        let kid_dict = kid_obj.as_dict().expect("/Kids entry must be a dict");
        let kid_type = kid_dict
            .get(b"Type")
            .ok()
            .and_then(|v| v.as_name().ok())
            .expect("/Kids entry missing /Type");
        assert!(
            matches!(kid_type, b"Page" | b"Pages"),
            "/Kids entry /Type must be /Page or /Pages, got {:?}",
            String::from_utf8_lossy(kid_type)
        );
        if kid_type == b"Page" {
            found_pages += 1;
        }
    }

    assert_eq!(
        found_pages as i64, declared_count,
        "Pages /Count ({}) doesn't match actual page entries ({})",
        declared_count, found_pages
    );

    // No dangling references: every Reference in the document must
    // resolve to an actual object.
    let mut dangling = Vec::new();
    for (_, obj) in doc.objects.iter() {
        walk_for_dangling(&doc, obj, &mut dangling);
    }
    assert!(
        dangling.is_empty(),
        "dangling references found: {:?}",
        dangling
    );

    found_pages
}

fn walk_for_dangling(doc: &Document, obj: &Object, out: &mut Vec<ObjectId>) {
    match obj {
        Object::Reference(id) => {
            if !doc.objects.contains_key(id) {
                out.push(*id);
            }
        }
        Object::Array(items) => {
            for item in items {
                walk_for_dangling(doc, item, out);
            }
        }
        Object::Dictionary(d) => {
            for (_, v) in d.iter() {
                walk_for_dangling(doc, v, out);
            }
        }
        Object::Stream(s) => {
            for (_, v) in s.dict.iter() {
                walk_for_dangling(doc, v, out);
            }
        }
        _ => {}
    }
}

mod minimal_documents {
    use super::*;

    #[test]
    fn empty_input() {
        let bytes = render("", "");
        // Should still emit at least one fallback page.
        let pages = validate(&bytes);
        assert!(pages >= 1, "expected at least one page, got {}", pages);
    }

    #[test]
    fn single_paragraph() {
        let bytes = render("Hello world.", "");
        assert_eq!(validate(&bytes), 1);
    }

    #[test]
    fn single_heading() {
        let bytes = render("# Title", "");
        assert_eq!(validate(&bytes), 1);
    }

    #[test]
    fn single_horizontal_rule() {
        let bytes = render("---", "");
        assert_eq!(validate(&bytes), 1);
    }
}

mod multi_page_documents {
    use super::*;

    #[test]
    fn two_page_document() {
        let bytes = render(&multi_page_markdown(15), "");
        let pages = validate(&bytes);
        assert!(pages >= 2, "expected ≥2 pages, got {}", pages);
    }

    #[test]
    fn long_document_passes_validation() {
        let md = multi_page_markdown(80);
        let bytes = render(&md, "");
        validate(&bytes);
    }

    #[test]
    fn pagebreak_marker_validates() {
        let md = "Page A.\n\n<!-- pagebreak -->\n\nPage B.\n";
        let bytes = render(md, "");
        let pages = validate(&bytes);
        assert!(pages >= 2, "page break didn't produce ≥2 pages: {}", pages);
    }

    #[test]
    fn many_pagebreaks_consistent_count() {
        let mut md = String::new();
        for i in 0..10 {
            md.push_str(&format!("Body paragraph {}\n\n<!-- pagebreak -->\n\n", i));
        }
        let bytes = render(&md, "");
        validate(&bytes);
    }
}

mod feature_combinations {
    use super::*;

    #[test]
    fn table_document() {
        let md = "\
| A | B | C |
|---|---|---|
| 1 | 2 | 3 |
| 4 | 5 | 6 |
";
        let bytes = render(md, "");
        validate(&bytes);
    }

    #[test]
    fn nested_lists() {
        let md = "\
- top
  - mid
    - deep
- second
";
        let bytes = render(md, "");
        validate(&bytes);
    }

    #[test]
    fn code_block_doc() {
        let md = "\
Some text.

```rust
fn main() { println!(\"hi\"); }
```

After.
";
        let bytes = render(md, "");
        validate(&bytes);
    }

    #[test]
    fn document_with_links_passes() {
        let md = "Visit [Example](https://example.com) for details.";
        let bytes = render(md, "");
        validate(&bytes);
    }

    #[test]
    fn document_with_footnotes_passes() {
        let md = "\
Body with note.[^1]

[^1]: First footnote definition.
";
        let bytes = render(md, "");
        validate(&bytes);
    }

    #[test]
    fn document_with_definition_list_passes() {
        let bytes = render("Term\n: definition body.\n", "");
        validate(&bytes);
    }

    #[test]
    fn document_with_blockquote_passes() {
        let bytes = render("> quoted text spanning a real paragraph.\n", "");
        validate(&bytes);
    }
}

mod config_variations {
    use super::*;

    #[test]
    fn document_with_title_page_passes() {
        let bytes = render(
            "Body content.",
            r##"
            [title_page]
            title = "Test Doc"
            author = "Test Author"
            "##,
        );
        let pages = validate(&bytes);
        assert!(pages >= 2, "title page should add a page, got {}", pages);
    }

    #[test]
    fn document_with_toc_passes() {
        let md = "# H1\n\nbody\n\n## H2\n\nbody\n\n## H2b\n\nbody\n";
        let bytes = render(
            md,
            r##"
            [toc]
            enabled = true
            "##,
        );
        validate(&bytes);
    }

    #[test]
    fn document_with_headers_and_footers_passes() {
        let bytes = render(
            &multi_page_markdown(10),
            r##"
            [header]
            left = "doc title"
            right = "{page}/{total_pages}"

            [footer]
            center = "© 2026"
            "##,
        );
        validate(&bytes);
    }

    #[test]
    fn document_with_paragraph_background_passes() {
        let bytes = render(
            "Para with background.",
            r##"
            [paragraph]
            background_color = "#FFFFCC"
            "##,
        );
        validate(&bytes);
    }

    #[test]
    fn github_theme_renders_valid_structure() {
        let md = "# Heading\n\nParagraph.\n\n- list\n";
        let bytes = markdown2pdf::parse_into_bytes(
            md.to_string(),
            markdown2pdf::config::ConfigSource::Theme("github"),
            None,
        )
        .expect("render");
        validate(&bytes);
    }

    #[test]
    fn academic_theme_renders_valid_structure() {
        let md = "# Title\n\nAbstract para.\n\n## Section\n\nBody.\n";
        let bytes = markdown2pdf::parse_into_bytes(
            md.to_string(),
            markdown2pdf::config::ConfigSource::Theme("academic"),
            None,
        )
        .expect("render");
        validate(&bytes);
    }

    fn theme_validates(name: &str) {
        let md = "# Heading\n\nParagraph with **bold** and `code`.\n\n\
                  - item one\n- item two\n\n> a quote\n\n\
                  | A | B |\n|---|---|\n| 1 | 2 |\n";
        let bytes = markdown2pdf::parse_into_bytes(
            md.to_string(),
            markdown2pdf::config::ConfigSource::Theme(name),
            None,
        )
        .unwrap_or_else(|e| panic!("{name} theme failed to render: {e}"));
        let pages = validate(&bytes);
        assert!(pages >= 1, "{name} theme produced no pages");
    }

    // The three bundled themes that had no correctness coverage.
    #[test]
    fn minimal_theme_renders_valid_structure() {
        theme_validates("minimal");
    }

    #[test]
    fn compact_theme_renders_valid_structure() {
        theme_validates("compact");
    }

    #[test]
    fn modern_theme_renders_valid_structure() {
        theme_validates("modern");
    }
}

mod feature_rich_document {
    use super::*;

    /// A self-contained document exercising most renderer features.
    /// Inline (not loaded from a file) so the test has no external
    /// dependency and is reproducible in any checkout / CI.
    const KITCHEN_SINK: &str = "\
# Top heading

Intro paragraph with **bold**, *italic*, `code`, ~~strike~~, a
[link](https://example.com), and a footnote.[^1]

## Lists

- bullet one
- bullet two
  - nested
1. ordered
2. ordered two

- [x] done task
- [ ] open task

## Quote and code

> A blockquote spanning
> two source lines.

```rust
fn main() { println!(\"hi\"); }
```

## Table

| Name  | Score | Grade |
|:------|:-----:|------:|
| Alice |  91   |   A   |
| Bob   |  72   |   C   |

## Definition list

Term
: a definition body.

## Math-free inline HTML

x<sup>2</sup> and H<sub>2</sub>O and <kbd>Ctrl</kbd>.

---

Closing paragraph after a thematic break.

[^1]: The footnote definition.
";

    #[test]
    fn kitchen_sink_passes_structural_validation() {
        let bytes = render(KITCHEN_SINK, "");
        validate(&bytes);
    }

    #[test]
    fn kitchen_sink_under_github_theme_validates() {
        let bytes = markdown2pdf::parse_into_bytes(
            KITCHEN_SINK.to_string(),
            markdown2pdf::config::ConfigSource::Theme("github"),
            None,
        )
        .expect("render");
        validate(&bytes);
    }

    #[test]
    fn kitchen_sink_under_academic_theme_validates() {
        let bytes = markdown2pdf::parse_into_bytes(
            KITCHEN_SINK.to_string(),
            markdown2pdf::config::ConfigSource::Theme("academic"),
            None,
        )
        .expect("render");
        validate(&bytes);
    }
}

mod adversarial_validates_too {
    //! Cross-check: a handful of adversarial cases should ALSO pass
    //! structural validation, not just byte-level sanity. Catches the
    //! case where rendering "works" but produces malformed object
    //! graphs.

    use super::*;

    #[test]
    fn empty_document_validates() {
        validate(&render("", ""));
    }

    #[test]
    fn only_whitespace_validates() {
        validate(&render("   \t\t  \n  ", ""));
    }

    #[test]
    fn massive_paragraph_validates() {
        let big = "word ".repeat(10_000);
        validate(&render(&big, ""));
    }

    #[test]
    fn nested_lists_twenty_deep_validates() {
        let mut md = String::new();
        for i in 0..20 {
            md.push_str(&" ".repeat(i * 2));
            md.push_str(&format!("- level {}\n", i));
        }
        validate(&render(&md, ""));
    }

    #[test]
    fn table_with_fifty_columns_validates() {
        let headers: Vec<String> = (0..50).map(|i| format!("c{}", i)).collect();
        let sep: Vec<String> = (0..50).map(|_| "---".to_string()).collect();
        let row: Vec<String> = (0..50).map(|i| format!("{}", i)).collect();
        let md = format!(
            "| {} |\n| {} |\n| {} |\n",
            headers.join(" | "),
            sep.join(" | "),
            row.join(" | ")
        );
        validate(&render(&md, ""));
    }

    #[test]
    fn unicode_heavy_validates() {
        let md = "# 你好\n\nمرحبا بالعالم\n\n👋🏻 emoji + עברית RTL.\n";
        validate(&render(md, ""));
    }
}

mod page_metadata {
    use super::*;

    #[test]
    fn each_page_has_mediabox() {
        let bytes = render(&multi_page_markdown(20), "");
        let doc = parse(&bytes);
        let pages: Vec<ObjectId> = doc.page_iter().collect();
        for pid in &pages {
            let dict = doc
                .objects
                .get(pid)
                .and_then(|o| o.as_dict().ok())
                .expect("page must be a dict");
            assert!(
                dict.get(b"MediaBox").is_ok() || inherit_from_parent(&doc, pid, b"MediaBox"),
                "page {:?} missing /MediaBox (not inherited either)",
                pid
            );
        }
    }

    #[test]
    fn pages_have_content_streams() {
        let bytes = render("hello world", "");
        let doc = parse(&bytes);
        let pages: Vec<ObjectId> = doc.page_iter().collect();
        for pid in &pages {
            let dict = doc
                .objects
                .get(pid)
                .and_then(|o| o.as_dict().ok())
                .expect("page must be a dict");
            assert!(
                dict.get(b"Contents").is_ok(),
                "page {:?} missing /Contents (no content stream)",
                pid
            );
        }
    }

    fn inherit_from_parent(doc: &Document, pid: &ObjectId, key: &[u8]) -> bool {
        let dict = match doc.objects.get(pid).and_then(|o| o.as_dict().ok()) {
            Some(d) => d,
            None => return false,
        };
        let Ok(parent_ref) = dict.get(b"Parent") else {
            return false;
        };
        let Ok(parent_id) = parent_ref.as_reference() else {
            return false;
        };
        let parent_id = resolve_ref(doc, parent_id);
        let Some(Object::Dictionary(pd)) = doc.objects.get(&parent_id) else {
            return false;
        };
        if pd.get(key).is_ok() {
            return true;
        }
        inherit_from_parent(doc, &parent_id, key)
    }
}

mod document_info {
    use super::*;

    #[test]
    fn frontmatter_title_appears_in_info_dict() {
        let md = "---\ntitle: My Document\nauthor: Test Author\n---\nBody.\n";
        let bytes = render(md, "");
        let doc = parse(&bytes);
        let info_ref = doc.trailer.get(b"Info").expect("trailer needs /Info");
        let info_id = info_ref.as_reference().expect("/Info must be a reference");
        let info_id = resolve_ref(&doc, info_id);
        let info = doc
            .objects
            .get(&info_id)
            .and_then(|o| o.as_dict().ok())
            .expect("/Info target must be a dict");

        // Title is hex-encoded as UTF-16-BE: PDF info strings use
        // FEFF BOM + UTF-16. lopdf returns the raw bytes; we decode.
        let title = info.get(b"Title").expect("/Info missing /Title");
        let title_text = decode_pdf_text(title).expect("title decodable");
        assert_eq!(title_text, "My Document");

        let author = info.get(b"Author").expect("/Info missing /Author");
        let author_text = decode_pdf_text(author).expect("author decodable");
        assert_eq!(author_text, "Test Author");
    }

    fn decode_pdf_text(obj: &Object) -> Option<String> {
        match obj {
            Object::String(bytes, _) => {
                if bytes.starts_with(&[0xFE, 0xFF]) {
                    // UTF-16-BE with BOM
                    let words: Vec<u16> = bytes[2..]
                        .chunks_exact(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect();
                    String::from_utf16(&words).ok()
                } else {
                    Some(String::from_utf8_lossy(bytes).to_string())
                }
            }
            _ => None,
        }
    }
}

mod document_language {
    use super::*;

    fn catalog_lang(bytes: &[u8]) -> Option<String> {
        let doc = parse(bytes);
        let cat = catalog(&doc);
        match cat.get(b"Lang").ok()? {
            Object::String(b, _) => Some(String::from_utf8_lossy(b).to_string()),
            _ => None,
        }
    }

    #[test]
    fn language_in_config_emits_catalog_lang() {
        let bytes = render(
            "Body.",
            r##"
            [metadata]
            language = "en-US"
            "##,
        );
        assert_eq!(catalog_lang(&bytes).as_deref(), Some("en-US"));
    }

    #[test]
    fn no_language_omits_catalog_lang() {
        let bytes = render("Body with no language configured.", "");
        assert_eq!(
            catalog_lang(&bytes),
            None,
            "/Lang must be absent when unset (don't fake a default)"
        );
    }

    #[test]
    fn language_other_than_english() {
        let bytes = render(
            "Inhalt.",
            r##"
            [metadata]
            language = "de"
            "##,
        );
        assert_eq!(catalog_lang(&bytes).as_deref(), Some("de"));
    }

    #[test]
    fn whitespace_only_language_is_ignored() {
        let bytes = render(
            "Body.",
            r##"
            [metadata]
            language = "   "
            "##,
        );
        assert_eq!(
            catalog_lang(&bytes),
            None,
            "whitespace-only language must not produce a /Lang entry"
        );
    }

    #[test]
    fn document_with_lang_still_structurally_valid() {
        let bytes = render(
            &multi_page_markdown(10),
            r##"
            [metadata]
            language = "en-GB"
            "##,
        );
        validate(&bytes);
        assert_eq!(catalog_lang(&bytes).as_deref(), Some("en-GB"));
    }
}
