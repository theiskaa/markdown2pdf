//! Font loading and management.
//!
//! This module provides a simple, predictable font loading system:
//!
//! - **Built-in fonts**: Helvetica, Times, Courier (instant, no file I/O)
//! - **System fonts**: Search known directories for TTF/OTF files
//! - **File paths**: Load directly from a specified path
//!
//! # Example
//! ```rust,no_run
//! use markdown2pdf::fonts::{FontSource, load_font_family};
//!
//! // Built-in (fastest)
//! let font = load_font_family(FontSource::Builtin("Helvetica"))?;
//!
//! // System font
//! let font = load_font_family(FontSource::system("Georgia"))?;
//!
//! // Explicit path
//! let font = load_font_family(FontSource::file("/path/to/font.ttf"))?;
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use genpdfi::error::{Error, ErrorKind};
use genpdfi::fonts::{FontData, FontFamily};
use log::{debug, info, warn};
use printpdf::BuiltinFont;

// =============================================================================
// Font Source
// =============================================================================

/// Specifies where to load a font from.
#[derive(Debug, Clone)]
pub enum FontSource {
    /// Built-in PDF font (Helvetica, Times, Courier). No file I/O needed.
    Builtin(&'static str),
    /// System font by name. Searches known directories.
    System(String),
    /// Direct file path to a TTF/OTF file.
    File(PathBuf),
    /// Font bytes
    Bytes(&'static [u8]),
}

impl FontSource {
    /// Create a system font source.
    pub fn system(name: impl Into<String>) -> Self {
        FontSource::System(name.into())
    }

    /// Create a file path font source.
    pub fn file(path: impl Into<PathBuf>) -> Self {
        FontSource::File(path.into())
    }
}

// =============================================================================
// Built-in Fonts
// =============================================================================

/// Maps font name to built-in PDF font.
fn get_builtin_font(name: &str) -> Option<BuiltinFont> {
    match name.to_lowercase().as_str() {
        "helvetica" | "arial" | "sans-serif" => Some(BuiltinFont::Helvetica),
        "times" | "times new roman" | "serif" => Some(BuiltinFont::TimesRoman),
        "courier" | "courier new" | "monospace" => Some(BuiltinFont::Courier),
        _ => None,
    }
}

/// Built-in font variants for a family.
struct BuiltinVariants {
    regular: BuiltinFont,
    bold: BuiltinFont,
    italic: BuiltinFont,
    bold_italic: BuiltinFont,
}

fn get_builtin_variants(base: BuiltinFont) -> BuiltinVariants {
    match base {
        BuiltinFont::Helvetica => BuiltinVariants {
            regular: BuiltinFont::Helvetica,
            bold: BuiltinFont::HelveticaBold,
            italic: BuiltinFont::HelveticaOblique,
            bold_italic: BuiltinFont::HelveticaBoldOblique,
        },
        BuiltinFont::TimesRoman => BuiltinVariants {
            regular: BuiltinFont::TimesRoman,
            bold: BuiltinFont::TimesBold,
            italic: BuiltinFont::TimesItalic,
            bold_italic: BuiltinFont::TimesBoldItalic,
        },
        BuiltinFont::Courier => BuiltinVariants {
            regular: BuiltinFont::Courier,
            bold: BuiltinFont::CourierBold,
            italic: BuiltinFont::CourierOblique,
            bold_italic: BuiltinFont::CourierBoldOblique,
        },
        // For any other variant, use Helvetica family
        _ => BuiltinVariants {
            regular: BuiltinFont::Helvetica,
            bold: BuiltinFont::HelveticaBold,
            italic: BuiltinFont::HelveticaOblique,
            bold_italic: BuiltinFont::HelveticaBoldOblique,
        },
    }
}

// =============================================================================
// System Font Directories
// =============================================================================

/// Returns known font directories for the current platform.
fn system_font_dirs() -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        vec![
            "/System/Library/Fonts",
            "/System/Library/Fonts/Supplemental",
            "/Library/Fonts",
        ]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/share/fonts/truetype",
            "/usr/share/fonts/TTF",
            "/usr/share/fonts/opentype",
            "/usr/local/share/fonts",
        ]
    } else if cfg!(target_os = "windows") {
        vec!["C:\\Windows\\Fonts"]
    } else {
        vec![]
    }
}

/// Find a font file by name in system directories.
/// Note: Skips .ttc (TrueType Collection) files as rusttype doesn't support them.
fn find_system_font(name: &str) -> Option<PathBuf> {
    let name_lower = name.to_lowercase();
    let patterns = [
        format!("{}.ttf", name),
        format!("{}.otf", name),
        // Handle "Arial Unicode MS" -> "Arial Unicode.ttf"
        format!("{}.ttf", name.replace(" MS", "")),
    ];

    for dir in system_font_dirs() {
        let dir_path = Path::new(dir);
        if !dir_path.exists() {
            continue;
        }

        if let Ok(entries) = fs::read_dir(dir_path) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                let file_lower = file_name_str.to_lowercase();

                // Skip .ttc files - rusttype can't handle font collections
                if file_lower.ends_with(".ttc") {
                    continue;
                }

                // Check exact matches first
                for pattern in &patterns {
                    if file_lower == pattern.to_lowercase() {
                        debug!("Found font '{}' at {:?}", name, entry.path());
                        return Some(entry.path());
                    }
                }

                // Check if filename starts with the font name (case-insensitive)
                if file_lower.starts_with(&name_lower)
                    && (file_lower.ends_with(".ttf") || file_lower.ends_with(".otf"))
                {
                    debug!("Found font '{}' at {:?}", name, entry.path());
                    return Some(entry.path());
                }
            }
        }
    }

    None
}

// =============================================================================
// Font Loading
// =============================================================================

/// Load a font family from the specified source.
///
/// For built-in fonts, this returns instantly with no file I/O.
/// For system/file fonts, the font is loaded and parsed once.
pub fn load_font_family(source: FontSource) -> Result<FontFamily<FontData>, Error> {
    load_font_family_impl(source, None)
}

/// Load a font family with subsetting for the given text.
///
/// Subsetting reduces PDF size by including only the glyphs actually used.
pub fn load_font_family_with_subsetting(
    source: FontSource,
    text: &str,
) -> Result<FontFamily<FontData>, Error> {
    load_font_family_impl(source, Some(text))
}

fn load_font_family_impl(
    source: FontSource,
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    match source {
        FontSource::Builtin(name) => load_builtin_family(name),
        FontSource::System(name) => load_system_family(&name, text),
        FontSource::File(path) => load_file_family(&path, text),
        FontSource::Bytes(data) => load_bytes_family(data, text),
    }
}

/// Load a built-in PDF font family.
fn load_builtin_family(name: &str) -> Result<FontFamily<FontData>, Error> {
    let builtin = get_builtin_font(name).ok_or_else(|| {
        Error::new(
            format!(
                "'{}' is not a built-in font. Use Helvetica, Times, or Courier.",
                name
            ),
            ErrorKind::InvalidFont,
        )
    })?;

    let variants = get_builtin_variants(builtin);

    // Load metrics from a system font that matches the built-in
    let metrics_data = load_builtin_metrics()?;
    let shared = Arc::new(metrics_data);

    let regular = FontData::new_shared(shared.clone(), Some(variants.regular))?;
    let bold = FontData::new_shared(shared.clone(), Some(variants.bold))?;
    let italic = FontData::new_shared(shared.clone(), Some(variants.italic))?;
    let bold_italic = FontData::new_shared(shared, Some(variants.bold_italic))?;

    info!("Loaded built-in font family: {}", name);

    Ok(FontFamily {
        regular,
        bold,
        italic,
        bold_italic,
    })
}

/// Load metrics data for built-in fonts from a system Helvetica-compatible font.
fn load_builtin_metrics() -> Result<Vec<u8>, Error> {
    // Try to find a Helvetica-compatible font for metrics
    let candidates = [
        "Helvetica",
        "Arial",
        "Liberation Sans",
        "DejaVu Sans",
        "FreeSans",
    ];

    for name in &candidates {
        if let Some(path) = find_system_font(name) {
            if let Ok(data) = fs::read(&path) {
                debug!("Using {} for built-in font metrics", name);
                return Ok(data);
            }
        }
    }

    Err(Error::new(
        "Could not find a font for built-in metrics. Install Arial or Helvetica.",
        ErrorKind::InvalidFont,
    ))
}

/// Load a system font family by name.
fn load_system_family(name: &str, text: Option<&str>) -> Result<FontFamily<FontData>, Error> {
    let path = find_system_font(name).ok_or_else(|| {
        Error::new(
            format!(
                "Font '{}' not found in system directories: {:?}",
                name,
                system_font_dirs()
            ),
            ErrorKind::InvalidFont,
        )
    })?;

    load_file_family(&path, text)
}

/// Load a font family from a file path.
fn load_file_family(path: &Path, text: Option<&str>) -> Result<FontFamily<FontData>, Error> {
    let data = fs::read(path).map_err(|e| {
        Error::new(
            format!("Failed to read font file {:?}: {}", path, e),
            ErrorKind::InvalidFont,
        )
    })?;

    let original_size = data.len();
    let shared = Arc::new(data);

    // Apply subsetting if text is provided
    let family = if let Some(text) = text {
        if text.is_empty() {
            create_font_family_from_data(shared)?
        } else {
            match subset_font_data(&shared, text) {
                Ok((subset_data, glyph_map)) => {
                    let subset_size = subset_data.len();
                    info!(
                        "Font subset: {} -> {} ({:.1}% reduction)",
                        format_bytes(original_size),
                        format_bytes(subset_size),
                        (1.0 - subset_size as f64 / original_size as f64) * 100.0
                    );
                    create_subset_font_family(shared, Arc::new(subset_data), glyph_map)?
                }
                Err(e) => {
                    warn!("Subsetting failed: {}. Using full font.", e);
                    create_font_family_from_data(shared)?
                }
            }
        }
    } else {
        create_font_family_from_data(shared)?
    };

    info!("Loaded font from {:?}", path);
    Ok(family)
}

/// Load a font family from raw bytes (e.g. embedded with include_bytes!).
fn load_bytes_family(
    data: &'static [u8],
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    let shared = Arc::new(data.to_vec());

    let family = if let Some(text) = text {
        if text.is_empty() {
            create_font_family_from_data(shared)?
        } else {
            match subset_font_data(&shared, text) {
                Ok((subset_data, glyph_map)) => {
                    create_subset_font_family(shared, Arc::new(subset_data), glyph_map)?
                }
                Err(e) => {
                    warn!("Subsetting failed: {}. Using full font.", e);
                    create_font_family_from_data(shared)?
                }
            }
        }
    } else {
        create_font_family_from_data(shared)?
    };

    info!("Loaded font from embedded bytes");
    Ok(family)
}

/// Create a font family from shared font data (no subsetting).
fn create_font_family_from_data(data: Arc<Vec<u8>>) -> Result<FontFamily<FontData>, Error> {
    let regular = FontData::new_shared(data.clone(), None)?;

    // Reuse the same parsed font for all variants
    Ok(FontFamily {
        regular: regular.clone(),
        bold: regular.clone(),
        italic: regular.clone(),
        bold_italic: regular,
    })
}

/// Create a font family with subset data.
fn create_subset_font_family(
    metrics_data: Arc<Vec<u8>>,
    subset_data: Arc<Vec<u8>>,
    glyph_map: genpdfi::fonts::GlyphIdMap,
) -> Result<FontFamily<FontData>, Error> {
    let regular = FontData::clone_with_data(
        &FontData::new_shared(metrics_data, None)?,
        subset_data,
        Some(glyph_map),
    );

    Ok(FontFamily {
        regular: regular.clone(),
        bold: regular.clone(),
        italic: regular.clone(),
        bold_italic: regular,
    })
}

/// Subset font data to include only glyphs for the given text.
fn subset_font_data(
    data: &[u8],
    text: &str,
) -> Result<(Vec<u8>, genpdfi::fonts::GlyphIdMap), Error> {
    let result = genpdfi::subsetting::subset_font_with_mapping(data, text)?;
    Ok((result.data, result.glyph_id_map))
}

/// Format bytes as human-readable string.
fn format_bytes(bytes: usize) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1}MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1}KB", bytes as f64 / 1_000.0)
    } else {
        format!("{}B", bytes)
    }
}

// =============================================================================
// Font Configuration
// =============================================================================

/// Configuration for font loading.
#[derive(Debug, Clone, Default)]
pub struct FontConfig {
    /// Default font name for body text.
    pub default_font: Option<String>,
    /// Font name for code blocks.
    pub code_font: Option<String>,
    /// Allows specifying a concrete [`FontSource`] for body text
    pub default_font_source: Option<FontSource>,
    /// Allows specifying a concrete [`FontSource`] for code text
    pub code_font_source: Option<FontSource>,
    /// Enable font subsetting for smaller PDFs.
    pub enable_subsetting: bool,
}

impl FontConfig {
    /// Create a new FontConfig with default settings.
    pub fn new() -> Self {
        Self {
            default_font: None,
            code_font: None,
            default_font_source: None,
            code_font_source: None,
            enable_subsetting: true,
        }
    }

    /// Set the default font.
    pub fn with_default_font(mut self, font: impl Into<String>) -> Self {
        self.default_font = Some(font.into());
        self
    }

    /// Set the code font.
    pub fn with_code_font(mut self, font: impl Into<String>) -> Self {
        self.code_font = Some(font.into());
        self
    }

    /// Set the default font source
    pub fn with_default_font_source(mut self, source: FontSource) -> Self {
        self.default_font_source = Some(source);
        self
    }

    /// Set the code font source
    pub fn with_code_font_source(mut self, source: FontSource) -> Self {
        self.code_font_source = Some(source);
        self
    }

    /// Enable or disable subsetting.
    pub fn with_subsetting(mut self, enabled: bool) -> Self {
        self.enable_subsetting = enabled;
        self
    }
}

// =============================================================================
// High-level API for markdown2pdf
// =============================================================================

/// Resolve a font name to a FontSource.
///
/// - Built-in names (Helvetica, Times, Courier) -> FontSource::Builtin
/// - Paths (contain / or \) -> FontSource::File
/// - Everything else -> FontSource::System
pub fn resolve_font_source(name: &str) -> FontSource {
    // Check if it's a built-in font
    if get_builtin_font(name).is_some() {
        return FontSource::Builtin(match name.to_lowercase().as_str() {
            "helvetica" | "arial" | "sans-serif" => "Helvetica",
            "times" | "times new roman" | "serif" => "Times",
            "courier" | "courier new" | "monospace" => "Courier",
            _ => "Helvetica",
        });
    }

    // Check if it's a file path
    if name.contains('/') || name.contains('\\') || name.ends_with(".ttf") || name.ends_with(".otf")
    {
        return FontSource::File(PathBuf::from(name));
    }

    // Otherwise treat as system font name
    FontSource::System(name.to_string())
}

/// Load a font for use in PDF generation.
///
/// This is the main entry point for font loading in markdown2pdf.
pub fn load_font(
    name: &str,
    config: Option<&FontConfig>,
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    let source = resolve_font_source(name);
    let enable_subsetting = config.map(|c| c.enable_subsetting).unwrap_or(true);

    if enable_subsetting && text.is_some() {
        load_font_family_with_subsetting(source, text.unwrap())
    } else {
        load_font_family(source)
    }
}

/// Load a built-in PDF font family by name.
///
/// This is the fastest path - no file I/O needed for rendering.
/// Supports: Helvetica, Times, Courier (and their aliases).
pub fn load_builtin_font_family(name: &str) -> Result<FontFamily<FontData>, Error> {
    load_font_family(FontSource::Builtin(match name.to_lowercase().as_str() {
        "helvetica" | "arial" | "sans-serif" => "Helvetica",
        "times" | "times new roman" | "serif" => "Times",
        "courier" | "courier new" | "monospace" => "Courier",
        _ => "Helvetica",
    }))
}

/// Load font with configuration (compatibility wrapper).
pub fn load_font_with_config(
    name: &str,
    config: Option<&FontConfig>,
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    load_font(name, config, text)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_builtin() {
        assert!(matches!(
            resolve_font_source("Helvetica"),
            FontSource::Builtin(_)
        ));
        assert!(matches!(
            resolve_font_source("Times"),
            FontSource::Builtin(_)
        ));
        assert!(matches!(
            resolve_font_source("Courier"),
            FontSource::Builtin(_)
        ));
    }

    #[test]
    fn test_resolve_file() {
        assert!(matches!(
            resolve_font_source("/path/to/font.ttf"),
            FontSource::File(_)
        ));
        assert!(matches!(
            resolve_font_source("font.ttf"),
            FontSource::File(_)
        ));
    }

    #[test]
    fn test_resolve_system() {
        assert!(matches!(
            resolve_font_source("Georgia"),
            FontSource::System(_)
        ));
        assert!(matches!(
            resolve_font_source("Arial Unicode MS"),
            FontSource::System(_)
        ));
    }

    #[test]
    fn test_system_font_dirs() {
        let dirs = system_font_dirs();
        assert!(!dirs.is_empty());
    }
}
