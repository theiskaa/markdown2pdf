# Changelog

All notable changes to **markdown2pdf** are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Each release section below is what ships as the GitHub Release notes.

## [1.6.0] - 2026-07-22

A security and robustness release. A crash on malformed HTML is fixed, remote fetching is hardened against SSRF and unbounded downloads, local image reads can now be confined to a directory, three RUSTSEC advisories are closed, and CI gained real gates for formatting, linting, and dependency audits.

Thanks to **Alexander Bruegmann** ([@Brueggus](https://github.com/Brueggus)), first-time contributor, for the table header background fix in [#121](https://github.com/theiskaa/markdown2pdf/pull/121).

- **Table header backgrounds render correctly**: a header row's `background_color` was resolved but never drawn, and the fill was missing again on every repeated header after a page or column break. Header and alternating-row fills now share one drawing path. Thanks to Alexander Bruegmann.
- **Fixed a crash on malformed HTML**: a block such as `<p </p>`, where the opening tag is never closed, made the wrapper-stripping code compute an inverted byte range and panic, taking down the calling process. Such blocks now fall back to verbatim HTML rendering. Reachable from a seven-character document, so any caller rendering untrusted Markdown was exposed.
- **Remote fetching hardened**: the renderer now resolves a remote image's host and validates every resolved address before connecting, then pins the connection to the address it checked. Loopback, private, link-local, CGNAT, and IPv6 NAT64 targets are refused, so a document can no longer reach cloud metadata endpoints or internal services. Redirects are capped and each hop is re-validated. Downloads are bounded in both size and total wall-clock time; previously the size cap trusted a server-supplied `Content-Length` and the timeout only measured idle time, so a slow-drip server could hold a fetch open indefinitely. The CLI's `--url` input gained the same size and time bounds. Set `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` to opt out on an intranet.
- **Local image reads can be confined**: a new `[security]` config block adds `image_root`, `allow_absolute_image_paths`, and `allow_remote_images`. With `image_root` set, image paths resolve inside it and anything escaping it, including through a symlink, is refused and degrades to alt text. Defaults are unchanged, so existing documents behave exactly as before; callers rendering untrusted Markdown should set `image_root`. See [docs/configuration.md](docs/configuration.md).
- **SVG parsing no longer loads external resources**: an untrusted SVG could previously reference local files through `<image href="...">` during parsing. Data URIs still work.
- **Crate documentation now matches the real schema**: the configuration examples on the docs.rs landing page used keys that no longer exist (`[heading.1]`, `size`, `textcolor`, `fontfamily`) and could not parse against the `deny_unknown_fields` schema. `parse_into_file` also had its documentation attached to the wrong function. A test now parses the documented examples so they cannot drift again.
- **Three advisories closed**: RUSTSEC-2026-0187 (`lopdf`, stack overflow), RUSTSEC-2026-0185 (`quinn-proto`, memory exhaustion), and RUSTSEC-2026-0009 (`time`, denial of service). `cargo audit` reports zero vulnerabilities.
- **printpdf 0.9 to 0.11**: collapses `lopdf` to a single `0.44` copy, where two versions previously coexisted, and drops the unmaintained `proc-macro-error` from the tree entirely.
- **CI covers the optional features and gates on quality**: `fetch` and `svg` gate real code that CI had never compiled. Builds and tests now run across a four-way feature matrix, and the `fmt`, `clippy`, and `cargo audit` jobs block a merge rather than reporting into the void. A weekly scheduled run catches newly published advisories.
- **Renderer internals reorganised**: network policy and image-path policy moved out of the layout engine into `net_guard`, `net_read`, and `image_policy`. No behavior change.
- **Whole tree reformatted**: `cargo fmt --all` applied crate-wide for the first time, with the development toolchain pinned so the result is reproducible. Purely mechanical.
- **Breaking**: `ResolveError` is now `#[non_exhaustive]`, so exhaustive `match`es need a wildcard arm, and `ResolveError::BadToml.source` changed from `toml::de::Error` to `Box<toml::de::Error>`, which shrinks the error type from 160 to 80 bytes. Code that constructs or exhaustively matches this type needs updating. The MSRV is now **1.88**, which the existing `lopdf` dependency already required in practice.

## [1.5.1] - 2026-07-17

Two rendering fixes: math `\text{…}` now renders non-Latin scripts, and documents with many headings no longer produce corrupt PDFs.

- **Math `\text{…}` renders non-Latin scripts** — characters STIX Two Math lacks (CJK, Arabic, Devanagari, Cyrillic, …) inside `\text{}`/`\operatorname{}` and as bare symbols are now outlined from the document's body font and `[defaults].fallback_fonts` faces instead of silently vanishing. RTL runs reorder to visual order (ordering only — no joining/shaping); zero-width format characters (ZWNJ / ZWJ / bidi marks) stay invisible; a character no configured font covers renders as a placeholder box with a warning instead of disappearing. Covered-Latin math output is byte-identical.
- **Corrupt PDFs with many headings** — bumped `lopdf` to 0.44, fixing an object-stream defect that could corrupt PDFs containing many headings.

Resolves [#115](https://github.com/theiskaa/markdown2pdf/issues/115) and [#119](https://github.com/theiskaa/markdown2pdf/issues/119).

## [1.5.0] - 2026-05-26

Layout polish — multi-column flow, plus targeted fixes to tables, defs, admonitions, HTML, math, page breaks, and Unicode rendering.

- **Multi-column layout** — `[page] columns = N` (1..=4), `column_gap_mm`; default `columns = 1` byte-identical to pre-feature output. Headers, footers, title page, and TOC stay single-column.
- **Admonitions** — nested-body footnotes resolve against doc-wide numbering; `caution`/`important` keep their authored label with canonical kind styling; auto-emitted strings seed the external-font subset; a page-bottom admonition keeps its label with its first body line.
- **GFM tables in list items / blockquotes / admonitions** — a 4-space-indented table inside any container body tokenises as a table instead of leaking literal pipes.
- **Definition lists** — bodies capture 4-space-indented continuations and re-lex in block context (code, table, blockquote, nested list, multiple paragraphs); multi-term groups (`Alpha\nBeta\n: shared`) and a second `:` block after a blank line are recognised.
- **HTML & links** — inline `<span>`/`<strong>`/`<em>`/`<code>`/`<sup>`/`<sub>` render semantically; `<div class="…">…</div>` unwraps; `<br/>`/`<hr/>` interpreted; unresolved wikilinks render in a dead-link colour; long URLs wrap at `/?&#` boundaries; `#dup-2` links resolve.
- **Math engine** — `$$ … = … $$` no longer eaten as a setext H1 underline; `\lvert`/`\rvert`/`\lVert`/`\rVert`/`\omicron` added; tall paren/bracket assemblies for 4+ row matrices stack in the right order; long display equations scale to fit; inline `$…$` no longer dropped when it is the only content in its paragraph.
- **Widow / orphan** — headings drag their first follow-on chunk across page breaks; a bullet glyph no longer renders alone at the page bottom.
- **Unicode by default** — when no font is configured, auto-pick an installed Unicode body font (Helvetica Neue / Geneva on macOS, Segoe UI / Arial / Tahoma on Windows, DejaVu Sans / Liberation Sans / Noto Sans on Linux) so `café`, em-dashes, smart quotes, and math symbols outside `$..$` stop becoming `?`. Same fallback fires when the configured name is a built-in alias whose only on-disk copy is a `.ttc` collection the loader skips. CJK / Arabic / Devanagari / emoji still need explicit `[defaults].fallback_fonts`.

Resolves [#102](https://github.com/theiskaa/markdown2pdf/issues/102), [#105](https://github.com/theiskaa/markdown2pdf/issues/105), [#106](https://github.com/theiskaa/markdown2pdf/issues/106), [#107](https://github.com/theiskaa/markdown2pdf/issues/107), [#108](https://github.com/theiskaa/markdown2pdf/issues/108), [#109](https://github.com/theiskaa/markdown2pdf/issues/109), and [#111](https://github.com/theiskaa/markdown2pdf/issues/111).

## [1.4.0] - 2026-05-23

Fallback font chain, end-to-end style-config fidelity, transliteration-aware measurement, and automatic config-file discovery.

- **Fallback font chain** — new `fallback_fonts` list on `[defaults]` (plus `FontConfig::with_fallback_fonts(...)`); each codepoint matches the primary first, then each fallback in declaration order. A codepoint covered by nothing degrades visibly (`?` with built-in, missing-glyph box with external) instead of failing. Names resolve like `font_family` (built-in aliases, system names, `.ttf`/`.otf` paths); missing fonts log and skip. Wrapping stays consistent across font boundaries.
- **Built-in measurement matches emission** — the transliterating built-in path used to price source codepoints (`•`, `—`, `…`, smart quotes, NBSP) at the font's average width while emitting their ASCII expansion. The cursor drifted and underlines/link rects landed beside their text on links separated by `•` or `—`. Measurement now routes through the same transliteration the emit path uses; Unicode-font documents unaffected.
- **Config file discovery** — with no `-c` flag, the CLI checks `MARKDOWN2PDF_CONFIG`, then `./markdown2pdf.toml`, then `~/.config/markdown2pdf/config.toml` before the default theme.
- **Style-config fidelity** — a long tail of style fields the renderer was silently dropping is now honoured: body font (system lookup returning a bold face as the regular weight is fixed, `[defaults].font_family` actually loads, heading bold from the style block embeds its variant faces, `[link].underline = false` suppresses link underline); per-block (`letter_spacing_pt`, `indent_pt`, `underline`/`strikethrough`/`text_align`, table `cell_padding`/`alternating_row_background`); inline (every `code_inline` and `[mark]` field including `font_family`/`padding`); layout (list `indent_per_level_pt`, new `bullet_gap_pt` on `[list.common]`, image caption styling, title-page cover image, admonition body inheritance). Center- and right-aligned paragraphs no longer collapse wrapped lines onto line one. Only `page.columns` still deferred — tracked as [#102](https://github.com/theiskaa/markdown2pdf/issues/102).

Resolves [#81](https://github.com/theiskaa/markdown2pdf/issues/81), [#97](https://github.com/theiskaa/markdown2pdf/issues/97), and [#100](https://github.com/theiskaa/markdown2pdf/issues/100).

## [1.3.0] - 2026-05-20

Merged GFM table cells, inline HTML anchors and structural wrappers, and admonition callout boxes — all additive, render path unchanged for docs that don't use them.

- **Merged table cells** — `>` extends the cell before it by one column, `^` continues the cell above by one row; markers match raw cell source so `\>`/`\^` stay literal. They chain and combine; layout keeps a spanned group whole across page breaks. Plain tables byte-identical.
- **Inline HTML anchors & structural wrappers** — `<a href="…" title="…">…</a>` rewrites to a real link token (clickable PDF annotation with tooltip, same path as `[text](url "title")`). `<div>`/`<section>`/`<figure>`/`<figcaption>` (joining existing `<p>`/`<center>`) unwrap. Malformed input falls through; unknown tags still render verbatim; no scripting introduced.
- **Admonition / callout boxes** — MkDocs `!!! note "Title"` (4-space-indented body) and GitHub `> [!WARNING]` both render as tinted boxes with accent border, bold header, and a vector icon (● ⓘ 💡 ⚠ ⊗, ≡ for unknown kinds). Five first-class kinds absorb aliases (`caution`/`error` → `danger`, `important` → `info`, `warn`/`attention` → `warning`, `hint` → `tip`); unknown kinds get a grey box with the raw label uppercased. New `[admonition]` config block plus per-kind `[admonition.<kind>]` overlays; every bundled theme ships its own palette.

Resolves [#83](https://github.com/theiskaa/markdown2pdf/issues/83), [#84](https://github.com/theiskaa/markdown2pdf/issues/84), and [#86](https://github.com/theiskaa/markdown2pdf/issues/86).

## [1.2.0] - 2026-05-18

LaTeX math is now parsed and typeset in-tree — `$…$` (inline) and `$$…$$` (display) stop leaking through as literal text.

- **In-tree TeX math engine** — fractions, radicals, sub/superscript stacks, big operators with limits, growing delimiters, matrices, accents, and the blackboard/script/fraktur alphabets, laid out over STIX Two Math's OpenType MATH metrics.
- **Vector outlines, not embedded font** — math draws as filled outlines, so no font is embedded and the equation isn't selectable (behaves like a figure). Matches LaTeX/MathJax/KaTeX-to-PDF pipelines and keeps PDFs small.
- **Pandoc-style delimiters** — `$` opener needs a non-space after it, closer needs a non-space before and no trailing digit; `\$` is literal; unterminated `$`/`$$` degrades to text.
- **Coverage** — `\frac`/`\binom`, `\sqrt[n]{}`, coupled sub/superscript stacks, big operators with `\limits`/`\nolimits`, growing `\left…\right`/`\big` delimiters, `pmatrix`/`cases`/`aligned` environments, accents (incl. stretchy `\widehat`), `\mathbb`/`\mathbf`/`\mathcal`/… alphabets, operator names. Unknown commands degrade to literal text; input depth bounded so adversarial markup can't blow up layout.
- **`[math]` config block** — `align` (`center`/`left`/`right`), `scale`, `color`, and block margins. See `docs/configuration.md`.

Resolves [#78](https://github.com/theiskaa/markdown2pdf/issues/78).

## [1.1.0] - 2026-05-18

Note-vault syntax: inline highlight, WikiLinks, and Pandoc inline footnotes — Obsidian/Pandoc/MediaWiki-style docs export to PDF without leaking syntax as literal text. Plus two lexer/validation edge cases surfaced during the work.

- **Inline footnotes (`text^[note body]`)** ([#80]) — Pandoc-style footnotes inline, no separate `[^id]:` definition. Lexer scans a balanced bracket pair (nesting and `\`-escapes respected), sub-lexes the body as inline content, and assigns a doc-unique internal label from a counter shared across nested sub-lexers. Inline and `[^id]` footnotes share one numbering sequence in first-reference order, the marker is the same superscript linking to `#footnote-N`, and bodies collect into the single tail **Footnotes** section without splitting their host paragraph. Unbalanced `^[` or empty `^[]` degrades to literal text.
- **WikiLinks (`[[Target]]`, `[[Target|Label]]`)** ([#82]) — target slugifies to an in-document anchor and flows through the same resolution path as `[text](#slug)`: match becomes a clickable internal jump; unmatched logs a warning and falls back to styled text so a partial export isn't broken. Unclosed `[[` or escaped `\[\[…\]\]` renders literally.
- **Inline highlight (`==text==`)** ([#79]) — new `Highlight` token painting a configurable background; nestable with other inline styles (`==**bold**==` is bold *and* highlighted). New `[mark]` config block with `background_color` (default `#FFF59D`). Dispatch runs after Setext detection so `===`/`---` lines still underline the paragraph above. Unterminated `==` degrades.
- **Lexer fix** — unclosed `[…` spanning a line break no longer emits a `.notdef` box. `parse_link`'s no-closing-bracket fallback used to flatten the multi-line body into one `Text` token with an embedded `\n`, which the renderer had no glyph for. Multi-line bodies now route through the pending-token path so each break survives as a real `Token::Newline`. Covered by an invariant regression test (no `Token::Text` ever carries a literal newline).
- **Validation fix** — footnote syntax no longer trips the "unmatched square brackets" warning. The crude `[` vs `]` tally falsely flagged `[^id]` refs/defs, inline `^[…]`, and the intentional `^[` degradation; footnote brackets are now neutralized before the tally and a genuine `[unclosed link` still warns.
- **Docs** — new syntax documented in `docs/configuration.md`; doc filenames lowercased (`docs/cli.md`, `docs/configuration.md`, `docs/library.md`) and all inbound references updated.

Resolves [#79](https://github.com/theiskaa/markdown2pdf/issues/79), [#80](https://github.com/theiskaa/markdown2pdf/issues/80), and [#82](https://github.com/theiskaa/markdown2pdf/issues/82).

## [1.0.0] - 2026-05-15

First stable release — the `genpdfi` dependency is removed and the old `pub mod pdf` is replaced by a fully rewritten in-tree rendering engine built directly on `printpdf 0.9`. The library now owns the entire path from the lexer's token stream to PDF bytes, with a complete TOML configuration system, six themes, a redesigned public API, and frontmatter support.

- **In-tree rendering engine** (`src/lib/render/`) — four-stage pipeline: token stream → block IR → positioned page op-streams → `printpdf 0.9` → an `lopdf` post-process pass for features printpdf doesn't expose (link tooltips, Catalog `/Lang`).
- **Markdown rendering** — headings 1–6 with anchors and PDF bookmarks; glyph-accurate line wrapping; inline bold/italic/monospace/strikethrough/underline/superscript/subscript/small-caps/small; ordered/unordered/GFM-task lists with arbitrary nesting and loose-vs-tight spacing; GFM tables with per-column alignment and header repeat across pages; blockquotes with configurable borders and cross-page backgrounds; fenced and indented code blocks; images (local PNG/JPEG, URL fetch and SVG rasterization behind features, HTML `<img>`, alignment, max-width, captions); GFM footnotes with bidirectional anchors and multi-line bodies; definition lists; cross-references (`[text](#slug)`); inline HTML mapped to run styles (`<sup>` `<sub>` `<u>` `<s>` `<del>` `<small>` `<kbd>`); hyphenation for overflow words; NBSP (U+00A0/U+202F/U+2007) respected by the wrapper.
- **Document features** — configurable page size/orientation and manual page breaks; headers/footers with page-number substitution and per-piece gap control; auto-generated TOC with clickable entries and convergent page numbering; title page (title/subtitle/author/date); PDF metadata (title/author/subject/keywords/creator) and Catalog `/Lang` for accessibility; inline link tooltips.
- **Configuration system** (`styling/`) — serde-derived `deny_unknown_fields` TOML schema with typo suggestions; cascade resolver (default theme → `inherits` chain → `[defaults]` → per-block → `--theme`); six bundled themes (`default`, `github`, `academic`, `minimal`, `compact`, `modern`); every visual choice user-overridable. Prose configuration guide and annotated reference config ship under `docs/`.
- **Frontmatter** — YAML / TOML parsed into document metadata.
- **Public API** — `parse_into_file`, `parse_into_bytes`, and symmetric `parse_into_file_with_style` / `parse_into_bytes_with_style`; path args accept `impl AsRef<Path>`; `ConfigSource` (`Default` / `Theme(&str)` / `File` / `Embedded`); `MdpError::ParseError` carries structured `line` + `column`. Page size configurable from TOML ([#12](https://github.com/theiskaa/markdown2pdf/issues/12)); images embedded into the PDF ([#13](https://github.com/theiskaa/markdown2pdf/issues/13)).
- **Robustness** — hostile/mistaken numeric config (zero/negative/NaN font size, line height, margins, custom page size) is clamped; bounded parser recursion returns a typed error instead of overflowing; linear-time table-start scan; U+0000 normalised to U+FFFD per CommonMark; real line/column in config errors; frontmatter tolerates a leading UTF-8 BOM. List bullets and task checkboxes are font-independent vector paths; underline/strike decorations darkened for legibility.
- **Breaking** — `pub mod pdf` removed (rendering is internal behind `parse_into_*` / `render_to_*`); `MdpError::ParseError` raw byte `position` became structured `line` + `column`; the `fetch` feature no longer needs a separate TLS feature (`rustls-tls` / `native-tls` removed) and `svg` is now opt-in; crate modernised to **edition 2024**, **MSRV 1.85**.
- **Security** — removing `genpdfi` ([#31](https://github.com/theiskaa/markdown2pdf/issues/31)) drops `encoding` (RUSTSEC-2021-0153), `lzw` (RUSTSEC-2020-0144), `rusttype` (RUSTSEC-2021-0140) and `stb_truetype` (RUSTSEC-2020-0020) from the dep graph; font handling now uses `ttf-parser`. Downstream consumers can drop the corresponding `cargo-deny` exclusions.

Resolves [#12](https://github.com/theiskaa/markdown2pdf/issues/12), [#13](https://github.com/theiskaa/markdown2pdf/issues/13), and [#31](https://github.com/theiskaa/markdown2pdf/issues/31).

## [0.4.0] - 2026-05-13

Major lexer overhaul toward full CommonMark + GFM compliance.

- **HTML handling** — raw inline HTML, block-element blocks (`<div>`/`<table>`/`<p>`), standalone-tag blocks, raw-content blocks (`<script>`/`<pre>`/`<style>`/`<textarea>`), block comments, processing instructions, CDATA, and declarations.
- **AST & resolution** — stack-based emphasis resolution; wider Link/Image AST with parsed inline content and preserved titles; loose-vs-tight list detection; blockquote lazy continuation; entity decoding in link text / URL / alt and reference labels; full HTML5 named-entity table.
- **Performance & correctness** — linear-time emphasis; BOM stripping; tab-aware block-marker detection; many CommonMark spec gaps closed (60.6% → 89.0% spec pass rate).
- **Packaging** — missing `build.rs` / `entities.json` added to the package.
- **Tests** — reorganised under `tests/markdown/` (~250 new cases) with a CommonMark spec runner and a stress suite; CI runs `cargo test` on every PR.

## [0.3.0] - 2026-05-10

Renderer breadth + CommonMark/GFM gap-closing.

- **Renderer** — inline images as links, real strikethrough, HTML-tag styling, soft-break newlines, blockquotes, and task-list checkboxes.
- **Lexer coverage** — stricter heading rules, lazy continuation, escapes, blockquote blocks, CRLF, hard breaks, entities, titles, reference links, mid-paragraph `#`, intra-word `_`.

## [0.2.2] - 2026-02-27

Built-in metric fallback + error-handling polish.

- **Font metrics** — embedded minimal TrueType font for built-in metric fallback; `FontSource::bytes()` constructor with documented priority.
- **Errors** — `Pdf::new` errors propagated through `parse_into_file` / `parse_into_bytes`; graceful fallback (no panics) on font-source loading failures.

## [0.2.1] - 2026-01-27

Bold / italic / underline / strikethrough styles applied to links.

## [0.2.0] - 2026-01-27

Tables, plus a font-system simplification.

- **Tables** — GFM table parsing and rendering with per-cell/header styling.
- **Internals** — font system simplified (removed `fontdb`) with a global cached font database; logging moved to the `log` crate; context-aware parsing via `ParseContext`; rendering and font-loading performance improved.

## [0.1.9] - 2025-11-14

Custom fonts and a pre-flight validation pass.

- **Fonts** — custom font loading with configuration options, alias/variant fallback chains, `.ttc` handling, and font subsetting (smaller PDFs); optional per-call font configuration on `parse_into_*`.
- **Diagnostics** — markdown-conversion validation system; richer `MdpError` diagnostics; CLI verbosity levels and `--dry-run`.

## [0.1.8] - 2025-10-07

Improved list parsing; JSON token-dump for debugging.

## [0.1.7] - 2025-09-21

Release automation (cargo-dist) and project housekeeping.

## [0.1.6] - 2025-07-21

`ConfigSource` enum with default / file / embedded configuration sources.

## [0.1.5] - 2025-07-16

`parse_into_bytes` for in-memory PDF generation; `parse` renamed to `parse_into_file`.

## [0.1.4] - 2025-07-09

Font discovery + OpenSSL polish.

- **Fonts** — comprehensive built-in and system font loading with caching and variant analysis.
- **Build & UX** — OpenSSL made optional; clearer CLI error guidance; assorted advisory cleanups.

## [0.1.3] - 2025-03-31

`before_spacing` text style; graceful handling of invalid image syntax; broader lexer/styling/config test coverage.

## [0.1.2] - 2024-12-01

URL input support for remote markdown files; CLI restructured.

## [0.1.1] - 2024-11-29

Hierarchical list rendering with proper indentation and mixed ordered/unordered nesting; embedded asset fonts.

## [0.1.0] - 2024-11-17

Initial release: a Markdown lexer and a `genpdfi`-backed PDF converter with basic styling, configuration via `mdprc`, code blocks, emphasis, links, and nested tokens.

[1.4.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v1.4.0
[1.3.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v1.3.0
[1.2.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v1.2.0
[1.1.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v1.1.0
[1.0.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v1.0.0

[#86]: https://github.com/theiskaa/markdown2pdf/issues/86
[#84]: https://github.com/theiskaa/markdown2pdf/issues/84
[#83]: https://github.com/theiskaa/markdown2pdf/issues/83
[#78]: https://github.com/theiskaa/markdown2pdf/issues/78
[#79]: https://github.com/theiskaa/markdown2pdf/issues/79
[#80]: https://github.com/theiskaa/markdown2pdf/issues/80
[#82]: https://github.com/theiskaa/markdown2pdf/issues/82
[0.4.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.4.0
[0.3.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.3.0
[0.2.2]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.2.2
[0.2.1]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.2.1
[0.2.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.2.0
[0.1.9]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.9
[0.1.8]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.8
[0.1.7]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.7
[0.1.6]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.6
[0.1.5]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.5
[0.1.4]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.4
[0.1.3]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.3
[0.1.2]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.2
[0.1.1]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.1
[0.1.0]: https://github.com/theiskaa/markdown2pdf/releases/tag/v0.1.0
