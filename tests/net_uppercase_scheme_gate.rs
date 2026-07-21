//! Process-isolated regression test for case-insensitive URL-scheme
//! detection (`is_http_url`) in the image pipeline.
//!
//! The original version of this test (moved here from
//! `tests/render/image_pipeline.rs`'s `security_image_confinement`
//! module) set `allow_remote_images = false` and pointed at
//! `HTTP://example.invalid/...`, asserting only that the render fell
//! back to alt text. `.invalid` never resolves, so that fallback
//! happens identically whether `HTTP://` was correctly routed down
//! the URL branch and refused by `allow_remote_images`, OR whether a
//! case-sensitive scheme check failed to recognize it as a URL at all
//! and it fell through to the local-file branch, where
//! `std::fs::read("HTTP://example.invalid/...")` fails to find any
//! such file on disk. Both paths degrade to the exact same
//! `[image: …]` text, so the assertion could not tell them apart —
//! proof: making `is_http_url` case-sensitive again left the test
//! green.
//!
//! The property actually worth pinning is "an uppercase-scheme URL is
//! fetched over the network like any other URL", which is only true
//! of the URL branch. So this test uses `allow_remote_images = true`
//! and a real loopback PNG server, and asserts the image WAS embedded
//! (the alt-text fallback must be absent) — something that can only
//! happen if `HTTP://…` was recognized as a URL and actually fetched;
//! the local-file branch has no way to produce a decoded image from
//! that literal, nonexistent path.
//!
//! Reaching the server requires bypassing the separate SSRF
//! host-block guard for loopback via
//! `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1`, which is process-global —
//! see `tests/net_size_cap.rs` for why that means this test needs its
//! own top-level (single-test, process-isolated) integration target
//! rather than living inside `tests/render.rs`.
#![cfg(feature = "fetch")]

#[path = "render/common.rs"]
mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// A minimal but genuinely valid 1x1 PNG — a real decoder would
/// accept it, so an embedded image can only mean the fetch actually
/// happened and decoded successfully.
fn valid_png_bytes() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(1, 1, image::Rgb([10, 200, 90]));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode fixture png");
    buf
}

#[test]
fn uppercase_scheme_url_routes_through_url_fetch_path() {
    // SAFETY: this binary contains exactly one test (this one), so no
    // other thread in this process reads or writes this var
    // concurrently.
    unsafe {
        std::env::set_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK", "1");
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let port = listener.local_addr().expect("local addr").port();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = tx.send(());
            let mut discard = [0u8; 1024];
            let _ = stream.read(&mut discard);
            let body = valid_png_bytes();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    let cfg = "[security]\nallow_remote_images = true\n";
    // Uppercase scheme, mixed-case host — must still be recognized as
    // a URL and fetched, not treated as a (nonexistent) local path.
    let md = format!("![sec upper](HTTP://127.0.0.1:{port}/x.png)\n");
    let bytes = render(&md, cfg);

    assert!(pdf_well_formed(&bytes), "PDF not well-formed");
    // The load-bearing assertion: a real image was embedded, so the
    // alt-text fallback must be ABSENT. This can only be true if
    // `HTTP://…` was routed down the URL branch and actually fetched
    // — the local-file branch has no file at that literal path to
    // read.
    assert!(
        !contains_text(&bytes, "[image: sec upper]"),
        "uppercase-scheme URL must be recognized and fetched, not fall back to alt text \
         (a fallback here means it was treated as a local file path instead of a URL)"
    );
    assert!(
        rx.try_recv().is_ok(),
        "expected the loopback PNG server to observe a connection — \
         the uppercase-scheme URL never reached the network, so it wasn't routed through \
         the URL fetch path"
    );

    // Hygiene: don't leak the escape hatch past this test, in case a
    // future harness or tooling ever reuses this process.
    unsafe {
        std::env::remove_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK");
    }
}
