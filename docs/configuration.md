# Configuration guide

`markdown2pdf` ships with sensible defaults, six bundled theme presets, and a TOML configuration surface that controls every visual choice the renderer makes: fonts, colors, spacing, alignment, page setup, headers / footers, table of contents, title page, and per-block typography for every markdown construct.

You can ship a useful PDF with zero config. You can also write 10 lines of TOML and produce a document that looks like GitHub, a paper, or a corporate report, without touching code.

## Quick start

Pick a theme and render:

```sh
markdown2pdf -p input.md --theme github -o out.pdf
```

Bundled themes:

| Theme       | Feel                                                     |
|-------------|----------------------------------------------------------|
| `default`   | Clean readable baseline (used when no theme is set)      |
| `github`    | GitHub README rendering                                  |
| `academic`  | Justified body, conservative serif feel                  |
| `minimal`   | Stripped-down typography, lots of breathing room         |
| `compact`   | Tight spacing for high-density reference docs            |
| `modern`    | Sharper hierarchy with a contemporary heading scale      |

Inspect the effective configuration for any combination:

```sh
markdown2pdf --theme academic --print-effective-config
```

That prints the full resolved style as TOML. Copy any section into your own config and tweak.

## Your own config file

Copy [`docs/config.toml`](config.toml) from the repo to wherever you want and pass it with `-c`:

```sh
markdown2pdf -p input.md -c my-config.toml -o out.pdf
```

Every field is optional. `theme = "github"` (or any other preset) inherits a known-good baseline; you override only what you care about.

To make a config the default without passing `-c` every time, place it where the binary discovers it automatically: `markdown2pdf.toml` in the project directory, or `markdown2pdf/config.toml` under your user config directory (`~/.config/markdown2pdf/config.toml` on macOS/Linux). The `MARKDOWN2PDF_CONFIG` environment variable also points at one. See [cli.md](cli.md) for the full lookup order.

Any field below can also be overridden per-run from the command line (winning over the config file and `--theme`). See [cli.md](cli.md#config-overrides) for `--title` / `--font-size` / `--margin` / `-V key=value` and the dotted-key syntax.

## How configuration resolves

The renderer composes the final style by cascading values in this order, from lowest priority to highest:

1. **Bundled `default` theme**: the lowest-priority baseline, always present.
2. **`inherits = "..."` chain**: each preset can extend another (the `github` preset, for example, inherits from `default`). Cycles are detected and rejected with `InheritsCycle`.
3. **`[defaults]` block** in any config: fields in `[defaults]` cascade into every block that doesn't override them. Useful when you want one body font across the whole doc.
4. **Per-block overrides**: `[paragraph]`, `[headings.h1]`, etc. take precedence over `[defaults]`.
5. **CLI flag override**: `--theme NAME` overrides any `theme = "..."` value in the config file itself.

## Pages

```toml
[page]
size = "A4"              # A4 | Letter | Legal | A3 | A5
                         # or { width_mm = 100.0, height_mm = 150.0 }
orientation = "portrait" # portrait | landscape
margins = { top = 22.6, right = 22.6, bottom = 22.6, left = 22.6 }  # mm
columns = 1              # 1 (multi-column is a follow-up)
column_gap_mm = 6.0
```

A `Mm` margin like `22.6` is millimeters; the renderer converts to PDF points internally.

## Defaults cascade

Every per-block section inherits any unset field from `[defaults]`:

```toml
[defaults]
font_family = "Helvetica"
font_size_pt = 11.0
font_weight = "normal"  # normal | bold | numeric 100..=900
font_style = "normal"   # normal | italic
text_color = "#1B1F23"
line_height = 1.5       # multiplier of font_size_pt
text_align = "left"     # left | center | right | justify
padding = 0.0
margin_before_pt = 0.0
margin_after_pt = 0.0
indent_pt = 0.0
fallback_fonts = ["Noto Sans CJK SC", "Noto Sans Arabic", "Symbola"]
```

### Body font

`font_family` in `[defaults]` selects the font that is loaded and embedded: a built-in alias (`Helvetica`, `Times`, `Courier`), a system font name, or a path to a `.ttf` / `.otf` file. A built-in alias uses a PDF base-14 font (no embedding; non-ASCII glyphs transliterate to ASCII). Any other name is resolved against the system font directories and embedded, which is required for Unicode glyphs such as `•`.

The `--default-font` CLI flag overrides this; when it is omitted the config's `font_family` is used.

### Fallback fonts

`fallback_fonts` is an ordered list of font names consulted when the primary body / code font lacks a glyph for a codepoint. Mixed-script documents (Latin + CJK, Arabic, Hebrew, math symbols, emoji) render each codepoint in the first configured font that covers it; characters unmatched by every font degrade to `?` rather than panicking.

Names resolve the same way as `font_family`: built-in aliases, system font names, or paths to `.ttf` / `.otf` files. The field is only read from `[defaults]`; per-block tables ignore it.

Programmatic callers can set the same list on `FontConfig`:

```rust
let cfg = FontConfig::new()
    .with_default_font("Helvetica")
    .with_fallback_fonts(["Noto Sans CJK SC", "Symbola"]);
```

Colors accept hex strings (`"#RRGGBB"`, `"#RGB"`), structs (`{ r = 255, g = 0, b = 0 }`), or arrays (`[255, 0, 0]`).

Padding and margin can be a scalar (applies to all sides), a pair (`[vertical, horizontal]`), a quad (`[top, right, bottom, left]`), or a struct (`{ top, right, bottom, left }`).

## Block types

Every block type accepts the same superset of fields plus a few construct-specific ones. Set only what you want to override.

### Paragraph

```toml
[paragraph]
text_align = "justify"
margin_after_pt = 4.0
small_caps = false
```

`text_align = "justify"` distributes inter-word slack on non-last lines via the PDF `Tw` (word-spacing) operator. The last line of a paragraph always stays left-aligned (typographic convention). When slack exceeds 30% of the column width, the line silently falls back to left-alignment to avoid grotesque stretches.

`small_caps = true` renders originally-lowercase letters at 78% size in uppercase (faux small caps); digits, punctuation, and originally-uppercase letters stay full-size.

### Headings 1–6

Each heading level has its own section. Drop any subsection to inherit from `[defaults]` (and the active theme).

```toml
[headings.h1]
font_size_pt = 22.0
font_weight = "bold"
text_align = "left"
margin_before_pt = 8.0
margin_after_pt = 4.0
small_caps = false

[headings.h2]
font_size_pt = 17.0
font_weight = "bold"
```

Headings automatically:
- Register as PDF bookmarks (the viewer's outline panel)
- Generate a GitHub-style slug anchor for `[text](#slug)` links

### Code blocks (fenced ` ``` `)

```toml
[code_block]
font_family = "Courier"
background_color = "#F6F8FA"
text_color = "#1F2328"
padding = { top = 8.0, right = 10.0, bottom = 8.0, left = 10.0 }
margin_before_pt = 6.0
margin_after_pt = 6.0

[code_block.border]
all = { width_pt = 0.5, color = "#E1E4E8", style = "solid" }
```

`border` accepts per-side (`top`, `right`, `bottom`, `left`) or `all` for uniform borders. Styles: `solid`, `dashed`, `dotted`.

### Inline code (`` ` ``)

```toml
[code_inline]
font_family = "Courier"
background_color = "#EFF1F3"
text_color = "#1F2328"
```

### Block quotes (`>`)

```toml
[blockquote]
font_style = "italic"
text_color = "#57606A"
indent_pt = 17.0
padding = { top = 2.0, right = 6.0, bottom = 2.0, left = 8.0 }
margin_before_pt = 4.0
margin_after_pt = 4.0

[blockquote.border]
left = { width_pt = 3.0, color = "#D0D7DE", style = "solid" }
```

### Lists

Three flavors: `unordered`, `ordered`, `task`. All inherit from `[list.common]`.

```toml
[list.common]
margin_after_pt = 0.5
indent_per_level_pt = 17.0
item_spacing_tight_pt = 0.5  # CommonMark "tight" list (no blank lines)
item_spacing_loose_pt = 2.0  # CommonMark "loose" list (any blank line)
bullet_gap_pt = 5.67         # horizontal gap between the bullet/number and the item text

[list.unordered]
bullet = "•"   # any glyph

[list.ordered]
bullet = "1."  # numeric format hint: "1." or "1)"

[list.task]
# Renderer emits [x] / [ ] for task items automatically.
```

### Tables (GFM)

```toml
[table]
row_gap_pt = 2.0
cell_padding = { top = 3.0, right = 4.0, bottom = 3.0, left = 4.0 }
margin_before_pt = 4.0
margin_after_pt = 4.0
# alternating_row_background = "#FAFBFC"   # uncomment for zebra stripes

[table.header]
font_weight = "bold"

[table.cell]
# Inherits from defaults; override per-cell font / color here.

[table.border.all]
width_pt = 0.5
color = "#D0D7DE"
style = "solid"
```

Column alignment (`:---`, `:---:`, `---:` in markdown) is honored. Header rows repeat at the top of each page the table spans.

### Images

```toml
[image]
max_width_pct = 100.0  # 1..=100; cap as a fraction of content width
align = "center"       # left | center | right
margin_before_pt = 4.0
margin_after_pt = 4.0
```

Images support:
- **Local files**: PNG and JPEG via the bundled `image` crate.
- **URL fetching**: `![alt](https://...)` works when compiled with `--features fetch`. Uses rustls (pure-Rust TLS). The fetch has a 5-second timeout and 10 MB cap; failures degrade to italic alt text.
- **SVG**: vector images (`.svg`) rasterize via `resvg` when compiled with `--features svg`. Useful for README hero images served by GitHub.
- **Captions**: `![alt](url "Caption text")` renders the title as a small italic caption beneath the image, wrap-constrained to the image's width when the image is narrower than the column.

### Links

```toml
[link]
text_color = "#0969DA"
underline = false
```

Links support tooltips via the markdown title attribute:

```markdown
See [the spec](https://example.com/spec "Hover tooltip here").
```

The tooltip lands in the PDF's `/Contents` entry on the link annotation; supported PDF viewers display it on hover.

Inline HTML anchors are recognised too, which is handy when content comes from HTML-converted sources:

```markdown
Visit <a href="https://example.com" title="Hover tooltip here">the site</a>.
```

The `href` becomes the link target and the optional `title` flows through the same tooltip path. Hrefs may use single or double quotes and the tag name / attributes are case-insensitive (`<A HREF="…">…</A>` works). An `<a>` without `href`, a self-closing `<a … />`, an unclosed opener, or a stray `</a>` degrades to literal markup rather than producing a broken annotation.

Internal cross-references resolve automatically:

```markdown
See [the conclusion](#conclusion) for context.

# Conclusion
```

`#conclusion` matches the GitHub-style slug of the heading text. If two headings have the same text, the second gets `-2`, the third `-3`, etc. Unresolved anchors log a warning and emit no annotation.

WikiLinks resolve through the same anchor machinery:

```markdown
See [[Conclusion]] or [[conclusion|the wrap-up]].

# Conclusion
```

`[[Target]]` links to the heading whose slug matches `Target`; `[[Target|Label]]` shows `Label` instead. There is no `[wikilink]` config block: a WikiLink renders with the `[link]` style above and resolves like any `#slug`, so an unmatched target logs a warning and falls back to styled text rather than breaking the export.

### Footnotes (`[^id]` and `^[…]`)

Two syntaxes are supported. The GFM reference form defines the note separately:

```markdown
Tea rewards patience[^steep].

[^steep]: Two to three minutes is a sensible start.
```

The Pandoc inline form writes the note in place, with no separate definition and no label to invent:

```markdown
Water just off the boil^[around 90–95 °C for black teas] is plenty.
```

Both share one numbering sequence, assigned in first-reference order as they appear in the document, so inline and reference footnotes interleave correctly. Every marker renders as a superscript number linking to its entry, and all notes are collected into a single **Footnotes** section appended at the end of the document. A reference definition may span multiple lines (continuation lines indented at least four spaces); a defined-but-unreferenced `[^id]:` is still listed so it never silently vanishes.

There is no `[footnote]` config block. Markers use the body / `[link]` style above, and the section heading uses Heading 2 typography. Malformed input degrades to literal text rather than breaking the export: an unbalanced `^[`, an empty `^[]`, or a `[^id]` with no matching definition all render as plain characters.

### Highlight (`==text==`)

```toml
[mark]
background_color = "#FFF59D"
```

`==text==` paints `background_color` behind the run (a soft yellow by default). It nests with other inline styles, so `==**bold**==` is both bold and highlighted:

```markdown
Some ==important== text, and a ==**bold mark**==.
```

`==` is only a highlight mid-content: a line that is exactly `===` (or `---`) still underlines the paragraph above it as a Setext heading, and an unterminated `==` renders as literal text.

### Admonitions (`!!! kind` / `> [!KIND]`)

```toml
[admonition]
padding = { top = 8.0, right = 12.0, bottom = 8.0, left = 14.0 }
margin_before_pt = 4.0
margin_after_pt = 4.0

[admonition.note]
accent_color = "#448AFF"
background_color = "#E7F2FF"
```

Two authoring syntaxes are recognised and both produce the same styled box:

```markdown
!!! warning "Watch out"
    A MkDocs-style admonition. The body is indented at least four
    spaces; blank lines inside the body are preserved.

> [!NOTE]
> A GitHub-flavoured alert. The marker may stand alone on the
> first line, or carry inline content after a space.
```

**First-class kinds** are `note`, `info`, `tip`, `warning`, and `danger`. The obvious aliases collapse to those so a document authored against either ecosystem renders the same way: `caution` and `error` map to `danger`, `important` maps to `info`, `warn` and `attention` map to `warning`, `hint` maps to `tip`.

**Unknown kinds** (`!!! bug "Repro"`, `> [!QUESTION]`) fall back to a `generic` palette and surface the raw label uppercased as the header so the author's intent is never erased:

```markdown
!!! bug "Repro steps"
    Renders in the generic grey box with "Repro steps" as the
    header.

!!! quirk
    No title — the header reads "QUIRK" verbatim.
```

The `[admonition]` block holds shared shape (padding, margins, font defaults). Per-kind sub-blocks layer colour and label overrides on top:

```toml
[admonition.danger]
accent_color = "#FF1744"
background_color = "#FFEBEE"
label = "STOP"        # overrides the default "DANGER" header
```

`accent_color` drives both the left border and the per-kind icon (`note` ●, `info` ⓘ, `tip` 💡, `warning` ⚠ +!, `danger` ⊗, `generic` ≡); icons are drawn as vector glyphs so they don't depend on any font's coverage. Each bundled theme ships its own palette: `github` matches GitHub's alert colours, `academic` / `minimal` stay restrained, `modern` leans vibrant.

A custom `"…"` title on the MkDocs form replaces the default header; inline markdown inside the title (emphasis, code) is preserved. Admonition bodies are block sequences (lists, fenced code, tables, even nested admonitions all work), and inline `<a href="…">` anchors inside the body still become clickable links.

### Math (`$…$`, `$$…$$`)

`$…$` is inline math and `$$…$$` is a centered display block:

```markdown
The identity $a^2 + b^2 = c^2$ holds, and

$$\int_0^1 x\,dx$$
```

The content between the delimiters is opaque TeX: no markdown parsing or escape decoding happens inside. A built-in TeX engine typesets it (TeXbook Appendix-G layout over STIX Two Math's OpenType MATH metrics): real fraction bars, radicals with indices, sub/ superscript stacks, big operators with limits, delimiters that grow to their content, matrices, `cases`, accents, Greek, and `\mathbb`/`\mathbf`/`\mathcal`/… alphabets. Inline math sits on the text baseline and wraps as one indivisible box; display math is its own block. A command the engine doesn't know degrades to literal text rather than failing.

The glyphs are drawn as filled **vector outlines**, not text: no font is embedded and the equation is not selectable, so it behaves like a figure in every PDF viewer (this matches how LaTeX-, MathJax-, and KaTeX-to-PDF pipelines treat math; selectable math would require tagged-PDF `/ActualText`).

Delimiter handling follows Pandoc, so prose with dollar signs is not mistaken for math:

- The opening `$` must be followed by a non-space character, and the closing `$` must be preceded by one and not directly followed by a digit: `$5 and $6` stays literal text.
- `\$` is a literal dollar (`\$5.00` renders as `$5.00`).
- Inline math is single-line; display math may span lines but a blank line ends it.
- An unterminated `$` or `$$` degrades to literal text rather than breaking the export.

Display blocks are configurable via `[math]`:

```toml
[math]
align = "center"   # center (default) | left | right
scale = 1.08        # display size as a multiple of the body size
color = "#1A1A1A"   # math ink (defaults to the paragraph text color)
margin_before_pt = 6
margin_after_pt = 6
```

`scale` applies to display blocks; inline math always tracks the surrounding text size. With no `[math]` table, display math is centered at `1.08×` the body size in the paragraph color, with the paragraph's block spacing.

### Horizontal rule (`---`)

```toml
[horizontal_rule]
color = "#D0D7DE"
thickness_pt = 0.5
style = "solid"   # solid | dashed | dotted
width_pct = 100.0
margin_before_pt = 6.0
margin_after_pt = 6.0
```

## Document features

### Metadata (PDF Info dict)

```toml
[metadata]
title = "My Document"
author = "Author Name"
subject = "Subject line"
keywords = ["one", "two"]
creator = "markdown2pdf"
language = "en-US"
```

`language` is a BCP-47 tag emitted as the PDF Catalog `/Lang` entry, used by screen readers to select a pronunciation dictionary. It is omitted entirely when unset (no faked default).

Non-ASCII values are encoded as UTF-16BE with a FEFF BOM (PDF spec compliant).

### Headers and footers

Three slots per row (left / center / right) with template variables. Available variables: `{page}`, `{total_pages}`, `{title}`, `{date}`, `{author}`.

```toml
[header]
left = "{title}"
right = "{page} / {total_pages}"
show_on_first_page = false

[footer]
center = "Page {page}"
```

Page numbers substitute correctly because the renderer collects raw pages first, then assembles each `PdfPage` once the total page count is known.

### Title page

```toml
[title_page]
title = "Document Title"
subtitle = "Optional subtitle"
author = "Author Name"
date = "2026-05-15"
# cover_image_path = "cover.png"   # deferred follow-up
```

When `title` is set, the renderer prepends a dedicated first page with the title (2.4× the base font size, bold), subtitle (1.4×), author (1.1×), and date (1.0×), all centered both horizontally and vertically. Headers / footers are suppressed on title pages.

### Auto-generated table of contents

```toml
[toc]
enabled = true
title = "Contents"
max_depth = 3
```

When `enabled = true`, every heading at or above `max_depth` becomes a TOC entry between the title page (if any) and the body. Each entry is a clickable `GoTo` link to its target heading. The renderer runs a convergence loop on page count (bounded at 3 iterations) so the displayed page numbers match the final post-TOC offsets.

## Security — confining image reads (`[security]`)

`[security]` is operator-only configuration: it governs what the *renderer* is allowed to do on the host it runs on, not how a document looks. It has no `### ` peers among the document-authorable features above: it belongs alongside `Hyphenation` / `Page breaks` / `Inline HTML` below, not with metadata, headers/footers, the title page, or the TOC.

```toml
[security]
image_root = "/srv/uploads"
allow_absolute_image_paths = true
allow_remote_images = true
```

**When you need this**: markdown can reference a local image by any path (`![](/etc/ssl/certs/logo.png)`, `![](../../.env)`), and by default the renderer reads it straight off disk and embeds the bytes in the PDF. That is fine for a person converting their own document, but if you render markdown **you did not author** (a server accepting user-submitted documents, a pipeline over untrusted input), a crafted document can pull any server-local image the process can read into the output the attacker receives. If that's your situation, set `image_root` to a directory the document is allowed to pull images from, typically the same directory the markdown itself came from, or a dedicated uploads folder.

- `image_root` (default: unset). When set, every local image path is resolved against this directory and confined to it. A relative path resolves inside it; any path (relative or absolute) that escapes it (including via a symlink planted inside the root) is refused. Unset preserves the historical behavior: relative paths resolve against the process's working directory and absolute paths are read as given.
- `allow_absolute_image_paths` (default: `true`). Set `false` to reject any absolute local image path outright, independent of `image_root`. This check runs *before* root confinement, so an absolute path is refused even if it points at a file genuinely inside `image_root`; set both knobs expecting them to compose, not `image_root` alone to be the deciding factor.
- `allow_remote_images` (default: `true`). Set `false` to reject `http`/`https` image references. Independent of whether the crate was compiled with the `fetch` feature: without it, remote images already fail.

A refused image degrades exactly like a missing or undecodable one: the renderer logs a warning and falls back to the italic `[image: ALT]` placeholder rather than failing the whole render. A path that doesn't exist (a typo, a moved file) is logged separately from an actual policy refusal, so you're not sent hunting through security config for what's really a bad path.

These three all default to the permissive, pre-existing behavior. A document can never set them itself (frontmatter is metadata-only), so they only ever come from your own config file, `-c` flag, or `ConfigSource::Embedded`.

**Known limitations**: this is a containment check, not a sandbox. Hardlinks inside `image_root` aren't detected (though creating one already requires write access inside the root, a stronger primitive than the image read it would buy); there is a TOCTOU window between the path being resolved and the file actually being read; and, as above, `allow_absolute_image_paths = false` is checked before root confinement.

## Hyphenation

The `split_long_words` pre-pass consults a Knuth-Liang English dictionary (`hyphenation` crate) to find break points in any word that exceeds the column width. When a dictionary break fits in the remaining space, the renderer emits `prefix + "-"` and continues with the suffix on the next chunk. Words the dictionary doesn't know (long URLs, identifiers, repeated-char tokens) fall back to UTF-8 char boundaries.

## Page breaks

Force a page break with a standalone HTML comment:

```markdown
First page content.

<!-- pagebreak -->

Second page content.
```

The marker is case-insensitive and whitespace-tolerant.

## Inline HTML

markdown2pdf understands a small, deliberately conservative subset of inline HTML. Anything outside the subset passes through as literal text: no scripting, no arbitrary HTML execution.

**Inline styling tags** apply to the wrapped text:

| Tag | Effect |
| --- | ------ |
| `<sup>` | superscript |
| `<sub>` | subscript |
| `<u>` | underline |
| `<s>`, `<del>`, `<strike>` | strikethrough |
| `<small>` | smaller text |
| `<kbd>` | monospace (keyboard input) |
| `<br>` / `<br/>` | soft line break |

**Anchors**: `<a href="…" title="…">…</a>` becomes a clickable PDF link annotation; see the [Links](#links) section above.

**Structural block wrappers** drop out so their children render as normal paragraphs: `<div>`, `<section>`, `<figure>`, `<figcaption>`, `<p>`, and `<center>` (with or without attributes). This is what lets documents converted from HTML keep working without showing literal `<div>` markup.

```markdown
<section>

Inner **markdown** still renders, including [links](https://example.com).

</section>
```

**Comments** (`<!-- … -->`) are invisible per CommonMark, and the special marker `<!-- pagebreak -->` forces a page break (see [Page breaks](#page-breaks)).

Everything else (`<span>`, `<aside>`, custom elements, raw `<script>` / `<style>` / `<pre>` / `<textarea>` blocks) renders verbatim as a monospace HTML block, so the source stays visible rather than being silently dropped or interpreted.

## Loading methods

Three ways to feed the renderer a config:

### 1. CLI flag (`-c`)

```sh
markdown2pdf -p input.md -c path/to/config.toml -o out.pdf
```

### 2. CLI theme override (`--theme`)

```sh
markdown2pdf -p input.md --theme github -o out.pdf
```

The `--theme NAME` flag overrides any `theme = "..."` value in the config file itself, useful for quickly switching presets without editing the config.

### 3. Library API

The Rust API takes a `ConfigSource`:

```rust
use markdown2pdf::config::ConfigSource;
use markdown2pdf::parse_into_bytes;

// Default style.
let bytes = parse_into_bytes(md, ConfigSource::Default, None)?;

// Load from a file path.
let bytes = parse_into_bytes(md, ConfigSource::File("config.toml"), None)?;

// Embed at compile time.
let cfg = include_str!("../my-config.toml");
let bytes = parse_into_bytes(md, ConfigSource::Embedded(cfg), None)?;
```

## Error messages

The schema uses `#[serde(deny_unknown_fields)]`, so typos in field names are caught at parse time. The error message includes the file path, line, column, the unknown field name, and a typo suggestion when a close match exists:

```text
error in config.toml at line 5, column 1: unknown field `text_colr`,
expected one of `font_family`, `font_size_pt`, ..., `text_color`, ...
  hint: did you mean `text_color`?
```

Unknown theme names get the same treatment:

```text
unknown theme preset `githb`
  did you mean `github`?
```

## See also

- [`docs/config.toml`](config.toml): the annotated reference config, drop in any of its sections to start tweaking.
