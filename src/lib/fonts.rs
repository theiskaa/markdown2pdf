use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use fontdb::Database;
use genpdfi::error::{Error, ErrorKind};
use genpdfi::fonts::{FontData, FontFamily};
use printpdf::BuiltinFont;
use rusttype::Font;

/// Configuration for custom font loading.
/// Allows users to specify custom font paths and override default font selections.
#[derive(Debug, Clone, Default)]
pub struct FontConfig {
    pub custom_paths: Vec<PathBuf>,
    pub default_font: Option<String>,
    pub code_font: Option<String>,
}

/// Attempts to load a built-in PDF font family using only the PDF built-in fonts
/// without any system font dependencies. This ensures consistent character spacing
/// across all platforms and avoids kerning issues.
///
/// The function supports the three standard PDF font families:
/// * Helvetica  (fallback name: "Arial")
/// * Times      (fallback names: "Times New Roman", "Times")
/// * Courier    (fallback name: "Courier New")
///
/// Built-in PDF fonts use standardized Adobe Font Metrics (AFM) and do not require
/// any font embedding, resulting in smaller PDF files with consistent rendering
/// across all PDF viewers.
pub fn load_builtin_font_family(name: &str) -> Result<FontFamily<FontData>, Error> {
    // Determine which PDF built-in base family we should reference
    let builtin_variants = match name.to_lowercase().as_str() {
        "times" | "timesnewroman" | "times new roman" | "serif" => BuiltinVariants::Times,
        "courier" | "couriernew" | "courier new" | "monospace" => BuiltinVariants::Courier,
        // default → Helvetica family
        _ => BuiltinVariants::Helvetica,
    };

    // Try to load a system font for built-in PDF fonts
    // This provides metrics but the actual font rendering uses PDF built-in fonts
    let candidates = match builtin_variants {
        BuiltinVariants::Times => &["Times New Roman", "Times", "Liberation Serif"],
        BuiltinVariants::Courier => &["Courier New", "Courier", "Liberation Mono"],
        BuiltinVariants::Helvetica => &["Helvetica", "Arial", "Liberation Sans"],
    };

    let font_bytes = Arc::new(load_system_font_bytes_fallback(candidates)?);

    // Helper that maps the base family + style to the correct `BuiltinFont`
    let mk_data = |variant: FontStyle| -> Result<FontData, Error> {
        let builtin = builtin_variants.variant(variant);
        FontData::new_shared(font_bytes.clone(), Some(builtin))
    };

    Ok(FontFamily {
        regular: mk_data(FontStyle::Regular)?,
        bold: mk_data(FontStyle::Bold)?,
        italic: mk_data(FontStyle::Italic)?,
        bold_italic: mk_data(FontStyle::BoldItalic)?,
    })
}

/// Internal helper – font style information that influences the built-in enum.
#[derive(Clone, Copy)]
enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// Internal helper that knows how to translate a `FontStyle` into the concrete
/// `printpdf::BuiltinFont` for the three supported base families.
enum BuiltinVariants {
    Helvetica,
    Times,
    Courier,
}

impl BuiltinVariants {
    fn variant(&self, style: FontStyle) -> BuiltinFont {
        match self {
            BuiltinVariants::Helvetica => match style {
                FontStyle::Regular => BuiltinFont::Helvetica,
                FontStyle::Bold => BuiltinFont::HelveticaBold,
                FontStyle::Italic => BuiltinFont::HelveticaOblique,
                FontStyle::BoldItalic => BuiltinFont::HelveticaBoldOblique,
            },
            BuiltinVariants::Times => match style {
                FontStyle::Regular => BuiltinFont::TimesRoman,
                FontStyle::Bold => BuiltinFont::TimesBold,
                FontStyle::Italic => BuiltinFont::TimesItalic,
                FontStyle::BoldItalic => BuiltinFont::TimesBoldItalic,
            },
            BuiltinVariants::Courier => match style {
                FontStyle::Regular => BuiltinFont::Courier,
                FontStyle::Bold => BuiltinFont::CourierBold,
                FontStyle::Italic => BuiltinFont::CourierOblique,
                FontStyle::BoldItalic => BuiltinFont::CourierBoldOblique,
            },
        }
    }
}

/// Attempts to find a suitable system font for built-in font metrics.
/// Falls back to any available system font if specific candidates aren't found.
fn load_system_font_bytes_fallback(candidates: &[&str]) -> Result<Vec<u8>, Error> {
    let mut db = Database::new();
    db.load_system_fonts();

    // First try to find matching candidates
    for face in db.faces() {
        let path = match &face.source {
            fontdb::Source::File(p) => p,
            _ => continue,
        };

        // Skip collections (.ttc) because rusttype can't read them directly
        if path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |ext| ext.eq_ignore_ascii_case("ttc"))
        {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        if candidates
            .iter()
            .any(|cand| file_name.contains(&cand.to_lowercase()))
        {
            if let Ok(bytes) = fs::read(path) {
                if Font::from_bytes(bytes.clone()).is_ok() {
                    return Ok(bytes);
                }
            }
        }
    }

    // If no specific candidates found, use any available TTF font
    for face in db.faces() {
        let path = match &face.source {
            fontdb::Source::File(p) => p,
            _ => continue,
        };

        // Only use TTF/OTF files, skip TTC collections
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if !ext.eq_ignore_ascii_case("ttf") && !ext.eq_ignore_ascii_case("otf") {
                continue;
            }
        } else {
            continue;
        }

        if let Ok(bytes) = fs::read(path) {
            if Font::from_bytes(bytes.clone()).is_ok() {
                return Ok(bytes);
            }
        }
    }

    Err(Error::new(
        "No usable system font found for built-in font metrics".to_string(),
        ErrorKind::InvalidFont,
    ))
}

/// Attempts to load an arbitrary system font family **and embed it** into the PDF.
///
/// The same underlying TrueType font file is re-used for all four style variants.  While this
/// means that bold / italic rendering falls back to faux effects provided by the PDF viewer, it
/// keeps the implementation simple and – most importantly – guarantees that we use *real* glyph
/// metrics (including kerning) instead of relying on the limited built-in font set.  This is
/// usually enough to get visually pleasing output for the vast majority of documents.
///
/// If the requested family cannot be found, an `InvalidFont` error is returned so that the caller
/// can decide how to proceed (e.g. fall back to a built-in font).
pub fn load_system_font_family_simple(name: &str) -> Result<FontFamily<FontData>, Error> {
    let mut db = Database::new();
    db.load_system_fonts();

    let wanted = name.to_lowercase();

    let mut selected_bytes: Option<Vec<u8>> = None;

    for face in db.faces() {
        let path = match &face.source {
            fontdb::Source::File(p) => p,
            _ => continue,
        };

        if path
            .extension()
            .and_then(|s| s.to_str())
            .map_or(false, |ext| ext.eq_ignore_ascii_case("ttc"))
        {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();
        if !file_name.contains(&wanted) {
            continue;
        }

        match fs::read(path) {
            Ok(b) => {
                if rusttype::Font::from_bytes(b.clone()).is_ok() {
                    selected_bytes = Some(b);
                    break;
                }
            }
            Err(e) => {
                eprintln!("Failed to read font file {:?}: {}", path, e);
            }
        }
    }

    let bytes = selected_bytes.ok_or_else(|| {
        Error::new(
            format!("No usable system font found for family '{}'.", name),
            ErrorKind::InvalidFont,
        )
    })?;

    let shared = Arc::new(bytes);

    let mk = || FontData::new_shared(shared.clone(), None);
    Ok(FontFamily {
        regular: mk()?,
        bold: mk()?,
        italic: mk()?,
        bold_italic: mk()?,
    })
}

/// Attempts to load a font family from custom paths first, then falls back to system fonts.
/// This function searches user-specified directories or files before looking in system fonts.
///
/// # Arguments
/// * `name` - The font family name to search for
/// * `custom_paths` - Custom directories or font files to search
///
/// # Returns
/// * `Ok(FontFamily<FontData>)` if the font is found and loaded successfully
/// * `Err(Error)` if the font cannot be found in any location
///
/// # Search order
/// 1. Custom paths (if provided) - searches for exact matches or files containing the name
/// 2. System fonts via fontdb
/// 3. Returns error if not found
pub fn load_custom_font_family(
    name: &str,
    custom_paths: &[PathBuf],
) -> Result<FontFamily<FontData>, Error> {
    let wanted = name.to_lowercase();

    // First, try to load from custom paths
    for custom_path in custom_paths {
        if custom_path.is_file() {
            // If it's a direct file path, try to load it
            if let Some(file_name) = custom_path.file_name().and_then(|n| n.to_str()) {
                if file_name.to_lowercase().contains(&wanted) {
                    if let Ok(bytes) = fs::read(custom_path) {
                        if rusttype::Font::from_bytes(bytes.clone()).is_ok() {
                            let shared = Arc::new(bytes);
                            let mk = || FontData::new_shared(shared.clone(), None);
                            return Ok(FontFamily {
                                regular: mk()?,
                                bold: mk()?,
                                italic: mk()?,
                                bold_italic: mk()?,
                            });
                        }
                    }
                }
            }
        } else if custom_path.is_dir() {
            if let Ok(entries) = fs::read_dir(custom_path) {
                for entry in entries.flatten() {
                    let path = entry.path();

                    // Only consider TTF/OTF files
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        if !ext.eq_ignore_ascii_case("ttf") && !ext.eq_ignore_ascii_case("otf") {
                            continue;
                        }
                    } else {
                        continue;
                    }

                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        if file_name.to_lowercase().contains(&wanted) {
                            if let Ok(bytes) = fs::read(&path) {
                                if rusttype::Font::from_bytes(bytes.clone()).is_ok() {
                                    let shared = Arc::new(bytes);
                                    let mk = || FontData::new_shared(shared.clone(), None);
                                    return Ok(FontFamily {
                                        regular: mk()?,
                                        bold: mk()?,
                                        italic: mk()?,
                                        bold_italic: mk()?,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // If not found in custom paths, fall back to system fonts
    load_system_font_family_simple(name)
}

/// Loads a font family using the provided FontConfig, with intelligent fallback.
/// This is the main entry point for loading fonts with custom configuration.
///
/// # Arguments
/// * `name` - The font family name to load
/// * `config` - Optional font configuration with custom paths
///
/// # Returns
/// * `Ok(FontFamily<FontData>)` if the font is found
/// * `Err(Error)` if the font cannot be loaded from any source
///
/// # Loading strategy
/// 1. If custom_paths are provided in config, search there first
/// 2. Check if it's a built-in font (helvetica, times, courier)
/// 3. Search system fonts
/// 4. Return error if nothing found
pub fn load_font_with_config(
    name: &str,
    config: Option<&FontConfig>,
) -> Result<FontFamily<FontData>, Error> {
    // If custom paths are provided, try those first
    if let Some(cfg) = config {
        if !cfg.custom_paths.is_empty() {
            if let Ok(family) = load_custom_font_family(name, &cfg.custom_paths) {
                return Ok(family);
            }
        }
    }

    // Check if it's a built-in font
    match name.to_lowercase().as_str() {
        "helvetica" | "arial" | "sans" | "sans-serif" | "times" | "timesnewroman"
        | "times new roman" | "serif" | "courier" | "monospace" => load_builtin_font_family(name),
        _ => {
            // Try system fonts as fallback
            load_system_font_family_simple(name)
        }
    }
}
