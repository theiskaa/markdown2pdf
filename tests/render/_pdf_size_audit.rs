//! Deep-dive PDF size auditor. Renders ~20 probe markdown snippets
//! through the live renderer, parses each result with `lopdf`, and
//! emits:
//!   - decoded-stream-payload breakdown by `/Type` / `/Subtype`
//!   - pre- vs post-compression delta (re-deflate / re-pack pass)
//!   - actual content-stream and Form-XObject byte dumps for the
//!     worst offenders, so byte-level patterns (float precision,
//!     repeated operators, redundant /Resources) are visible
//!   - multi-page resource-duplication checks (same font dict
//!     referenced N times across pages?)
//!
//! Reproducible on any host: every PDF is rendered in-process from
//! a static string. Nothing on disk is read.
//!
//! Opt-in:
//!   cargo test --test render _pdf_size_audit -- --ignored --nocapture

#![allow(clippy::print_stdout)]

use lopdf::{Document, Object, SaveOptions};
use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;
use std::collections::{BTreeMap, HashMap};

fn probes() -> Vec<(&'static str, String)> {
    vec![
        ("empty", "\n".into()),
        ("hello_world", "# Hello\n\nPlain prose.\n".into()),
        (
            "paragraphs+table",
            "# Paragraphs\n\nFirst with **bold**, *italic*, `code`.\n\n> quoted\n\n| h1 | h2 |\n| -- | -- |\n| 1  | 2  |\n".into(),
        ),
        (
            "link_heavy",
            "# Links\n\nSee [a](https://example.com/a), [b](https://example.com/b), [c](https://example.com/c).\n".into(),
        ),
        ("link_storm", concat_links_md(20)),
        ("math_inline", "Mass-energy: $E = mc^2$ here.\n".into()),
        (
            "math_display",
            "$$\n\\int_0^\\infty \\frac{x^2}{e^x - 1} \\, dx = \\frac{2\\pi^4}{15}\n$$\n".into(),
        ),
        (
            "math_repeated_glyph",
            "$$\n1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1\n$$\n".into(),
        ),
        (
            "math_same_eq_twice",
            // Same equation rendered twice. If cross-equation glyph
            // dedup is in place, form count should match the single-
            // equation case; otherwise it doubles.
            "$$ a + b = c $$\n\n$$ a + b = c $$\n".into(),
        ),
        (
            "math_same_eq_x10",
            repeat_blocks_md("$$ a + b = c $$\n\n", 10),
        ),
        ("math_storm", MATH_STORM.into()),
        (
            "code_block",
            "```rust\nfn main() {\n    println!(\"hello\");\n}\n```\n".into(),
        ),
        ("hr_storm", repeat_blocks_md("---\n\nparagraph\n\n", 30)),
        ("table_storm", TABLE_STORM.into()),
        (
            "admonitions_all",
            "!!! note\n    a\n\n!!! info\n    b\n\n!!! tip\n    c\n\n!!! warning\n    d\n\n!!! danger\n    e\n".into(),
        ),
        ("admonitions_x10", repeat_blocks_md("!!! warning\n    body\n\n", 10)),
        ("page_storm_10", big_doc(10)),
        ("page_storm_50", big_doc(50)),
        (
            "highlight_storm",
            "All ==highlighted== words ==need== a ==background== fill ==so== they ==accumulate== ==fast==.\n".into(),
        ),
    ]
}

const MATH_STORM: &str = "\
$$ a + b = c $$
$$ x^2 + y^2 = z^2 $$
$$ \\sum_{i=1}^n i = \\frac{n(n+1)}{2} $$
$$ \\int_0^1 x^2 dx = \\frac{1}{3} $$
$$ \\frac{d}{dx} \\sin x = \\cos x $$
$$ \\nabla \\cdot \\vec{E} = \\frac{\\rho}{\\varepsilon_0} $$
$$ e^{i\\pi} + 1 = 0 $$
$$ \\binom{n}{k} = \\frac{n!}{k!(n-k)!} $$
$$ \\sqrt{2} \\approx 1.414 $$
$$ \\lim_{n \\to \\infty} \\frac{1}{n} = 0 $$
";

const TABLE_STORM: &str = "\
| h1 | h2 | h3 | h4 |
| -- | -- | -- | -- |
| a  | b  | c  | d  |
| e  | f  | g  | h  |
| i  | j  | k  | l  |
| m  | n  | o  | p  |
| q  | r  | s  | t  |
| u  | v  | w  | x  |
| y  | z  | 1  | 2  |
| 3  | 4  | 5  | 6  |
| 7  | 8  | 9  | 0  |
";

fn concat_links_md(n: usize) -> String {
    let mut s = String::from("# Link storm\n\n");
    for i in 0..n {
        s.push_str(&format!("[link {0}](https://example.com/p{0}), ", i));
    }
    s.push('\n');
    s
}

fn repeat_blocks_md(block: &str, n: usize) -> String {
    let mut s = String::new();
    for _ in 0..n {
        s.push_str(block);
    }
    s
}

fn big_doc(pages_target: usize) -> String {
    let para = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
                tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, \
                quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo \
                consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse \
                cillum dolore eu fugiat nulla pariatur.";
    let mut s = String::new();
    for _ in 0..(pages_target * 8) {
        s.push_str(para);
        s.push_str("\n\n");
    }
    s
}

#[derive(Default)]
struct CategoryTally {
    count: usize,
    raw_bytes: usize,
    dict_bytes: usize,
}

impl CategoryTally {
    fn record(&mut self, raw: usize, dict: usize) {
        self.count += 1;
        self.raw_bytes += raw;
        self.dict_bytes += dict;
    }
}

fn category(obj: &Object) -> String {
    match obj {
        Object::Stream(s) => name_pair(&s.dict, "stream"),
        Object::Dictionary(d) => name_pair(d, "dict"),
        _ => "other".into(),
    }
}

fn name_pair(d: &lopdf::Dictionary, kind: &str) -> String {
    let ty = d.get(b"Type").ok().and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned());
    let sub = d.get(b"Subtype").ok().and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).into_owned());
    match (ty.as_deref(), sub.as_deref()) {
        (Some(t), Some(s)) => format!("{kind}:/{t}//{s}"),
        (Some(t), None) => format!("{kind}:/{t}"),
        (None, Some(s)) => format!("{kind}:?/{s}"),
        (None, None) if kind == "stream" => "stream:content".into(),
        (None, None) => format!("{kind}:?"),
    }
}

fn approx_obj_bytes(obj: &Object) -> usize {
    match obj {
        Object::String(s, _) => s.len() + 2,
        Object::Name(n) => n.len() + 1,
        Object::Integer(n) => n.to_string().len(),
        Object::Real(f) => format!("{:.4}", f).len(),
        Object::Boolean(_) => 5,
        Object::Reference(_) => 8,
        Object::Null => 4,
        Object::Array(items) => items.iter().map(approx_obj_bytes).sum::<usize>() + 2,
        Object::Dictionary(d) => {
            d.iter().map(|(k, v)| k.len() + 2 + approx_obj_bytes(v)).sum::<usize>() + 4
        }
        Object::Stream(_) => 0,
    }
}

fn approx_dict_bytes(obj: &Object) -> usize {
    match obj {
        Object::Stream(s) => {
            s.dict.iter().fold(0, |a, (k, v)| a + k.len() + 2 + approx_obj_bytes(v))
        }
        Object::Dictionary(d) => {
            d.iter().fold(0, |a, (k, v)| a + k.len() + 2 + approx_obj_bytes(v))
        }
        _ => approx_obj_bytes(obj),
    }
}

/// Re-serialize without object/xref streams, no Flate, so we can
/// see the raw operator soup. Returns (uncompressed_size,
/// gzip_only_size).
fn uncompressed_size(doc_bytes: &[u8]) -> Option<usize> {
    let mut doc = Document::load_mem(doc_bytes).ok()?;
    // Decompress every stream that has a filter so .content holds
    // the post-filter bytes; saving with `compress: false` then
    // writes them raw.
    doc.decompress();
    let mut out = Vec::new();
    let _ = doc.save_with_options(&mut out, SaveOptions {
        use_object_streams: false,
        use_xref_streams: false,
        ..Default::default()
    });
    Some(out.len())
}

fn audit(label: &str, bytes: &[u8]) {
    let total = bytes.len();
    let Ok(doc) = Document::load_mem(bytes) else {
        println!("UNREADABLE {label} ({total} B)");
        return;
    };
    let page_count = doc.page_iter().count();
    let mut tally: BTreeMap<String, CategoryTally> = BTreeMap::new();
    let mut total_payload = 0usize;
    let mut form_xobject_ids: Vec<lopdf::ObjectId> = Vec::new();
    let mut content_stream_lens: Vec<usize> = Vec::new();
    let mut font_ref_counter: HashMap<lopdf::ObjectId, usize> = HashMap::new();

    for (id, obj) in doc.objects.iter() {
        let cat = category(obj);
        let raw = if let Object::Stream(s) = obj { s.content.len() } else { 0 };
        total_payload += raw;
        let dict = approx_dict_bytes(obj);
        tally.entry(cat).or_default().record(raw, dict);
        if let Object::Stream(s) = obj {
            let subtype = s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
            if subtype == Some(&b"Form"[..]) {
                form_xobject_ids.push(*id);
            }
            // Content streams have no /Type and no /Subtype; they
            // appear as `stream:content` in the tally.
            if s.dict.get(b"Type").is_err() && s.dict.get(b"Subtype").is_err() {
                content_stream_lens.push(s.content.len());
            }
        }
    }

    // Count how often each /Font object is referenced from any
    // page's /Resources/Font dict; tells us whether fonts are
    // shared via the page tree or duplicated per page.
    for pid in doc.page_iter() {
        if let Ok(font_dict) = doc.get_object(pid).and_then(|p| {
            p.as_dict().and_then(|d| d.get(b"Resources"))
        }).and_then(|r| match r {
            Object::Reference(rid) => doc.get_object(*rid).and_then(|o| o.as_dict()),
            other => other.as_dict(),
        }).and_then(|res| res.get(b"Font").and_then(|f| match f {
            Object::Reference(rid) => doc.get_object(*rid).and_then(|o| o.as_dict()),
            other => other.as_dict(),
        })) {
            for (_, v) in font_dict.iter() {
                if let Object::Reference(rid) = v {
                    *font_ref_counter.entry(*rid).or_insert(0) += 1;
                }
            }
        }
    }

    // Pre-compression / structural-overhead estimate.
    let uncompressed = uncompressed_size(bytes).unwrap_or(0);

    println!("=== {label}");
    println!(
        "    file_size={total} B  pages={page_count}  objects={}  payload_decoded={total_payload} B (~{:.1}x file)  uncompressed_form={} B (~{:.1}x file)",
        doc.objects.len(),
        total_payload as f64 / total.max(1) as f64,
        uncompressed,
        uncompressed as f64 / total.max(1) as f64,
    );
    println!(
        "    forms={}  content_streams={}  font_refs_by_page={:?}",
        form_xobject_ids.len(),
        content_stream_lens.len(),
        font_ref_counter.values().collect::<Vec<_>>(),
    );

    let mut rows: Vec<(&String, &CategoryTally)> = tally.iter().collect();
    rows.sort_by(|a, b| {
        (b.1.raw_bytes + b.1.dict_bytes).cmp(&(a.1.raw_bytes + a.1.dict_bytes))
    });
    for (cat, t) in rows.iter().take(10) {
        if t.raw_bytes + t.dict_bytes < 50 {
            continue;
        }
        let avg = if t.count > 0 {
            (t.raw_bytes + t.dict_bytes) / t.count
        } else {
            0
        };
        println!(
            "    {:<28} n={:<4} payload={:<7} dict~{:<7} avg/obj={}",
            cat, t.count, t.raw_bytes, t.dict_bytes, avg,
        );
    }
    println!();
}

/// Inflate a Flate-encoded stream payload via lopdf's built-in
/// `decompressed_content`. Returns the original bytes when there
/// is no filter or decoding fails.
fn inflate_stream(s: &lopdf::Stream) -> Vec<u8> {
    let mut copy = s.clone();
    let _ = copy.decompress();
    copy.content
}

/// Pretty-print decompressed bytes; replace newlines with ↵ and
/// truncate to `limit`.
fn snippet(bytes: &[u8], limit: usize) -> String {
    let s = String::from_utf8_lossy(bytes);
    let truncated: String = s.chars().take(limit).collect();
    truncated.replace('\n', "↵")
}

fn dump_math_forms(label: &str, bytes: &[u8], limit: usize) {
    let Ok(doc) = Document::load_mem(bytes) else { return };
    println!("--- {label}: first {limit} Form XObject streams (inflated) ---");
    let mut shown = 0usize;
    let mut decompressed_total = 0usize;
    for (_, obj) in doc.objects.iter() {
        if shown >= limit {
            break;
        }
        let Object::Stream(s) = obj else { continue };
        let subtype = s.dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok());
        if subtype != Some(&b"Form"[..]) {
            continue;
        }
        let dict_keys: Vec<String> = s.dict.iter()
            .map(|(k, _)| String::from_utf8_lossy(k).into_owned())
            .collect();
        let inflated = inflate_stream(s);
        decompressed_total += inflated.len();
        let matrix = s.dict.get(b"Matrix").ok().map(|o| format!("{o:?}"));
        let bbox = s.dict.get(b"BBox").ok().map(|o| format!("{o:?}"));
        println!(
            "    keys={:?}  matrix={}  bbox={}",
            dict_keys,
            matrix.unwrap_or_else(|| "?".into()),
            bbox.unwrap_or_else(|| "?".into()),
        );
        println!(
            "      compressed={} B  inflated={} B  ratio={:.1}x",
            s.content.len(), inflated.len(),
            inflated.len() as f64 / s.content.len().max(1) as f64,
        );
        println!("      ops: {}", snippet(&inflated, 240));
        shown += 1;
    }
    if shown > 0 {
        println!("    (decompressed total for {} forms: {} B)", shown, decompressed_total);
    }
    println!();
}

fn dump_first_page_content(label: &str, bytes: &[u8], take: usize) {
    let Ok(doc) = Document::load_mem(bytes) else { return };
    println!("--- {label}: page content stream (inflated) ---");
    for (_, obj) in doc.objects.iter() {
        let Object::Stream(s) = obj else { continue };
        if s.dict.get(b"Type").is_ok() || s.dict.get(b"Subtype").is_ok() {
            continue;
        }
        let inflated = inflate_stream(s);
        println!(
            "    compressed={} B  inflated={} B  ratio={:.1}x",
            s.content.len(), inflated.len(),
            inflated.len() as f64 / s.content.len().max(1) as f64,
        );
        println!("    ops: {}", snippet(&inflated, take));
        break;
    }
    println!();
}

/// Quick scan of a content stream for tell-tale inefficiencies:
/// float precision (digits after decimal), BT/ET pair density,
/// repeated graphics state push/pop.
fn op_pattern_stats(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let bt = s.matches(" BT").count() + s.matches("\nBT").count();
    let et = s.matches(" ET").count() + s.matches("\nET").count();
    let q = s.matches(" q\n").count() + s.matches("\nq\n").count();
    let qq = s.matches(" Q\n").count() + s.matches("\nQ\n").count();
    let tf = s.matches(" Tf").count();
    let cm = s.matches(" cm").count();
    let mut decimals = [0usize; 10];
    let mut in_frac = false;
    let mut frac_digits = 0usize;
    for c in s.chars() {
        if c == '.' {
            in_frac = true;
            frac_digits = 0;
        } else if in_frac && c.is_ascii_digit() {
            frac_digits += 1;
        } else if in_frac {
            decimals[frac_digits.min(9)] += 1;
            in_frac = false;
        }
    }
    format!(
        "BT/ET={bt}/{et}  q/Q={q}/{qq}  Tf={tf}  cm={cm}  frac_digit_distribution={:?}",
        &decimals[1..],
    )
}

#[test]
#[ignore]
fn audit_probe_renders() {
    let mut renders: Vec<(String, Vec<u8>)> = Vec::new();
    for (label, md) in probes() {
        match parse_into_bytes(md.to_string(), ConfigSource::Default, None) {
            Ok(b) => renders.push((label.to_string(), b)),
            Err(e) => println!("RENDER FAIL {label}: {e}"),
        }
    }
    for (label, bytes) in &renders {
        audit(label, bytes);
    }
    // Drill into the worst math offender + the admonition icons +
    // a multi-page doc for cross-page resource sharing.
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "math_display") {
        dump_math_forms(label, b, 3);
    }
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "math_repeated_glyph") {
        dump_math_forms(label, b, 4);
    }
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "admonitions_all") {
        dump_first_page_content(label, b, 800);
    }
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "page_storm_10") {
        dump_first_page_content(label, b, 600);
    }
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "math_same_eq_twice") {
        dump_math_forms(label, b, 0);
        // Just print form count from the audit above; the dump
        // limit=0 still prints the header so the reader can see
        // we looked.
        if let Ok(doc) = Document::load_mem(b) {
            let forms = doc.objects.values().filter(|o| {
                if let Object::Stream(s) = o {
                    s.dict.get(b"Subtype").ok().and_then(|x| x.as_name().ok())
                        == Some(&b"Form"[..])
                } else {
                    false
                }
            }).count();
            println!("  {label}: form count = {forms} (single equation = 5 forms)");
        }
    }
    if let Some((label, b)) = renders.iter().find(|(l, _)| l == "math_same_eq_x10") {
        if let Ok(doc) = Document::load_mem(b) {
            let forms = doc.objects.values().filter(|o| {
                if let Object::Stream(s) = o {
                    s.dict.get(b"Subtype").ok().and_then(|x| x.as_name().ok())
                        == Some(&b"Form"[..])
                } else {
                    false
                }
            }).count();
            println!("  {label}: form count = {forms} (single equation = 5 forms; \
                      with cross-eq dedup expected ~5, without ~50)");
        }
    }
    // Operator-pattern stats on the worst content-stream cases.
    for label in ["paragraphs+table", "table_storm", "hr_storm",
                  "admonitions_x10", "page_storm_50"] {
        if let Some((_, b)) = renders.iter().find(|(l, _)| l == label) {
            if let Ok(doc) = Document::load_mem(b) {
                for (_, obj) in doc.objects.iter() {
                    let Object::Stream(s) = obj else { continue };
                    if s.dict.get(b"Type").is_ok() || s.dict.get(b"Subtype").is_ok() {
                        continue;
                    }
                    let inflated = inflate_stream(s);
                    println!("  ops[{label}]: {} (inflated={} B)",
                             op_pattern_stats(&inflated), inflated.len());
                    break;
                }
            }
        }
    }
}
