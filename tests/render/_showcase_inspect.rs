//! One-shot helper test: render the /tmp/html_showcase.md sample,
//! enumerate every link annotation, and print them. Not part of the
//! normal suite — opt-in via `--ignored`.

#![allow(clippy::print_stdout)]

use lopdf::{Document, Object};
use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;

fn is_link(d: &lopdf::Dictionary) -> bool {
    d.get(b"Subtype")
        .and_then(|o| o.as_name())
        .map(|n| n == b"Link")
        .unwrap_or(false)
}

fn deref_once<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Object> {
    match obj {
        Object::Reference(id) => doc.objects.get(id),
        other => Some(other),
    }
}

#[test]
#[ignore]
fn dump_showcase_annotations() {
    let md = std::fs::read_to_string("/tmp/html_showcase.md")
        .expect("write /tmp/html_showcase.md first");
    let bytes = parse_into_bytes(md, ConfigSource::Embedded(""), None).unwrap();
    std::fs::write("/tmp/html_showcase.pdf", &bytes).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();

    let mut count = 0usize;
    let mut visit = |d: &lopdf::Dictionary| {
        if !is_link(d) {
            return;
        }
        count += 1;
        let uri = d
            .get(b"A")
            .and_then(|o| o.as_dict())
            .and_then(|a| a.get(b"URI"))
            .and_then(|o| o.as_str())
            .ok()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        let tip = d
            .get(b"Contents")
            .and_then(|o| o.as_str())
            .ok()
            .map(|b| String::from_utf8_lossy(b).into_owned());
        let rect = d
            .get(b"Rect")
            .and_then(|o| o.as_array())
            .map(|a| {
                a.iter()
                    .map(|v| match v {
                        Object::Real(f) => *f,
                        Object::Integer(n) => *n as f32,
                        _ => 0.0,
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        println!("LINK #{count} uri={uri:<45} tip={tip:?} rect={rect:?}");
    };

    for (_, obj) in doc.objects.iter() {
        if let Object::Dictionary(d) = obj {
            visit(d);
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
                visit(d);
            }
        }
    }

    println!("\nTOTAL LINK ANNOTATIONS: {count}");
    println!("PDF saved to /tmp/html_showcase.pdf");
}
