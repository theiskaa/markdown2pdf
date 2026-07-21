# Command-line usage

The `markdown2pdf` binary converts a Markdown file, a string, or a remote URL into a styled PDF. Styling is resolved from a bundled theme and an optional TOML configuration, and every value in that configuration can be overridden per invocation directly on the command line. Because overrides take precedence over both the configuration file and the selected theme, most one-off documents need no configuration file at all: a theme plus a handful of flags is enough.

This document covers every flag, the way styling is resolved, the font system, and the runtime override mechanism in full. The configuration schema itself (every section and field that a config file or an override can set) is documented separately in [configuration.md](configuration.md), with an annotated, copy-and-tweak starting point in [config.toml](config.toml).

## Input and output

The binary accepts exactly one input source. A Markdown file is supplied with `-p`/`--path`, a literal Markdown string with `-s`/`--string`, and a remote document with `-u`/`--url` (the latter requires a build that includes the `fetch` feature, described under [Fonts and build features](#fonts-and-build-features)). If more than one source is supplied the precedence is path, then url, then string. The output path is given with `-o`/`--output` and defaults to `./output.pdf` when omitted.

Converting a file is the common case:

```sh
markdown2pdf -p notes.md -o notes.pdf
```

A string is convenient for short snippets and scripting. Note that a literal `\n` inside double quotes is not a newline to the shell; use a `$'...'` quoted string when real line breaks are required:

```sh
markdown2pdf -s $'# Title\n\nSome **bold** text.' -o title.pdf
```

Fetching directly from a URL is useful for rendering a project's README without cloning it:

```sh
markdown2pdf -u https://raw.githubusercontent.com/owner/repo/main/README.md --theme github -o readme.pdf
```

## Run modes

By default the tool prints a short success line and any pre-flight validation warnings. Passing `-v`/`--verbose` expands this to include the full validation report and the resulting file size, which is helpful when diagnosing why an output is larger than expected. Passing `-q`/`--quiet` suppresses everything except errors and makes the process exit with a non-zero status on failure, which is the mode to use inside CI pipelines and shell loops. The two are mutually exclusive.

The `--dry-run` flag runs the full lexer and validation pass but writes no PDF, exiting non-zero if the document fails validation. It is the fastest way to gate a commit or a build on document validity. The `--version` flag prints the binary version and exits.

A folder can be batch-converted by combining quiet mode with a shell loop; the non-zero exit on failure makes the loop abort on the first bad document when `set -e` is active:

```sh
set -e
for f in docs/*.md; do
  markdown2pdf -p "$f" --quiet --theme compact -o "build/$(basename "${f%.md}").pdf"
done
```

## How styling is resolved

Styling is composed in layers, each able to override the one below it. The bundled `default` theme provides the baseline. A selected theme preset is layered on top of it, chosen either with `--theme NAME` or by a `theme = "NAME"` line inside a configuration file, with the command-line flag winning if both are present. The configuration file is applied next: its `[defaults]` block cascades into every block that does not set a field explicitly, and per-block sections such as `[paragraph]` or `[headings.h1]` take precedence over `[defaults]`. Command-line overrides are applied last and therefore win over everything.

The six bundled presets are `default`, `github`, `academic`, `minimal`, `compact`, and `modern`. A preset is selected with `--theme`:

```sh
markdown2pdf -p input.md --theme github -o out.pdf
```

A configuration file is supplied with `-c`/`--config-path` and is parsed against the same schema as everything else, so an unknown or misspelled key produces a clear error with a "did you mean" suggestion rather than being silently ignored:

```sh
markdown2pdf -p input.md -c brand.toml -o out.pdf
```

When `-c` is omitted the binary looks for a configuration file automatically, in this order: the path in the `MARKDOWN2PDF_CONFIG` environment variable, then `markdown2pdf.toml` in the current directory (per-project), then `markdown2pdf/config.toml` under the user config directory (`$XDG_CONFIG_HOME`, else `~/.config`, else `%APPDATA%`). The first file that exists is used; if none is found the bundled `default` theme applies. An explicit `-c` always wins over discovery. A discovered path is reported under `--verbose`.

Because the layering can be hard to reason about by inspection, `--print-effective-config` resolves the theme, the configuration file, and every override into the final style and prints it as TOML, then exits. It requires no input document and is the authoritative way to answer "what styling will actually be applied":

```sh
markdown2pdf --theme academic -V headings.h1.font_size_pt=28 --print-effective-config
```

## Fonts and build features

The font system has four modes. The built-in fonts (Helvetica, Times, and Courier) require no files and render identically everywhere, including minimal Docker images and CI runners with no system fonts installed; they are the default and the fastest path. A system font is selected by name and is searched for in the standard operating-system font directories. A font file is loaded directly when the value looks like a path to a `.ttf` or `.otf`. In all cases glyph subsetting is automatic: only the glyphs that actually appear in the document are embedded, so a Unicode document set in a large CJK typeface produces a PDF measured in tens of kilobytes rather than tens of megabytes.

The body font is selected with `--default-font` and the font used for code blocks and inline code with `--code-font`:

```sh
markdown2pdf -p doc.md --default-font Georgia --code-font "Courier New"
markdown2pdf -p doc.md --default-font /usr/share/fonts/Inter.ttf
```

If non-ASCII text renders as empty boxes, the active font lacks those glyphs; switch to a Unicode-capable font such as `--default-font "Noto Sans"` or a path to a font that covers the required script.

Network input via `-u` is gated behind the `fetch` build feature, which is not compiled into the default binary. Installing or building with that feature enables URL fetching and uses a pure-Rust TLS stack, so no system OpenSSL is required:

```sh
cargo install markdown2pdf --features fetch
```

## Overriding configuration at runtime

Every field that a configuration file can set can also be set on the command line, where it takes precedence over both the file and the theme. There are two complementary mechanisms, and they can be mixed in a single invocation.

The typed convenience flags cover the values that change most often. They are discoverable through `--help`, validated as they are parsed, and the dimension flags understand units. `--title` and `--author` set the corresponding PDF metadata. `--font-size` sets the base body size. `--margin` sets a uniform page margin on all four sides. `--page-size` accepts `A4`, `Letter`, `Legal`, `A3`, or `A5`, and `--orientation` accepts `portrait` or `landscape`. `--page-numbers` places a `page / total` counter in the footer center. A typical branded report combines several of them:

```sh
markdown2pdf -p report.md \
  --theme academic \
  --title "Quarterly Report" --author "Jane Doe" \
  --font-size 11 --margin 2.5cm --page-numbers \
  -o report.pdf
```

Dimension values are unit-aware. A bare number is interpreted in the schema's native unit (millimetres for margins, points for font size), while a suffix is converted: `25` and `25mm` are both 25 mm, `2.5cm` is 25 mm, `1in` is 25.4 mm, and `72pt` is 25.4 mm. The `--font-size` flag accepts a bare number or an explicit `pt` suffix only, since centimetre or inch type sizes are not meaningful.

For everything the typed flags do not cover, the repeatable `-V KEY=VALUE` flag reaches any field in the schema. The key is a dotted path that mirrors the configuration structure exactly (`page.size`, `headings.h1.font_size_pt`, `blockquote.text_color`, and so on), and the value is interpreted as TOML. A value of `true` or `false` becomes a boolean, a value that parses as a number becomes a number, a value that begins with `[`, `{`, or a quote is passed through verbatim so that arrays and inline tables can be written directly, and anything else becomes a string, which is what makes `#RRGGBB` colors, font names, alignment keywords, and footer templates work without extra quoting:

```sh
markdown2pdf -p doc.md \
  -V page.size=Letter \
  -V paragraph.text_align=justify \
  -V headings.h1.font_size_pt=28 \
  -V blockquote.text_color=#888888 \
  -V metadata.keywords='["rust","pdf"]' \
  -o doc.pdf
```

Three behaviours are worth knowing in advance. The map form of `page.margins` requires all four sides, so `-V page.margins.top=25` fails with a missing-field error; use `--margin` for a uniform margin, `-V page.margins=25` for a uniform scalar, or the full array `-V "page.margins=[20,25,20,25]"` ordered top, right, bottom, left. The theme preset is selected before overrides are merged, so `-V theme=github` has no effect and `--theme github` must be used to switch presets. Finally, the numeric heuristic means a value like `-V metadata.title=2024` is sent as an integer and rejected because `title` is a string field; force a string by quoting inside the value, as in `-V 'metadata.title="2024"'`.

An unknown key or an out-of-schema value never silently passes through. It surfaces as the same typed configuration error a malformed file would produce, including a suggestion for the closest valid field name, so a typo in an override is caught immediately rather than producing a wrong document.
