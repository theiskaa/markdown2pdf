//! Font configuration and resolution.
//!
//! This module exposes the public font configuration surface for
//! markdown2pdf consumers (CLI flags, library callers) plus the
//! platform-specific path resolution used by [`validation`] to warn
//! about missing fonts.
//!
//! Font *loading* and PDF embedding live inside the renderer module
//! ([`crate::render`]), which owns the printpdf bindings. Keeping this
//! file backend-agnostic means changing the renderer's font stack
//! doesn't ripple into the public configuration API.

use std::path::{Path, PathBuf};

/// Specifies where to load a font from.
#[derive(Debug, Clone)]
pub enum FontSource {
    /// Built-in PDF font (Helvetica, Times, Courier). No file I/O needed.
    Builtin(&'static str),
    /// System font by name. The renderer searches known directories.
    System(String),
    /// Direct file path to a TTF/OTF file.
    File(PathBuf),
    /// Raw font bytes (e.g. from `include_bytes!`). Useful for embedding
    /// fonts in GUI apps or for tests.
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

    /// Create a font source from raw bytes (e.g. `include_bytes!`).
    pub fn bytes(data: &'static [u8]) -> Self {
        FontSource::Bytes(data)
    }
}

/// Configuration for fonts used in the generated PDF.
///
/// Both `default_font` and `code_font` accept friendly names ("Georgia",
/// "Helvetica", "/path/to/font.ttf") and are resolved at render time.
/// Explicit `*_source` fields take priority when set.
#[derive(Debug, Clone, Default)]
pub struct FontConfig {
    /// Default font name for body text.
    pub default_font: Option<String>,
    /// Font name for code blocks.
    pub code_font: Option<String>,
    /// Font source for body text. Takes priority over `default_font` if set.
    pub default_font_source: Option<FontSource>,
    /// Font source for code blocks. Takes priority over `code_font` if set.
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

    /// Set the default body font.
    pub fn with_default_font(mut self, font: impl Into<String>) -> Self {
        self.default_font = Some(font.into());
        self
    }

    /// Set the code font.
    pub fn with_code_font(mut self, font: impl Into<String>) -> Self {
        self.code_font = Some(font.into());
        self
    }

    /// Set the font source for body text directly.
    pub fn with_default_font_source(mut self, source: FontSource) -> Self {
        self.default_font_source = Some(source);
        self
    }

    /// Set the font source for code blocks directly.
    pub fn with_code_font_source(mut self, source: FontSource) -> Self {
        self.code_font_source = Some(source);
        self
    }

    /// Enable or disable font subsetting.
    pub fn with_subsetting(mut self, enabled: bool) -> Self {
        self.enable_subsetting = enabled;
        self
    }
}

/// Names recognized as PDF Type 1 built-ins. The renderer's font module
/// maps these to printpdf's `BuiltinFont`.
pub fn is_builtin_font_name(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "helvetica"
            | "arial"
            | "sans-serif"
            | "times"
            | "times new roman"
            | "serif"
            | "courier"
            | "courier new"
            | "monospace"
    )
}

/// Resolve a font name (CLI / TOML config / API caller) to a [`FontSource`].
///
/// - Built-in names (Helvetica, Times, Courier and aliases) -> `Builtin`
/// - Paths (contain `/`, `\`, or end in `.ttf`/`.otf`) -> `File`
/// - Everything else -> `System` (name lookup happens at load time)
pub fn resolve_font_source(name: &str) -> FontSource {
    if is_builtin_font_name(name) {
        return FontSource::Builtin(match name.to_lowercase().as_str() {
            "helvetica" | "arial" | "sans-serif" => "Helvetica",
            "times" | "times new roman" | "serif" => "Times",
            "courier" | "courier new" | "monospace" => "Courier",
            _ => "Helvetica",
        });
    }
    if name.contains('/') || name.contains('\\') || name.ends_with(".ttf") || name.ends_with(".otf")
    {
        return FontSource::File(PathBuf::from(name));
    }
    FontSource::System(name.to_string())
}

/// Returns known font directories for the current platform.
pub fn system_font_dirs() -> Vec<&'static str> {
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

/// Search system font directories for a TTF/OTF file matching `name`.
/// Skips `.ttc` (TrueType Collection) files — most font parsers don't
/// handle them.
pub fn find_system_font(name: &str) -> Option<PathBuf> {
    let name_lower = name.to_lowercase();
    let patterns = [
        format!("{}.ttf", name),
        format!("{}.otf", name),
        format!("{}.ttf", name.replace(" MS", "")),
    ];

    for dir in system_font_dirs() {
        let dir_path = Path::new(dir);
        if !dir_path.exists() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir_path) else {
            continue;
        };
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let file_lower = file_name.to_string_lossy().to_lowercase();

            if file_lower.ends_with(".ttc") {
                continue;
            }

            if patterns.iter().any(|p| file_lower == p.to_lowercase()) {
                return Some(entry.path());
            }

            if file_lower.starts_with(&name_lower)
                && (file_lower.ends_with(".ttf") || file_lower.ends_with(".otf"))
            {
                return Some(entry.path());
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_name_recognized() {
        assert!(is_builtin_font_name("Helvetica"));
        assert!(is_builtin_font_name("helvetica"));
        assert!(is_builtin_font_name("Times New Roman"));
        assert!(is_builtin_font_name("courier"));
        assert!(!is_builtin_font_name("Georgia"));
    }

    #[test]
    fn resolve_builtin() {
        assert!(matches!(
            resolve_font_source("Helvetica"),
            FontSource::Builtin("Helvetica")
        ));
        assert!(matches!(
            resolve_font_source("arial"),
            FontSource::Builtin("Helvetica")
        ));
    }

    #[test]
    fn resolve_path() {
        assert!(matches!(
            resolve_font_source("/some/path/font.ttf"),
            FontSource::File(_)
        ));
        assert!(matches!(
            resolve_font_source("relative.otf"),
            FontSource::File(_)
        ));
    }

    #[test]
    fn resolve_system() {
        assert!(matches!(
            resolve_font_source("Georgia"),
            FontSource::System(_)
        ));
    }

    #[test]
    fn system_font_dirs_present() {
        // Don't assert anything platform-specific — just verify the
        // function returns successfully.
        let _ = system_font_dirs();
    }
}
