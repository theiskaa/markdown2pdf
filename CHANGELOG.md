# Changelog

All notable changes to **markdown2pdf** are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Each release section below is what ships as the GitHub Release notes.

## [1.5.0]

A layout polish release. Multi-column page flow lands; a focused
pass closes the open rendering bugs around tables, definition
lists, admonitions, HTML and link rendering, and page breaks.

**Multi-column page layout.** `[page] columns = N` (1..=4) flows
body into N equal-width columns with `column_gap_mm` between them;
blocks too tall break to the next column, then to a new page.
Headers, footers, title page, and TOC stay single-column.
`columns = 1` (default) is byte-identical to pre-feature output.

**Admonition rendering gaps.** Footnotes in nested bodies resolve
against doc-wide numbering; `caution` / `important` keep their
author-typed label with canonical kind styling; auto-emitted
strings seed the external-font subset (no more `□` notdef boxes);
a page-bottom admonition keeps its label with its first body line.

**GFM tables inside list items.** A 4-space-indented table inside
a list-item, blockquote, or admonition body tokenises as a table
instead of leaking literal pipes.

**Definition lists.** Definition bodies capture 4-space-indented
continuations and re-lex them in block context, so one definition
can hold a code block, table, blockquote, nested list, or multiple
paragraphs. Multi-term groups (`Alpha\nBeta\n: shared`) and a
second `:` block after a blank line are recognised.

**HTML and link rendering.** Inline `<span>` / `<strong>` / `<em>`
/ `<code>` / `<sup>` / `<sub>` and friends render semantically,
attributes and all. `<div class="…">body</div>` unwraps;
`<br/>` / `<hr/>` are interpreted. Unresolved wikilinks render in
a dead-link colour. Long URLs wrap at `/?&#` boundaries; non-URL
tokens stop splitting at `#`. `#dup-2` links resolve.

**Widow / orphan control.** Headings drag their first follow-on
chunk across page breaks; a bullet glyph no longer renders alone
at the page bottom.

Resolves [#102](https://github.com/theiskaa/markdown2pdf/issues/102), [#106](https://github.com/theiskaa/markdown2pdf/issues/106), [#107](https://github.com/theiskaa/markdown2pdf/issues/107), [#108](https://github.com/theiskaa/markdown2pdf/issues/108), and [#109](https://github.com/theiskaa/markdown2pdf/issues/109).

## [1.4.0] - 2026-05-23

**Fallback font chain.** A new `fallback_fonts` list on `[defaults]`
in the TOML config (and `FontConfig::with_fallback_fonts(...)` for
programmatic callers) takes an ordered list of font names. At render
time each codepoint is matched against the primary first, then each
fallback in declaration order; the first font that has a real glyph
emits it. A codepoint covered by nothing in the chain degrades
visibly (a `?` with the built-in primary, a missing-glyph box with an
external Unicode primary) rather than failing the render. Names
resolve the same way as `font_family`: built-in aliases, system font
names, or paths to `.ttf` / `.otf` files; a font that can't be
located logs a warning and is skipped. Wrapping and emission stay
consistent across font boundaries, so mixed-script paragraphs don't
overlap or break early.

**Built-in measurement matches emission.** When no Unicode font is
configured, the renderer transliterates non-ASCII punctuation to
ASCII before emission (`•` → `*`, `—` → `--`, `…` → `...`, `(c)`,
`(R)`, `(TM)`, smart quotes, NBSP). Line measurement, however, priced
those source codepoints at the font's average glyph width, so the
measured advance disagreed with what was actually drawn. The layout
cursor drifted ahead of the PDF text matrix by the difference on
every transliterated character, and the error compounded along the
line — decorations and link hit-rectangles ended up beside their
text rather than under it, most visibly on a line of several links
separated by `•` or `—`. Measurement now routes every codepoint
through the same transliteration the emit path uses, so the two
agree exactly. Documents with no transliterated characters, and any
document rendered with a Unicode font, are unaffected.

**Config file discovery.** With no `-c` flag, the CLI now finds a
config automatically — `MARKDOWN2PDF_CONFIG`, then `./markdown2pdf.toml`,
then `~/.config/markdown2pdf/config.toml` — before the default theme.

**Style-config fidelity.** A class of style fields the renderer was
silently dropping is now honoured end-to-end. Body font: system-font
lookup returning a bold face (e.g. `Tahoma Bold.ttf`) as the regular
weight is fixed (exact filename wins), `[defaults].font_family`
actually loads and embeds, heading bold from the style block is
applied with its bold/italic faces embedded, and `[link].underline =
false` suppresses the link underline. Per-block: `letter_spacing_pt`,
`indent_pt`, `underline` / `strikethrough` / `text_align`, table
`cell_padding` and `alternating_row_background`. Inline: every
`code_inline` and `[mark]` field including `font_family` and
`padding`. Layout: list `indent_per_level_pt` (plus a new
`bullet_gap_pt` knob on `[list.common]`, defaulting to the previous
`5.67` pt), image caption styling, title-page cover image, admonition
body inheritance — nested lists inside a blockquote / admonition now
adopt the container's typography. Center- and right-aligned
paragraphs no longer collapse their wrapped lines onto line one.
Multi-column page layout (`page.columns`) is the only audited field
still deferred — [#102](https://github.com/theiskaa/markdown2pdf/issues/102).

Resolves [#81](https://github.com/theiskaa/markdown2pdf/issues/81), [#97](https://github.com/theiskaa/markdown2pdf/issues/97), and [#100](https://github.com/theiskaa/markdown2pdf/issues/100).

## [1.3.0] - 2026-05-20

Three additions: merged GFM table cells, inline HTML anchors and
structural wrappers, and admonition callout boxes. Each closes a gap
where converted or richer markdown previously leaked its source
markup into the rendered output. All three are purely additive — the
existing render path is unchanged for documents that don't use them.

**Merged table cells.** GFM tables now accept two span markers
inside the pipe-table grammar: a `>` cell extends the cell before it
across one more column, and a `^` cell continues the cell directly
above it down one more row. Markers are matched against the raw cell
source so backslash-escaped `\>` / `\^` is always literal content,
they chain (`^` over several rows, several `>` in a row) and combine
(a cell that spans both columns and rows), and the layout engine
keeps a spanned group whole when it would otherwise cross a page
break. Plain tables with no markers render exactly as before.

**Inline HTML anchors and structural wrappers.** Inline
`<a href="…" title="…">…</a>` is rewritten to a real link token
before lowering, so it becomes a clickable PDF annotation on the
same path as `[text](url "title")`, tooltip included. Structural
block wrappers — `<div>`, `<section>`, `<figure>`, `<figcaption>`,
joining the existing `<p>` and `<center>` — drop out so their
children render as normal paragraphs instead of as literal HTML.
Malformed input (no `href`, self-closing, unclosed, orphan `</a>`,
opener split across a paragraph break) falls through to the existing
pass-through, and tags outside the recognised subset still render
verbatim; no scripting is introduced.

**Admonition / callout boxes.** Two authoring conventions are
recognised and produce the same output: the MkDocs form
`!!! note "Optional title"` with a 4-space-indented body, and the
GitHub alert form `> [!WARNING]` inside a blockquote. Each kind
renders as a tinted box with an accent left border, a bold header
(default per kind or the author's custom title), and a vector icon —
`note` ●, `info` ⓘ, `tip` 💡, `warning` ⚠, `danger` ⊗, generic ≡ for
unknown kinds. Five first-class kinds absorb the obvious aliases
(`caution`/`error` → `danger`, `important` → `info`, `warn` /
`attention` → `warning`, `hint` → `tip`); unknown kinds fall back to
a generic grey box with the raw label uppercased as the header. A
new `[admonition]` config block holds shared shape, per-kind
`[admonition.<kind>]` blocks layer colour and label on top, and
every bundled theme ships its own palette.

Resolves [#83](https://github.com/theiskaa/markdown2pdf/issues/83), [#84](https://github.com/theiskaa/markdown2pdf/issues/84), and [#86](https://github.com/theiskaa/markdown2pdf/issues/86).

## [1.2.0] - 2026-05-18

A mathematics release. `$…$` (inline) and `$$…$$` (display) LaTeX
math are now parsed and **typeset** rather than leaking through as
literal text — markdown2pdf gains a small in-tree TeX math engine
instead of deferring to an external typesetter. Fractions, radicals,
sub/superscript stacks, big operators with limits, delimiters that
grow to their content, matrices, accents, and the
blackboard/script/fraktur alphabets all render the way TeX sets them,
laid out over STIX Two Math's OpenType MATH metrics.

The math is drawn as filled **vector outlines**, so no font is
embedded and the equation is not selectable — it behaves like a
figure in every PDF viewer, which matches how LaTeX-, MathJax-, and
KaTeX-to-PDF pipelines treat math and keeps output PDFs small.
Display blocks are configurable through a new `[math]` style block.
No breaking changes; purely additive.

Delimiter handling follows Pandoc (a `$` opener needs a non-space
after it, a closer a non-space before it and no trailing digit, `\$`
is a literal dollar, an unterminated `$`/`$$` degrades to text), and
the engine covers `\frac`/`\binom`, `\sqrt[n]{}`, coupled sub/super-
script stacks, big operators with `\limits`/`\nolimits`, growing
`\left…\right`/`\big` delimiters, `pmatrix`/`cases`/`aligned`
environments, accents (incl. stretchy `\widehat`), the `\mathbb`/
`\mathbf`/`\mathcal`/… alphabets, and operator names — unknown
commands degrade to literal text, and input depth is bounded so
adversarial markup can't blow up layout. The `[math]` block takes
`align` (`center`/`left`/`right`), `scale`, `color`, and block
margins; see `docs/configuration.md`.

Resolves [#78](https://github.com/theiskaa/markdown2pdf/issues/78).

## [1.1.0] - 2026-05-18

A note-vault syntax release. Three Markdown extensions that are
standard in Obsidian / Pandoc / MediaWiki-style tools — inline
highlight, WikiLinks, and Pandoc inline footnotes — are now parsed
and rendered, so a vault or a Pandoc-flavoured document exports to PDF
without its syntax leaking through as literal text. The release also
hardens two lexer/validation edge cases surfaced while building the
above. No breaking changes; purely additive plus fixes.

Resolves [#79](https://github.com/theiskaa/markdown2pdf/issues/79), [#80](https://github.com/theiskaa/markdown2pdf/issues/80), and [#82](https://github.com/theiskaa/markdown2pdf/issues/82).

### Added

- **Inline footnotes (`text^[note body]`)** ([#80]). Pandoc-style
  footnotes written in place, with no separate `[^id]:` definition.
  The lexer dispatches on `^[`, scans a balanced bracket pair
  (bracket nesting and `\`-escapes respected), sub-lexes the body as
  inline content, and assigns a document-unique internal label from a
  counter shared across nested sub-lexers (so a note inside a list
  item or table cell never collides with one in the body). Lowering
  treats it as a reference + definition pair, so inline and regular
  `[^id]` footnotes **share one numbering sequence** (assigned in
  first-reference document order), the marker is the same superscript
  linking to `#footnote-N`, and every body is collected into the
  single tail **Footnotes** section without splitting its host
  paragraph. An unbalanced `^[` or an empty `^[]` degrades to literal
  text — never a panic.
- **WikiLinks (`[[Target]]`, `[[Target|Label]]`)** ([#82]). The
  destination is the target slugified to an in-document anchor and
  flows through the exact same resolution path as a
  `[text](#slug)` cross-reference: a match becomes a clickable
  internal jump; an unmatched target logs a warning and falls back to
  styled text so a partial export is never broken. An unclosed `[[`
  or an escaped `\[\[…\]\]` renders literally.
- **Inline highlight / mark (`==text==`)** ([#79]). New `Highlight`
  inline token painting a configurable background behind the run,
  nestable with other inline styles (`==**bold**==` is bold *and*
  highlighted). New `[mark]` config block with `background_color`
  (default a soft yellow `#FFF59D`). Dispatch runs after Setext
  detection, so a line that is exactly `===` / `---` still underlines
  the paragraph above it; an unterminated `==` degrades to literal
  text.

### Fixed

- **Lexer: unclosed `[…` spanning a line break no longer emits a
  missing-glyph box.** `parse_link`'s no-closing-bracket fallback
  flattened a multi-line body into one `Text` token with an embedded
  literal `\n`. That raw newline reached the renderer, which has no
  glyph for U+000A and painted a `.notdef` box on the embedded-font
  path. Multi-line bodies now route through the pending-token path so
  each break survives as a real `Token::Newline` and lowering turns
  it into a space, exactly like any soft break. Pre-existing;
  reproducible with any unclosed `[` across a newline. Covered by a
  new invariant regression test (no `Token::Text` ever carries a
  literal newline).
- **Validation: footnote syntax no longer trips the "unmatched
  square brackets" warning.** The heuristic did a raw `[` vs `]`
  tally, so `[^id]` references/definitions, inline `^[…]`, and the
  intentionally-degrading stray `^[` were falsely flagged as broken
  link syntax. Footnote brackets are now neutralized before the
  tally; a genuine `[unclosed link` still warns.

### Documentation

- Documented the three new syntaxes in `docs/configuration.md`
  (WikiLinks, Highlight `[mark]`, and a new Footnotes section
  covering both `[^id]` and `^[…]`).
- Lowercased the user-facing doc filenames (`docs/cli.md`,
  `docs/configuration.md`, `docs/library.md`) for consistency and
  updated every inbound reference.

## [1.0.0] - 2026-05-15

First stable release. The dependency on `genpdfi` (a `genpdf` fork) is
**completely removed** and the old `genpdfi`-backed `pdf` module is
replaced by a fully rewritten, in-tree rendering engine built directly
on `printpdf 0.9`. The library now owns the entire path from the
lexer's token stream to PDF bytes — no third-party layout engine sits
between the library and the PDF backend. Alongside the engine rewrite
this release adds a complete TOML configuration system with six bundled
themes, a redesigned public API, and frontmatter support.

Resolves [#12](https://github.com/theiskaa/markdown2pdf/issues/12), [#13](https://github.com/theiskaa/markdown2pdf/issues/13), and [#31](https://github.com/theiskaa/markdown2pdf/issues/31).

### Breaking changes

- `pub mod pdf` is removed; rendering is internal behind
  `parse_into_*` / `render_to_*`.
- `MdpError::ParseError` field shape changed: a raw byte `position`
  became structured `line` + `column`.
- The `fetch` feature no longer needs a separate TLS feature;
  `rustls-tls` / `native-tls` were removed and `svg` is now opt-in.
- Crate modernized to **edition 2024** with an **MSRV of 1.85**.

### Added

- **In-tree rendering engine** (`src/lib/render/`) — a four-stage
  pipeline: token stream → block IR → positioned page op-streams →
  `printpdf 0.9` → an `lopdf` post-process pass for features printpdf
  does not expose (link tooltips, Catalog `/Lang`).
- **Markdown rendering:** headings (1–6) with anchors and PDF
  bookmarks; glyph-accurate line wrapping; inline bold / italic /
  monospace / strikethrough / underline / superscript / subscript /
  small-caps / small; ordered, unordered and GFM task lists with
  arbitrary nesting and loose/tight spacing; GFM tables with
  per-column alignment and header repeat across page breaks;
  blockquotes with configurable borders and cross-page backgrounds;
  fenced and indented code blocks; images (local PNG/JPEG, URL fetch
  and SVG rasterization behind features, HTML `<img>`, alignment,
  max-width, captions); GFM footnotes with bidirectional clickable
  anchors and multi-line definition bodies; definition lists;
  cross-references (`[text](#slug)`); inline HTML mapped to run styles
  (`<sup>` `<sub>` `<u>` `<s>` `<del>` `<small>` `<kbd>`), framing
  tags dropped, unknown tags passed through; hyphenation for overflow
  words; non-breaking spaces (U+00A0/U+202F/U+2007) respected by the
  wrapper.
- **Document features:** configurable page size and orientation,
  manual page breaks; headers/footers with page-number substitution
  and per-piece gap control; auto-generated table of contents with
  clickable entries and convergent page numbering; title page
  (title / subtitle / author / date); PDF metadata
  (title/author/subject/keywords/creator) and Catalog `/Lang` for
  accessibility; inline link tooltips.
- **Configuration system** (`styling/` module): a serde-derived,
  `deny_unknown_fields` TOML schema with typo suggestions on unknown
  keys; a cascade resolver (default theme → `inherits` chain →
  `[defaults]` → per-block → `--theme`); six bundled themes —
  `default`, `github`, `academic`, `minimal`, `compact`, `modern`.
  Every visual choice is user-overridable. A prose configuration
  guide and an annotated reference config ship under `docs/`.
- **YAML / TOML frontmatter** parsed into document metadata.
- **Public API:** `parse_into_file`, `parse_into_bytes`, and the
  symmetric `parse_into_file_with_style` / `parse_into_bytes_with_style`;
  path arguments now accept `impl AsRef<Path>`; `ConfigSource`
  (`Default` / `Theme(&str)` / `File` / `Embedded`) lets callers pick
  a bundled theme by name; `MdpError::ParseError` carries structured
  `line` + `column`.
- Page size is now configurable from the TOML config
  ([#12](https://github.com/theiskaa/markdown2pdf/issues/12)) and
  images are embedded into the PDF
  ([#13](https://github.com/theiskaa/markdown2pdf/issues/13)).

### Changed

- All dependencies bumped to current versions; added
  `printpdf 0.9`, `lopdf`, `hyphenation`, `image`, and `resvg`
  (optional).
- The confusing `fetch` + `rustls-tls` + `native-tls` feature triple
  is replaced by a single `fetch` feature that bundles rustls; `svg`
  is opt-in.
- Internal lexer types (`ParseContext`, `DelimRun`) are hidden from
  the public surface.

### Removed

- The `genpdfi` dependency and the old `pub mod pdf`.
- The `rustls-tls` and `native-tls` features (folded into `fetch`).

### Fixed

- Hostile or mistaken numeric config (zero/negative/NaN font size,
  line height, margins, custom page size) is clamped so the renderer
  never hangs or crashes.
- Renderer robustness pass: bounded parser recursion so deeply nested
  input returns a typed error instead of overflowing the stack;
  linear-time table-start scan; U+0000 normalized to U+FFFD per
  CommonMark; real line/column in config errors; frontmatter tolerates
  a leading UTF-8 BOM.
- List bullets and task checkboxes are drawn as font-independent
  vector paths (no more `*`/`[ ]` fallback under the built-in font);
  underline/strike decorations are darkened for legibility.

### Security

- Removing `genpdfi` eliminates the unmaintained transitive crates
  reported in [#31](https://github.com/theiskaa/markdown2pdf/issues/31):
  `encoding` (RUSTSEC-2021-0153), `lzw` (RUSTSEC-2020-0144),
  `rusttype` (RUSTSEC-2021-0140) and `stb_truetype`
  (RUSTSEC-2020-0020) are no longer anywhere in the dependency graph.
  Font handling now uses `ttf-parser`; downstream consumers can drop
  the corresponding `cargo-deny` exclusions.

## [0.4.0] - 2026-05-13

Major lexer overhaul toward full CommonMark + GFM compliance.

### Added

- Full inline and block HTML handling: raw inline HTML, block-element
  blocks (`<div>`/`<table>`/`<p>`), standalone-tag blocks,
  raw-content blocks (`<script>`/`<pre>`/`<style>`/`<textarea>`),
  block comments, processing instructions, CDATA, and declarations.
- Stack-based emphasis resolution, wider Link/Image AST with parsed
  inline content and preserved titles, loose-vs-tight list detection,
  blockquote lazy continuation, entity decoding in link text/URL/alt
  and reference labels, and a complete HTML5 named-entity table.

### Fixed

- Linear-time emphasis, BOM stripping, tab-aware block-marker
  detection, and a large number of CommonMark spec gaps closed
  (60.6% → 89.0% spec pass rate).
- Missing `build.rs` / `entities.json` added to the package.

### Changed

- Tests reorganized under `tests/markdown/` (~250 new cases) with a
  CommonMark spec runner and a stress suite; CI runs `cargo test` on
  every PR.

## [0.3.0] - 2026-05-10

### Added

- Renderer support for inline images as links, real strikethrough,
  HTML-tag styling, soft-break newlines, blockquotes, and task-list
  checkboxes.

### Fixed

- Broader CommonMark/GFM coverage: stricter heading rules, lazy
  continuation, escapes, blockquote blocks, CRLF, hard breaks,
  entities, titles, reference links, mid-paragraph `#`, and
  intra-word `_`.

## [0.2.2] - 2026-02-27

### Added

- Embedded minimal TrueType font for built-in metric fallback;
  `FontSource::bytes()` constructor with documented priority.

### Fixed

- `Pdf::new` errors are propagated through `parse_into_file` /
  `parse_into_bytes`; graceful fallback (no panics) on font-source
  loading.

## [0.2.1] - 2026-01-27

### Added

- Bold / italic / underline / strikethrough styles applied to links.

## [0.2.0] - 2026-01-27

### Added

- GFM table parsing and rendering with per-cell/header styling.

### Changed

- Font system simplified (removed `fontdb`) with a global cached font
  database; logging moved to the `log` crate; context-aware parsing
  via `ParseContext`; rendering and font-loading performance improved.

## [0.1.9] - 2025-11-14

### Added

- Custom font loading with configuration options, alias/variant
  fallback chains, `.ttc` handling, and font subsetting (smaller
  PDFs); optional per-call font configuration on `parse_into_*`;
  a markdown-conversion validation system; richer `MdpError`
  diagnostics; CLI verbosity levels and `--dry-run`.

## [0.1.8] - 2025-10-07

### Added

- Improved list parsing; JSON token-dump for debugging.

## [0.1.7] - 2025-09-21

### Changed

- Release automation (cargo-dist) and project housekeeping.

## [0.1.6] - 2025-07-21

### Added

- `ConfigSource` enum with default / file / embedded configuration
  sources.

## [0.1.5] - 2025-07-16

### Added

- `parse_into_bytes` for in-memory PDF generation; `parse` renamed to
  `parse_into_file`.

## [0.1.4] - 2025-07-09

### Added

- Comprehensive built-in and system font loading with caching and
  variant analysis.

### Changed

- OpenSSL made optional; clearer CLI error guidance; assorted
  advisory cleanups.

## [0.1.3] - 2025-03-31

### Added

- `before_spacing` text style; graceful handling of invalid image
  syntax; broader lexer/styling/config test coverage.

## [0.1.2] - 2024-12-01

### Added

- URL input support for remote markdown files; CLI restructured.

## [0.1.1] - 2024-11-29

### Added

- Hierarchical list rendering with proper indentation and mixed
  ordered/unordered nesting; embedded asset fonts.

## [0.1.0] - 2024-11-17

Initial release: a Markdown lexer and a `genpdfi`-backed PDF
converter with basic styling, configuration via `mdprc`, code blocks,
emphasis, links, and nested tokens.

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
