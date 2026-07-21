//! Network-fetch hardening (Plan 002): the remote-image path must
//! refuse SSRF-shaped hosts and must never buffer an unbounded body.
//!
//! The host-policy predicates themselves (`ipv4_blocked`,
//! `ipv6_blocked`, `url_host_allowed`, `check_url_host`) are private
//! to the library and covered directly by unit tests inside
//! `src/lib/render/layout.rs`'s `tests::host_policy` module. This
//! file exercises the guard *behaviorally*, through the public
//! `render` entry point.
//!
//! The size-cap regression test used to live here too. It needed
//! `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` to reach a loopback test
//! server, which is process-global and therefore unsafe to set from a
//! test that shares a process with the `blocked_hosts` tests below —
//! it has since moved to its own top-level integration target,
//! `tests/net_size_cap.rs`, which Cargo compiles as a separate
//! process for exactly that isolation.
#![cfg(feature = "fetch")]

use super::common::*;

/// A document whose image URL points at a blocked host must still
/// render successfully and degrade to the `[image: alt]` fallback
/// rather than hang, panic, or attempt a connection.
///
/// Critically, "falls back to alt text" is not by itself proof the
/// guard did anything — a connection that merely fails (nothing
/// listening, `ECONNREFUSED`, a network-unreachable link-local
/// address) degrades exactly the same way. A prior version of this
/// file pointed at unreachable ports and could not tell "blocked by
/// the guard" apart from "connection failed anyway" — deleting the
/// guard call entirely left every one of those tests green.
///
/// To make that failure mode impossible, the tests that *can* stand
/// up a controllable endpoint (loopback and `localhost`, which this
/// process can always bind) run a real HTTP server on that exact
/// host/port that serves a real, valid PNG. If the guard is bypassed,
/// the fetch SUCCEEDS, the image embeds, and `contains_text` for the
/// alt-text fallback fails — a loud, specific failure instead of a
/// silently-passing no-op. Each of those tests additionally confirms
/// the server observed no connection at all, proving the block
/// happens at the guard rather than downstream of a connection
/// attempt.
///
/// The remaining two cases (a link-local cloud-metadata address, and
/// the `file://` scheme) have no controllable TCP endpoint available
/// in a test sandbox — `169.254.169.254` generally isn't bindable
/// without owning that interface, and `file://` isn't a TCP fetch at
/// all — so they stay behavioral-only, refused by the same
/// scheme/address checks proven observable in the loopback cases.
mod blocked_hosts {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A minimal but genuinely valid 1x1 PNG, encoded via the `image`
    /// crate (already a dev-dependency) rather than hand-rolled bytes
    /// — the point is that a real decoder would accept it, so a
    /// missing embed can only mean the fetch never happened.
    fn valid_png_bytes() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([220, 20, 60]));
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .expect("encode fixture png");
        buf
    }

    /// Spawn a one-shot HTTP server on `listener` that serves a real
    /// PNG to whatever connects, and signal `tx` the moment it
    /// accepts a connection — so the caller can later assert that
    /// signal never fired.
    fn spawn_png_server(listener: TcpListener, tx: mpsc::Sender<()>) {
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
    }

    /// Render `md` and assert it degraded to the `[image: needle]`
    /// fallback. When `conn_rx` is given, also assert the paired
    /// server never observed a connection — the guard must have
    /// refused *before* dialing out, not merely failed to make sense
    /// of a response.
    fn assert_blocked(md: &str, needle: &str, conn_rx: Option<&mpsc::Receiver<()>>) {
        let bytes = render(md, "");
        assert!(pdf_well_formed(&bytes), "PDF not well-formed");
        let wrapped = format!("[image: {needle}]");
        assert!(
            contains_text(&bytes, &wrapped),
            "expected fallback `{wrapped}` — guard should have blocked the fetch \
             (if this fails, the guard let a real image through)"
        );
        if let Some(rx) = conn_rx {
            assert!(
                rx.try_recv().is_err(),
                "guard failed to block: the local PNG server observed a connection \
                 for `{needle}` — the fetch reached the network"
            );
        }
    }

    #[test]
    fn loopback_ipv4() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();
        spawn_png_server(listener, tx);

        let md = format!("![LOOPBACK_V4](http://127.0.0.1:{port}/x.png)\n");
        assert_blocked(&md, "LOOPBACK_V4", Some(&rx));
    }

    #[test]
    fn localhost_hostname() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local listener");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();
        spawn_png_server(listener, tx);

        // `localhost` is blocked by literal-string match before any
        // resolution happens, so it never actually needs to reach
        // 127.0.0.1 — but binding the real server here still proves
        // that IF it somehow got through, we would have caught it.
        let md = format!("![LOCALHOST](http://localhost:{port}/x.png)\n");
        assert_blocked(&md, "LOCALHOST", Some(&rx));
    }

    #[test]
    fn loopback_ipv6() {
        // Some sandboxes disable IPv6 loopback binding; skip cleanly
        // rather than fail on an environment limitation unrelated to
        // the guard under test.
        let Ok(listener) = TcpListener::bind("[::1]:0") else {
            eprintln!("skipping loopback_ipv6: could not bind [::1]:0 in this environment");
            return;
        };
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();
        spawn_png_server(listener, tx);

        let md = format!("![LOOPBACK_V6](http://[::1]:{port}/x.png)\n");
        assert_blocked(&md, "LOOPBACK_V6", Some(&rx));
    }

    #[test]
    fn cloud_metadata_address() {
        // 169.254.169.254 is link-local; this process cannot bind a
        // controllable listener there, so there is no connection
        // signal to assert on. Refused by the same resolved-address
        // check exercised (with a real listener) in `loopback_ipv4`.
        assert_blocked(
            "![METADATA](http://169.254.169.254/latest/meta-data/)\n",
            "METADATA",
            None,
        );
    }

    #[test]
    fn file_scheme_is_blocked() {
        // Not a TCP fetch at all — refused by the scheme check before
        // any host/address logic runs.
        assert_blocked("![FILESCHEME](file:///etc/hostname)\n", "FILESCHEME", None);
    }
}
