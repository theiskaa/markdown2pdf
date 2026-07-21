//! Shared read-side plumbing for a capped, deadline-bounded HTTP body
//! read, used by both the library's document-triggered image fetch
//! (`net_guard::fetch_url`) and the CLI's operator-typed `--url`
//! fetch (`src/bin/main.rs`).
//!
//! The binary is a separate crate from this library and can only see
//! its public API, and none of this is API we want to publish just
//! for internal plumbing — so, mirroring the pattern the test suite
//! already uses for `tests/render/common.rs`
//! (`#[path = "render/common.rs"] mod common;`), `src/bin/main.rs`
//! includes this exact file via
//! `#[path = "../lib/render/net_read.rs"] mod net_read;` and compiles
//! it a second time as part of the binary crate. `net_guard.rs` isn't
//! shared the same way: it also carries the SSRF host-block predicates
//! (`ipv4_blocked` and friends), which the CLI's operator-typed fetch
//! deliberately does NOT apply (see the comment at that call site in
//! `main.rs`) — including the whole file would either pull in that
//! guard by accident or leave those items unused-and-warning under
//! `-D warnings` in the binary. This file carries only the read-side
//! plumbing both call sites genuinely share.
#![cfg(feature = "fetch")]

use std::io::Read;
use std::time::Instant;

/// The hard byte ceiling on a fetched response body. Shared by the
/// library's document-triggered image fetch and the CLI's
/// operator-typed `--url` fetch so the two can't quietly drift apart
/// — they used to each hardcode `10 * 1024 * 1024` separately.
pub(crate) const MAX_FETCH_BYTES: u64 = 10 * 1024 * 1024;

/// `Read` adapter enforcing a hard wall-clock deadline across every
/// read call, on top of (not instead of) the client's own idle
/// timeout. reqwest's blocking `.timeout()` behaves as an IDLE
/// timeout while reading a response body — any forward progress
/// resets it — so a server trickling data slower than the size cap
/// but faster than the idle timeout can keep a `read_to_end` alive
/// indefinitely. This wrapper fails the read once wall-clock time
/// passes `deadline`, bounding total transfer time to roughly the
/// deadline plus one more idle-timeout period: the read straddling
/// the deadline can still block up to the idle timeout before this
/// adapter gets a chance to check the clock again. Bounded and
/// honest, not an instantaneous cutoff.
pub(crate) struct DeadlineReader<R> {
    pub(crate) inner: R,
    pub(crate) deadline: Instant,
}

impl<R: Read> Read for DeadlineReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "fetch exceeded total time budget",
            ));
        }
        self.inner.read(buf)
    }
}

/// Read `reader` to completion under a [`DeadlineReader`] wall-clock
/// cutoff, capped one byte past `MAX_FETCH_BYTES` via `Read::take` so
/// an over-size (or `Content-Length`-lying) body is detectable without
/// ever buffering the whole thing. Returns the raw bytes — which may
/// be `MAX_FETCH_BYTES + 1` bytes long — leaving it to the caller to
/// size-check and phrase its own "too big" error message (the
/// library's and the CLI's read the same way but word the error
/// differently).
pub(crate) fn read_capped_with_deadline<R: Read>(
    reader: R,
    deadline: Instant,
) -> Result<Vec<u8>, String> {
    let bounded = DeadlineReader {
        inner: reader,
        deadline,
    };
    let mut limited = bounded.take(MAX_FETCH_BYTES + 1);
    let mut buf = Vec::new();
    limited.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}
