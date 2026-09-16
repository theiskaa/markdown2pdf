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

/// Returns the platform's system-wide font directories.
///
/// These are only the fixed roots. The lookup in [`find_system_font`]
/// also searches per-user font directories and walks subdirectories;
/// see [`font_search_dirs`] for the full list of roots it starts from.
pub fn system_font_dirs() -> Vec<&'static str> {
    if cfg!(target_os = "macos") {
        vec![
            "/System/Library/Fonts",
            "/System/Library/Fonts/Supplemental",
            "/Library/Fonts",
        ]
    } else if cfg!(target_os = "linux") {
        vec!["/usr/share/fonts", "/usr/local/share/fonts"]
    } else if cfg!(target_os = "windows") {
        vec!["C:\\Windows\\Fonts"]
    } else {
        vec![]
    }
}

/// Returns every root directory [`find_system_font`] searches: the
/// system-wide [`system_font_dirs`] followed by the current user's font
/// directories. Subdirectories of each root are searched too.
///
/// Per-user directories are `~/Library/Fonts` on macOS,
/// `$XDG_DATA_HOME/fonts` (default `~/.local/share/fonts`) and
/// `~/.fonts` on Linux, and `%LOCALAPPDATA%\Microsoft\Windows\Fonts`
/// on Windows.
pub fn font_search_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = system_font_dirs().into_iter().map(PathBuf::from).collect();
    let home = std::env::home_dir();
    if cfg!(target_os = "macos") {
        dirs.extend(home.map(|h| h.join("Library/Fonts")));
    } else if cfg!(target_os = "linux") {
        let data_home = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home.as_ref().map(|h| h.join(".local/share")));
        dirs.extend(data_home.map(|d| d.join("fonts")));
        dirs.extend(home.map(|h| h.join(".fonts")));
    } else if cfg!(target_os = "windows") {
        dirs.extend(
            std::env::var_os("LOCALAPPDATA")
                .map(|d| PathBuf::from(d).join("Microsoft\\Windows\\Fonts")),
        );
    }
    dirs
}

/// How deep [`find_system_font`] descends below each search root.
/// Distribution packages nest two levels deep
/// (`/usr/share/fonts/truetype/dejavu/`); the extra headroom covers
/// hand-organized user font folders without walking arbitrarily deep.
const FONT_DIR_MAX_DEPTH: usize = 4;

/// Search the platform's font directories (see [`font_search_dirs`])
/// for a TTF/OTF file matching `name`. Skips `.ttc` (TrueType
/// Collection) files — most font parsers don't handle them.
pub fn find_system_font(name: &str) -> Option<PathBuf> {
    find_system_font_in(name, &font_search_dirs())
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
    const CANDIDATES: &[&str] = &[
        "Helvetica Neue",
        "Geneva",
        "Lucida Grande",
        "Arial Unicode MS",
    ];
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

/// Lowercase `s` and drop spaces, hyphens, and underscores, so
/// `DejaVu Sans`, `DejaVuSans`, and `dejavu-sans` compare equal.
pub(crate) fn normalize_font_name(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Weight names recognized in font file names, normalized by
/// [`normalize_font_name`].
const WEIGHT_NAMES: &[(u16, &[&str])] = &[
    (100, &["thin", "hairline"]),
    (200, &["extralight", "ultralight"]),
    (300, &["light"]),
    (400, &["regular", "normal", "roman", "book"]),
    (500, &["medium"]),
    (600, &["semibold", "demibold"]),
    (700, &["bold"]),
    (800, &["extrabold", "ultrabold"]),
    (900, &["black", "heavy"]),
];

/// Parse a normalized file-name suffix made only of an optional weight
/// name (or a numeric `100`..`900`) followed by an optional `italic` /
/// `oblique`. The empty suffix is the regular upright face.
pub(crate) fn parse_face_style(suffix: &str) -> Option<(u16, bool)> {
    let (weight, italic) = match suffix
        .strip_suffix("italic")
        .or_else(|| suffix.strip_suffix("oblique"))
    {
        Some(rest) => (rest, true),
        None => (suffix, false),
    };
    if weight.is_empty() {
        return Some((400, italic));
    }
    WEIGHT_NAMES
        .iter()
        .find(|(w, names)| names.contains(&weight) || weight.parse() == Ok(*w))
        .map(|&(w, _)| (w, italic))
}

/// Split a normalized font stem into family, weight, and slant, taking
/// the longest trailing style suffix: `fooextrabolditalic` is
/// `("foo", 800, true)`. A stem with no recognized suffix is a regular
/// upright face of its own family.
pub(crate) fn split_face_stem(stem: &str) -> (&str, u16, bool) {
    stem.char_indices()
        .skip(1)
        .find_map(|(i, _)| parse_face_style(&stem[i..]).map(|(w, it)| (&stem[..i], w, it)))
        .unwrap_or((stem, 400, false))
}

/// Collect `.ttf` / `.otf` files below `dir`, at most `depth` levels
/// down, in sorted order so ties resolve the same way on every run.
/// Canonical directory paths are tracked so symlink cycles terminate.
fn collect_font_files(
    dir: &Path,
    depth: usize,
    seen: &mut std::collections::HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) {
    let Ok(canonical) = dir.canonicalize() else {
        return;
    };
    if !seen.insert(canonical) {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            if depth > 0 {
                collect_font_files(&path, depth - 1, seen, out);
            }
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
        {
            out.push(path);
        }
    }
}

/// `find_system_font` with the search directories injected, so the
/// matching logic can be exercised against a controlled directory.
///
/// Names match file stems ignoring case, spaces, hyphens, and
/// underscores. In order of preference: an exact stem
/// (`DejaVuSans.ttf` for `DejaVu Sans`), then the family's explicit
/// regular face (`NotoSans-Regular.ttf` for `Noto Sans`), then the
/// shortest stem that starts with the name (`Tahoma Bold.ttf` before
/// `Tahoma Bold Italic.ttf`). Earlier directories win ties.
fn find_system_font_in(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    let wanted: Vec<String> = [name.to_string(), name.replace(" MS", "")]
        .iter()
        .map(|n| normalize_font_name(n))
        .collect();
    let mut seen = std::collections::HashSet::new();
    let mut best: Option<((u8, usize), PathBuf)> = None;
    for dir in dirs {
        let mut files = Vec::new();
        collect_font_files(dir, FONT_DIR_MAX_DEPTH, &mut seen, &mut files);
        for path in files {
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let stem = normalize_font_name(stem);
            let rank = wanted
                .iter()
                .filter_map(|w| {
                    let suffix = stem.strip_prefix(w.as_str())?;
                    Some(if suffix.is_empty() {
                        (0, 0)
                    } else if parse_face_style(suffix) == Some((400, false)) {
                        (1, 0)
                    } else {
                        (2, stem.len())
                    })
                })
                .min();
            let Some(rank) = rank else {
                continue;
            };
            if rank.0 == 0 {
                return Some(path);
            }
            if best.as_ref().is_none_or(|(r, _)| rank < *r) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
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
        let _ = font_search_dirs();
    }

    /// Builds a throwaway directory containing the named empty files
    /// (which may include subdirectories) and runs `f` with its path. Cleans up afterwards. The directory
    /// name is made unique with a process-wide atomic counter so the
    /// parallel font tests can't collide on each other's files.
    fn with_font_dir(files: &[&str], f: impl FnOnce(PathBuf)) {
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
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        }
        f(dir.clone());
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

    #[test]
    fn find_system_font_ignores_case_and_separators() {
        with_font_dir(&["DejaVuSansMono.ttf", "DejaVuSans.ttf"], |dir| {
            let found = find_system_font_in("DejaVu Sans", &[dir]).unwrap();
            assert_eq!(found.file_name().unwrap(), "DejaVuSans.ttf");
        });
        with_font_dir(&["liberation_serif.OTF"], |dir| {
            assert!(find_system_font_in("Liberation-Serif", &[dir]).is_some());
        });
    }

    #[test]
    fn find_system_font_prefers_the_family_regular_face() {
        // `NotoSansArabic-Regular` also contains "regular" and sorts
        // first, but it's a different family.
        with_font_dir(
            &[
                "NotoSansArabic-Regular.ttf",
                "NotoSans-Bold.ttf",
                "NotoSans-Regular.ttf",
            ],
            |dir| {
                let found = find_system_font_in("Noto Sans", &[dir]).unwrap();
                assert_eq!(found.file_name().unwrap(), "NotoSans-Regular.ttf");
            },
        );
    }

    #[test]
    fn find_system_font_searches_nested_directories() {
        // Debian and Fedora package fonts two levels below the root.
        with_font_dir(&["truetype/dejavu/DejaVuSans.ttf"], |dir| {
            let found = find_system_font_in("DejaVu Sans", &[dir]).unwrap();
            assert!(found.ends_with("truetype/dejavu/DejaVuSans.ttf"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn find_system_font_survives_symlink_cycles() {
        with_font_dir(&["fonts/Foo.ttf"], |dir| {
            std::os::unix::fs::symlink(&dir, dir.join("fonts/loop")).unwrap();
            assert!(find_system_font_in("Foo", std::slice::from_ref(&dir)).is_some());
            assert!(find_system_font_in("Missing", &[dir]).is_none());
        });
    }

    #[test]
    fn face_stems_split_into_family_weight_and_slant() {
        for (stem, expected) in [
            ("fooregular", ("foo", 400, false)),
            ("fooextrabolditalic", ("foo", 800, true)),
            ("foosemibold", ("foo", 600, false)),
            ("foo700oblique", ("foo", 700, true)),
            ("fooitalic", ("foo", 400, true)),
            ("timesnewroman", ("timesnew", 400, false)),
            ("arialblack", ("arial", 900, false)),
            ("foo", ("foo", 400, false)),
            ("bold", ("bold", 400, false)),
        ] {
            assert_eq!(split_face_stem(stem), expected, "{stem}");
        }
        assert_eq!(parse_face_style(""), Some((400, false)));
        assert_eq!(parse_face_style("narrow"), None);
    }
}
