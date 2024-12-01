use clap::{Arg, Command};
use markdown2pdf::assets;
use std::fs;
use std::path::PathBuf;
use std::process;

#[derive(Debug)]
enum AppError {
    FileReadError(std::io::Error),
    ConversionError(String),
    PathError(String),
}

fn get_markdown_input(matches: &clap::ArgMatches) -> Result<String, AppError> {
    if let Some(file_path) = matches.get_one::<String>("path") {
        fs::read_to_string(file_path).map_err(|e| AppError::FileReadError(e))
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
    let markdown = get_markdown_input(&matches)?;
    let output_path = get_output_path(&matches)?;
    let output_path_str = output_path
        .to_str()
        .ok_or_else(|| AppError::PathError("Invalid output path".to_string()))?;

    markdown2pdf::parse(markdown, output_path_str)
        .map_err(|e| AppError::ConversionError(e.to_string()))?;

    println!("[✓] Successfully saved PDF to {}", output_path_str);
    Ok(())
}

fn main() {
    let matches = Command::new("markdown2pdf")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Convert Markdown files or strings to PDF")
        .arg(
            Arg::new("path")
                .short('p')
                .long("path")
                .value_name("FILE_PATH")
                .help("Path to the markdown file")
                .conflicts_with("string"),
        )
        .arg(
            Arg::new("string")
                .short('s')
                .long("string")
                .value_name("MARKDOWN_STRING")
                .help("Markdown content as a string")
                .conflicts_with("path"),
        )
        .arg(
            Arg::new("output")
                .short('o')
                .long("output")
                .value_name("OUTPUT_PATH")
                .help("Path to the output PDF file (defaults to ./output.pdf)"),
        )
        .get_matches();

    if !matches.contains_id("path") && !matches.contains_id("string") {
        let help_text = assets::get_text_asset("help").unwrap();
        println!("{}", help_text);
        process::exit(1);
    }

    if let Err(e) = run(matches) {
        match e {
            AppError::FileReadError(e) => eprintln!("[X] Error reading file: {}", e),
            AppError::ConversionError(e) => eprintln!("[X] Conversion error: {}", e),
            AppError::PathError(e) => eprintln!("[X] Path error: {}", e),
        }
        process::exit(1);
    }
}
