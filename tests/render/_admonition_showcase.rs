//! One-shot helper: render /tmp/admonition_full_showcase.md,
//! save the multi-page PDF to /tmp, then split each page into
//! its own single-page PDF under /tmp/admonition_pages/ so each
//! one can be thumbnailed via qlmanage and inspected. Also
//! renders the same minimal snippet through every bundled theme
//! to /tmp/admonition_theme_<name>.pdf for side-by-side theme
//! verification.
//!
//! Opt-in via `cargo test --test render _admonition_showcase --
//! --ignored --nocapture`.

#![allow(clippy::print_stdout)]

use lopdf::{Document, Object};
use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;

fn split_pages(bytes: &[u8], out_dir: &str) -> Vec<String> {
    std::fs::create_dir_all(out_dir).expect("mkdir out dir");
    let doc = Document::load_mem(bytes).expect("PDF must parse");
    let page_ids: Vec<lopdf::ObjectId> = doc.page_iter().collect();
    let total = page_ids.len();
    let mut paths = Vec::with_capacity(total);

    for (idx, page_id) in page_ids.iter().enumerate() {
        // Clone the full doc, then walk every other page id and
        // detach it from the page tree. The simpler API
        // `delete_pages` works on 1-based page numbers per lopdf.
        let mut single = doc.clone();
        let to_delete: Vec<u32> = (0..total as u32)
            .filter(|j| *j != idx as u32)
            .map(|j| j + 1)
            .collect();
        if !to_delete.is_empty() {
            single.delete_pages(&to_delete);
        }
        let _ = page_id; // page_id only used implicitly via iteration order
        let mut out = Vec::new();
        single.save_to(&mut out).expect("save single page");
        let path = format!("{out_dir}/page_{:02}.pdf", idx + 1);
        std::fs::write(&path, &out).expect("write single page");
        paths.push(path);
    }
    paths
}

#[test]
#[ignore]
fn render_full_showcase_and_split_pages() {
    let md = std::fs::read_to_string("/tmp/admonition_full_showcase.md")
        .expect("write /tmp/admonition_full_showcase.md first");
    let bytes = parse_into_bytes(md, ConfigSource::Default, None).expect("render must succeed");
    std::fs::write("/tmp/admonition_full_showcase.pdf", &bytes).expect("write showcase pdf");
    let pages = split_pages(&bytes, "/tmp/admonition_pages");
    println!("MAIN SHOWCASE: {} pages", pages.len());
    for p in &pages {
        println!("  {}", p);
    }
    println!("PDF: /tmp/admonition_full_showcase.pdf");

    // Mini per-theme snippet for theme comparison.
    let snippet = "\
# Admonition kinds — theme comparison

!!! note
    A note callout.

!!! info
    An info callout.

!!! tip
    A tip callout.

!!! warning
    A warning callout.

!!! danger
    A danger callout.

!!! bug \"Unknown kind\"
    Generic fallback.
";
    for theme in [
        "default", "github", "academic", "minimal", "compact", "modern",
    ] {
        let cfg = format!("theme = \"{theme}\"\n");
        let bytes = parse_into_bytes(snippet.to_string(), ConfigSource::Embedded(&cfg), None)
            .expect("render must succeed");
        let path = format!("/tmp/admonition_theme_{theme}.pdf");
        std::fs::write(&path, &bytes).expect("write theme pdf");
        println!("THEME {theme}: {path}");
    }

    // Always emit a sentinel so the user sees we ran end-to-end.
    println!("DONE");

    // Suppress unused-import warnings when the helper compiles
    // without the trait.
    let _ = Object::Null;
}
