//! Reference / collapsed / shortcut image tests, focused on the
//! reference-resolution path in `parse_image`.

use markdown2pdf::markdown::*;

use super::common::parse;


fn first_image_url_title(tokens: &[Token]) -> Option<(String, Option<String>)> {
    tokens.iter().find_map(|t| {
        if let Token::Image { url, title, .. } = t {
            Some((url.clone(), title.clone()))
        } else {
            None
        }
    })
}

#[test]
fn full_reference_propagates_title() {
    let tokens = parse("![alt][lab]\n\n[lab]: /u \"the title\"\n");
    let (url, title) = first_image_url_title(&tokens).unwrap();
    assert_eq!(url, "/u");
    assert_eq!(title.as_deref(), Some("the title"));
}

#[test]
fn collapsed_reference_propagates_title() {
    let tokens = parse("![alt][]\n\n[alt]: /u \"x\"\n");
    let (url, title) = first_image_url_title(&tokens).unwrap();
    assert_eq!(url, "/u");
    assert_eq!(title.as_deref(), Some("x"));
}

#[test]
fn shortcut_reference_propagates_title() {
    let tokens = parse("![alt]\n\n[alt]: /u \"x\"\n");
    let (url, title) = first_image_url_title(&tokens).unwrap();
    assert_eq!(url, "/u");
    assert_eq!(title.as_deref(), Some("x"));
}

#[test]
fn case_folded_label_matches() {
    let tokens = parse("![ALT]\n\n[alt]: /u\n");
    assert!(first_image_url_title(&tokens).is_some());
}

#[test]
fn whitespace_collapsed_label_matches() {
    let tokens = parse("![multi   word]\n\n[multi word]: /u\n");
    assert!(first_image_url_title(&tokens).is_some());
}

#[test]
fn unresolved_reference_image_falls_back() {
    let tokens = parse("![alt][missing]");
    assert!(first_image_url_title(&tokens).is_none());
    assert!(Token::collect_all_text(&tokens).contains("missing"));
}

#[test]
fn unresolved_shortcut_image_falls_back() {
    let tokens = parse("![nodef]");
    assert!(first_image_url_title(&tokens).is_none());
    assert!(Token::collect_all_text(&tokens).contains("nodef"));
}

#[test]
fn definition_defined_after_image_use_still_resolves() {
    let tokens = parse("Some prose with ![pic][lab] in it.\n\n[lab]: /pic.png\n");
    let (url, _) = first_image_url_title(&tokens).unwrap();
    assert_eq!(url, "/pic.png");
}
