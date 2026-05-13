use markdown2pdf::markdown::*;

use super::common::parse;


fn collected(input: &str) -> String {
    Token::collect_all_text(&parse(input))
}

#[test]
fn xml_safe_entities() {
    assert_eq!(collected("a &amp; b"), "a & b");
    assert_eq!(collected("&lt;tag&gt;"), "<tag>");
    assert_eq!(collected("she said &quot;hi&quot;"), "she said \"hi\"");
    assert_eq!(collected("it&apos;s"), "it's");
}

#[test]
fn common_html_named_entities() {
    assert_eq!(collected("&copy; 2025"), "© 2025");
    assert_eq!(collected("&reg; mark"), "® mark");
    assert_eq!(collected("&trade;"), "™");
    assert_eq!(collected("&mdash;"), "—");
    assert_eq!(collected("&ndash;"), "–");
    assert_eq!(collected("&hellip;"), "…");
}

#[test]
fn numeric_decimal_reference() {
    assert_eq!(collected("&#35;"), "#");
    assert_eq!(collected("&#65;"), "A");
    assert_eq!(collected("&#8212;"), "—"); // em dash
}

#[test]
fn numeric_hex_reference() {
    assert_eq!(collected("&#x23;"), "#");
    assert_eq!(collected("&#x41;"), "A");
    assert_eq!(collected("&#X41;"), "A"); // capital X also valid
    assert_eq!(collected("&#x2014;"), "—");
}

#[test]
fn unknown_entity_passes_through() {
    assert_eq!(collected("&zzznotreal;"), "&zzznotreal;");
}

#[test]
fn missing_semicolon_passes_through() {
    // CommonMark requires terminating semicolon; without one, no decoding.
    assert_eq!(collected("&amp foo"), "&amp foo");
}

#[test]
fn lone_ampersand_is_literal() {
    assert_eq!(collected("a & b"), "a & b");
}

#[test]
fn entity_inside_emphasis() {
    let tokens = parse("*alpha &amp; beta*");
    if let Token::Emphasis { content, .. } = &tokens[0] {
        let inner = Token::collect_all_text(content);
        assert!(inner.contains("alpha & beta"), "got {:?}", inner);
    } else {
        panic!("expected emphasis, got {:?}", tokens);
    }
}

#[test]
fn entity_not_decoded_inside_code_span() {
    // Code spans are literal — entity stays as-is.
    let tokens = parse("`&amp;`");
    assert_eq!(tokens, vec![Token::Code { language: "".to_string(), content: "&amp;".to_string(), block: false }]);
}

#[test]
fn invalid_numeric_passes_through() {
    // Out-of-range / malformed numerics pass through unchanged.
    assert_eq!(collected("&#xZZZ;"), "&#xZZZ;");
    assert_eq!(collected("&#abc;"), "&#abc;");
}

#[test]
fn extended_named_entities_decode() {
    // Sample entries spanning the alphabet / character planes.
    assert_eq!(collected("&alpha;"), "\u{03B1}");
    assert_eq!(collected("&beta;"), "\u{03B2}");
    assert_eq!(collected("&Pi;"), "\u{03A0}");
    assert_eq!(collected("&infin;"), "\u{221E}");
    assert_eq!(collected("&euro;"), "\u{20AC}");
    assert_eq!(collected("&para;"), "\u{00B6}");
    assert_eq!(collected("&shy;"), "\u{00AD}"); // soft hyphen
}

#[test]
fn longest_named_entity_decodes() {
    // 31-char body; verifies the lookahead is wide enough.
    assert_eq!(
        collected("&CounterClockwiseContourIntegral;"),
        "\u{2233}"
    );
}

#[test]
fn multi_codepoint_named_entities_decode() {
    // Some entries map to two code points.
    assert_eq!(collected("&fjlig;"), "fj");
    assert_eq!(collected("&ThickSpace;"), "\u{205F}\u{200A}");
}

#[test]
fn entity_names_are_case_sensitive() {
    // Per HTML5: `Aacute` and `aacute` are distinct entries.
    assert_eq!(collected("&Aacute;"), "\u{00C1}");
    assert_eq!(collected("&aacute;"), "\u{00E1}");
}

#[test]
fn numeric_null_becomes_replacement_char() {
    // code point 0 → U+FFFD.
    assert_eq!(collected("&#0;"), "\u{FFFD}");
    assert_eq!(collected("&#x0;"), "\u{FFFD}");
}

#[test]
fn numeric_surrogate_becomes_replacement_char() {
    // Surrogates D800..=DFFF → U+FFFD.
    assert_eq!(collected("&#xD800;"), "\u{FFFD}");
    assert_eq!(collected("&#xDFFF;"), "\u{FFFD}");
    assert_eq!(collected("&#55296;"), "\u{FFFD}"); // 0xD800 decimal
}

#[test]
fn numeric_out_of_range_becomes_replacement_char() {
    // > U+10FFFF → U+FFFD.
    assert_eq!(collected("&#x110000;"), "\u{FFFD}");
    assert_eq!(collected("&#1114112;"), "\u{FFFD}");
}

#[test]
fn numeric_overflow_passes_through_literal() {
    // A digit string that overflows u32 isn't a valid numeric reference;
    // it should appear verbatim (not silently decode to FFFD).
    assert_eq!(collected("&#999999999999;"), "&#999999999999;");
}

#[test]
fn empty_numeric_digits_passes_through() {
    // `&#;` and `&#x;` are malformed — no decoding.
    assert_eq!(collected("&#;"), "&#;");
    assert_eq!(collected("&#x;"), "&#x;");
}

#[test]
fn legacy_non_semicolon_entity_passes_through() {
    // only semicolon-terminated entries decode, even
    // though browsers accept some legacy forms like `&amp` or `&AElig`.
    assert_eq!(collected("&AElig hello"), "&AElig hello");
}

#[test]
fn many_entities_in_one_paragraph() {
    // Stress: a fistful of decodings in a single token stream.
    let text = collected("&alpha; &beta; &gamma; &delta; &epsilon;");
    assert_eq!(text, "\u{03B1} \u{03B2} \u{03B3} \u{03B4} \u{03B5}");
}

#[test]
fn unknown_long_entity_does_not_runaway() {
    // A bogus `&` with no `;` for many chars must NOT consume the rest
    // of the document — emits literal `&` and the rest stays text.
    let text = collected("a &thisnameisreallylongandnotrealatall but continues here.");
    assert!(text.starts_with("a &thisname"), "got: {:?}", text);
    assert!(text.contains("continues here"), "got: {:?}", text);
}
