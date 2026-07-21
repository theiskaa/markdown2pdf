//! Normalize HTML strings so the lexer's rendered output and the spec's
//! expected HTML can be string-compared. Keep the transforms surgical —
//! anything we normalize away is a textual difference we've decided doesn't
//! reflect a real lexer correctness issue.

pub fn normalize(html: &str) -> String {
    let mut s = html.to_string();
    s = collapse_self_closing(&s);
    s = collapse_attribute_form(&s);
    s = normalize_entities(&s);
    s = strip_block_boundary_whitespace(&s);
    s.trim().to_string()
}

fn collapse_self_closing(s: &str) -> String {
    // `<br>` ≡ `<br/>` ≡ `<br />` — canonicalize to `<br />`.
    let tags = ["br", "hr", "img", "input"];
    let mut out = s.to_string();
    for tag in &tags {
        let bare = format!("<{}>", tag);
        let slash_no_space = format!("<{}/>", tag);
        let canonical = format!("<{} />", tag);
        out = out.replace(&bare, &canonical);
        out = out.replace(&slash_no_space, &canonical);
    }
    out
}

fn collapse_attribute_form(s: &str) -> String {
    // `attr=""` ≡ `attr` — the CommonMark spec emits `disabled=""` and our
    // renderer matches that, so this is a no-op today. Stub left for when we
    // need it.
    s.to_string()
}

fn normalize_entities(s: &str) -> String {
    // The spec output sometimes uses `&#39;` for `'` and `&quot;` for `"`
    // in non-attribute positions; sometimes leaves the literal char. We
    // canonicalize a few common forms so the comparator isn't tripped up by
    // equivalent encodings of the same byte.
    let mut out = s.replace("&#39;", "'");
    out = out.replace("&apos;", "'");
    // Keep `&quot;` as-is — it appears inside attribute values where it MUST
    // remain encoded.
    out
}

fn strip_block_boundary_whitespace(s: &str) -> String {
    // Collapse whitespace immediately inside the open/close of block-level
    // tags (`<p> foo </p>` → `<p>foo</p>`). Leave inner text intact.
    // Implementation: regex-free string scan over byte slice.
    let block_tags = [
        "p",
        "li",
        "blockquote",
        "ul",
        "ol",
        "h1",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "div",
        "table",
        "thead",
        "tbody",
        "tr",
        "th",
        "td",
        "pre",
    ];
    let mut out = s.to_string();
    for tag in &block_tags {
        let open_pat = format!("<{}>", tag);
        let close_pat = format!("</{}>", tag);
        // Squash a single newline immediately after the open or before the
        // close to nothing. Two-pass replace because the lexer renderer and
        // spec sometimes disagree on whether to include `\n` after a block
        // opener.
        out = squash_newlines_after(&out, &open_pat);
        out = squash_newlines_before(&out, &close_pat);
    }
    out
}

fn squash_newlines_after(s: &str, marker: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    let mbytes = marker.as_bytes();
    while i < bytes.len() {
        if bytes[i..].starts_with(mbytes) {
            out.push_str(marker);
            i += mbytes.len();
            while i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b' ') {
                i += 1;
            }
        } else {
            // Push one UTF-8 char.
            let c_start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] & 0b1100_0000) == 0b1000_0000 {
                i += 1;
            }
            out.push_str(std::str::from_utf8(&bytes[c_start..i]).unwrap_or(""));
        }
    }
    out
}

fn squash_newlines_before(s: &str, marker: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let bytes = s.as_bytes();
    let mbytes = marker.as_bytes();
    while i < bytes.len() {
        if bytes[i..].starts_with(mbytes) {
            while out.ends_with('\n') || out.ends_with(' ') {
                out.pop();
            }
            out.push_str(marker);
            i += mbytes.len();
        } else {
            let c_start = i;
            i += 1;
            while i < bytes.len() && (bytes[i] & 0b1100_0000) == 0b1000_0000 {
                i += 1;
            }
            out.push_str(std::str::from_utf8(&bytes[c_start..i]).unwrap_or(""));
        }
    }
    out
}
