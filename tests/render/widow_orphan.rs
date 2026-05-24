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
        .map(|pid| doc.get_page_content(pid).expect("page content stream"))
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
            heading_page, body_page,
            "Section {i} orphaned: heading on page {} but body on page {}",
            heading_page + 1,
            body_page + 1
        );
    }
}

