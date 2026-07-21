//! Process-isolated regression test for the `[security]
//! allow_remote_images = false` gate.
//!
//! The original version of this test (moved here from
//! `tests/render/image_pipeline.rs`'s `security_image_confinement`
//! module) pointed at `https://example.invalid/...` — `.invalid` is an
//! RFC 2606 reserved TLD that never resolves, so a DNS failure
//! produces exactly the same `[image: …]` fallback that a genuine
//! policy refusal produces. Proof it detected nothing: wrapping the
//! `allow_remote_images` gate in `if false && …` still left the test
//! green, because the fetch would have failed on DNS resolution
//! anyway.
//!
//! To make the guard's absence observable, this binds a real
//! `TcpListener` on loopback serving a valid PNG and asserts the
//! server saw NO connection — a claim that only holds if the
//! `allow_remote_images` check actually short-circuited before any
//! networking happened.
//!
//! Loopback is normally refused by the separate SSRF host-block guard
//! (see `tests/render/net_guard.rs`), which would make "no connection
//! observed" true for the wrong reason (blocked by the host-block
//! guard rather than by `allow_remote_images`) if that guard fired
//! first. It doesn't here — `allow_remote_images` is checked in
//! `decode_image_file` *before* `fetch_url_bytes` (and therefore the
//! host-block guard) is ever called — but to make a neutered
//! `allow_remote_images` gate observably fail this test (rather than
//! silently falling through to get blocked by the host-block guard
//! instead, which would let this test stay green for the wrong
//! reason), `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` is set so that IF
//! the gate under test were removed, the fetch would actually reach
//! the loopback server. That env var is process-global and Rust runs
//! `#[test]` fns as threads sharing one process per test binary, so —
//! exactly as documented in `tests/net_size_cap.rs` — this test lives
//! in its own top-level integration target, alone, for real process
//! isolation.
#![cfg(feature = "fetch")]

#[path = "render/common.rs"]
mod common;
use common::*;

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;

/// A minimal but genuinely valid 1x1 PNG — a real decoder would
/// accept it, so a missing embed can only mean the fetch never
/// happened.
fn valid_png_bytes() -> Vec<u8> {
    let img = image::RgbImage::from_pixel(1, 1, image::Rgb([90, 40, 180]));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode fixture png");
    buf
}

#[test]
fn remote_image_refused_when_allow_remote_images_false() {
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

    let cfg = "[security]\nallow_remote_images = false\n";
    let md = format!("![sec remote](http://127.0.0.1:{port}/should-not-fetch.png)\n");
    let bytes = render(&md, cfg);

    assert!(pdf_well_formed(&bytes), "PDF not well-formed");
    assert!(
        contains_text(&bytes, "[image: sec remote]"),
        "remote image must fall back to alt text when allow_remote_images = false"
    );
    assert!(
        rx.try_recv().is_err(),
        "guard failed to block: the local PNG server observed a connection — \
         the fetch reached the network even though allow_remote_images = false"
    );

    // Hygiene: don't leak the escape hatch past this test, in case a
    // future harness or tooling ever reuses this process.
    unsafe {
        std::env::remove_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK");
    }
}
