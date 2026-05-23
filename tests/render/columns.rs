//! Multi-column layout coverage. Verifies that `[page].columns` flows
//! body content into N side-by-side columns instead of a single wide
//! flow, that column-break / page-break interact correctly, and that
//! the schema clamp (1..=4) holds.
//!
//! Assertions are coarse: rather than pinning the wrap points (font-
//! metric-sensitive), they look at the distribution of `Td` x-cursor
//! ops in the decompressed content stream. Td x sits at the column's
//! left edge for left-aligned runs and at the column's left + padding
//! for nested blocks, so distinct columns produce distinct x clusters.

use super::common::*;

/// Filler markdown long enough that two columns can't swallow it on
/// one page — guarantees the wrap engine has to spill into column 1
/// (and beyond, in 3- and 4-column runs).
fn long_body(n: usize) -> String {
    let mut out = String::new();
    for i in 0..n {
        out.push_str(&format!(
            "## Section {i}\n\nLorem ipsum dolor sit amet, consectetur adipiscing \
elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. \
Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut \
aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
voluptate velit esse cillum dolore eu fugiat nulla pariatur.\n\n"
        ));
    }
    out
}

/// All `x` values from `<x> <y> Td` ops anywhere in the decompressed
/// PDF byte stream. `Td` writes the absolute text-line origin in PDF
/// user space — the first column emits one cluster of x values around
/// the left margin, each subsequent column emits its own cluster.
fn td_xs(bytes: &[u8]) -> Vec<f32> {
    let decoded = scan(bytes);
    let s = String::from_utf8_lossy(&decoded);
    let mut xs = Vec::new();
    for line in s.lines() {
        // `Td` lines look like `<x> <y> Td`; skip `TD`, `Tf`, `Tj`, etc.
        let trimmed = line.trim_end();
        if !trimmed.ends_with(" Td") {
            continue;
        }
        let mut it = trimmed.split_whitespace();
        let x = it.next();
        let y = it.next();
        let op = it.next();
        if op != Some("Td") {
            continue;
        }
        if let (Some(xs_), Some(_)) = (x.and_then(|t| t.parse::<f32>().ok()), y) {
            xs.push(xs_);
        }
    }
    xs
}

/// Cluster Td x values into bins ~10pt wide and return the bin
/// centers, sorted. Two columns separated by a few-mm gap end up in
/// well-separated bins; nested blocks (blockquote padding, code-block
/// padding) sit a few points to the right of the column's nominal
/// left, but stay inside its bin.
fn x_clusters(mut xs: Vec<f32>, bin_pt: f32) -> Vec<f32> {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut clusters: Vec<(f32, usize)> = Vec::new(); // (sum, count)
    for x in xs {
        if let Some((sum, n)) = clusters.last_mut() {
            if (x - *sum / *n as f32).abs() < bin_pt {
                *sum += x;
                *n += 1;
                continue;
            }
        }
        clusters.push((x, 1));
    }
    clusters.into_iter().map(|(s, n)| s / n as f32).collect()
}

/// Number of distinct column x-edges expected in a render: cluster the
/// Td x values at a granularity smaller than the inter-column gap but
/// larger than typical block-padding offsets.
fn distinct_column_edges(bytes: &[u8]) -> usize {
    // The smallest inter-column distance in our test renders is the
    // column gap (>= 6mm = ~17pt); the largest intra-column shift is
    // the admonition / blockquote padding (~10pt). A 12pt bin width
    // straddles that gap cleanly.
    x_clusters(td_xs(bytes), 12.0).len()
}

#[test]
fn default_render_is_single_column() {
    // No [page].columns set; behavior is exactly the pre-column flow.
    let bytes = render(&long_body(8), "");
    let edges = distinct_column_edges(&bytes);
    assert!(
        edges <= 3,
        "single-column render should have at most a handful of x edges \
         (left margin + padding nests), got {edges}"
    );
}

#[test]
fn two_columns_emits_text_in_both() {
    let bytes = render(
        &long_body(10),
        r##"
        [page]
        columns = 2
        column_gap_mm = 8
        "##,
    );
    let xs = td_xs(&bytes);
    assert!(!xs.is_empty(), "expected some Td ops in the body");
    let min_x = xs.iter().cloned().fold(f32::INFINITY, f32::min);
    let max_x = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    // Column 1's left edge sits past the page midpoint; on letter
    // (612pt wide) with default margins the midpoint is ~306pt and
    // col 1 starts around ~309pt. A single-column render has every
    // Td x clustered at the left margin (~45pt) and never crosses the
    // midline. Pick 200pt as a safe threshold — well past the widest
    // table or padding offset, well short of col 1's nominal left.
    assert!(
        max_x - min_x > 200.0,
        "expected Td x to span both columns (min={min_x:.1}, max={max_x:.1})"
    );
}

#[test]
fn two_columns_creates_at_least_two_column_clusters() {
    let bytes = render(
        &long_body(10),
        r##"
        [page]
        columns = 2
        column_gap_mm = 8
        "##,
    );
    assert!(
        distinct_column_edges(&bytes) >= 2,
        "two-column layout should produce at least two column x edges"
    );
}

#[test]
fn paragraph_after_column_break_uses_new_column_geometry() {
    // Regression for the begin_block / end_block save-restore bug:
    // when a block ends in column 0 and advance_y triggers a column
    // advance, the *next* block's begin_block must see the new
    // column's indents, not the saved-at-begin-time column 0 indents.
    //
    // The failure mode pre-fix: subsequent paragraphs in column 1
    // still wrap at column 0's x. Symptom in the Td stream: only
    // *one* Td op lands at column 1's x (the first one after the
    // column break, before end_block restores stale indents), and
    // everything afterwards collapses back to column 0's x.
    //
    // We assert > 1 Td op in column 1's region so a recurrence of
    // the bug fails this test, even if the very first post-break
    // line happens to position correctly.
    let bytes = render(
        &long_body(12),
        r##"
        [page]
        columns = 2
        column_gap_mm = 8
        "##,
    );
    let xs = td_xs(&bytes);
    // Letter @ 612pt with default 16mm margins => midpoint ~306pt;
    // col 1 left lands ~309pt. Anything past 250pt is unambiguously
    // in column 1.
    let in_col1 = xs.iter().filter(|&&x| x > 250.0).count();
    assert!(
        in_col1 > 1,
        "expected several Td ops in column 1 after the break, got {in_col1} \
         (xs={xs:?})"
    );
}

#[test]
fn three_columns_emits_three_distinct_clusters() {
    let bytes = render(
        &long_body(18),
        r##"
        [page]
        columns = 3
        column_gap_mm = 6
        "##,
    );
    assert!(
        distinct_column_edges(&bytes) >= 3,
        "three-column layout should produce at least three column x edges"
    );
}

#[test]
fn four_columns_emits_four_distinct_clusters() {
    let bytes = render(
        &long_body(24),
        r##"
        [page]
        columns = 4
        column_gap_mm = 4
        "##,
    );
    assert!(
        distinct_column_edges(&bytes) >= 4,
        "four-column layout should produce at least four column x edges"
    );
}

#[test]
fn columns_above_four_clamp_to_four() {
    // Schema clamp is 1..=4; an absurd value must not blow geometry
    // (a 99-column page on a 6-inch body would give negative col
    // widths) and must not render more than four x clusters.
    let bytes = render(
        &long_body(20),
        r##"
        [page]
        columns = 99
        column_gap_mm = 4
        "##,
    );
    assert!(pdf_well_formed(&bytes));
    assert!(
        distinct_column_edges(&bytes) <= 4,
        "columns=99 should be clamped to 4, but found more x clusters"
    );
}

#[test]
fn columns_zero_clamps_to_single_column() {
    let bytes = render(
        &long_body(6),
        r##"
        [page]
        columns = 0
        "##,
    );
    assert!(pdf_well_formed(&bytes));
    // Same expectation as default render: no second-column cluster.
    let xs = td_xs(&bytes);
    let max_x = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    assert!(
        max_x < 250.0,
        "columns=0 should collapse to single-column (max Td x={max_x:.1})"
    );
}

#[test]
fn negative_column_gap_does_not_break_geometry() {
    // Hostile gap value: must not produce negative column widths or
    // crash the wrap engine. Renderer floors the gap at 0.
    let bytes = render(
        &long_body(10),
        r##"
        [page]
        columns = 2
        column_gap_mm = -50.0
        "##,
    );
    assert!(pdf_well_formed(&bytes));
    assert!(distinct_column_edges(&bytes) >= 2);
}

#[test]
fn images_tables_and_code_blocks_survive_a_column_break() {
    // Mixed-block document: ensures non-paragraph block types
    // (table, code block, blockquote, admonition) don't corrupt the
    // column-break path. Pre-fix, the table's per-cell save/restore
    // mostly self-contained, but a code block following a table that
    // straddled a column would inherit stale indents — same root cause
    // as the paragraph bug.
    let md = r##"
# Heading

Body paragraph with enough text to do useful work in the first column.
Lorem ipsum dolor sit amet, consectetur adipiscing elit.

| Name  | Role      | Count |
| ----- | --------- | ----- |
| Alice | author    | 12    |
| Bob   | reviewer  | 7     |

```rust
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}
```

> A quoted block that wraps over a few lines so that subsequent
> blocks have somewhere to spill into.

Closing paragraph. Lorem ipsum dolor sit amet, consectetur adipiscing
elit. Sed do eiusmod tempor incididunt ut labore et dolore magna
aliqua. Ut enim ad minim veniam, quis nostrud exercitation.

"##;
    let mut doc = String::new();
    for _ in 0..6 {
        doc.push_str(md);
    }
    let bytes = render(
        &doc,
        r##"
        [page]
        columns = 2
        column_gap_mm = 8
        "##,
    );
    assert!(pdf_well_formed(&bytes));
    assert!(
        distinct_column_edges(&bytes) >= 2,
        "mixed-block doc should still flow into a second column"
    );
}
