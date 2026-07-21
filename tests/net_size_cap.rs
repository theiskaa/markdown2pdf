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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

/// Prove the size cap actually bounds the download: a server that
/// never sends `Content-Length` and just keeps writing a body far
/// larger than the 10 MB cap must have the connection cut by the
/// client once the cap is hit, rather than the client reading the
/// stream to completion.
///
/// The body content here (raw zero bytes) is never a decodable image
/// either way, so "falls back to alt text" alone proves nothing — a
/// prior version of this test asserted only that, and stayed green
/// even with `MAX_BYTES` raised to 10 GiB (i.e. with the cap
/// effectively disabled), because the fallback then was just "not a
/// valid image", not "cut off by the cap".
///
/// What actually distinguishes a working cap is how many bytes the
/// SERVER manages to push down the socket before the client hangs up:
/// it counts every byte accepted by `write_all`, and this test asserts
/// that count stays bounded well under the `TARGET_BYTES` the server
/// was willing to send. The target is set to 8x the 10 MB cap
/// specifically so loopback OS socket-buffer slack (which on a local
/// connection can absorb several MB before the writer ever blocks)
/// can't by itself account for the whole thing — with the cap intact,
/// the client stops reading a little past 10 MB and the connection
/// gets torn down (a `write_all` past that point returns an error, or
/// the loop is still short of `TARGET_BYTES` when the join times out);
/// with the cap disabled, the client keeps reading until the server
/// has written the entire `TARGET_BYTES`, so `written` reaches it.
#[test]
fn oversize_streamed_body_is_cut_off_not_buffered() {
    // SAFETY: this binary contains exactly one test (this one), so no
    // other thread in this process reads or writes this var
    // concurrently.
    unsafe {
        std::env::set_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK", "1");
    }

    const TARGET_BYTES: usize = 80 * 1024 * 1024; // 8x the 10 MB cap

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
    let addr = listener.local_addr().expect("local addr");

    let written_bytes = Arc::new(AtomicUsize::new(0));
    let written_bytes_writer = Arc::clone(&written_bytes);
    let (done_tx, done_rx) = mpsc::channel();

    let handle = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Drain the request line/headers minimally; we don't need
            // to parse them, just stop reading and start writing a
            // response with no Content-Length.
            let mut discard = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut discard);

            let header = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n";
            if stream.write_all(header).is_err() {
                let _ = done_tx.send(());
                return;
            }
            // Far more filler than the 10 MB cap. Writes in a loop,
            // counting every byte actually accepted by the socket, so
            // the client cutting the connection early is directly
            // observable rather than inferred.
            let chunk = vec![0u8; 64 * 1024];
            let mut written = 0usize;
            while written < TARGET_BYTES {
                if stream.write_all(&chunk).is_err() {
                    break;
                }
                written += chunk.len();
                written_bytes_writer.store(written, Ordering::SeqCst);
            }
        }
        let _ = done_tx.send(());
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

    // Wait for the server thread to finish writing (or give up once
    // the client has dropped the connection). Bounded so a hung write
    // can't hang the whole test suite; if it times out, `written` is
    // read from whatever the atomic last observed, which is still a
    // valid (and in that case damning) measurement.
    let _ = done_rx.recv_timeout(Duration::from_secs(15));
    let _ = handle.join();

    // The load-bearing assertion: the server must not have been
    // allowed to write anywhere near the full `TARGET_BYTES`. A
    // disabled/raised cap lets the client keep reading to completion,
    // so `written` reaches `TARGET_BYTES`; a working ~10 MB cap cuts
    // the connection well before that.
    let written = written_bytes.load(Ordering::SeqCst);
    assert!(
        written < TARGET_BYTES / 2,
        "server was allowed to write {written} of {TARGET_BYTES} target bytes — \
         the size cap did not cut the connection off"
    );

    // Hygiene: don't leak the escape hatch past this test, in case a
    // future harness or tooling ever reuses this process.
    unsafe {
        std::env::remove_var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK");
    }
}
