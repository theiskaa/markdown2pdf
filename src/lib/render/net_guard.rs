//! SSRF-hardened host policy and the actual HTTP fetch for
//! document-triggered remote images. Everything here is pure policy
//! and networking with zero dependency on `Engine`, layout state, or
//! PDF drawing — `layout::Engine::fetch_url_bytes` is a thin
//! cache-then-delegate wrapper around [`fetch_url`].
//!
//! The read-side plumbing (`DeadlineReader`, the capped-read helper,
//! and the shared byte cap) lives in the sibling `net_read` module
//! instead of here, because it's also shared with the CLI's
//! `--url` fetch in `src/bin/main.rs` — see that module's doc comment
//! for why the sharing boundary is drawn there rather than around
//! this whole file.
#![cfg(feature = "fetch")]

use super::net_read::{read_capped_with_deadline, MAX_FETCH_BYTES};

/// Is this IPv4 address one we refuse to let the renderer connect to?
/// Loopback, private (RFC 1918), link-local, unspecified, broadcast,
/// and carrier-grade NAT (`100.64.0.0/10`, RFC 6598 — widely used by
/// cloud providers, Kubernetes, and NAT gateways as an internal
/// range) are all blocked — none of these should ever be the target
/// of a document-triggered fetch.
fn ipv4_blocked(ip: &std::net::Ipv4Addr) -> bool {
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || is_cgnat(ip)
}

/// `100.64.0.0/10`: second octet's top two bits are `01` (64..=127).
fn is_cgnat(ip: &std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (o[1] & 0b1100_0000) == 0b0100_0000
}

/// Is this IPv6 address one we refuse to let the renderer connect to?
/// Also unwraps embedded IPv4 addresses and re-checks them with
/// `ipv4_blocked` — an IPv6 address is not a different address space,
/// it's a different envelope, and several of those envelopes are
/// well-known SSRF bypasses: IPv4-compatible (`::a.b.c.d`) and
/// IPv4-mapped (`::ffff:a.b.c.d`) addresses via `to_ipv4()`, and the
/// NAT64 well-known prefix `64:ff9b::/96` (RFC 6052), which is a
/// genuine route to internal IPv4 targets on an IPv6-only network
/// behind a NAT64/DNS64 gateway.
fn ipv6_blocked(ip: &std::net::Ipv6Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() {
        return true;
    }
    // Covers both the IPv4-compatible and IPv4-mapped forms (unlike
    // `to_ipv4_mapped`, which only covers the latter).
    if let Some(v4) = ip.to_ipv4() {
        return ipv4_blocked(&v4);
    }
    let seg = ip.segments();
    if seg[0] == 0x0064 && seg[1] == 0xff9b && seg[2] == 0 && seg[3] == 0 && seg[4] == 0 && seg[5] == 0
    {
        let v4 = std::net::Ipv4Addr::new(
            (seg[6] >> 8) as u8,
            (seg[6] & 0xff) as u8,
            (seg[7] >> 8) as u8,
            (seg[7] & 0xff) as u8,
        );
        return ipv4_blocked(&v4);
    }
    // fc00::/7 unique-local and fe80::/10 link-local. `is_unique_local`
    // and `is_unicast_link_local` are still unstable on our MSRV (1.85),
    // so the prefixes are matched by hand.
    (seg[0] & 0xfe00) == 0xfc00 || (seg[0] & 0xffc0) == 0xfe80
}

fn socket_addr_blocked(sa: &std::net::SocketAddr) -> bool {
    match sa.ip() {
        std::net::IpAddr::V4(v4) => ipv4_blocked(&v4),
        std::net::IpAddr::V6(v6) => ipv6_blocked(&v6),
    }
}

/// Outcome of [`check_url_host`]: either the private-network escape
/// hatch is set (no resolution attempted, nothing to pin), or the
/// host resolved and every one of its addresses passed the block
/// list — in which case `addr` is the specific address the connection
/// should be pinned to via `ClientBuilder::resolve`, and `host` is the
/// bracket-stripped hostname to pin it under.
#[derive(Debug)]
enum HostDecision {
    Skipped,
    Pinned {
        host: String,
        addr: std::net::SocketAddr,
    },
}

/// Decide whether a URL is safe for the renderer to fetch on behalf
/// of an untrusted document, resolving the host and validating the
/// addresses it actually resolves to (not just its literal text) —
/// blocking non-http(s) schemes and any resolved address that is
/// loopback, private, link-local, CGNAT, or otherwise internal, which
/// covers the classic SSRF targets: cloud metadata endpoints
/// (`169.254.169.254`, including hostnames like
/// `metadata.google.internal` that merely resolve there),
/// `localhost`, and IPv6/NAT64 embeddings of the same.
///
/// EVERY address the host resolves to is checked — not just the
/// first — and the whole URL is refused if any one of them is
/// blocked, since which address a later connection actually uses is
/// not under our control.
///
/// Set `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` to skip host resolution
/// and all address blocking entirely (intranet users rendering
/// trusted documents). The scheme check still applies even with the
/// escape hatch set.
///
/// Known limitation: this closes the DNS-rebinding window for the
/// request that pins to the address returned here (see
/// `fetch_url`), but a redirect hop that gets re-validated by
/// this same function cannot also be pinned — reqwest resolves and
/// connects to that hop itself, after the fact. A TOCTOU window
/// (the validated address changing before reqwest connects) remains
/// on every hop past the first; it is not closed by this change, only
/// bounded, by the redirect-count cap.
fn check_url_host(url: &str) -> Result<HostDecision, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid url {}: {}", url, e))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme `{}` is not allowed for fetches", other)),
    }

    if std::env::var("MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK").as_deref() == Ok("1") {
        return Ok(HostDecision::Skipped);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| format!("url {} has no host", url))?;

    if host.eq_ignore_ascii_case("localhost") || host.to_ascii_lowercase().ends_with(".localhost")
    {
        return Err(format!("host `{}` is blocked (loopback)", host));
    }

    // `host_str()` serializes an IPv6 literal wrapped in its `[...]`
    // brackets (e.g. `[::1]`), which `ToSocketAddrs` does not accept
    // as a bare host — strip them before resolving.
    let bare_host = host
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(host);

    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| format!("url {} has no resolvable port", url))?;

    use std::net::ToSocketAddrs;
    let addrs: Vec<std::net::SocketAddr> = (bare_host, port)
        .to_socket_addrs()
        .map_err(|e| format!("could not resolve host `{}`: {}", host, e))?
        .collect();
    if addrs.is_empty() {
        return Err(format!("host `{}` resolved to no addresses", host));
    }
    for sa in &addrs {
        if socket_addr_blocked(sa) {
            return Err(format!(
                "host `{}` resolves to blocked address `{}`",
                host,
                sa.ip()
            ));
        }
    }

    Ok(HostDecision::Pinned {
        host: bare_host.to_string(),
        addr: addrs[0],
    })
}

/// `check_url_host`, discarding the resolved-address pin. Used by the
/// redirect policy (each hop only needs a follow/refuse decision —
/// pinning a redirect target isn't possible, see `check_url_host`'s
/// doc comment) and by anything that just needs a yes/no answer.
fn url_host_allowed(url: &str) -> Result<(), String> {
    check_url_host(url).map(|_| ())
}

/// Fetch a remote URL's bytes, guarded against SSRF: non-http(s)
/// schemes are rejected, the host is resolved via `ToSocketAddrs`,
/// and EVERY resolved address is checked against the loopback /
/// private / link-local / CGNAT / NAT64 block list (see
/// `check_url_host`) before the connection is pinned — with
/// `ClientBuilder::resolve` — to the exact address that was
/// validated, closing the DNS-rebinding window on this initial
/// request. Each redirect hop is re-resolved and re-validated the
/// same way by the redirect policy below, but cannot be pinned
/// (reqwest resolves and connects to an approved hop itself, after
/// the policy returns); a residual TOCTOU window remains on hops
/// after the first, bounded by `MAX_REDIRECTS`. Set
/// `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` to skip all of the above
/// (scheme check still applies).
///
/// Hard caps: a streamed `MAX_FETCH_BYTES` body limit (read one byte
/// past the cap so an over-size or `Content-Length`-lying body is
/// caught without ever being fully buffered) and a total
/// transfer-time budget of roughly `TIMEOUT_SECS` plus one more
/// idle-timeout period — see [`super::net_read::DeadlineReader`]'s
/// doc comment for why reqwest's own `.timeout()` doesn't already
/// bound that.
///
/// `TIMEOUT_SECS` is 5s here because this fetch is document-triggered
/// — a markdown author's image reference, not something the operator
/// running the renderer typed — so an untrusted document can only
/// hold up rendering for a short, fixed budget. The CLI's `--url`
/// fetch (`src/bin/main.rs`) uses a longer 15s budget for the
/// opposite reason: that URL is operator-typed, so tolerating a
/// slower deliberately-chosen endpoint is worth more than CLI
/// responsiveness. See the comment at that call site too.
pub(crate) fn fetch_url(url: &str) -> Result<Vec<u8>, String> {
    const TIMEOUT_SECS: u64 = 5;
    // `attempt.previous()` includes the URL that triggered each
    // redirect, with the ORIGINAL request URL as its first entry
    // — so once `previous().len()` reaches MAX_REDIRECTS,
    // MAX_REDIRECTS - 1 redirect hops have already been followed.
    // With MAX_REDIRECTS = 3 that's 2 hops actually followed, not
    // 3 (verified). Safe direction — more restrictive than the
    // number suggests — so this comment is what changed, not the
    // enforced count.
    const MAX_REDIRECTS: usize = 3;

    let decision = check_url_host(url)?;

    let mut builder = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            // Every hop is re-validated against the same host
            // policy as the initial request — otherwise a
            // server we approved could 302 us straight at
            // `169.254.169.254` or `localhost`. See
            // `check_url_host`'s doc comment for the residual
            // TOCTOU this can't close on redirect hops.
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.stop();
            }
            match url_host_allowed(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(e) => attempt.error(e),
            }
        }));
    if let HostDecision::Pinned { host, addr } = &decision {
        builder = builder.resolve(host, *addr);
    }
    let client = builder.build().map_err(|e| format!("http client init: {}", e))?;
    let resp = client.get(url).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    // Hard wall-clock deadline across the whole body read — see the
    // fn doc comment for why reqwest's own timeout doesn't already do
    // this. Read one byte past the cap so an over-size body is
    // detectable without ever buffering the whole thing.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(TIMEOUT_SECS);
    let buf = read_capped_with_deadline(resp, deadline)?;
    if buf.len() as u64 > MAX_FETCH_BYTES {
        return Err(format!(
            "image at {} exceeds the {} byte cap",
            url, MAX_FETCH_BYTES
        ));
    }
    Ok(buf)
}

// Host-policy predicates backing `fetch_url`'s SSRF guard. Direct
// unit tests on the pure predicates, since the behavioral
// (render-degrades-to-alt-text) coverage lives in
// `tests/render/net_guard.rs`.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_blocked_cases() {
        for addr in [
            "127.0.0.1",
            "10.0.0.1",
            "192.168.1.1",
            "169.254.169.254",
            // CGNAT (RFC 6598) — the second octet spans 64..=127.
            "100.64.0.1",
            "100.100.100.100",
            "100.127.255.255",
        ] {
            let ip: std::net::Ipv4Addr = addr.parse().unwrap();
            assert!(ipv4_blocked(&ip), "{} should be blocked", addr);
        }
    }

    #[test]
    fn ipv4_cgnat_boundary_not_over_blocked() {
        // 100.63.x.x and 100.128.x.x sit just outside 100.64.0.0/10
        // and must stay allowed — a shifted mask would silently
        // widen the block to unrelated public-ish space.
        for addr in ["100.63.255.255", "100.128.0.0"] {
            let ip: std::net::Ipv4Addr = addr.parse().unwrap();
            assert!(!ipv4_blocked(&ip), "{} should not be blocked", addr);
        }
    }

    #[test]
    fn ipv4_allowed_case() {
        let ip: std::net::Ipv4Addr = "93.184.216.34".parse().unwrap();
        assert!(!ipv4_blocked(&ip), "public ipv4 should be allowed");
    }

    #[test]
    fn ipv6_blocked_cases() {
        for addr in ["::1", "fe80::1", "fc00::1"] {
            let ip: std::net::Ipv6Addr = addr.parse().unwrap();
            assert!(ipv6_blocked(&ip), "{} should be blocked", addr);
        }
    }

    #[test]
    fn ipv6_mapped_ipv4_uses_ipv4_rules() {
        // ::ffff:169.254.169.254 — the cloud metadata address in
        // its IPv4-mapped IPv6 form must still be blocked.
        let ip: std::net::Ipv6Addr = "::ffff:169.254.169.254".parse().unwrap();
        assert!(ipv6_blocked(&ip));
    }

    #[test]
    fn ipv6_compatible_ipv4_embeddings_are_blocked() {
        // Deprecated IPv4-compatible form (::a.b.c.d, no `ffff`
        // marker segment) — `to_ipv4_mapped()` alone misses this,
        // `to_ipv4()` catches both forms.
        for addr in ["::127.0.0.1", "::10.0.0.5"] {
            let ip: std::net::Ipv6Addr = addr.parse().unwrap();
            assert!(ipv6_blocked(&ip), "{} should be blocked", addr);
        }
    }

    #[test]
    fn ipv6_nat64_embeddings_are_blocked() {
        // 64:ff9b::/96 (RFC 6052) embeds an IPv4 address in the
        // low 32 bits — a genuine bypass route on IPv6-only
        // networks behind a NAT64/DNS64 gateway.
        for addr in ["64:ff9b::127.0.0.1", "64:ff9b::169.254.169.254"] {
            let ip: std::net::Ipv6Addr = addr.parse().unwrap();
            assert!(ipv6_blocked(&ip), "{} should be blocked", addr);
        }
    }

    #[test]
    fn ipv6_nat64_embedding_of_public_ip_is_allowed() {
        let ip: std::net::Ipv6Addr = "64:ff9b::5808:d822".parse().unwrap();
        assert!(!ipv6_blocked(&ip), "NAT64-embedded public ip should be allowed");
    }

    #[test]
    fn url_host_allowed_blocks_private_and_loopback() {
        for url in [
            "http://127.0.0.1:9/x.png",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost:9/x.png",
            "http://[::1]:9/x.png",
            "http://sub.localhost/x.png",
            "http://100.64.0.1:9/x.png",
        ] {
            assert!(url_host_allowed(url).is_err(), "{} should be blocked", url);
        }
    }

    #[test]
    fn url_host_allowed_blocks_non_http_schemes() {
        assert!(url_host_allowed("file:///etc/hostname").is_err());
    }

    #[test]
    fn url_host_allowed_permits_public_hosts() {
        // Numeric IPs only, so this test resolves nothing over the
        // network (`ToSocketAddrs` on an IP literal is a pure
        // parse) and stays fast and deterministic in offline CI.
        for url in ["http://93.184.216.34/x.png", "https://1.1.1.1/x.png"] {
            assert!(url_host_allowed(url).is_ok(), "{} should be allowed", url);
        }
    }

    #[test]
    fn url_host_allowed_pins_resolved_address() {
        // check_url_host is the function fetch_url actually uses to
        // obtain a pin target; url_host_allowed just discards it.
        // Assert the pin directly so the resolve -> validate -> pin
        // wiring itself is under test, not only the pass/fail
        // outcome.
        match check_url_host("http://93.184.216.34:443/x.png") {
            Ok(HostDecision::Pinned { host, addr }) => {
                assert_eq!(host, "93.184.216.34");
                assert_eq!(addr.ip().to_string(), "93.184.216.34");
                assert_eq!(addr.port(), 443);
            }
            other => panic!("expected a pinned decision, got {other:?}"),
        }
    }
}
