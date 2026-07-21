//! Process-isolated regression test for the remote-fetch size cap.
//!
//! Proving the cap actually bounds a streamed download requires a
//! server this process controls, which means a loopback target — and
//! reaching one means bypassing the SSRF guard via
//! `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1`. Env vars are process-global
//! and Rust's default test harness runs every `#[test]` as a thread
//! within the SAME process as every other test in its binary, so
//! setting this here used to silently disable the guard for the
//! `blocked_hosts` tests in `tests/render/net_guard.rs` too, whenever
//! they happened to share a process with this one — instrumented runs
//! showed the blocked-host checks observed the var already set in 6
//! out of 6 runs.
//!
//! Cargo compiles each top-level `tests/*.rs` file as its own test
//! binary (its own OS process), so moving this test into its own
//! top-level file — rather than a `mod` included into
//! `tests/render.rs` — isolates the env var to a process that
//! contains no other tests. That's process isolation, not a
//! mutex/serial-test workaround: there is nothing else in this binary
//! for the var to race against. Do not add sibling tests to this file
//! without re-checking that this isolation still holds.
#![cfg(feature = "fetch")]

#[path = "render/common.rs"]
mod common;
use common::*;

use std::io::Write;
use std::net::TcpListener;

/// Prove the size cap actually bounds the download: a server that
/// never sends `Content-Length` and just keeps writing must still
/// have the connection cut once the 10 MB cap is hit, rather than the
/// client buffering the whole stream.
#[test]
fn oversize_streamed_body_is_cut_off_not_buffered() {
    // SAFETY: this binary contains exactly one test (this one), so no
    // other thread in this process reads or writes this var
    // concurrently.
    unsafe {
        std::env::set_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK", "1");
    }

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");

    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Drain the request line/headers minimally; we don't need
            // to parse them, just stop reading and start writing a
            // response with no Content-Length.
            let mut discard = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut discard);

            let header = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n";
            if stream.write_all(header).is_err() {
                return;
            }
            // 11 MB of filler, well past the 10 MB cap. Writes in a
            // loop so the client can cut the connection once it hits
            // the cap instead of us building an 11 MB buffer up front
            // — but even if the whole thing gets queued by the OS,
            // this thread just exits when the write fails (client
            // dropped the socket).
            let chunk = vec![0u8; 64 * 1024];
            let mut written = 0usize;
            let target = 11 * 1024 * 1024;
            while written < target {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
                written += chunk.len();
            }
        }
    });

    let md = format!(
        "![OVERSIZE](http://{}:{}/big.png)\n",
        addr.ip(),
        addr.port()
    );
    let bytes = render(&md, "");
    assert!(pdf_well_formed(&bytes), "PDF not well-formed");
    assert!(
        contains_text(&bytes, "[image: OVERSIZE]"),
        "oversize body should have been rejected, falling back to alt text"
    );

    // Don't hang the test suite if the server thread is stuck on a
    // write the client never reads from.
    let _ = handle.join();

    // Hygiene: don't leak the escape hatch past this test, in case a
    // future harness or tooling ever reuses this process.
    unsafe {
        std::env::remove_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK");
    }
}
