use clap::{Arg, ArgAction, Command};
use markdown2pdf::validation;
#[cfg(feature = "fetch")]
use reqwest::blocking::Client;
use std::fs;
use std::path::PathBuf;
use std::process;

// `DeadlineReader`, `read_capped_with_deadline`, and `MAX_FETCH_BYTES`
// are shared with the library's own document-triggered image fetch
// (`markdown2pdf::render`'s private `net_guard` module) by literally
// including the same source file, the same way `tests/render.rs`
// shares `tests/render/common.rs` via `#[path = ...]`. This binary is
// a separate crate from the library and can only see its public API
// — and none of this is API we want to publish just for internal
// plumbing — so re-compiling the shared source is the way to avoid
// duplicating it. See `src/lib/render/net_read.rs`'s doc comment for
// why only this smaller file is shared rather than all of
// `net_guard.rs`: that module also carries the SSRF host-block
// predicates, which this CLI's operator-typed `--url` fetch
// deliberately does not apply (see the comment on that call site
// below).
#[cfg(feature = "fetch")]
#[path = "../lib/render/net_read.rs"]
mod net_read;

/// Split a dimension token into its numeric part and unit suffix.
/// `"2.5cm"` -> `(2.5, "cm")`, `"25"` -> `(25.0, "")`.
fn split_num_unit(s: &str) -> Result<(f64, &str), String> {
    let s = s.trim();
    let idx = s
        .find(|c: char| c.is_ascii_alphabetic() || c == '%')
        .unwrap_or(s.len());
    let (n, u) = s.split_at(idx);
    let num: f64 = n
        .trim()
        .parse()
        .map_err(|_| format!("`{}` is not a number", s))?;
    Ok((num, u.trim()))
}

/// Parse a margin length to millimetres (the schema's native margin
/// unit). Bare numbers are mm; `cm`/`mm`/`in`/`pt` are converted.
fn parse_margin_mm(s: &str) -> Result<f64, String> {
    let (num, unit) = split_num_unit(s)?;
    Ok(match unit {
        "" | "mm" => num,
        "cm" => num * 10.0,
        "in" => num * 25.4,
        "pt" => num * 25.4 / 72.0,
        other => {
            return Err(format!(
                "unknown length unit `{}` (use mm, cm, in, or pt)",
                other
            ));
        }
    })
}

/// Parse a font size to points (the schema's native font unit). Bare
/// numbers and a `pt` suffix are accepted; other units are rejected.
fn parse_font_pt(s: &str) -> Result<f64, String> {
    let (num, unit) = split_num_unit(s)?;
    match unit {
        "" | "pt" => Ok(num),
        other => Err(format!(
            "font size unit `{}` not supported (use a bare number or `pt`)",
            other
        )),
    }
}

/// Render a `-V key=VALUE` right-hand side as a TOML value. Values
/// that already look like TOML compound/quoted literals (`[..]`,
/// `{..}`, `"..."`) pass through verbatim so users can write
/// arrays / inline tables. Otherwise: `true`/`false` -> bool,
/// integer -> int, float -> float, anything else -> a quoted,
/// escaped basic string (covers `#RRGGBB`, font names, `{page}`
/// templates, alignment words).
fn toml_value(v: &str) -> String {
    let t = v.trim();
    if (t.starts_with('[') && t.ends_with(']'))
        || (t.starts_with('{') && t.ends_with('}'))
        || (t.starts_with('"') && t.ends_with('"') && t.len() >= 2)
    {
        return t.to_string();
    }
    if t == "true" || t == "false" {
        return t.to_string();
    }
    if t.parse::<i64>().is_ok() || t.parse::<f64>().is_ok() {
        return t.to_string();
    }
    let esc = t.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", esc)
}

/// Quote + escape a string we control (typed-flag values are always
/// strings: title, author, page size name, orientation, templates).
fn toml_string(v: &str) -> String {
    format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Build a single TOML fragment from the override flags. Returns
/// `None` when no override flag was supplied. The fragment is parsed
/// and validated by the library against the real config schema.
fn build_overrides(m: &clap::ArgMatches) -> Result<Option<String>, AppError> {
    let mut lines: Vec<String> = Vec::new();

    if let Some(t) = m.get_one::<String>("title") {
        lines.push(format!("metadata.title = {}", toml_string(t)));
    }
    if let Some(a) = m.get_one::<String>("author") {
        lines.push(format!("metadata.author = {}", toml_string(a)));
    }
    if let Some(fs_) = m.get_one::<String>("font-size") {
        let pt = parse_font_pt(fs_).map_err(AppError::ConversionError)?;
        lines.push(format!("defaults.font_size_pt = {}", pt));
    }
    if let Some(mg) = m.get_one::<String>("margin") {
        let mm = parse_margin_mm(mg).map_err(AppError::ConversionError)?;
        lines.push(format!(
            "page.margins = {{ top = {mm}, right = {mm}, bottom = {mm}, left = {mm} }}"
        ));
    }
    if let Some(ps) = m.get_one::<String>("page-size") {
        lines.push(format!("page.size = {}", toml_string(ps)));
    }
    if let Some(o) = m.get_one::<String>("orientation") {
        lines.push(format!("page.orientation = {}", toml_string(o)));
    }
    if m.get_flag("page-numbers") {
        lines.push(format!(
            "footer.center = {}",
            toml_string("{page} / {total_pages}")
        ));
    }
    if let Some(vars) = m.get_many::<String>("var") {
        for kv in vars {
            let (key, value) = kv.split_once('=').ok_or_else(|| {
                AppError::ConversionError(format!(
                    "-V expects KEY=VALUE, got `{}`",
                    kv
                ))
            })?;
            let key = key.trim();
            if key.is_empty() {
                return Err(AppError::ConversionError(format!(
                    "-V key is empty in `{}`",
                    kv
                )));
            }
            lines.push(format!("{} = {}", key, toml_value(value)));
        }
    }

    if lines.is_empty() {
        Ok(None)
    } else {
        Ok(Some(lines.join("\n")))
    }
}

#[derive(Debug)]
enum AppError {
    FileReadError(std::io::Error),
    ConversionError(String),
    PathError(String),
    #[cfg(feature = "fetch")]
    NetworkError(String),
}

/// Verbosity level for output
#[derive(Debug, Clone, Copy, PartialEq)]
enum Verbosity {
    Quiet,   // No output except errors
    Normal,  // Standard output
    Verbose, // Detailed output
}

fn get_markdown_input(matches: &clap::ArgMatches) -> Result<String, AppError> {
    if let Some(file_path) = matches.get_one::<String>("path") {
        return fs::read_to_string(file_path).map_err(AppError::FileReadError);
    }

    // The `url` argument is only registered when the `fetch` feature
    // is compiled in. Querying clap for an argument id that was never
    // defined panics, so this whole branch must be cfg-gated — in a
    // non-fetch build the `url` id genuinely does not exist.
    //
    // No private-host guard here (unlike the library's remote-image
    // fetch): this URL is typed by the operator running the CLI, not
    // pulled from an untrusted document, so an SSRF guard would only
    // get in the way of a legitimate `--url http://localhost:8080/doc.md`
    // workflow.
    #[cfg(feature = "fetch")]
    if let Some(url) = matches.get_one::<String>("url") {
        // 15s, vs. the library's own 5s for a document-triggered image
        // fetch (`src/lib/render/net_guard.rs`): that fetch is
        // triggered by an untrusted markdown document and bounded
        // tightly on purpose, while this one is a URL the operator
        // typed directly on the command line, so it's worth tolerating
        // a slower, deliberately-chosen endpoint over strict CLI
        // responsiveness.
        const TIMEOUT_SECS: u64 = 15;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
            .build()
            .map_err(|e| AppError::NetworkError(e.to_string()))?;
        let resp = client
            .get(url)
            .send()
            .map_err(|e| AppError::NetworkError(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(AppError::NetworkError(format!("HTTP {}", resp.status())));
        }

        // Hard wall-clock deadline across the whole body read — see
        // `net_read::DeadlineReader`'s doc comment for why reqwest's
        // own `.timeout()` doesn't already bound this. Reads one byte
        // past the cap so an over-size body is detectable without
        // ever buffering the whole thing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
        let buf = net_read::read_capped_with_deadline(resp, deadline)
            .map_err(AppError::NetworkError)?;
        if buf.len() as u64 > net_read::MAX_FETCH_BYTES {
            return Err(AppError::NetworkError(format!(
                "response from {} exceeds the {} byte cap",
                url,
                net_read::MAX_FETCH_BYTES
            )));
        }

        // A non-UTF-8 response is not markdown; don't silently
        // lossy-convert it into something that looks plausible.
        let body = String::from_utf8(buf)
            .map_err(|e| AppError::NetworkError(format!("response is not valid UTF-8: {}", e)))?;
        return Ok(body);
    }

    if let Some(markdown_string) = matches.get_one::<String>("string") {
        Ok(markdown_string.to_string())
    } else {
        Err(AppError::ConversionError("No input provided".to_string()))
    }
}

fn get_output_path(matches: &clap::ArgMatches) -> Result<PathBuf, AppError> {
    let current_dir = std::env::current_dir().map_err(|e| AppError::PathError(e.to_string()))?;

    Ok(matches
        .get_one::<String>("output")
        .map(|p| current_dir.join(p))
        .unwrap_or_else(|| current_dir.join("output.pdf")))
}

/// Locate a config file when `-c` was not given, in precedence order:
/// the `MARKDOWN2PDF_CONFIG` env var, `./markdown2pdf.toml` in the
/// working directory, then the per-user config. Returns the first
/// that exists; `None` falls back to the built-in default theme.
fn discover_config_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MARKDOWN2PDF_CONFIG") {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let project = PathBuf::from("markdown2pdf.toml");
    if project.is_file() {
        return Some(project);
    }
    user_config_file().filter(|p| p.is_file())
}

/// `<config-dir>/markdown2pdf/config.toml`, where the config dir is
/// `XDG_CONFIG_HOME`, else `~/.config`, else `%APPDATA%`.
fn user_config_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("markdown2pdf").join("config.toml"))
}

fn run(matches: clap::ArgMatches) -> Result<(), AppError> {
    let verbosity = if matches.get_flag("quiet") {
        Verbosity::Quiet
    } else if matches.get_flag("verbose") {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    };

    let dry_run = matches.get_flag("dry-run");

    // Per-parameter CLI overrides (highest priority in the cascade).
    let overrides = build_overrides(&matches)?;

    // Pick the config file once, so `--print-effective-config` and a
    // real render agree. An explicit -c wins; otherwise discover one
    // (env var, project, then per-user) before falling back to the
    // built-in default theme.
    let config_path: Option<PathBuf> = matches
        .get_one::<String>("config-path")
        .map(PathBuf::from)
        .or_else(discover_config_file);
    let config_source = match &config_path {
        Some(p) => markdown2pdf::config::ConfigSource::File(p.to_str().ok_or_else(|| {
            AppError::PathError("config path is not valid UTF-8".to_string())
        })?),
        None => markdown2pdf::config::ConfigSource::Default,
    };

    // `--print-effective-config` resolves the style and dumps it as
    // TOML; no markdown input required. Handled before any markdown
    // I/O so users can inspect the effective config in isolation.
    if matches.get_flag("print-effective-config") {
        let theme_override = matches.get_one::<String>("theme").map(|s| s.as_str());
        let style = markdown2pdf::config::load_config_strict_with_overrides(
            config_source,
            theme_override,
            overrides.as_deref(),
        )
        .map_err(|e| AppError::ConversionError(e.to_string()))?;
        let toml = toml::to_string_pretty(&style)
            .map_err(|e| AppError::ConversionError(e.to_string()))?;
        println!("{}", toml);
        return Ok(());
    }

    let markdown = get_markdown_input(&matches)?;
    let output_path = get_output_path(&matches)?;
    let output_path_str = output_path
        .to_str()
        .ok_or_else(|| AppError::PathError("Invalid output path".to_string()))?;

    let font_config = if matches.contains_id("default-font") || matches.contains_id("code-font") {
        let default_font = matches
            .get_one::<String>("default-font")
            .map(|s| s.to_string());

        let code_font = matches
            .get_one::<String>("code-font")
            .map(|s| s.to_string());

        Some(markdown2pdf::fonts::FontConfig {
            default_font,
            code_font,
            enable_subsetting: true,
            default_font_source: None,
            code_font_source: None,
            fallback_fonts: Vec::new(),
            fallback_font_sources: Vec::new(),
        })
    } else {
        None
    };

    // Load the resolved style up front so validation can see any
    // `[defaults].fallback_fonts` configured — without that, the
    // Unicode-without-font warning fires even when fallbacks fully
    // cover the document.
    let theme_override = matches.get_one::<String>("theme").map(|s| s.as_str());
    let resolved_style = markdown2pdf::config::load_config_strict_with_overrides(
        config_source,
        theme_override,
        overrides.as_deref(),
    )
    .map_err(|e| AppError::ConversionError(e.to_string()))?;

    // With no font on the CLI, fall back to the fonts named in the
    // resolved style ([defaults].font_family / [code_block]). This
    // lets a config file select an embeddable system font without
    // the caller also passing --default-font.
    let font_config = font_config.or_else(|| {
        let default_font = resolved_style.paragraph.font_family.clone();
        let code_font = resolved_style.code_block.font_family.clone();
        if default_font.is_none() && code_font.is_none() {
            return None;
        }
        Some(markdown2pdf::fonts::FontConfig {
            default_font,
            code_font,
            enable_subsetting: true,
            default_font_source: None,
            code_font_source: None,
            fallback_fonts: Vec::new(),
            fallback_font_sources: Vec::new(),
        })
    });

    if verbosity != Verbosity::Quiet {
        let warnings = validation::validate_conversion(
            &markdown,
            font_config.as_ref(),
            &resolved_style.fallback_fonts,
            Some(output_path_str),
        );

        if !warnings.is_empty() {
            if verbosity == Verbosity::Verbose {
                eprintln!("\nPre-flight validation:");
            }
            for warning in &warnings {
                eprintln!("{}", warning);
            }
            eprintln!(); // Empty line after warnings
        } else if verbosity == Verbosity::Verbose {
            eprintln!("Pre-flight validation passed\n");
        }

        if dry_run {
            println!("Dry-run validation complete. No PDF generated.");
            if warnings.is_empty() {
                println!("No issues detected. Run without --dry-run to generate PDF.");
            } else {
                println!("{} warning(s) found. Review above and run without --dry-run to generate PDF anyway.", warnings.len());
            }
            return Ok(());
        }
    } else if dry_run {
        let warnings = validation::validate_conversion(
            &markdown,
            font_config.as_ref(),
            &resolved_style.fallback_fonts,
            Some(output_path_str),
        );
        if warnings.is_empty() {
            return Ok(());
        } else {
            return Err(AppError::ConversionError(format!(
                "{} validation warnings",
                warnings.len()
            )));
        }
    }

    if verbosity == Verbosity::Verbose {
        eprintln!("Generating PDF...");
        if let Some(path) = &config_path {
            eprintln!("   Config: {}", path.display());
        }
        if let Some(cfg) = &font_config {
            if let Some(font) = &cfg.default_font {
                eprintln!("   Font: {}", font);
            }
        }
    }

    markdown2pdf::parse_into_file_with_style(
        markdown,
        output_path_str,
        resolved_style,
        font_config.as_ref(),
    )
    .map_err(|e| AppError::ConversionError(e.to_string()))?;

    if verbosity != Verbosity::Quiet {
        println!("Successfully saved PDF to {}", output_path_str);

        if verbosity == Verbosity::Verbose {
            if let Ok(metadata) = fs::metadata(output_path_str) {
                let size_kb = metadata.len() as f64 / 1024.0;
                if size_kb < 1024.0 {
                    println!("   Size: {:.1} KB", size_kb);
                } else {
                    println!("   Size: {:.2} MB", size_kb / 1024.0);
                }
            }
        }
    }

    Ok(())
}

fn main() {
    let cmd = Command::new("markdown2pdf")
        .version(env!("CARGO_PKG_VERSION"))
        // `-V` is freed from clap's auto version flag (pandoc parity)
        // and reused for `--var` overrides; `--version` stays as a
        // long-only flag.
        .disable_version_flag(true)
        .arg(
            Arg::new("version")
                .long("version")
                .help("Print version and exit")
                .action(ArgAction::Version),
        )
        .about("Markdown to PDF transpiler")
        .after_help(
            "EXAMPLES:\n  \
            markdown2pdf -p document.md -o output.pdf\n  \
            markdown2pdf -s \"# Hello World\" --default-font Georgia\n  \
            markdown2pdf -p doc.md --theme github --page-numbers\n  \
            markdown2pdf -p doc.md --title \"Report\" --font-size 11 --margin 2.5cm\n  \
            markdown2pdf -p doc.md -V blockquote.text_color=#888888 -V headings.h1.font_size_pt=28\n\
            \nCONFIG OVERRIDES:\n  \
            Typed flags and -V KEY=VALUE override the config file and\n  \
            --theme at runtime. -V keys mirror the TOML schema (dotted),\n  \
            e.g. -V page.size=Letter -V paragraph.text_align=justify.\n  \
            Dimensions accept cm/mm/in/pt; a bare number is mm (margins)\n  \
            or pt (font size). Note: -V page.margins.top=N needs all four\n  \
            sides; use --margin or -V page.margins=N (uniform) instead.\n",
        )
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .value_name("FILE_PATH")
                .help("Path to the markdown file")
                .conflicts_with("string"),
        );

    let cmd = cmd.arg(
        Arg::new("string")
            .short('s')
            .long("string")
            .value_name("MARKDOWN_STRING")
            .help("Markdown content as a string")
            .conflicts_with("path"),
    );

    #[cfg(feature = "fetch")]
    let cmd = cmd
        .mut_arg("path", |a| a.conflicts_with("url"))
        .mut_arg("string", |a| a.conflicts_with("url"))
        .arg(
            Arg::new("url")
                .short('u')
                .long("url")
                .value_name("URL")
                .help("URL to fetch markdown content from (requires 'fetch' feature)")
                .conflicts_with_all(["string", "path"]),
        );

    let mut cmd = cmd
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_PATH")
                .help("Path to the output PDF file (defaults to ./output.pdf)"),
        )
        .arg(
            Arg::new("default-font")
                .long("default-font")
                .value_name("FONT_NAME")
                .help("Default font family (e.g., Helvetica, Georgia, or system font name)"),
        )
        .arg(
            Arg::new("code-font")
                .long("code-font")
                .value_name("FONT_NAME")
                .help("Font for code blocks (default: Courier)"),
        )
        .arg(
            Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Show detailed output including validation warnings and file size")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with("quiet"),
        )
        .arg(
            Arg::new("quiet")
                .short('q')
                .long("quiet")
                .help("Suppress all output except errors")
                .action(clap::ArgAction::SetTrue)
                .conflicts_with("verbose"),
        )
        .arg(
            Arg::new("dry-run")
                .long("dry-run")
                .help("Validate input without generating PDF")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("config-path")
                .short('c')
                .long("config-path")
                .value_name("FILE_PATH")
                .help("Path to a markdown2pdf config.toml (overrides built-in defaults)"),
        )
        .arg(
            Arg::new("theme")
                .long("theme")
                .value_name("NAME")
                .help("Theme preset: default | github | academic | minimal | compact | modern"),
        )
        .arg(
            Arg::new("print-effective-config")
                .long("print-effective-config")
                .help("Print the fully-resolved style as TOML and exit")
                .action(clap::ArgAction::SetTrue),
        )
        .next_help_heading("Config overrides (win over config file & --theme)")
        .arg(
            Arg::new("title")
                .long("title")
                .value_name("TEXT")
                .help("Document title (PDF metadata)"),
        )
        .arg(
            Arg::new("author")
                .long("author")
                .value_name("TEXT")
                .help("Document author (PDF metadata)"),
        )
        .arg(
            Arg::new("font-size")
                .long("font-size")
                .value_name("SIZE")
                .help("Base body font size, e.g. 11 or 11pt"),
        )
        .arg(
            Arg::new("margin")
                .long("margin")
                .value_name("LEN")
                .help("Uniform page margin, e.g. 25, 25mm, 2.5cm, 1in"),
        )
        .arg(
            Arg::new("page-size")
                .long("page-size")
                .value_name("NAME")
                .help("Page size: A4 | Letter | Legal | A3 | A5"),
        )
        .arg(
            Arg::new("orientation")
                .long("orientation")
                .value_name("DIR")
                .help("Page orientation: portrait | landscape"),
        )
        .arg(
            Arg::new("page-numbers")
                .long("page-numbers")
                .help("Add `page / total` to the footer center")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            Arg::new("var")
                .short('V')
                .long("var")
                .value_name("KEY=VALUE")
                .action(ArgAction::Append)
                .help(
                    "Override any config field (dotted TOML key), repeatable. \
                     e.g. -V page.size=Letter -V headings.h1.font_size_pt=28",
                ),
        );

    let matches = cmd.clone().get_matches();

    #[cfg(feature = "fetch")]
    let has_url = matches.contains_id("url");
    #[cfg(not(feature = "fetch"))]
    let has_url = false;

    let only_printing_config = matches.get_flag("print-effective-config");
    if !only_printing_config
        && !matches.contains_id("path")
        && !matches.contains_id("string")
        && !has_url
    {
        cmd.print_help().unwrap();
        println!();
        process::exit(1);
    }

    if let Err(e) = run(matches) {
        match e {
            AppError::FileReadError(e) => eprintln!("[X] Error reading file: {}", e),
            AppError::ConversionError(e) => eprintln!("[X] Conversion error: {}", e),
            AppError::PathError(e) => eprintln!("[X] Path error: {}", e),
            #[cfg(feature = "fetch")]
            AppError::NetworkError(e) => eprintln!("[X] Network error: {}", e),
        }
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margin_units_convert_to_mm() {
        assert_eq!(parse_margin_mm("25").unwrap(), 25.0);
        assert_eq!(parse_margin_mm("25mm").unwrap(), 25.0);
        assert_eq!(parse_margin_mm("2.5cm").unwrap(), 25.0);
        assert_eq!(parse_margin_mm("1in").unwrap(), 25.4);
        assert!((parse_margin_mm("72pt").unwrap() - 25.4).abs() < 1e-6);
    }

    #[test]
    fn margin_rejects_unknown_unit() {
        assert!(parse_margin_mm("2.5furlongs").is_err());
        assert!(parse_margin_mm("abc").is_err());
    }

    #[test]
    fn font_size_accepts_bare_and_pt_only() {
        assert_eq!(parse_font_pt("11").unwrap(), 11.0);
        assert_eq!(parse_font_pt("11pt").unwrap(), 11.0);
        assert!(parse_font_pt("11cm").is_err());
        assert!(parse_font_pt("x").is_err());
    }

    #[test]
    fn toml_value_typing_heuristic() {
        assert_eq!(toml_value("true"), "true");
        assert_eq!(toml_value("false"), "false");
        assert_eq!(toml_value("28"), "28");
        assert_eq!(toml_value("11.5"), "11.5");
        // hex color -> quoted string
        assert_eq!(toml_value("#888888"), "\"#888888\"");
        // alignment word -> quoted string
        assert_eq!(toml_value("justify"), "\"justify\"");
        // arrays / inline tables / pre-quoted pass through verbatim
        assert_eq!(toml_value("[20,25,20,25]"), "[20,25,20,25]");
        assert_eq!(toml_value("{ top = 1 }"), "{ top = 1 }");
        assert_eq!(toml_value("\"already\""), "\"already\"");
    }

    #[test]
    fn toml_string_escapes() {
        assert_eq!(toml_string("plain"), "\"plain\"");
        assert_eq!(toml_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_string("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn split_num_unit_basic() {
        assert_eq!(split_num_unit("2.5cm").unwrap(), (2.5, "cm"));
        assert_eq!(split_num_unit("25").unwrap(), (25.0, ""));
        assert_eq!(split_num_unit(" 11 pt ").unwrap(), (11.0, "pt"));
        assert!(split_num_unit("cm").is_err());
    }
}
