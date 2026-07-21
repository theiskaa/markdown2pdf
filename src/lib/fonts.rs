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
    /// Ordered list of fallback font *names* (system / path / built-in
    /// alias). Resolved the same way as `default_font` at render time.
    /// Composed with `fallback_font_sources` (sources first, then names)
    /// and with any `fallback_fonts` set on `[defaults]` in the TOML
    /// config.
    pub fallback_fonts: Vec<String>,
    /// Ordered list of pre-resolved fallback font sources. Useful when
    /// embedding fonts via `include_bytes!` or pointing at a known
    /// path. Composed before `fallback_fonts`.
    pub fallback_font_sources: Vec<FontSource>,
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
            fallback_fonts: Vec::new(),
            fallback_font_sources: Vec::new(),
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

    /// Replace the fallback-font name list. See [`FontConfig::fallback_fonts`].
    pub fn with_fallback_fonts<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.fallback_fonts = names.into_iter().map(Into::into).collect();
        self
    }

    /// Append one fallback name to the existing list.
    pub fn add_fallback_font(mut self, name: impl Into<String>) -> Self {
        self.fallback_fonts.push(name.into());
        self
    }

    /// Append one pre-resolved fallback font source.
    pub fn add_fallback_font_source(mut self, source: FontSource) -> Self {
        self.fallback_font_sources.push(source);
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

/// Search the platform's system font directories for a TTF/OTF file
/// matching `name`. Skips `.ttc` (TrueType Collection) files — most
/// font parsers don't handle them.
pub fn find_system_font(name: &str) -> Option<PathBuf> {
    find_system_font_in(name, &system_font_dirs())
}

/// Probe a per-OS list of likely-installed Unicode body fonts and
/// return the first one that resolves. The built-in Type 1 Helvetica
/// the renderer otherwise falls back to is ASCII-only (lopdf's
/// WinAnsi encoder passes UTF-8 bytes through unchanged, so anything
/// outside ASCII becomes `?`), which means an unconfigured user
/// renders accented Latin, em-dashes, smart quotes, arrows, and math
/// symbols as `?`. Auto-picking a system Unicode font preserves
/// fidelity for the default-config path without bundling any font.
///
/// Scripts the picked font doesn't cover (CJK, Arabic, Devanagari,
/// emoji, …) still need an explicit `[defaults].fallback_fonts` or
/// `FontConfig::with_fallback_fonts` — the auto-pick aims at the
/// common-case Latin+punctuation degradation, not full multi-script
/// coverage.
///
/// `.ttc` collection files are silently skipped by [`find_system_font`],
/// so candidates like `Helvetica Neue` or `Lucida Grande` won't
/// resolve on current macOS even though they're listed; the list
/// keeps them so the same probe stays correct once a `.ttc`-capable
/// loader lands. Until then, `Geneva` (always present in
/// `/System/Library/Fonts`) is the macOS winner.
pub fn default_body_source() -> Option<FontSource> {
    #[cfg(target_os = "macos")]
    const CANDIDATES: &[&str] =
        &["Helvetica Neue", "Geneva", "Lucida Grande", "Arial Unicode MS"];
    #[cfg(target_os = "windows")]
    const CANDIDATES: &[&str] = &["Segoe UI", "Arial", "Tahoma"];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const CANDIDATES: &[&str] = &["DejaVu Sans", "Liberation Sans", "Noto Sans"];
    for name in CANDIDATES {
        if find_system_font(name).is_some() {
            return Some(FontSource::System((*name).to_string()));
        }
    }
    None
}

/// `find_system_font` with the search directories injected, so the
/// matching logic can be exercised against a controlled directory.
fn find_system_font_in(name: &str, dirs: &[&str]) -> Option<PathBuf> {
    let name_lower = name.to_lowercase();
    let patterns: Vec<String> = [
        format!("{}.ttf", name),
        format!("{}.otf", name),
        format!("{}.ttf", name.replace(" MS", "")),
    ]
    .iter()
    .map(|p| p.to_lowercase())
    .collect();

    // An exact filename match always wins, but directory enumeration
    // order is unspecified — a prefix like `Tahoma Bold.ttf` can be
    // visited before the exact `Tahoma.ttf`. So scan every entry for
    // an exact match first; only if none exists fall back to the
    // shortest-named prefix match (regular faces have shorter names
    // than their `X Bold` / `X Italic` siblings).
    let mut prefix_match: Option<PathBuf> = None;
    for dir in dirs {
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

            if patterns.contains(&file_lower) {
                return Some(entry.path());
            }

            if file_lower.starts_with(&name_lower)
                && (file_lower.ends_with(".ttf") || file_lower.ends_with(".otf"))
            {
                let shorter = prefix_match
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| file_lower.len() < n.to_string_lossy().len())
                    .unwrap_or(true);
                if shorter {
                    prefix_match = Some(entry.path());
                }
            }
        }
    }

    prefix_match
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

    /// Builds a throwaway directory containing the named empty files
    /// and runs `f` with its path. Cleans up afterwards. The directory
    /// name is made unique with a process-wide atomic counter so the
    /// parallel font tests can't collide on each other's files.
    fn with_font_dir(files: &[&str], f: impl FnOnce(&str)) {
        use std::sync::atomic::{AtomicU32, Ordering};
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "m2pdf_fonttest_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in files {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        f(dir.to_str().unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_system_font_prefers_exact_over_prefix() {
        // `Tahoma Bold.ttf` sorts before `Tahoma.ttf` and may be
        // enumerated first — the exact match must still win, else the
        // bold face gets used as the regular weight.
        with_font_dir(&["Tahoma Bold.ttf", "Tahoma.ttf"], |dir| {
            let found = find_system_font_in("Tahoma", &[dir]).unwrap();
            assert_eq!(found.file_name().unwrap(), "Tahoma.ttf");
        });
    }

    #[test]
    fn find_system_font_prefix_fallback_picks_shortest() {
        // No exact `Tahoma.ttf` — fall back to the shortest-named
        // prefix match rather than whatever the OS lists first.
        with_font_dir(&["Tahoma Italic.ttf", "Tahoma Bold.ttf"], |dir| {
            let found = find_system_font_in("Tahoma", &[dir]).unwrap();
            assert_eq!(found.file_name().unwrap(), "Tahoma Bold.ttf");
        });
    }

    #[test]
    fn find_system_font_skips_ttc() {
        with_font_dir(&["Helvetica Neue.ttc"], |dir| {
            assert!(find_system_font_in("Helvetica Neue", &[dir]).is_none());
        });
    }
}
