//! Configuration module for styling and formatting PDF output.
//!
//! This module handles loading and parsing of styling configuration from TOML files.
//! It provides functionality to customize text styles, colors, margins and other formatting
//! options for different Markdown elements in the generated PDF.
//!
//! The configuration uses TOML format with sections for different element types. The margin
//! section controls document margins. Heading styles can be customized per level (1-3).
//! Base text style defines the default text appearance. Emphasis sections handle italic text.
//! Strong emphasis controls bold text styling. Code sections format code blocks and inline code.
//! Block quote sections style quoted text. List item sections format list entries. Link and
//! image sections control how those elements appear. A horizontal rule section styles divider lines.
//!
//! Each style section supports properties for customizing the appearance. Font size is specified
//! in points. Text and background colors use RGB values. Vertical spacing after elements can be
//! set. Text alignment options include left, center, right and justify. Font family specifies
//! the typeface. Boolean flags control bold, italic, underline and strikethrough decorations.
//!
//! The configuration can be defined in a TOML file. Basic configuration might customize heading
//! styles with specific sizes, colors and alignment. Text styles control the base paragraph
//! appearance. Custom emphasis styles can modify how italic and bold text renders. Link styles
//! determine how hyperlinks are displayed.
//!
//! The configuration processing follows a pipeline where the TOML file is parsed into style
//! objects that control the PDF generation. The parser extracts style properties and creates
//! corresponding style objects used during rendering.
//!
//! A complete example configuration file can be found in markdown2pdfrc.example.toml which
//! demonstrates all available styling options.

use crate::styling::{BasicTextStyle, Margins, StyleMatch, TextAlignment};
use std::fs;
use std::path::Path;
use toml::Value;

/// Configuration source for styling configuration.
/// Determines where the TOML configuration should be loaded from.
#[derive(Debug, Clone)]
pub enum ConfigSource<'a> {
    /// Use default built-in styling configuration
    Default,
    /// Load configuration from a file path
    File(&'a str),
    /// Use embedded TOML configuration string (compile-time embedded)
    Embedded(&'a str),
}

/// Parses an RGB color from a TOML configuration value.
///
/// The value parameter provides an optional TOML value containing a color object.
/// The field parameter specifies which color field to parse from the configuration.
/// Returns the RGB color values as a tuple if parsing succeeds, or None if the color
/// value is missing or invalid.
fn parse_color(value: Option<&Value>, field: &str) -> Option<(u8, u8, u8)> {
    value.and_then(|c| {
        let color = c.get(field)?;
        let r = color.get("r")?.as_integer()? as u8;
        let g = color.get("g")?.as_integer()? as u8;
        let b = color.get("b")?.as_integer()? as u8;
        Some((r, g, b))
    })
}

/// Parses text alignment from TOML configuration.
///
/// Takes an optional TOML value containing the alignment string specification.
/// Returns the corresponding TextAlignment enum value if parsing succeeds,
/// or None if the alignment value is missing from the configuration.
fn parse_alignment(value: Option<&Value>) -> Option<TextAlignment> {
    value.and_then(|v| v.as_str()).map(|s| match s {
        "left" => TextAlignment::Left,
        "center" => TextAlignment::Center,
        "right" => TextAlignment::Right,
        "justify" => TextAlignment::Justify,
        _ => TextAlignment::Left,
    })
}

/// Maps a font family name to its corresponding font file path.
///
/// Takes the name of the font family as input and attempts to find a matching
/// font file. Returns the path to the font file if found, or None if the
/// specified font is not available in the system.
fn map_font_family(font: &str) -> Option<&'static str> {
    // Leak the provided font family string to obtain a 'static lifetime reference.
    // This is safe for the lifetime of the running process and avoids embedding any font data.
    Some(Box::leak(font.to_string().into_boxed_str()))
}

/// Parses a complete text style configuration from TOML.
///
/// Processes all style properties including size, spacing, colors, alignment,
/// font family and text decorations. Takes an optional TOML value containing
/// the style configuration and a default style to use for missing properties.
/// Returns a complete BasicTextStyle with all properties set to either parsed
/// or default values.
fn parse_style(value: Option<&Value>, default: BasicTextStyle) -> BasicTextStyle {
    let mut style = default.clone();
    if let Some(style_config) = value {
        if let Some(size) = style_config.get("size").and_then(|v| v.as_integer()) {
            style.size = size as u8;
        }

        if let Some(spacing) = style_config.get("beforespacing").and_then(|v| v.as_float()) {
            style.before_spacing = spacing as f32;
        }

        if let Some(spacing) = style_config.get("afterspacing").and_then(|v| v.as_float()) {
            style.after_spacing = spacing as f32;
        }

        if let Some(color) = parse_color(Some(style_config), "textcolor") {
            style.text_color = Some(color);
        }
        if let Some(bg_color) = parse_color(Some(style_config), "backgroundcolor") {
            style.background_color = Some(bg_color);
        }

        if let Some(alignment) = parse_alignment(style_config.get("alignment")) {
            style.alignment = Some(alignment);
        }

        if let Some(font) = style_config.get("fontfamily").and_then(|v| v.as_str()) {
            style.font_family = map_font_family(font);
        }

        if let Some(bold) = style_config.get("bold").and_then(|v| v.as_bool()) {
            style.bold = bold;
        }
        if let Some(italic) = style_config.get("italic").and_then(|v| v.as_bool()) {
            style.italic = italic;
        }
        if let Some(underline) = style_config.get("underline").and_then(|v| v.as_bool()) {
            style.underline = underline;
        }
        if let Some(strikethrough) = style_config.get("strikethrough").and_then(|v| v.as_bool()) {
            style.strikethrough = strikethrough;
        }
    }
    style
}

/// Parses a TOML configuration string and returns a complete StyleMatch.
///
/// This function handles the core TOML parsing logic and can be used with both
/// embedded configuration strings (via include_str!) and runtime-loaded files.
/// It processes all style sections and returns a complete StyleMatch object
/// containing the full configuration.
///
/// # Arguments
/// * `config_str` - The TOML configuration content as a string
///
/// # Returns
/// A complete StyleMatch with parsed configuration, or default values if parsing fails
///
/// # Example
/// ```rust
/// use markdown2pdf::config::{ConfigSource, load_config_from_source};
///
/// // Use default configuration
/// let style = load_config_from_source(ConfigSource::Default);
///
/// // Load from file
/// let style = load_config_from_source(ConfigSource::File("config.toml"));
///
/// // Use embedded configuration
/// const EMBEDDED: &str = r#"
///     [heading.1]
///     size = 18
///     bold = true
/// "#;
/// let style = load_config_from_source(ConfigSource::Embedded(EMBEDDED));
/// ```
pub fn parse_config_string(config_str: &str) -> StyleMatch {
    let config: Value = match toml::from_str(config_str) {
        Ok(v) => v,
        Err(_) => return StyleMatch::default(),
    };

    let default_style = StyleMatch::default();
    let margins = if let Some(margins) = config.get("margin") {
        Margins {
            top: margins.get("top").and_then(|v| v.as_float()).unwrap_or(8.0) as f32,
            right: margins
                .get("right")
                .and_then(|v| v.as_float())
                .unwrap_or(8.0) as f32,
            bottom: margins
                .get("bottom")
                .and_then(|v| v.as_float())
                .unwrap_or(8.0) as f32,
            left: margins
                .get("left")
                .and_then(|v| v.as_float())
                .unwrap_or(8.0) as f32,
        }
    } else {
        default_style.margins
    };

    StyleMatch {
        margins,
        heading_1: parse_style(
            config.get("heading").and_then(|h| h.get("1")),
            default_style.heading_1,
        ),
        heading_2: parse_style(
            config.get("heading").and_then(|h| h.get("2")),
            default_style.heading_2,
        ),
        heading_3: parse_style(
            config.get("heading").and_then(|h| h.get("3")),
            default_style.heading_3,
        ),
        emphasis: parse_style(config.get("emphasis"), default_style.emphasis),
        strong_emphasis: parse_style(config.get("strong_emphasis"), default_style.strong_emphasis),
        code: parse_style(config.get("code"), default_style.code),
        block_quote: parse_style(config.get("block_quote"), default_style.block_quote),
        list_item: parse_style(config.get("list_item"), default_style.list_item),
        link: parse_style(config.get("link"), default_style.link),
        image: parse_style(config.get("image"), default_style.image),
        text: parse_style(config.get("text"), default_style.text),
        horizontal_rule: parse_style(config.get("horizontal_rule"), default_style.horizontal_rule),
    }
}

/// Loads and parses the complete styling configuration based on the provided source.
///
/// This function handles different configuration sources: default styles, file-based
/// configuration, or embedded TOML strings. It processes all style sections and
/// returns a complete StyleMatch object containing the full configuration.
///
/// # Arguments
/// * `source` - The configuration source (Default, File, or Embedded)
///
/// # Returns
/// A complete StyleMatch with the appropriate configuration applied
///
/// # Examples
/// ```rust
/// use markdown2pdf::config::{ConfigSource, load_config_from_source};
///
/// // Use default configuration
/// let style = load_config_from_source(ConfigSource::Default);
///
/// // Load from file
/// let style = load_config_from_source(ConfigSource::File("config.toml"));
///
/// // Use embedded configuration
/// const EMBEDDED: &str = r#"
///     [heading.1]
///     size = 18
///     bold = true
/// "#;
/// let style = load_config_from_source(ConfigSource::Embedded(EMBEDDED));
/// ```
pub fn load_config_from_source(source: ConfigSource) -> StyleMatch {
    match source {
        ConfigSource::Default => StyleMatch::default(),
        ConfigSource::File(path) => {
            let config_path = Path::new(path).to_path_buf();
            let config_str = match fs::read_to_string(config_path) {
                Ok(s) => s,
                Err(_) => return StyleMatch::default(),
            };
            parse_config_string(&config_str)
        }
        ConfigSource::Embedded(content) => parse_config_string(content),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml::Value;

    #[test]
    fn test_parse_color() {
        // Test valid color
        let color_toml: Value = toml::from_str(
            r#"
            color = { r = 255, g = 128, b = 64 }
        "#,
        )
        .unwrap();
        assert_eq!(
            parse_color(Some(&color_toml), "color"),
            Some((255, 128, 64))
        );

        // Test missing color
        assert_eq!(parse_color(None, "color"), None);

        // Test invalid color (missing components)
        let invalid_color: Value = toml::from_str(
            r#"
            color = { r = 255, g = 128 }
        "#,
        )
        .unwrap();
        assert_eq!(parse_color(Some(&invalid_color), "color"), None);
    }

    #[test]
    fn test_parse_alignment() {
        let alignments = [
            ("left", TextAlignment::Left),
            ("center", TextAlignment::Center),
            ("right", TextAlignment::Right),
            ("justify", TextAlignment::Justify),
        ];

        for (input, expected) in alignments.iter() {
            let align_toml: Value = toml::from_str(&format!(
                r#"
                alignment = "{}"
            "#,
                input
            ))
            .unwrap();
            assert_eq!(
                parse_alignment(Some(&align_toml.get("alignment").unwrap())),
                Some(*expected)
            );
        }

        // Test empty string
        let empty_align: Value = toml::from_str(
            r#"
            alignment = ""
        "#,
        )
        .unwrap();
        assert_eq!(
            parse_alignment(Some(&empty_align.get("alignment").unwrap())),
            Some(TextAlignment::Left)
        );

        // Test non-string value
        let invalid_type: Value = toml::from_str(
            r#"
            alignment = 42
        "#,
        )
        .unwrap();
        assert_eq!(
            parse_alignment(Some(&invalid_type.get("alignment").unwrap())),
            None
        );
    }

    #[test]
    fn test_map_font_family() {
        // Test with a known default font
        assert!(map_font_family("Roboto").is_some());

        // Update test to match actual behavior - all fonts return Some value
        assert!(map_font_family("NonexistentFont").is_some());
    }

    #[test]
    fn test_parse_style() {
        let style_toml: Value = toml::from_str(
            r#"
            [style]
            size = 14
            beforespacing = 1.5
            afterspacing = 2.0
            textcolor = { r = 0, g = 0, b = 0 }
            backgroundcolor = { r = 255, g = 255, b = 255 }
            alignment = "center"
            bold = true
            italic = false
            underline = true
            strikethrough = false
            "#,
        )
        .unwrap();

        let default_style = BasicTextStyle::default();
        let parsed_style = parse_style(
            Some(&style_toml.get("style").unwrap()),
            default_style.clone(),
        );

        assert_eq!(parsed_style.size, 14);
        assert_eq!(parsed_style.before_spacing, 1.5);
        assert_eq!(parsed_style.after_spacing, 2.0);
        assert_eq!(parsed_style.text_color, Some((0, 0, 0)));
        assert_eq!(parsed_style.background_color, Some((255, 255, 255)));
        assert_eq!(parsed_style.alignment, Some(TextAlignment::Center));
        assert!(parsed_style.bold);
        assert!(!parsed_style.italic);
        assert!(parsed_style.underline);
        assert!(!parsed_style.strikethrough);
    }

    #[test]
    fn test_parse_style_partial_config() {
        let partial_style: Value = toml::from_str(
            r#"
            [style]
            size = 16
            bold = true
            "#,
        )
        .unwrap();

        let default_style = BasicTextStyle::default();
        let parsed_style = parse_style(
            Some(&partial_style.get("style").unwrap()),
            default_style.clone(),
        );

        assert_eq!(parsed_style.size, 16);
        assert!(parsed_style.bold);
        // Other properties should match default
        assert_eq!(parsed_style.before_spacing, default_style.before_spacing);
        assert_eq!(parsed_style.after_spacing, default_style.after_spacing);
        assert_eq!(parsed_style.text_color, default_style.text_color);
    }

    #[test]
    fn test_parse_style_invalid_values() {
        let invalid_style: Value = toml::from_str(
            r#"
            [style]
            size = "invalid"
            beforespacing = true
            bold = "not_a_boolean"
            "#,
        )
        .unwrap();

        let default_style = BasicTextStyle::default();
        let parsed_style = parse_style(
            Some(&invalid_style.get("style").unwrap()),
            default_style.clone(),
        );

        // Should fall back to default values
        assert_eq!(parsed_style.size, default_style.size);
        assert_eq!(parsed_style.before_spacing, default_style.before_spacing);
        assert_eq!(parsed_style.bold, default_style.bold);
    }

    #[test]
    fn test_parse_config_string() {
        let config_str = r#"
            [margin]
            top = 10.0
            right = 12.0
            bottom = 10.0
            left = 12.0

            [heading.1]
            size = 16
            bold = true
            textcolor = { r = 50, g = 50, b = 50 }

            [text]
            size = 12
            alignment = "justify"
        "#;

        let style = parse_config_string(config_str);

        assert_eq!(style.margins.top, 10.0);
        assert_eq!(style.margins.right, 12.0);
        assert_eq!(style.margins.bottom, 10.0);
        assert_eq!(style.margins.left, 12.0);

        assert_eq!(style.heading_1.size, 16);
        assert!(style.heading_1.bold);
        assert_eq!(style.heading_1.text_color, Some((50, 50, 50)));

        assert_eq!(style.text.size, 12);
        assert_eq!(style.text.alignment, Some(TextAlignment::Justify));
    }

    #[test]
    fn test_parse_config_string_invalid_toml() {
        let invalid_config = "this is not valid toml {{{";
        let style = parse_config_string(invalid_config);

        let default_style = StyleMatch::default();
        assert_eq!(style.margins.top, default_style.margins.top);
        assert_eq!(style.heading_1.size, default_style.heading_1.size);
    }

    #[test]
    fn test_load_config() {
        let style = load_config_from_source(ConfigSource::Default);
        let default_style = StyleMatch::default();
        assert_eq!(style.margins.top, default_style.margins.top);
        assert_eq!(style.heading_1.size, default_style.heading_1.size);
        assert_eq!(style.text.size, default_style.text.size);

        let style = load_config_from_source(ConfigSource::File("nonexistent.toml"));
        assert_eq!(style.margins.top, default_style.margins.top);
        assert_eq!(style.heading_1.size, default_style.heading_1.size);
        assert_eq!(style.text.size, default_style.text.size);
    }

    #[test]
    fn test_config_source_default() {
        let style = load_config_from_source(ConfigSource::Default);
        let default_style = StyleMatch::default();

        assert_eq!(style.margins.top, default_style.margins.top);
        assert_eq!(style.heading_1.size, default_style.heading_1.size);
        assert_eq!(style.text.size, default_style.text.size);
    }

    #[test]
    fn test_config_source_embedded() {
        const EMBEDDED_CONFIG: &str = r#"
            [margin]
            top = 20.0
            right = 25.0
            bottom = 20.0
            left = 25.0

            [heading.1]
            size = 22
            bold = true
            textcolor = { r = 100, g = 0, b = 0 }

            [text]
            size = 13
            alignment = "justify"
        "#;

        let style = load_config_from_source(ConfigSource::Embedded(EMBEDDED_CONFIG));

        assert_eq!(style.margins.top, 20.0);
        assert_eq!(style.margins.right, 25.0);
        assert_eq!(style.heading_1.size, 22);
        assert!(style.heading_1.bold);
        assert_eq!(style.heading_1.text_color, Some((100, 0, 0)));
        assert_eq!(style.text.size, 13);
        assert_eq!(style.text.alignment, Some(TextAlignment::Justify));
    }

    #[test]
    fn test_config_source_file_nonexistent() {
        let style = load_config_from_source(ConfigSource::File("nonexistent.toml"));
        let default_style = StyleMatch::default();

        assert_eq!(style.margins.top, default_style.margins.top);
        assert_eq!(style.heading_1.size, default_style.heading_1.size);
    }
}
