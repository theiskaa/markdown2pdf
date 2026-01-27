use clap::{Arg, Command};
use markdown2pdf::validation;
#[cfg(feature = "fetch")]
use reqwest::blocking::Client;
use std::fs;
use std::path::PathBuf;
use std::process;

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
        fs::read_to_string(file_path).map_err(|e| AppError::FileReadError(e))
    } else if let Some(_url) = matches.get_one::<String>("url") {
        #[cfg(feature = "fetch")]
        {
            Client::new()
                .get(_url)
                .send()
                .map_err(|e| AppError::NetworkError(e.to_string()))?
                .text()
                .map_err(|e| AppError::NetworkError(e.to_string()))
        }
        #[cfg(not(feature = "fetch"))]
        {
            Err(AppError::ConversionError(
                "URL fetching is not enabled. Please rebuild with --features fetch or --features native-tls".to_string()
            ))
        }
    } else if let Some(markdown_string) = matches.get_one::<String>("string") {
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

fn run(matches: clap::ArgMatches) -> Result<(), AppError> {
    // Determine verbosity level
    let verbosity = if matches.get_flag("quiet") {
        Verbosity::Quiet
    } else if matches.get_flag("verbose") {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    };

    // Check for dry-run mode
    let dry_run = matches.get_flag("dry-run");

    let markdown = get_markdown_input(&matches)?;
    let output_path = get_output_path(&matches)?;
    let output_path_str = output_path
        .to_str()
        .ok_or_else(|| AppError::PathError("Invalid output path".to_string()))?;

    // Extract font configuration from CLI arguments
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
        })
    } else {
        None
    };

    // Run validation checks
    if verbosity != Verbosity::Quiet {
        let warnings =
            validation::validate_conversion(&markdown, font_config.as_ref(), Some(output_path_str));

        if !warnings.is_empty() {
            if verbosity == Verbosity::Verbose {
                eprintln!("\n🔍 Pre-flight validation:");
            }
            for warning in &warnings {
                eprintln!("{}", warning);
            }
            eprintln!(); // Empty line after warnings
        } else if verbosity == Verbosity::Verbose {
            eprintln!("✓ Pre-flight validation passed\n");
        }

        // If dry-run, stop here
        if dry_run {
            println!("✓ Dry-run validation complete. No PDF generated.");
            if warnings.is_empty() {
                println!("✓ No issues detected. Run without --dry-run to generate PDF.");
            } else {
                println!("⚠️  {} warning(s) found. Review above and run without --dry-run to generate PDF anyway.", warnings.len());
            }
            return Ok(());
        }
    } else if dry_run {
        let warnings =
            validation::validate_conversion(&markdown, font_config.as_ref(), Some(output_path_str));
        if warnings.is_empty() {
            return Ok(());
        } else {
            return Err(AppError::ConversionError(format!(
                "{} validation warnings",
                warnings.len()
            )));
        }
    }

    // Generate PDF
    if verbosity == Verbosity::Verbose {
        eprintln!("📄 Generating PDF...");
        if let Some(cfg) = &font_config {
            if let Some(font) = &cfg.default_font {
                eprintln!("   Font: {}", font);
            }
        }
    }

    markdown2pdf::parse_into_file(
        markdown,
        output_path_str,
        markdown2pdf::config::ConfigSource::Default,
        font_config.as_ref(),
    )
    .map_err(|e| AppError::ConversionError(e.to_string()))?;

    if verbosity != Verbosity::Quiet {
        println!("✅ Successfully saved PDF to {}", output_path_str);

        // Show file size in verbose mode
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
        .about("Convert Markdown files or strings to PDF")
        .after_help(
            "EXAMPLES:\n  \
            markdown2pdf -p document.md -o output.pdf\n  \
            markdown2pdf -s \"# Hello World\" --default-font Georgia\n  \
            markdown2pdf -p doc.md --verbose --dry-run\n",
        )
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .value_name("FILE_PATH")
                .help("Path to the markdown file")
                .conflicts_with("string"),
        );

    let cmd = cmd
        .arg(
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
        );

    let matches = cmd.clone().get_matches();

    #[cfg(feature = "fetch")]
    let has_url = matches.contains_id("url");
    #[cfg(not(feature = "fetch"))]
    let has_url = false;

    if !matches.contains_id("path") && !matches.contains_id("string") && !has_url {
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
