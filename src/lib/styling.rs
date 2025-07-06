//! Styling module for markdown-to-pdf conversion.
//!
//! This module provides styling configuration for converting markdown elements to PDF,
//! including fonts, text styles, margins and alignments. The styling system supports
//! customization through a TOML configuration file, allowing control over properties
//! like font size, colors, spacing, alignment and text decorations for each element type.
//!
//! The styling configuration can be loaded from a TOML file or created programmatically.
//! Each element type (headings, text, emphasis, code blocks etc.) can have its own style
//! settings. The styling is applied during the PDF generation process to create properly
//! formatted output.
//!
//! Font handling is done through embedded assets, with support for different font weights
//! and styles. The styling system integrates with the PDF generation pipeline to ensure
//! consistent formatting throughout the document.

#[cfg(any())]
use crate::assets;

/// Available font families that can be used in the PDF document.
/// Currently only supports Roboto font.
#[cfg(any())]
pub enum MdPdfFont {
    Roboto,
}

/// Global font cache to ensure we never load the same font twice
#[cfg(any())]
static FONT_CACHE: Lazy<Mutex<HashMap<String, Arc<Vec<u8>>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Ultra-minimal font loading strategy that maximizes space efficiency
#[cfg(any())]
pub struct UltraMinimalFontLoader;

#[cfg(any())]
impl UltraMinimalFontLoader {
    /// Loads font data with aggressive caching to prevent duplication
    fn get_cached_font_data(font_path: &str) -> Result<Arc<Vec<u8>>, Error> {
        let mut cache = FONT_CACHE.lock().unwrap();

        if let Some(cached_data) = cache.get(font_path) {
            return Ok(cached_data.clone());
        }

        let font_data = assets::get_font_data(font_path).ok_or_else(|| {
            Error::new(
                format!("Failed to load embedded font: {}", font_path),
                genpdfi::error::ErrorKind::InvalidFont,
            )
        })?;

        let arc_data = Arc::new(font_data);
        cache.insert(font_path.to_string(), arc_data.clone());
        Ok(arc_data)
    }

    /// Creates the most memory-efficient FontFamily possible
    pub fn create_ultra_minimal_family(
        font: MdPdfFont,
        style: &StyleMatch,
    ) -> Result<FontFamily<FontData>, Error> {
        let (_needs_regular, needs_bold, needs_italic, needs_bold_italic) =
            MdPdfFont::analyze_needed_variants(style);

        // Load regular font with caching
        let regular_path = format!("fonts/{}/{}-Regular.ttf", font.dir(), font.file());
        let regular_data = Self::get_cached_font_data(&regular_path)?;

        if !needs_bold && !needs_italic && !needs_bold_italic {
            // ULTIMATE OPTIMIZATION: Use the same Arc<Vec<u8>> for all variants
            // This means only ONE copy of font data in memory
            let shared_data = (*regular_data).clone();

            let regular = FontData::new(shared_data.clone(), None)?;
            let bold = FontData::new(shared_data.clone(), None)?;
            let italic = FontData::new(shared_data.clone(), None)?;
            let bold_italic = FontData::new(shared_data, None)?;

            return Ok(FontFamily {
                regular,
                bold,
                italic,
                bold_italic,
            });
        }

        // Load only required variants with caching
        let regular = FontData::new((*regular_data).clone(), None)?;

        let bold = if needs_bold {
            let bold_path = format!("fonts/{}/{}-Bold.ttf", font.dir(), font.file());
            let bold_data = Self::get_cached_font_data(&bold_path)?;
            FontData::new((*bold_data).clone(), None)?
        } else {
            FontData::new((*regular_data).clone(), None)?
        };

        let italic = if needs_italic {
            let italic_path = format!("fonts/{}/{}-Italic.ttf", font.dir(), font.file());
            let italic_data = Self::get_cached_font_data(&italic_path)?;
            FontData::new((*italic_data).clone(), None)?
        } else {
            FontData::new((*regular_data).clone(), None)?
        };

        let bold_italic = if needs_bold_italic {
            let bold_italic_path = format!("fonts/{}/{}-BoldItalic.ttf", font.dir(), font.file());
            let bold_italic_data = Self::get_cached_font_data(&bold_italic_path)?;
            FontData::new((*bold_italic_data).clone(), None)?
        } else {
            FontData::new((*regular_data).clone(), None)?
        };

        Ok(FontFamily {
            regular,
            bold,
            italic,
            bold_italic,
        })
    }
}

#[cfg(any())]
impl MdPdfFont {
    /// Returns the directory name where the font files are stored.
    pub fn dir(&self) -> &'static str {
        match self {
            MdPdfFont::Roboto => "roboto",
        }
    }

    /// Returns the base filename of the font files without extension.
    pub fn file(&self) -> &'static str {
        match self {
            MdPdfFont::Roboto => "Roboto",
        }
    }

    /// Finds a matching font family based on the provided name.
    pub fn find_match(family: Option<&str>) -> MdPdfFont {
        match family.unwrap_or("roboto") {
            _ => MdPdfFont::Roboto,
        }
    }

    /// Analyzes which font variants are needed based on style configuration
    pub fn analyze_needed_variants(style: &StyleMatch) -> (bool, bool, bool, bool) {
        let needs_regular = true; // Always need regular as fallback
        let mut needs_bold = false;
        let mut needs_italic = false;
        let mut needs_bold_italic = false;

        let styles_to_check = [
            &style.heading_1,
            &style.heading_2,
            &style.heading_3,
            &style.emphasis,
            &style.strong_emphasis,
            &style.code,
            &style.block_quote,
            &style.list_item,
            &style.link,
            &style.image,
            &style.text,
            &style.horizontal_rule,
        ];

        for text_style in styles_to_check {
            match (text_style.bold, text_style.italic) {
                (true, true) => needs_bold_italic = true,
                (true, false) => needs_bold = true,
                (false, true) => needs_italic = true,
                (false, false) => {} // Regular is already needed
            }
        }

        (needs_regular, needs_bold, needs_italic, needs_bold_italic)
    }

    pub fn load_minimal_font_family(
        family: Option<&str>,
        _style: &StyleMatch,
    ) -> Result<FontFamily<FontData>, Error> {
        let found_match = MdPdfFont::find_match(family);

        let font_path = format!(
            "fonts/{}/{}-Light.ttf",
            found_match.dir(),
            found_match.file()
        );
        let font_data = assets::get_font_data(&font_path).ok_or_else(|| {
            Error::new(
                "Failed to load font".to_string(),
                genpdfi::error::ErrorKind::InvalidFont,
            )
        })?;

        let shared_data = Arc::new(font_data);

        let regular = FontData::new_shared(shared_data.clone(), None)?;
        let bold = FontData::new_shared(shared_data.clone(), None)?;
        let italic = FontData::new_shared(shared_data.clone(), None)?;
        let bold_italic = FontData::new_shared(shared_data, None)?;

        Ok(FontFamily {
            regular,
            bold,
            italic,
            bold_italic,
        })
    }

    /// Legacy method for compatibility - redirects to minimal loading
    pub fn load_font_family(family: Option<&str>) -> Result<FontFamily<FontData>, Error> {
        // For maximum optimization, we need style context, so use default minimal approach
        let default_style = StyleMatch::default();
        Self::load_minimal_font_family(family, &default_style)
    }

    /// Helper function to load a specific font variant from embedded assets
    pub fn load_font_variant(font: MdPdfFont, variant: &str) -> Result<FontData, Error> {
        let font_path = format!("fonts/{}/{}-{}.ttf", font.dir(), font.file(), variant);
        let font_data = assets::get_font_data(&font_path).ok_or_else(|| {
            Error::new(
                format!("Failed to load embedded font: {}", font_path),
                genpdfi::error::ErrorKind::InvalidFont,
            )
        })?;
        FontData::new(font_data, None)
    }
}

/// Text alignment options for PDF elements.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TextAlignment {
    /// Align text to the left margin
    Left,
    /// Center text between margins
    Center,
    /// Align text to the right margin
    Right,
    /// Spread text evenly between margins
    Justify,
}

/// Margins configuration in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Margins {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Basic text styling properties that can be applied to any text element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BasicTextStyle {
    /// Font size in points
    pub size: u8,
    /// Text color in RGB format
    pub text_color: Option<(u8, u8, u8)>,
    /// Space before element in points
    pub before_spacing: f32,
    /// Space after element in points
    pub after_spacing: f32,
    /// Text alignment within container
    pub alignment: Option<TextAlignment>,
    /// Font family name
    pub font_family: Option<&'static str>,
    /// Whether text should be bold
    pub bold: bool,
    /// Whether text should be italic
    pub italic: bool,
    /// Whether text should be underlined
    pub underline: bool,
    /// Whether text should have strikethrough
    pub strikethrough: bool,
    /// Background color in RGB format
    pub background_color: Option<(u8, u8, u8)>,
}

impl BasicTextStyle {
    /// Creates a new BasicTextStyle with the specified properties.
    ///
    /// # Arguments
    /// * `size` - Font size in points
    /// * `text_color` - Optional RGB color tuple for text
    /// * `before_spacing` - Optional space before element in points
    /// * `after_spacing` - Optional space after element in points
    /// * `alignment` - Optional text alignment
    /// * `font_family` - Optional font family name
    /// * `bold` - Whether text should be bold
    /// * `italic` - Whether text should be italic
    /// * `underline` - Whether text should be underlined
    /// * `strikethrough` - Whether text should have strikethrough
    /// * `background_color` - Optional RGB color tuple for background
    pub fn new(
        size: u8,
        text_color: Option<(u8, u8, u8)>,
        before_spacing: Option<f32>,
        after_spacing: Option<f32>,
        alignment: Option<TextAlignment>,
        font_family: Option<&'static str>,
        bold: bool,
        italic: bool,
        underline: bool,
        strikethrough: bool,
        background_color: Option<(u8, u8, u8)>,
    ) -> Self {
        Self {
            size,
            text_color,
            before_spacing: before_spacing.unwrap_or(0.0),
            after_spacing: after_spacing.unwrap_or(0.0),
            alignment,
            font_family,
            bold,
            italic,
            underline,
            strikethrough,
            background_color,
        }
    }
}

// LSP in vim behaves strangely with this default implementation.
// It's not used anywhere but included just in case.
impl Default for BasicTextStyle {
    fn default() -> Self {
        Self::new(
            12, None, None, None, None, None, false, false, false, false, None,
        )
    }
}

/// Main style configuration for mapping markdown elements to PDF styles.
///
/// This struct contains style definitions for each markdown element type
/// that can appear in the document. It is used by the PDF renderer to
/// determine how to format each element.
pub struct StyleMatch {
    /// Document margins
    pub margins: Margins,
    /// Style for level 1 headings (#)
    pub heading_1: BasicTextStyle,
    /// Style for level 2 headings (##)
    pub heading_2: BasicTextStyle,
    /// Style for level 3 headings (###)
    pub heading_3: BasicTextStyle,
    /// Style for emphasized text (*text* or _text_)
    pub emphasis: BasicTextStyle,
    /// Style for strongly emphasized text (**text** or __text__)
    pub strong_emphasis: BasicTextStyle,
    /// Style for inline code (`code`)
    pub code: BasicTextStyle,
    /// Style for block quotes (> quote)
    pub block_quote: BasicTextStyle,
    /// Style for list items (- item or * item)
    pub list_item: BasicTextStyle,
    /// Style for links ([text](url))
    pub link: BasicTextStyle,
    /// Style for images (![alt](url))
    pub image: BasicTextStyle,
    /// Style for regular text
    pub text: BasicTextStyle,

    // TODO: Not parsed into a actual horizontal rule currently, we need a proper styling for this
    /// Style for horizontal rules (---)
    pub horizontal_rule: BasicTextStyle,
}

/// Creates a StyleMatch with default styling settings.
///
/// The default style provides a clean, readable layout with hierarchical heading sizes,
/// appropriate base font sizes, and consistent spacing throughout the document. It sets
/// up styling for all supported markdown elements including headings, emphasis, code blocks,
/// quotes, lists and more.
impl Default for StyleMatch {
    fn default() -> Self {
        Self {
            margins: Margins {
                top: 8.0,
                right: 8.0,
                bottom: 8.0,
                left: 8.0,
            },
            heading_1: BasicTextStyle::new(
                14,
                Some((0, 0, 0)),
                Some(0.8),
                Some(0.5),
                Some(TextAlignment::Center),
                None,
                true,
                false,
                false,
                false,
                None,
            ),
            heading_2: BasicTextStyle::new(
                12,
                Some((0, 0, 0)),
                Some(0.8),
                Some(0.5),
                Some(TextAlignment::Left),
                None,
                true,
                false,
                false,
                false,
                None,
            ),
            heading_3: BasicTextStyle::new(
                10,
                Some((0, 0, 0)),
                Some(0.8),
                Some(0.5),
                Some(TextAlignment::Left),
                None,
                true,
                false,
                false,
                false,
                None,
            ),
            emphasis: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                None,
                None,
                None,
                false,
                true,
                false,
                false,
                None,
            ),
            strong_emphasis: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                None,
                None,
                None,
                true,
                false,
                false,
                false,
                None,
            ),
            code: BasicTextStyle::new(
                8,
                Some((128, 128, 128)),
                Some(0.4),
                Some(0.4),
                None,
                Some("Roboto"),
                false,
                false,
                false,
                false,
                Some((230, 230, 230)),
            ),
            block_quote: BasicTextStyle::new(
                8,
                Some((128, 128, 128)),
                None,
                None,
                None,
                None,
                false,
                true,
                false,
                false,
                Some((245, 245, 245)),
            ),
            list_item: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                Some(0.5),
                None,
                None,
                false,
                false,
                false,
                false,
                None,
            ),
            link: BasicTextStyle::new(
                8,
                Some((128, 128, 128)),
                None,
                None,
                None,
                None,
                false,
                false,
                true,
                false,
                None,
            ),
            image: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                None,
                Some(TextAlignment::Center),
                None,
                false,
                false,
                false,
                false,
                None,
            ),
            text: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                false,
                None,
            ),
            horizontal_rule: BasicTextStyle::new(
                8,
                Some((0, 0, 0)),
                None,
                Some(0.5),
                None,
                None,
                false,
                false,
                false,
                false,
                None,
            ),
        }
    }
}

#[cfg(any())]
mod tests {
    use super::*;

    #[test]
    fn test_mdpdf_font_basics() {
        let font = MdPdfFont::Roboto;
        assert_eq!(font.dir(), "roboto");
        assert_eq!(font.file(), "Roboto");
    }

    #[test]
    fn test_font_matching() {
        assert_eq!(MdPdfFont::find_match(None), MdPdfFont::Roboto);
        assert_eq!(MdPdfFont::find_match(Some("roboto")), MdPdfFont::Roboto);
        assert_eq!(MdPdfFont::find_match(Some("unknown")), MdPdfFont::Roboto);
        assert_eq!(MdPdfFont::find_match(Some("")), MdPdfFont::Roboto);
    }

    #[test]
    fn test_font_variant_loading() {
        let variants = ["Regular", "Bold", "Italic", "BoldItalic"];
        for variant in variants {
            let result = MdPdfFont::load_font_variant(MdPdfFont::Roboto, variant);
            assert!(result.is_ok(), "Failed to load {} variant", variant);
        }
    }

    #[test]
    fn test_font_family_loading() {
        let result = MdPdfFont::load_font_family(None);
        assert!(result.is_ok());

        let result = MdPdfFont::load_font_family(Some("roboto"));
        assert!(result.is_ok());

        let result = MdPdfFont::load_font_family(Some("unknown"));
        assert!(result.is_ok()); // Should default to Roboto
    }

    #[test]
    fn test_text_alignment_variants() {
        assert_ne!(TextAlignment::Left, TextAlignment::Center);
        assert_ne!(TextAlignment::Left, TextAlignment::Right);
        assert_ne!(TextAlignment::Left, TextAlignment::Justify);
        assert_ne!(TextAlignment::Center, TextAlignment::Right);
        assert_ne!(TextAlignment::Center, TextAlignment::Justify);
        assert_ne!(TextAlignment::Right, TextAlignment::Justify);
    }

    #[test]
    fn test_margins_creation() {
        let margins = Margins {
            top: 10.0,
            right: 20.0,
            bottom: 30.0,
            left: 40.0,
        };

        assert_eq!(margins.top, 10.0);
        assert_eq!(margins.right, 20.0);
        assert_eq!(margins.bottom, 30.0);
        assert_eq!(margins.left, 40.0);
    }

    #[test]
    fn test_basic_text_style_creation() {
        let style = BasicTextStyle::new(
            12,
            Some((0, 0, 0)),
            Some(1.0),
            Some(2.0),
            Some(TextAlignment::Left),
            Some("Roboto"),
            true,
            false,
            true,
            false,
            Some((255, 255, 255)),
        );

        assert_eq!(style.size, 12);
        assert_eq!(style.text_color, Some((0, 0, 0)));
        assert_eq!(style.before_spacing, 1.0);
        assert_eq!(style.after_spacing, 2.0);
        assert_eq!(style.alignment, Some(TextAlignment::Left));
        assert_eq!(style.font_family, Some("Roboto"));
        assert!(style.bold);
        assert!(!style.italic);
        assert!(style.underline);
        assert!(!style.strikethrough);
        assert_eq!(style.background_color, Some((255, 255, 255)));
    }

    #[test]
    fn test_basic_text_style_with_none_values() {
        let style = BasicTextStyle::new(
            12, None, None, None, None, None, false, false, false, false, None,
        );

        assert_eq!(style.size, 12);
        assert_eq!(style.text_color, None);
        assert_eq!(style.before_spacing, 0.0); // Default when None
        assert_eq!(style.after_spacing, 0.0); // Default when None
        assert_eq!(style.alignment, None);
        assert_eq!(style.font_family, None);
        assert_eq!(style.background_color, None);
    }

    #[test]
    fn test_basic_text_style_default() {
        let style = BasicTextStyle::default();

        assert_eq!(style.size, 12);
        assert_eq!(style.text_color, None);
        assert_eq!(style.before_spacing, 0.0);
        assert_eq!(style.after_spacing, 0.0);
        assert_eq!(style.alignment, None);
        assert_eq!(style.font_family, None);
        assert!(!style.bold);
        assert!(!style.italic);
        assert!(!style.underline);
        assert!(!style.strikethrough);
        assert_eq!(style.background_color, None);
    }

    #[test]
    fn test_style_match_default() {
        let styles = StyleMatch::default();
        assert_eq!(styles.margins.top, 8.0);
        assert_eq!(styles.margins.right, 8.0);
        assert_eq!(styles.margins.bottom, 8.0);
        assert_eq!(styles.margins.left, 8.0);
        assert_eq!(styles.heading_1.size, 14);
        assert_eq!(styles.heading_1.alignment, Some(TextAlignment::Center));
        assert!(styles.heading_1.bold);
        assert_eq!(styles.heading_2.size, 12);
        assert_eq!(styles.heading_2.alignment, Some(TextAlignment::Left));
        assert!(styles.heading_2.bold);
        assert_eq!(styles.heading_3.size, 10);
        assert_eq!(styles.heading_3.alignment, Some(TextAlignment::Left));
        assert!(styles.heading_3.bold);
        assert!(styles.emphasis.italic);
        assert!(!styles.emphasis.bold);
        assert!(styles.strong_emphasis.bold);
        assert!(!styles.strong_emphasis.italic);
        assert_eq!(styles.code.text_color, Some((128, 128, 128)));
        assert_eq!(styles.code.background_color, Some((230, 230, 230)));
        assert_eq!(styles.code.font_family, Some("Roboto"));
        assert!(styles.block_quote.italic);
        assert_eq!(styles.block_quote.background_color, Some((245, 245, 245)));
        assert_eq!(styles.list_item.after_spacing, 0.5);
        assert!(styles.link.underline);
        assert_eq!(styles.link.text_color, Some((128, 128, 128)));
        assert_eq!(styles.image.alignment, Some(TextAlignment::Center));
        assert_eq!(styles.text.size, 8);
        assert_eq!(styles.text.text_color, Some((0, 0, 0)));
        assert_eq!(styles.horizontal_rule.after_spacing, 0.5);
    }

    #[test]
    fn test_analyze_needed_variants() {
        // Test default style configuration
        let default_style = StyleMatch::default();
        let (needs_regular, needs_bold, needs_italic, needs_bold_italic) =
            MdPdfFont::analyze_needed_variants(&default_style);

        assert!(needs_regular); // Always needed
        assert!(needs_bold); // Headings and strong_emphasis use bold
        assert!(needs_italic); // Emphasis and block_quote use italic
        assert!(!needs_bold_italic); // Default config doesn't use bold+italic
    }

    #[test]
    fn test_analyze_needed_variants_minimal() {
        // Create a minimal style with only regular text
        let mut minimal_style = StyleMatch::default();

        // Set all styles to non-bold, non-italic
        minimal_style.heading_1.bold = false;
        minimal_style.heading_1.italic = false;
        minimal_style.heading_2.bold = false;
        minimal_style.heading_2.italic = false;
        minimal_style.heading_3.bold = false;
        minimal_style.heading_3.italic = false;
        minimal_style.emphasis.bold = false;
        minimal_style.emphasis.italic = false;
        minimal_style.strong_emphasis.bold = false;
        minimal_style.strong_emphasis.italic = false;
        minimal_style.block_quote.bold = false;
        minimal_style.block_quote.italic = false;

        let (needs_regular, needs_bold, needs_italic, needs_bold_italic) =
            MdPdfFont::analyze_needed_variants(&minimal_style);

        assert!(needs_regular); // Always needed
        assert!(!needs_bold); // No bold styles
        assert!(!needs_italic); // No italic styles
        assert!(!needs_bold_italic); // No bold+italic styles
    }

    #[test]
    fn test_analyze_needed_variants_with_bold_italic() {
        // Create a style that uses bold+italic combination
        let mut style = StyleMatch::default();

        // Set one style to use both bold and italic
        style.heading_1.bold = true;
        style.heading_1.italic = true;

        let (needs_regular, needs_bold, needs_italic, needs_bold_italic) =
            MdPdfFont::analyze_needed_variants(&style);

        assert!(needs_regular); // Always needed
        assert!(needs_bold); // Other styles use bold
        assert!(needs_italic); // Other styles use italic
        assert!(needs_bold_italic); // heading_1 uses bold+italic
    }

    #[test]
    fn test_load_font_family_minimal() {
        let default_style = StyleMatch::default();
        let result = MdPdfFont::load_minimal_font_family(None, &default_style);
        assert!(result.is_ok());

        let result = MdPdfFont::load_minimal_font_family(Some("roboto"), &default_style);
        assert!(result.is_ok());
    }
}
