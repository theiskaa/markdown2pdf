//! Image-pipeline edge cases (W7e). Generates real raster fixtures
//! at test time (via the `image` dev-dep) and feeds them through the
//! renderer to assert: valid output, graceful fallback on bad input,
//! decoded-dimension bounding, alpha/format handling never panics.

use super::common::*;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use std::io::Cursor;

/// Write `img` to a temp file in `fmt`, return the path string.
fn write_temp(img: &DynamicImage, fmt: ImageFormat, name: &str) -> String {
    let dir = std::env::temp_dir();
    let ext = match fmt {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        _ => "img",
    };
    let path = dir.join(format!("m2p_w7e_{}.{}", name, ext));
    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), fmt)
        .expect("encode test image");
    std::fs::write(&path, buf).expect("write test image");
    path.to_string_lossy().to_string()
}

fn render_md(md: &str) -> Vec<u8> {
    render(md, "")
}

mod valid_images {
    use super::*;

    #[test]
    fn small_rgb_png_renders() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            32,
            32,
            image::Rgb([10, 120, 200]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "small_rgb");
        let bytes = render_md(&format!("![blue square]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        // A real image becomes an XObject — the alt-text fallback
        // (`[image: blue square]`) must NOT appear.
        assert!(
            !contains(&bytes, b"[image: blue square]"),
            "valid PNG fell back to alt text"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn small_jpeg_renders() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            48,
            24,
            image::Rgb([200, 30, 30]),
        ));
        let p = write_temp(&img, ImageFormat::Jpeg, "small_jpeg");
        let bytes = render_md(&format!("![red]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        assert!(!contains(&bytes, b"[image: red]"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn rgba_png_with_transparency_does_not_crash() {
        let mut rgba = RgbaImage::new(40, 40);
        for (x, _y, px) in rgba.enumerate_pixels_mut() {
            // Left half opaque green, right half fully transparent.
            *px = if x < 20 {
                image::Rgba([0, 200, 0, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            };
        }
        let img = DynamicImage::ImageRgba8(rgba);
        let p = write_temp(&img, ImageFormat::Png, "rgba");
        let bytes = render_md(&format!("![alpha]({})\n", p));
        assert!(
            pdf_well_formed(&bytes),
            "RGBA PNG with transparency broke the PDF"
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn grayscale_png_does_not_crash() {
        let img = DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            30,
            30,
            image::Luma([128]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "gray");
        let bytes = render_md(&format!("![gray]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        let _ = std::fs::remove_file(&p);
    }
}

mod degenerate_and_hostile {
    use super::*;

    #[test]
    fn one_by_one_pixel_image() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            1,
            1,
            image::Rgb([255, 255, 255]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "one_px");
        let bytes = render_md(&format!("![dot]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn corrupt_png_bytes_fall_back_to_alt() {
        let dir = std::env::temp_dir();
        let path = dir.join("m2p_w7e_corrupt.png");
        // Valid PNG signature, garbage body — decoder must error.
        let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend(std::iter::repeat(0xAB).take(200));
        std::fs::write(&path, &bytes).unwrap();
        let pdf = render_md(&format!(
            "![broken image]({})\n",
            path.to_string_lossy()
        ));
        assert!(pdf_well_formed(&pdf));
        assert!(
            contains(&pdf, b"broken image") || contains_text(&pdf, "broken image"),
            "corrupt image should fall back to its alt text"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn truncated_jpeg_does_not_crash() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            64,
            64,
            image::Rgb([1, 2, 3]),
        ));
        let mut buf = Vec::new();
        img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Jpeg)
            .unwrap();
        buf.truncate(buf.len() / 2); // cut the file in half
        let path = std::env::temp_dir().join("m2p_w7e_trunc.jpg");
        std::fs::write(&path, &buf).unwrap();
        let pdf = render_md(&format!("![t]({})\n", path.to_string_lossy()));
        assert!(pdf_well_formed(&pdf));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn zero_byte_file_falls_back() {
        let path = std::env::temp_dir().join("m2p_w7e_empty.png");
        std::fs::write(&path, []).unwrap();
        let pdf = render_md(&format!(
            "![empty file]({})\n",
            path.to_string_lossy()
        ));
        assert!(pdf_well_formed(&pdf));
        assert!(
            contains(&pdf, b"empty file") || contains_text(&pdf, "empty file"),
            "0-byte image should fall back to alt text"
        );
        let _ = std::fs::remove_file(&path);
    }
}

mod dimension_bounding {
    use super::*;

    #[test]
    fn oversized_image_is_downscaled_not_rejected() {
        // 6000x100 exceeds the 4000px ceiling. It must still render
        // (downscaled), NOT fall back to alt text, and the resulting
        // PDF must stay bounded — well under what a 6000px-wide raw
        // raster would produce.
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            6000,
            100,
            image::Rgb([90, 90, 90]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "wide");
        let bytes = render_md(&format!("![huge]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        assert!(
            !contains(&bytes, b"[image: huge]"),
            "oversized image must downscale, not fall back"
        );
        // A 6000x100 RGB raster is ~1.8MB raw; downscaled to ≤4000px
        // wide the embedded image (+ PDF overhead) should be well
        // under 4MB. Generous ceiling — the point is it's bounded.
        assert!(
            bytes.len() < 4_000_000,
            "downscaled-image PDF unexpectedly large: {} bytes",
            bytes.len()
        );
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn tall_oversized_image_downscaled() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            80,
            5000,
            image::Rgb([12, 34, 56]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "tall");
        let bytes = render_md(&format!("![tall]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        assert!(!contains(&bytes, b"[image: tall]"));
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn image_exactly_at_ceiling_renders() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            4000,
            10,
            image::Rgb([7, 7, 7]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "at_ceiling");
        let bytes = render_md(&format!("![edge]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        let _ = std::fs::remove_file(&p);
    }
}

mod html_img_paths {
    use super::*;

    #[test]
    fn html_img_with_real_local_file_renders() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            20,
            20,
            image::Rgb([0, 0, 0]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "html_local");
        let bytes = render_md(&format!("<img src=\"{}\" alt=\"x\">\n", p));
        assert!(pdf_well_formed(&bytes));
        assert!(!contains(&bytes, b"[image: x]"));
        let _ = std::fs::remove_file(&p);
    }
}

/// `[security]` image-confinement policy (Plan 005). The defaults
/// (`image_root` unset, `allow_absolute_image_paths = true`,
/// `allow_remote_images = true`) preserve the historical, unconfined
/// behavior — the `default_config_still_embeds_image` case below is
/// the backward-compatibility regression guard. The other cases prove
/// the guard actually refuses reads it should refuse: each places a
/// real, decodable image *outside* the configured root and asserts it
/// is NOT embedded (rather than only asserting the render succeeds),
/// so a neutered guard would fail these tests.
mod security_image_confinement {
    use super::*;

    #[test]
    fn absolute_path_outside_image_root_falls_back_to_alt_text() {
        // A real PNG that decodes fine on its own, but lives outside
        // the configured root.
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            16,
            16,
            image::Rgb([50, 60, 70]),
        ));
        let outside = write_temp(&img, ImageFormat::Png, "sec_outside");

        // Root is an unrelated, empty temp directory that does not
        // contain `outside`.
        let root = std::env::temp_dir().join(format!(
            "m2p_sec_root_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();

        let cfg = format!(
            "[security]\nimage_root = \"{}\"\n",
            root.to_string_lossy().replace('\\', "\\\\")
        );
        let md = format!("![sec outside]({})\n", outside);
        let cfg_owned = markdown2pdf::config::ConfigSource::Embedded(&cfg);
        let font_cfg = markdown2pdf::fonts::FontConfig::new()
            .with_default_font_source(markdown2pdf::fonts::FontSource::Builtin("Helvetica"));
        let bytes = markdown2pdf::parse_into_bytes(md, cfg_owned, Some(&font_cfg))
            .expect("render must succeed even when the image is refused");

        assert!(pdf_well_formed(&bytes));
        // The load-bearing assertion: with the guard doing its job the
        // image must NOT be embedded, so the alt-text placeholder must
        // appear instead. A neutered guard would embed the image and
        // fail this.
        assert!(
            contains_text(&bytes, "[image: sec outside]"),
            "image outside image_root must fall back to alt text, not embed"
        );

        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn default_config_still_embeds_image() {
        // No `[security]` block at all — proves the defaults preserve
        // historical (unconfined) behavior. Same shape as the
        // pre-existing `valid_images::small_rgb_png_renders` test.
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(
            16,
            16,
            image::Rgb([10, 20, 30]),
        ));
        let p = write_temp(&img, ImageFormat::Png, "sec_default_ok");
        let bytes = render_md(&format!("![sec default]({})\n", p));
        assert!(pdf_well_formed(&bytes));
        assert!(
            !contains(&bytes, b"[image: sec default]"),
            "default config (no [security] block) must still embed local images"
        );
        let _ = std::fs::remove_file(&p);
    }

    // `remote_image_falls_back_when_allow_remote_images_false` and
    // `uppercase_scheme_url_is_treated_as_url_and_refused` used to
    // live here, both pointed at `https://example.invalid/...` /
    // `HTTP://example.invalid/...`. `.invalid` (RFC 2606) never
    // resolves, so a DNS failure produces the exact same `[image: …]`
    // fallback a genuine policy refusal does — neither test could
    // tell "refused by the guard" apart from "failed to resolve
    // anyway" (proof: wrapping the `allow_remote_images` gate in
    // `if false && …` left both green). They needed a real,
    // controllable network endpoint to observe the difference, which
    // in turn needs `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` (loopback
    // is otherwise blocked by the separate SSRF host-block guard) —
    // a process-global env var unsafe to set from a test sharing this
    // binary with the rest of `tests/render.rs`. They now live as
    // their own process-isolated integration targets:
    // `tests/net_remote_image_gate.rs` and
    // `tests/net_uppercase_scheme_gate.rs`, mirroring the isolation
    // `tests/net_size_cap.rs` already established for the same
    // reason.
}

/// Every "image not shown" path must emit the same italic
/// `[image: ALT]` placeholder so readers can spot at-a-glance which
/// inline glyphs stood in for an image — regardless of whether the
/// failure was a missing local file, an unreachable URL, or an inline
/// image inside a list / admonition / blockquote / table cell.
///
/// The italic-ness itself is verified by the before/after visual
/// diff committed alongside the fix; the text-level assertion here
/// pins the wrapper-and-format invariant for the long haul.
mod fallback_consistency {
    use super::*;

    /// `<context-description>, <markdown source containing the
    /// failing image>` pairs. Each source must contain exactly one
    /// `[image: NEEDLE]` after the fix.
    fn cases() -> &'static [(&'static str, &'static str, &'static str)] {
        &[
            (
                "top-level standalone missing local",
                "![NEEDLE_TOP_LOCAL](does-not-exist-tl.png)\n",
                "NEEDLE_TOP_LOCAL",
            ),
            (
                "top-level standalone unreachable URL",
                "![NEEDLE_TOP_URL](https://example.invalid/missing.png)\n",
                "NEEDLE_TOP_URL",
            ),
            (
                "top-level inline (mixed with text)",
                "Prose with ![NEEDLE_INLINE](does-not-exist-i.png) inline.\n",
                "NEEDLE_INLINE",
            ),
            (
                "inside a list item",
                "- bullet with ![NEEDLE_LIST](does-not-exist-l.png) inline.\n",
                "NEEDLE_LIST",
            ),
            (
                "inside an admonition body",
                "> [!NOTE]\n> note with ![NEEDLE_ADMO](does-not-exist-a.png) inline.\n",
                "NEEDLE_ADMO",
            ),
            (
                "inside a blockquote",
                "> quote with ![NEEDLE_BQUOTE](does-not-exist-b.png) inline.\n",
                "NEEDLE_BQUOTE",
            ),
            (
                "inside a table cell",
                "| L | R |\n| - | - |\n| ![NEEDLE_TABLE](does-not-exist-t.png) | plain |\n",
                "NEEDLE_TABLE",
            ),
        ]
    }

    #[test]
    fn every_fallback_context_emits_italic_wrapper() {
        for (label, md, needle) in cases() {
            let bytes = render_md(md);
            assert!(pdf_well_formed(&bytes), "{label}: PDF malformed");
            let wrapped = format!("[image: {needle}]");
            assert!(
                contains_text(&bytes, &wrapped),
                "{label}: missing `{wrapped}` wrapper — fallback is inconsistent across contexts"
            );
            // Negative: the bare alt must NOT appear *unwrapped*
            // anywhere outside the wrapper. The wrapper contains the
            // needle, so we strip every wrapped occurrence and assert
            // no naked needle remains.
            let scanned = String::from_utf8_lossy(&scan(&bytes)).to_string();
            let stripped = scanned.replace(&wrapped, "");
            assert!(
                !stripped.contains(needle),
                "{label}: bare `{needle}` appears outside the `[image: …]` wrapper — fallback isn't using the shared format"
            );
        }
    }

    /// Image with empty alt text emits nothing visible — `[image: ]`
    /// would be uglier than skipping. Mirrors `render_image_fallback`'s
    /// same-case behavior so block and inline paths agree.
    #[test]
    fn empty_alt_image_renders_no_wrapper() {
        let md = "Prose with ![](does-not-exist-empty.png) here.\n";
        let bytes = render_md(md);
        assert!(pdf_well_formed(&bytes));
        let scanned = String::from_utf8_lossy(&scan(&bytes)).to_string();
        assert!(
            !scanned.contains("[image: "),
            "empty-alt image must not emit an `[image: ]` placeholder"
        );
    }
}
