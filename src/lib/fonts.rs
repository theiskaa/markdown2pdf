use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use fontdb::Database;
use genpdfi::error::{Error, ErrorKind};
use genpdfi::fonts::{FontData, FontFamily};
use log::{debug, info, trace, warn};
use once_cell::sync::Lazy;
use printpdf::BuiltinFont;
use rusttype::Font;

/// Global cached font database to avoid expensive repeated system font scans.
/// This is initialized once on first access and reused for all subsequent font lookups.
static FONT_DATABASE: Lazy<Database> = Lazy::new(|| {
    let mut db = Database::new();
    db.load_system_fonts();
    db
});

/// Returns common aliases for a font name.
///
/// This allows users to specify "Arial" and have the system try
/// "Helvetica", "Liberation Sans", etc.
fn get_font_aliases(name: &str) -> Vec<&'static str> {
    match name.to_lowercase().as_str() {
        "arial" => vec!["Helvetica", "Liberation Sans", "FreeSans"],
        "helvetica" => vec!["Arial", "Liberation Sans", "FreeSans"],
        "times new roman" | "times" => {
            vec!["Times", "Times New Roman", "Liberation Serif", "FreeSerif"]
        }
        "courier new" | "courier" => vec!["Courier", "Courier New", "Liberation Mono", "FreeMono"],
        "verdana" => vec!["DejaVu Sans", "Bitstream Vera Sans"],
        "georgia" => vec!["Liberation Serif", "FreeSerif"],
        "comic sans ms" | "comic sans" => vec!["Comic Neue", "Comic Relief"],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_aliases() {
        // Test Arial aliases
        let arial_aliases = get_font_aliases("Arial");
        assert!(arial_aliases.contains(&"Helvetica"));
        assert!(arial_aliases.contains(&"Liberation Sans"));

        // Test case insensitivity
        let arial_lower = get_font_aliases("arial");
        assert_eq!(arial_aliases, arial_lower);

        // Test Times New Roman aliases
        let times_aliases = get_font_aliases("Times New Roman");
        assert!(times_aliases.contains(&"Liberation Serif"));
        assert!(times_aliases.contains(&"Times"));

        // Test "times" also works
        let times_short = get_font_aliases("times");
        assert_eq!(times_aliases, times_short);

        // Test unknown font returns empty
        let unknown = get_font_aliases("UnknownFont123");
        assert!(unknown.is_empty());

        // Test Verdana
        let verdana = get_font_aliases("Verdana");
        assert!(verdana.contains(&"DejaVu Sans"));
    }
}

/// Font style variant types
#[derive(Debug, Clone, Copy)]
enum FontVariant {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontVariant {
    /// Returns common naming suffixes for this variant
    fn suffixes(&self) -> &[&str] {
        match self {
            FontVariant::Regular => &["Regular", ""],
            FontVariant::Bold => &["Bold", "Bd", "B"],
            FontVariant::Italic => &["Italic", "It", "I", "Oblique"],
            FontVariant::BoldItalic => &["BoldItalic", "Bold Italic", "BoldIt", "BdIt", "BI"],
        }
    }

    fn _name(&self) -> &str {
        match self {
            FontVariant::Regular => "Regular",
            FontVariant::Bold => "Bold",
            FontVariant::Italic => "Italic",
            FontVariant::BoldItalic => "BoldItalic",
        }
    }
}

/// Configuration for custom font loading.
/// Allows users to specify custom font paths and override default font selections.
#[derive(Debug, Clone)]
pub struct FontConfig {
    /// Custom font directories or files to search
    pub custom_paths: Vec<PathBuf>,
    /// Override for the default text font (if None, uses style config)
    pub default_font: Option<String>,
    /// Override for the code font (if None, uses courier)
    pub code_font: Option<String>,
    /// Fallback fonts to use when primary font doesn't have a character
    /// These fonts are tried in order when a character is missing
    pub fallback_fonts: Vec<String>,
    /// Enable font subsetting to reduce PDF file size (default: true)
    pub enable_subsetting: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            custom_paths: Vec::new(),
            default_font: None,
            code_font: None,
            fallback_fonts: Vec::new(),
            enable_subsetting: true, // Enabled by default for smaller PDFs
        }
    }
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
/// Uses direct file paths first to avoid expensive database initialization.
fn load_system_font_bytes_fallback(candidates: &[&str]) -> Result<Vec<u8>, Error> {
    // FAST PATH: Try direct file paths first (avoids FONT_DATABASE initialization)
    let common_font_dirs = if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts/Supplemental", "/Library/Fonts"]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/share/fonts/truetype",
            "/usr/share/fonts/TTF",
            "/usr/local/share/fonts",
        ]
    } else if cfg!(target_os = "windows") {
        vec!["C:\\Windows\\Fonts"]
    } else {
        vec![]
    };

    // Try direct file access for each candidate in common directories
    for dir in &common_font_dirs {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() {
            continue;
        }

        for candidate in candidates {
            let patterns = [
                format!("{}.ttf", candidate),
                format!("{}.otf", candidate),
                format!("{} Regular.ttf", candidate),
                format!("{}-Regular.ttf", candidate),
            ];

            for pattern in &patterns {
                let font_path = dir_path.join(pattern);
                if font_path.exists() {
                    if let Ok(bytes) = fs::read(&font_path) {
                        if Font::from_bytes(bytes.clone()).is_ok() {
                            return Ok(bytes);
                        }
                    }
                }
            }
        }
    }

    // SLOW PATH: Fall back to FONT_DATABASE if direct access fails
    let db = &*FONT_DATABASE;

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
    let mut candidates = vec![name];
    let aliases = get_font_aliases(name);
    candidates.extend(aliases);

    // FAST PATH: Try direct file paths first (avoids expensive FONT_DATABASE initialization)
    let common_font_dirs: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts/Supplemental", "/Library/Fonts"]
    } else if cfg!(target_os = "linux") {
        vec![
            "/usr/share/fonts/truetype",
            "/usr/share/fonts/TTF",
            "/usr/local/share/fonts",
        ]
    } else if cfg!(target_os = "windows") {
        vec!["C:\\Windows\\Fonts"]
    } else {
        vec![]
    };

    for dir in &common_font_dirs {
        let dir_path = std::path::Path::new(dir);
        if !dir_path.exists() {
            continue;
        }

        for candidate in &candidates {
            let wanted = candidate.to_lowercase().replace(" ms", ""); // "Arial Unicode MS" -> "arial unicode"
                                                                      // Try common naming patterns
            let patterns = [
                format!("{}.ttf", candidate),
                format!("{}.otf", candidate),
                format!("{} Regular.ttf", candidate),
                format!("{}-Regular.ttf", candidate),
                // Handle "Arial Unicode MS" -> "Arial Unicode.ttf"
                // FIXME: we gonna need better conditions here.
                format!("{}.ttf", candidate.replace(" MS", "").replace(" ms", "")),
            ];

            for pattern in &patterns {
                let font_path = dir_path.join(pattern);
                if font_path.exists() {
                    if let Ok(bytes) = fs::read(&font_path) {
                        let file_stem = font_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_lowercase();
                        if !file_stem.starts_with(&wanted) && !wanted.starts_with(&file_stem) {
                            continue;
                        }

                        if let Ok(regular) = FontData::new_shared(Arc::new(bytes), None) {
                            return Ok(FontFamily {
                                regular: regular.clone(),
                                bold: regular.clone(),
                                italic: regular.clone(),
                                bold_italic: regular,
                            });
                        }
                    }
                }
            }
        }
    }

    // SLOW PATH: Fall back to FONT_DATABASE if direct access fails
    let db = &*FONT_DATABASE;

    for candidate_name in candidates {
        let wanted = candidate_name.to_lowercase();

        // Use scoring to prefer exact matches over partial matches
        // Score: 3 = exact family name, 2 = exact filename, 1 = partial match
        let mut best_score = 0u8;
        let mut best_path: Option<std::path::PathBuf> = None;

        for face in db.faces() {
            let path = match &face.source {
                fontdb::Source::File(p) => p,
                _ => continue,
            };

            // Skip .ttc files early
            let is_ttc = path
                .extension()
                .and_then(|s| s.to_str())
                .map_or(false, |ext| ext.eq_ignore_ascii_case("ttc"));
            if is_ttc {
                continue;
            }

            let face_family = face.families.first().map(|(name, _)| name.to_lowercase());

            // Calculate match score
            let mut score = 0u8;

            // Exact family name match (highest priority)
            if face_family.as_ref().map_or(false, |f| f == &wanted) {
                score = 3;
            }
            // Exact filename match (e.g., "Georgia.ttf" for "georgia")
            else {
                let file_stem = path
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_lowercase();
                if file_stem == wanted {
                    score = 2;
                }
                // Partial family name match (lower priority)
                else if face_family.as_ref().map_or(false, |f| f.contains(&wanted)) {
                    score = 1;
                }
                // Partial filename match (lowest priority, skip for now to avoid SFGeorgian issue)
            }

            if score > best_score {
                best_score = score;
                best_path = Some(path.clone());

                // If we have an exact family match, stop searching
                if score == 3 {
                    break;
                }
            }
        }

        if let Some(path) = best_path {
            match fs::read(&path) {
                Ok(b) => {
                    if candidate_name != name {
                        debug!("Using '{}' as alias for '{}'", candidate_name, name);
                    }
                    debug!("Loaded font from {:?} (match score: {})", path, best_score);

                    let shared = Arc::new(b);
                    // Parse font ONCE, then clone for variants (avoids 4x parsing overhead)
                    // Skip separate validation - FontData::new_shared will fail if font is invalid
                    match FontData::new_shared(shared.clone(), None) {
                        Ok(regular) => {
                            return Ok(FontFamily {
                                regular: regular.clone(),
                                bold: regular.clone(),
                                italic: regular.clone(),
                                bold_italic: regular,
                            });
                        }
                        Err(e) => {
                            debug!("Font {:?} invalid: {}, trying next", path, e);
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read font file {:?}: {}", path, e);
                }
            }
        }
    }

    Err(Error::new(
        format!("No usable system font found for family '{}'.", name),
        ErrorKind::InvalidFont,
    ))
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
    if let Ok(family) = load_font_family_with_variants(name, custom_paths) {
        info!("Loaded font '{}' with proper variants", name);
        return Ok(family);
    }

    debug!("Searching for single font file for '{}'", name);
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

/// Searches for a specific font variant file in custom paths.
///
/// Tries multiple naming patterns for font variants:
/// - NotoSans-Bold.ttf
/// - NotoSansBold.ttf
/// - NotoSans_Bold.ttf
/// - notosans-bold.ttf
///
/// Also tries font name aliases (e.g., Arial -> Helvetica)
fn find_font_variant_in_paths(
    base_name: &str,
    variant: FontVariant,
    custom_paths: &[PathBuf],
) -> Option<Vec<u8>> {
    let mut candidates = vec![base_name];
    let aliases = get_font_aliases(base_name);
    candidates.extend(aliases);

    for candidate in candidates {
        let base_lower = candidate.to_lowercase().replace(" ", "");

        for custom_path in custom_paths {
            if !custom_path.is_dir() {
                continue;
            }

            let Ok(entries) = fs::read_dir(custom_path) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
                    continue;
                };

                if !ext.eq_ignore_ascii_case("ttf") && !ext.eq_ignore_ascii_case("otf") {
                    continue;
                }

                let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                let file_lower = file_name.to_lowercase();
                for suffix in variant.suffixes() {
                    if suffix.is_empty() && !matches!(variant, FontVariant::Regular) {
                        continue;
                    }

                    let patterns = if suffix.is_empty() {
                        vec![format!("{}.ttf", base_lower), format!("{}.otf", base_lower)]
                    } else {
                        vec![
                            format!("{}-{}.ttf", base_lower, suffix.to_lowercase()),
                            format!("{}{}.ttf", base_lower, suffix.to_lowercase()),
                            format!("{}_{}.ttf", base_lower, suffix.to_lowercase()),
                            format!("{} {}.ttf", base_lower, suffix.to_lowercase()),
                            format!("{}-{}.otf", base_lower, suffix.to_lowercase()),
                            format!("{}{}.otf", base_lower, suffix.to_lowercase()),
                        ]
                    };

                    for pattern in &patterns {
                        if file_lower.contains(pattern) || file_lower == *pattern {
                            if let Ok(bytes) = fs::read(&path) {
                                if Font::from_bytes(bytes.clone()).is_ok() {
                                    if candidate != base_name {
                                        debug!(
                                            "Found '{}' variant as alias for '{}'",
                                            candidate, base_name
                                        );
                                    }
                                    return Some(bytes);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Loads a font family with proper variant files (Bold, Italic, BoldItalic).
///
/// This function searches for actual variant font files instead of reusing
/// the regular font for all styles. Falls back to regular font if variants aren't found.
///
/// # Arguments
/// * `name` - The font family base name
/// * `custom_paths` - Directories to search for font files
///
/// # Returns
/// A FontFamily with actual variant files loaded
pub fn load_font_family_with_variants(
    name: &str,
    custom_paths: &[PathBuf],
) -> Result<FontFamily<FontData>, Error> {
    let regular_bytes = find_font_variant_in_paths(name, FontVariant::Regular, custom_paths)
        .ok_or_else(|| {
            Error::new(
                format!("Could not find regular variant for font '{}'", name),
                ErrorKind::InvalidFont,
            )
        })?;

    let bold_bytes = find_font_variant_in_paths(name, FontVariant::Bold, custom_paths);
    let italic_bytes = find_font_variant_in_paths(name, FontVariant::Italic, custom_paths);
    let bold_italic_bytes = find_font_variant_in_paths(name, FontVariant::BoldItalic, custom_paths);

    let regular_shared = Arc::new(regular_bytes);
    let regular = FontData::new_shared(regular_shared.clone(), None)?;

    let bold = if let Some(bytes) = bold_bytes {
        FontData::new_shared(Arc::new(bytes), None).unwrap_or_else(|_| {
            warn!("Bold variant invalid, using regular");
            regular.clone()
        })
    } else {
        debug!(
            "No Bold variant found for '{}', using regular (faux bold)",
            name
        );
        regular.clone()
    };

    let italic = if let Some(bytes) = italic_bytes {
        FontData::new_shared(Arc::new(bytes), None).unwrap_or_else(|_| {
            warn!("Italic variant invalid, using regular");
            regular.clone()
        })
    } else {
        debug!(
            "No Italic variant found for '{}', using regular (faux italic)",
            name
        );
        regular.clone()
    };

    let bold_italic = if let Some(bytes) = bold_italic_bytes {
        FontData::new_shared(Arc::new(bytes), None).unwrap_or_else(|_| {
            warn!("BoldItalic variant invalid, using bold");
            bold.clone()
        })
    } else {
        debug!(
            "No BoldItalic variant found for '{}', using bold (faux italic)",
            name
        );
        bold.clone()
    };

    Ok(FontFamily {
        regular,
        bold,
        italic,
        bold_italic,
    })
}

/// Loads a font family using the provided FontConfig, with intelligent fallback.
/// This is the main entry point for loading fonts with custom configuration.
///
/// # Arguments
/// * `name` - The font family name to load
/// * `config` - Optional font configuration with custom paths
/// * `text` - Optional text content for font subsetting
///
/// # Returns
/// * `Ok(FontFamily<FontData>)` if the font is found
/// * `Err(Error)` if the font cannot be loaded from any source
///
/// # Loading strategy
/// 1. If custom_paths are provided in config, search there first
/// 2. Check if it's a built-in font (helvetica, times, courier)
/// 3. Search system fonts
/// 4. Apply font subsetting if enabled and text is provided
/// 5. Return error if nothing found
pub fn load_font_with_config(
    name: &str,
    config: Option<&FontConfig>,
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    // Check if subsetting is enabled (defaults to true for smaller PDFs)
    let enable_subsetting = config.map(|c| c.enable_subsetting).unwrap_or(true);

    // Check if fallback fonts are specified - if so, return a chain-based result
    // Note: We can't apply subsetting to fallback chains yet, so this path doesn't support it
    if let Some(cfg) = config {
        if !cfg.fallback_fonts.is_empty() {
            info!(
                "Loading font '{}' with {} fallback(s)...",
                name,
                cfg.fallback_fonts.len()
            );
            // For now, use the legacy fallback selection approach
            // TODO: Integrate fallback chains into the rendering pipeline
            let family =
                load_font_with_fallbacks(name, &cfg.fallback_fonts, &cfg.custom_paths, text)?;
            return apply_subsetting_if_enabled(family, enable_subsetting, text);
        }
    }

    // Try custom paths first if provided (no fallbacks)
    if let Some(cfg) = config {
        if !cfg.custom_paths.is_empty() {
            if let Ok(family) = load_custom_font_family(name, &cfg.custom_paths) {
                return apply_subsetting_if_enabled(family, enable_subsetting, text);
            }
        }
    }

    // Check if it's a built-in font (no subsetting for built-in fonts)
    let family = match name.to_lowercase().as_str() {
        "helvetica" | "arial" | "sans" | "sans-serif" | "times" | "timesnewroman"
        | "times new roman" | "serif" | "courier" | "monospace" => {
            return load_builtin_font_family(name); // Built-in fonts don't use subsetting
        }
        _ => {
            // Try system fonts as fallback
            load_system_font_family_simple(name)?
        }
    };

    apply_subsetting_if_enabled(family, enable_subsetting, text)
}

/// Applies font subsetting if enabled and text is provided.
///
/// This function uses a separation of concerns approach:
/// - Full font data is kept for metrics (glyph widths, kerning) via rusttype
/// - Subset font data is used for PDF embedding (smaller file size)
/// - A glyph ID mapping ensures correct rendering in the PDF
fn apply_subsetting_if_enabled(
    family: FontFamily<FontData>,
    enable_subsetting: bool,
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    if !enable_subsetting || text.is_none() {
        return Ok(family);
    }

    let text = text.unwrap();
    if text.is_empty() {
        return Ok(family);
    }

    // Get the original (full) font data for metrics
    let original_data = match family.regular.get_data() {
        Ok(data) => data,
        Err(e) => {
            warn!(
                "Could not get font data for subsetting: {}, using full font",
                e
            );
            return Ok(family);
        }
    };

    // Subset the font and get the glyph ID mapping
    let subset_result = match genpdfi::subsetting::subset_font_with_mapping(original_data, text) {
        Ok(result) => result,
        Err(e) => {
            warn!("Font subsetting failed: {}, using full font", e);
            return Ok(family);
        }
    };

    let subset_arc = Arc::new(subset_result.data);

    // Log size reduction for debugging
    let original_size = original_data.len();
    let subset_size = subset_arc.len();
    if original_size > 0 {
        let reduction = ((original_size - subset_size) as f64 / original_size as f64) * 100.0;
        info!(
            "Font subset: {} -> {} ({:.1}% reduction)",
            format_size(original_size),
            format_size(subset_size),
            reduction
        );
    }

    // Clone with subset data (avoids re-parsing font)
    let regular = FontData::clone_with_data(
        &family.regular,
        subset_arc.clone(),
        Some(subset_result.glyph_id_map.clone()),
    );

    // Reuse the same subset for all variants (they share the same base font)
    Ok(FontFamily {
        regular: regular.clone(),
        bold: regular.clone(),
        italic: regular.clone(),
        bold_italic: regular,
    })
}

/// Applies font subsetting to a fallback chain to reduce PDF file size.
///
/// This function subsets each font in the chain (primary and all fallbacks) based on
/// which characters from the document that font actually needs to render.
///
/// # Arguments
/// * `chain_family` - The fallback chain family to subset
/// * `text` - The document text to analyze for character usage
///
/// # Returns
/// A new FontFamily<FontFallbackChain> with subset fonts
pub fn apply_subsetting_to_chain(
    chain_family: FontFamily<genpdfi::fonts::FontFallbackChain>,
    text: &str,
) -> Result<FontFamily<genpdfi::fonts::FontFallbackChain>, Error> {
    if text.is_empty() {
        return Ok(chain_family);
    }

    let subset_regular = subset_chain(&chain_family.regular, text)?;
    let subset_bold = subset_chain(&chain_family.bold, text)?;
    let subset_italic = subset_chain(&chain_family.italic, text)?;
    let subset_bold_italic = subset_chain(&chain_family.bold_italic, text)?;

    Ok(FontFamily {
        regular: subset_regular,
        bold: subset_bold,
        italic: subset_italic,
        bold_italic: subset_bold_italic,
    })
}

/// Subsets a single fallback chain by subsetting each font based on what it renders.
fn subset_chain(
    chain: &genpdfi::fonts::FontFallbackChain,
    text: &str,
) -> Result<genpdfi::fonts::FontFallbackChain, Error> {
    let segments = chain.segment_text(text);
    use std::collections::HashMap;

    let mut font_chars: HashMap<*const FontData, String> = HashMap::new();
    for (segment_text, font_data) in &segments {
        let font_ptr = *font_data as *const FontData;
        font_chars
            .entry(font_ptr)
            .or_insert_with(String::new)
            .push_str(segment_text);
    }

    let primary_text = font_chars
        .get(&(chain.primary() as *const FontData))
        .map(|s| s.as_str())
        .unwrap_or("");

    let subset_primary = if !primary_text.is_empty() {
        trace!("Subsetting primary font ({} chars)...", primary_text.len());
        subset_single_font(chain.primary(), primary_text)?
    } else {
        chain.primary().clone()
    };

    let mut subset_fallbacks = Vec::new();
    for (idx, fallback) in chain.fallbacks().iter().enumerate() {
        let fallback_text = font_chars
            .get(&(fallback as *const FontData))
            .map(|s| s.as_str())
            .unwrap_or("");

        let subset_fallback = if !fallback_text.is_empty() {
            trace!(
                "Subsetting fallback {} ({} chars)...",
                idx + 1,
                fallback_text.len()
            );
            subset_single_font(fallback, fallback_text)?
        } else {
            trace!("Fallback {} not used, skipping subsetting", idx + 1);
            fallback.clone()
        };

        subset_fallbacks.push(subset_fallback);
    }

    let mut new_chain = genpdfi::fonts::FontFallbackChain::new(subset_primary);
    for fallback in subset_fallbacks {
        new_chain = new_chain.with_fallback(fallback);
    }

    Ok(new_chain)
}

/// Subsets a single FontData based on the provided text.
fn subset_single_font(font: &FontData, text: &str) -> Result<FontData, Error> {
    let original_data = font.get_data()?;
    let original_size = original_data.len();

    let subset_data = genpdfi::subsetting::subset_font(original_data, text).map_err(|e| {
        warn!("Font subsetting failed: {}, using full font", e);
        e
    })?;

    let subset_size = subset_data.len();
    let reduction = ((original_size - subset_size) as f64 / original_size as f64) * 100.0;

    trace!(
        "{} -> {} ({:.1}% reduction)",
        format_size(original_size),
        format_size(subset_size),
        reduction
    );

    // Create new FontData with subset data (no builtin font for embedded fonts)
    FontData::new_shared(Arc::new(subset_data), None)
}

/// Formats a byte size in a human-readable format.
fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Loads a Unicode-capable system font with good international character support.
///
/// This function attempts to find and load a font from the system that supports
/// a wide range of Unicode characters, making it suitable for documents with
/// international text (Romanian, Cyrillic, Greek, etc.).
///
/// # Priority Order
/// 1. Noto Sans - Google's comprehensive Unicode font
/// 2. DejaVu Sans - Popular open-source Unicode font
/// 3. Liberation Sans - Red Hat's Unicode font
/// 4. Arial Unicode MS - Microsoft's Unicode font (if available)
/// 5. Fallback to Helvetica (PDF built-in, limited to Windows-1252)
///
/// # Arguments
/// * `text` - Optional text to check coverage for. If provided, will verify the selected font supports all characters.
///
/// # Returns
/// * `Ok(FontFamily<FontData>)` - A font family with good Unicode support
/// * `Err(Error)` - If no suitable font could be loaded
///
/// # Example
/// ```rust,no_run
/// use markdown2pdf::fonts::load_unicode_system_font;
///
/// // Load best available Unicode font
/// let font = load_unicode_system_font(None)?;
///
/// // Load and verify coverage for Romanian text
/// let romanian_text = "ăâîșț ĂÂÎȘȚ";
/// let font = load_unicode_system_font(Some(romanian_text))?;
/// # Ok::<(), genpdfi::error::Error>(())
/// ```
pub fn load_unicode_system_font(text: Option<&str>) -> Result<FontFamily<FontData>, Error> {
    // Priority list of Unicode-capable fonts
    // macOS-specific fonts are prioritized since they're more likely to be available
    let unicode_fonts = [
        "Noto Sans",
        "DejaVu Sans",
        "Liberation Sans",
        "Arial Unicode MS",
        "SF Pro",         // macOS system font (good Unicode)
        "Helvetica Neue", // macOS (better than Helvetica)
        "Segoe UI",       // Windows default (good Unicode support)
        "Roboto",         // Android default
    ];

    let mut tried_fonts = Vec::new();

    // Try each Unicode font
    for font_name in &unicode_fonts {
        if let Ok(family) = load_system_font_family_simple(font_name) {
            // If text provided, check coverage
            if let Some(text) = text {
                let coverage = family.regular.check_coverage(text);
                if coverage.is_complete() {
                    info!("Using system font '{}' (100% coverage)", font_name);
                    return Ok(family);
                } else {
                    debug!(
                        "Font '{}' has only {:.1}% coverage, trying next...",
                        font_name,
                        coverage.coverage_percent()
                    );
                    tried_fonts.push(format!(
                        "{} ({:.1}% coverage)",
                        font_name,
                        coverage.coverage_percent()
                    ));
                }
            } else {
                // No text to check, font found is good enough
                info!("Using system font '{}'", font_name);
                return Ok(family);
            }
        } else {
            tried_fonts.push(format!("{} (not found)", font_name));
        }
    }

    warn!("No suitable Unicode font found, falling back to Helvetica (limited character support)");
    if !tried_fonts.is_empty() {
        debug!("Fonts tried:");
        for font in &tried_fonts {
            debug!("  - {}", font);
        }
    }
    debug!("To fix this, install a Unicode font:");
    debug!("  brew install font-noto-sans  (Homebrew)");
    debug!("  Or download from https://fonts.google.com/noto");
    load_builtin_font_family("helvetica")
}

/// Extracts primary fonts from a fallback chain family.
///
/// This creates a `FontFamily<FontData>` from a `FontFamily<FontFallbackChain>`
/// by taking the primary font from each chain. This is useful for compatibility
/// with genpdfi which expects `FontData` rather than chains.
///
/// # Arguments
/// * `chain_family` - The fallback chain family to extract primaries from
///
/// # Returns
/// A `FontFamily<FontData>` containing the primary fonts
pub fn extract_primary_fonts(
    chain_family: &FontFamily<genpdfi::fonts::FontFallbackChain>,
) -> FontFamily<FontData> {
    FontFamily {
        regular: chain_family.regular.primary().clone(),
        bold: chain_family.bold.primary().clone(),
        italic: chain_family.italic.primary().clone(),
        bold_italic: chain_family.bold_italic.primary().clone(),
    }
}

/// Returns a list of sensible default fallback fonts for the given primary font.
///
/// These fallbacks are tried in order when characters are missing from the primary font:
/// 1. Unicode fonts (Noto Sans, DejaVu Sans, Arial Unicode)
/// 2. System fonts (SF Pro on macOS, Segoe UI on Windows)
/// 3. Emoji fonts for emoji support
///
/// # Arguments
/// * `primary_name` - The primary font name (used to avoid redundant fallbacks)
///
/// # Returns
/// A vector of fallback font names
pub fn get_default_fallback_fonts(primary_name: &str) -> Vec<String> {
    let primary_lower = primary_name.to_lowercase();

    let candidates = vec![
        "Noto Sans",
        "DejaVu Sans",
        "Arial Unicode MS",
        "SF Pro",           // macOS
        "Segoe UI",         // Windows
        "Roboto",           // Android/Chrome OS
        "Noto Color Emoji", // Emoji support
    ];

    candidates
        .into_iter()
        .filter(|name| name.to_lowercase() != primary_lower)
        .map(|s| s.to_string())
        .collect()
}

/// Loads fonts and creates a fallback chain for handling mixed-script documents.
///
/// This function creates a `FontFallbackChain` by:
/// 1. Loading the primary font
/// 2. Loading all specified fallback fonts
/// 3. Creating a chain where fonts are tried in order
///
/// When rendering text, the chain will automatically select the appropriate font
/// for each character based on glyph coverage.
///
/// # Arguments
/// * `primary_name` - Name of the primary font to load
/// * `fallback_names` - List of fallback font names to try
/// * `custom_paths` - Custom paths to search for fonts
/// * `text` - Optional text for validation (currently unused, kept for API compatibility)
///
/// # Returns
/// A `FontFamily` where each variant (regular, bold, etc.) is a `FontFallbackChain`
///
/// # Example
/// ```no_run
/// use markdown2pdf::fonts::load_font_with_fallback_chain;
/// use std::path::PathBuf;
///
/// let chain = load_font_with_fallback_chain(
///     "Noto Sans",
///     &["DejaVu Sans".to_string(), "Arial".to_string()],
///     &[PathBuf::from("./fonts")],
///     Some("Hello мир! 👋")
/// )?;
/// # Ok::<(), genpdfi::error::Error>(())
/// ```
pub fn load_font_with_fallback_chain(
    primary_name: &str,
    fallback_names: &[String],
    custom_paths: &[PathBuf],
    _text: Option<&str>,
) -> Result<FontFamily<genpdfi::fonts::FontFallbackChain>, Error> {
    use genpdfi::fonts::FontFallbackChain;

    // Load primary font
    let primary_family = if !custom_paths.is_empty() {
        load_custom_font_family(primary_name, custom_paths)
            .or_else(|_| load_system_font_family_simple(primary_name))
    } else {
        load_system_font_family_simple(primary_name)
    }?;

    // Load all fallback fonts
    let mut fallback_families = Vec::new();
    for fallback_name in fallback_names {
        let fallback_family = if !custom_paths.is_empty() {
            load_custom_font_family(fallback_name, custom_paths)
                .or_else(|_| load_system_font_family_simple(fallback_name))
        } else {
            load_system_font_family_simple(fallback_name)
        };

        if let Ok(family) = fallback_family {
            info!("Loaded fallback font '{}'", fallback_name);
            fallback_families.push(family);
        } else {
            warn!("Fallback font '{}' not found, skipping", fallback_name);
        }
    }

    // Create fallback chains for each variant
    let regular_chain = {
        let mut chain = FontFallbackChain::new(primary_family.regular);
        for family in &fallback_families {
            chain = chain.with_fallback(family.regular.clone());
        }
        chain
    };

    let bold_chain = {
        let mut chain = FontFallbackChain::new(primary_family.bold);
        for family in &fallback_families {
            chain = chain.with_fallback(family.bold.clone());
        }
        chain
    };

    let italic_chain = {
        let mut chain = FontFallbackChain::new(primary_family.italic);
        for family in &fallback_families {
            chain = chain.with_fallback(family.italic.clone());
        }
        chain
    };

    let bold_italic_chain = {
        let mut chain = FontFallbackChain::new(primary_family.bold_italic);
        for family in &fallback_families {
            chain = chain.with_fallback(family.bold_italic.clone());
        }
        chain
    };

    info!(
        "Created fallback chain: {} + {} fallback(s)",
        primary_name,
        fallback_families.len()
    );

    Ok(FontFamily {
        regular: regular_chain,
        bold: bold_chain,
        italic: italic_chain,
        bold_italic: bold_italic_chain,
    })
}

/// Loads a font with fallback support for better coverage (legacy function).
///
/// This function tries to find the best font for the given text by:
/// 1. Loading the primary font
/// 2. Loading all fallback fonts
/// 3. Checking coverage for each
/// 4. Selecting the font with the best coverage
///
/// **Note**: This function is kept for backward compatibility. New code should use
/// `load_font_with_fallback_chain()` which returns actual fallback chains instead
/// of selecting a single best font.
///
/// # Arguments
/// * `primary_name` - Name of the primary font to load
/// * `fallback_names` - List of fallback font names to try
/// * `custom_paths` - Custom paths to search for fonts
/// * `text` - Optional text to check coverage for
///
/// # Returns
/// The font with the best coverage for the given text
pub fn load_font_with_fallbacks(
    primary_name: &str,
    fallback_names: &[String],
    custom_paths: &[PathBuf],
    text: Option<&str>,
) -> Result<FontFamily<FontData>, Error> {
    let mut tried_fonts = Vec::new();

    // Try to load primary font first
    let primary = if !custom_paths.is_empty() {
        load_custom_font_family(primary_name, custom_paths)
            .or_else(|_| load_system_font_family_simple(primary_name))
    } else {
        load_system_font_family_simple(primary_name)
    };

    // If no text to check, just return primary (or first fallback that works)
    if text.is_none() {
        if let Ok(font) = primary {
            return Ok(font);
        }
        tried_fonts.push(format!("{} (not found)", primary_name));

        // Try fallbacks
        for fallback_name in fallback_names {
            if let Ok(font) = load_system_font_family_simple(fallback_name) {
                info!("Using fallback font '{}'", fallback_name);
                return Ok(font);
            }
            tried_fonts.push(format!("{} (not found)", fallback_name));
        }

        warn!("Could not load font '{}' or any fallbacks", primary_name);
        debug!("Fonts tried:");
        for font in &tried_fonts {
            debug!("  - {}", font);
        }
        return Err(Error::new(
            format!("Could not load font '{}' or any fallbacks", primary_name),
            ErrorKind::InvalidFont,
        ));
    }

    let text = text.unwrap();
    let mut best_font: Option<FontFamily<FontData>> = None;
    let mut best_coverage = 0.0;
    let mut best_name = String::new();

    // Check primary font coverage
    if let Ok(font) = primary {
        let coverage = font.regular.check_coverage(text);
        if coverage.is_complete() {
            info!("Primary font '{}' has 100% coverage", primary_name);
            return Ok(font);
        }

        debug!(
            "Primary font '{}' coverage: {:.1}%",
            primary_name,
            coverage.coverage_percent()
        );

        best_coverage = coverage.coverage_percent();
        best_font = Some(font);
        best_name = primary_name.to_string();
        tried_fonts.push(format!(
            "{} ({:.1}% coverage)",
            primary_name,
            coverage.coverage_percent()
        ));
    } else {
        tried_fonts.push(format!("{} (not found)", primary_name));
    }

    // Try each fallback
    for fallback_name in fallback_names {
        let fallback = if !custom_paths.is_empty() {
            load_custom_font_family(fallback_name, custom_paths)
                .or_else(|_| load_system_font_family_simple(fallback_name))
        } else {
            load_system_font_family_simple(fallback_name)
        };

        if let Ok(font) = fallback {
            let coverage = font.regular.check_coverage(text);

            if coverage.is_complete() {
                info!("Fallback font '{}' has 100% coverage", fallback_name);
                return Ok(font);
            }

            debug!(
                "Fallback font '{}' coverage: {:.1}%",
                fallback_name,
                coverage.coverage_percent()
            );

            if coverage.coverage_percent() > best_coverage {
                best_coverage = coverage.coverage_percent();
                best_font = Some(font);
                best_name = fallback_name.clone();
            }
            tried_fonts.push(format!(
                "{} ({:.1}% coverage)",
                fallback_name,
                coverage.coverage_percent()
            ));
        } else {
            tried_fonts.push(format!("{} (not found)", fallback_name));
        }
    }

    // Return the font with best coverage
    if let Some(font) = best_font {
        if best_coverage < 100.0 {
            warn!(
                "Selected font '{}' with {:.1}% coverage (some characters may not display)",
                best_name, best_coverage
            );
            debug!("Fonts tried:");
            for font in &tried_fonts {
                debug!("  - {}", font);
            }
            debug!("To fix this, install a Unicode font:");
            debug!("  brew install font-noto-sans  (Homebrew)");
            debug!("  Or download from https://fonts.google.com/noto");
        } else {
            info!(
                "Selected font '{}' with {:.1}% coverage",
                best_name, best_coverage
            );
        }
        Ok(font)
    } else {
        warn!("Could not load font '{}' or any fallbacks", primary_name);
        debug!("Fonts tried:");
        for font in &tried_fonts {
            debug!("  - {}", font);
        }
        debug!("To fix this, install a Unicode font:");
        debug!("  brew install font-noto-sans  (Homebrew)");
        debug!("  Or download from https://fonts.google.com/noto");
        Err(Error::new(
            format!("Could not load font '{}' or any fallbacks", primary_name),
            ErrorKind::InvalidFont,
        ))
    }
}
