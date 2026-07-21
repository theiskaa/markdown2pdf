//! Widow / orphan control. A heading must not end up as the last
//! item on a page while its body wraps to the next — the
//! `keep_with_next_break` rule in the layout engine guarantees this.

use super::common::*;
use lopdf::Document;

/// Decompressed content stream for each page in the document, in
/// page order. Each entry is the raw operator bytes for one page.
fn page_streams(bytes: &[u8]) -> Vec<Vec<u8>> {
    let doc = Document::load_mem(bytes).expect("rendered PDF must parse");
    doc.page_iter()
        .map(|pid| doc.get_page_content(pid))
        .collect()
}

fn page_contains(stream: &[u8], needle: &str) -> bool {
    String::from_utf8_lossy(stream).contains(needle)
}

/// Multiple `## Section N` headings spread through a long document,
/// each followed by a uniquely-numbered body sentence. The varying
/// chapter sizes mean at least one heading lands in the
/// "fits-but-body-wraps" window without the fix.
fn many_section_fixture(n_sections: usize) -> String {
    let mut out = String::new();
    for i in 1..=n_sections {
        out.push_str(&format!("## Section {i}\n\n"));
        out.push_str(&format!(
            "BODYMARK{i} body sentence for section {i}. Lorem ipsum dolor sit \
amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut \
labore et dolore magna aliqua.\n\n"
        ));
        out.push_str(
            "Filler paragraph. Lorem ipsum dolor sit amet, consectetur \
adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore \
magna aliqua. Ut enim ad minim veniam.\n\n",
        );
        out.push_str(
            "Second filler paragraph for variability. Quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.\n\n",
        );
        if i % 3 == 0 {
            out.push_str("- bullet one\n- bullet two\n- bullet three\n\n");
        }
        if i % 4 == 0 {
            out.push_str("```\nlet x = 42;\n```\n\n");
        }
    }
    out
}

#[test]
fn heading_lands_on_same_page_as_its_body() {
    let md = many_section_fixture(20);
    let bytes = render(&md, "");
    let streams = page_streams(&bytes);
    assert!(streams.len() >= 2, "fixture must span multiple pages");

    for i in 1..=20 {
        let heading_needle = format!("Section {i}");
        let body_needle = format!("BODYMARK{i}");

        let heading_page = streams
            .iter()
            .position(|s| page_contains(s, &heading_needle))
            .unwrap_or_else(|| panic!("heading '{heading_needle}' not found in any page"));
        let body_page = streams
            .iter()
            .position(|s| page_contains(s, &body_needle))
            .unwrap_or_else(|| panic!("body '{body_needle}' not found in any page"));

        assert_eq!(
            heading_page,
            body_page,
            "Section {i} orphaned: heading on page {} but body on page {}",
            heading_page + 1,
            body_page + 1
        );
    }
}

/// Pads page 1 with enough filler paragraphs that the *next* block
/// lands near the page bottom, regardless of its kind. The marker
/// `LEADMARK<id>` appears once at the start of the probe block so the
/// caller can locate it.
fn padded_until_bottom(filler_paragraphs: usize, probe: &str) -> String {
    let mut out = String::new();
    out.push_str("# Probe\n\n");
    for i in 0..filler_paragraphs {
        out.push_str(&format!("Filler {i}. "));
        out.push_str(&"Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(4));
        out.push_str("\n\n");
    }
    out.push_str(probe);
    out
}

/// A wrapped heading must drag its follow-on paragraph across the
/// page break. The original heuristic assumed `header_h = 1 line` so a
/// heading that wraps to two lines would underestimate by a whole
/// line, leaving its first body sentence orphaned on the next page.
#[test]
fn multi_line_heading_stays_with_body() {
    let probe = "## A long heading whose text overflows the column width and \
wraps onto a second visual line because the words just keep going\n\n\
MULTILINEBODY This is the first body sentence; it must land on the same \
page as the wrapped heading above.\n\n";
    // Sweep filler counts that put the probe heading in the
    // "fits-but-body-wraps" window; the exact count depends on font
    // metrics so we try a range and require at least one hit, then
    // assert that for every hit the heading and body are on the same
    // page.
    let mut checked = 0usize;
    for n in 14..=30 {
        let md = padded_until_bottom(n, probe);
        let bytes = render(&md, "");
        let streams = page_streams(&bytes);
        if streams.len() < 2 {
            continue;
        }
        let h_page = streams
            .iter()
            .position(|s| page_contains(s, "A long heading"));
        let b_page = streams
            .iter()
            .position(|s| page_contains(s, "MULTILINEBODY"));
        if let (Some(h), Some(b)) = (h_page, b_page) {
            checked += 1;
            assert_eq!(
                h,
                b,
                "multi-line heading orphaned at filler={n}: heading on \
page {} but body on page {}",
                h + 1,
                b + 1
            );
        }
    }
    assert!(
        checked > 0,
        "no fixture in the sweep produced both heading and body — \
test would silently pass even on broken renderer"
    );
}

/// Heading immediately followed by an admonition: admonitions have
/// substantially larger top padding than paragraphs, so the
/// "1 line of paragraph" heuristic underestimates. Use lookahead at
/// the actual next-block style.
#[test]
fn heading_followed_by_admonition_not_orphaned() {
    let probe = "## Followed by an admonition\n\n\
> [!WARNING]\n\
> ADMOMARK warning body that should land on the same page as its heading.\n\n";
    let mut checked = 0usize;
    for n in 14..=30 {
        let md = padded_until_bottom(n, probe);
        let bytes = render(&md, "");
        let streams = page_streams(&bytes);
        if streams.len() < 2 {
            continue;
        }
        let h_page = streams
            .iter()
            .position(|s| page_contains(s, "Followed by an admonition"));
        let b_page = streams.iter().position(|s| page_contains(s, "ADMOMARK"));
        if let (Some(h), Some(b)) = (h_page, b_page) {
            checked += 1;
            assert_eq!(
                h,
                b,
                "heading→admonition orphaned at filler={n}: heading on \
page {} but admonition on page {}",
                h + 1,
                b + 1
            );
        }
    }
    assert!(
        checked > 0,
        "no fixture in the sweep landed both heading and admonition body"
    );
}

/// Heading immediately followed by a code block: code blocks have
/// their own padding and line metrics; verify the lookahead picks the
/// right style.
#[test]
fn heading_followed_by_code_block_not_orphaned() {
    let probe = "## Followed by a code block\n\n\
```\n\
CODEMARK = \"first code line that must follow its heading\"\n\
let x = 1;\n\
```\n\n";
    let mut checked = 0usize;
    for n in 14..=30 {
        let md = padded_until_bottom(n, probe);
        let bytes = render(&md, "");
        let streams = page_streams(&bytes);
        if streams.len() < 2 {
            continue;
        }
        let h_page = streams
            .iter()
            .position(|s| page_contains(s, "Followed by a code block"));
        let b_page = streams.iter().position(|s| page_contains(s, "CODEMARK"));
        if let (Some(h), Some(b)) = (h_page, b_page) {
            checked += 1;
            assert_eq!(
                h,
                b,
                "heading→code orphaned at filler={n}: heading on page {} \
but code on page {}",
                h + 1,
                b + 1
            );
        }
    }
    assert!(
        checked > 0,
        "no fixture landed both heading and code marker"
    );
}

/// Heading immediately followed by a list: first list item carries
/// its bullet marker — the heading lookahead must reserve enough
/// space that bullet + first text line land with the heading.
#[test]
fn heading_followed_by_list_not_orphaned() {
    let probe = "## Followed by a list\n\n\
- BULLETMARK first bullet text that must land with its heading\n\
- second bullet\n\
- third bullet\n\n";
    let mut checked = 0usize;
    for n in 14..=30 {
        let md = padded_until_bottom(n, probe);
        let bytes = render(&md, "");
        let streams = page_streams(&bytes);
        if streams.len() < 2 {
            continue;
        }
        let h_page = streams
            .iter()
            .position(|s| page_contains(s, "Followed by a list"));
        let b_page = streams.iter().position(|s| page_contains(s, "BULLETMARK"));
        if let (Some(h), Some(b)) = (h_page, b_page) {
            checked += 1;
            assert_eq!(
                h,
                b,
                "heading→list orphaned at filler={n}: heading on page {} \
but first bullet on page {}",
                h + 1,
                b + 1
            );
        }
    }
    assert!(
        checked > 0,
        "no fixture landed both heading and first bullet"
    );
}

/// Bullet-orphan smoke: a long list spans multiple pages without
/// crashing or losing any item. The bullet-orphan guard in
/// `render_list` advances to a new column/page when bullet + first
/// text line wouldn't fit, so no item is silently dropped. Visual
/// verification (the bullet glyph staying with its text) is done via
/// the before/after PDFs — text-based inspection can't see the
/// vector-drawn `•` glyph.
#[test]
fn long_list_renders_without_dropping_items() {
    let mut md = String::from("# Bullet orphan probe\n\n");
    for i in 0..120 {
        md.push_str(&format!(
            "- BULLET{i} item {i} with enough text to occupy a meaningful \
chunk of width on the page.\n"
        ));
    }
    md.push('\n');
    let bytes = render(&md, "");
    let streams = page_streams(&bytes);
    assert!(
        streams.len() >= 2,
        "fixture must span multiple pages to exercise the orphan path"
    );
    // Every item's marker must appear somewhere — no item is silently
    // dropped by an over-eager advance_column.
    let joined = streams
        .iter()
        .flat_map(|s| s.iter().copied())
        .collect::<Vec<u8>>();
    let joined_text = String::from_utf8_lossy(&joined);
    for i in 0..120 {
        let needle = format!("BULLET{i} ");
        assert!(
            joined_text.contains(&needle),
            "item {i} missing from rendered output"
        );
    }
}

/// The auto-emitted "Footnotes" section heading must stay on the same
/// page as its first footnote definition. Mirrors the original
/// `keep_with_next_break` behavior for the footnote-heading path.
#[test]
fn footnotes_section_heading_stays_with_first_entry() {
    let mut md = String::from("# Footnote orphan probe\n\n");
    for i in 0..18 {
        md.push_str(&format!("Filler {i}. "));
        md.push_str(&"Lorem ipsum dolor sit amet. ".repeat(6));
        md.push_str(" Reference[^1] tucked in.\n\n");
    }
    md.push_str(
        "[^1]: FOOTNOTEDEFMARK the one footnote definition that must \
stay glued to its auto-emitted heading.\n",
    );
    let bytes = render(&md, "");
    let streams = page_streams(&bytes);
    // The auto-emitted heading text is `Footnotes` (rendered by
    // `render_footnote_definitions`). Locate its page and assert the
    // definition is on the same page.
    let h_page = streams
        .iter()
        .position(|s| page_contains(s, "Footnotes"))
        .expect("auto-emitted Footnotes heading not found");
    let b_page = streams
        .iter()
        .position(|s| page_contains(s, "FOOTNOTEDEFMARK"))
        .expect("footnote definition marker not found");
    assert_eq!(
        h_page,
        b_page,
        "Footnotes heading on page {} orphaned from its definition on \
page {}",
        h_page + 1,
        b_page + 1
    );
}

/// An admonition landing right at the page bottom must not split
/// its kind-label strip from its body — the label-only strip on
/// page 1 with body on page 2 is the bug from GH #107. Sweep
/// filler counts to find the orphan-prone landings, then assert
/// label + body always share a page.
#[test]
fn admonition_label_stays_with_body_at_page_boundary() {
    let mut checked = 0usize;
    for n in 52..=62 {
        let mut md = String::from("# t\n\n");
        for i in 0..n {
            md.push_str(&format!("P{i}. line\n\n"));
        }
        md.push_str(
            "> [!CAUTION]\n> ADMOORPHAN long body content. Lorem ipsum dolor \
sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut \
labore et dolore magna aliqua.\n",
        );
        let bytes = render(&md, "");
        let streams = page_streams(&bytes);
        if streams.len() < 2 {
            continue;
        }
        let label_page = streams.iter().position(|s| page_contains(s, "CAUTION"));
        let body_page = streams.iter().position(|s| page_contains(s, "ADMOORPHAN"));
        if let (Some(l), Some(b)) = (label_page, body_page) {
            checked += 1;
            assert_eq!(
                l,
                b,
                "admonition orphaned at filler={n}: label on page {} but \
body on page {}",
                l + 1,
                b + 1
            );
        }
    }
    assert!(
        checked > 0,
        "no fixture in the sweep produced both label and body — test would \
silently pass on a broken renderer"
    );
}
