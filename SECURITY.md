# Security Policy

markdown2pdf parses Markdown that its caller did not necessarily write, reads images off the local filesystem, optionally fetches remote resources, and loads fonts. Anyone using it as a library to render user-submitted documents is running an untrusted-input parser on their server, which makes responsible disclosure genuinely valuable. Reports are appreciated.

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report privately through GitHub's [private vulnerability reporting](https://github.com/theiskaa/markdown2pdf/security/advisories/new), the "Report a vulnerability" button under the repository's **Security** tab. If you cannot use that, email **me@theiskaa.com** with the details.

A useful report includes:

- the version or commit you tested,
- the feature flags you built with (`fetch` and `svg` are off by default),
- a clear description of the issue and its impact,
- the steps to reproduce it, ideally a minimal Markdown document that triggers it,
- and any thoughts on a fix, if you have them.

Please give a reasonable window to investigate and address the issue before any public disclosure. You will get an acknowledgement, updates as the fix progresses, and credit in the release notes if you would like it.

## The trust model, so expectations are clear

The interesting case is a service that converts Markdown somebody else wrote. Someone converting their own file on their own machine is not a victim of their own document, so a few design choices matter for how you deploy this:

- **A document names the image paths it wants, and by default they are read.** `![](/etc/ssl/certs/logo.png)` reads that path. Confinement exists but is opt-in: set `image_root` in the `[security]` config block and image paths resolve inside it, with anything escaping (including through a symlink) refused and degraded to alt text. The defaults stay permissive so existing documents keep working. **If you render untrusted Markdown, set `image_root`.** See [docs/configuration.md](docs/configuration.md).
- **Remote fetching is off unless you ask for it.** The `fetch` feature gates remote images and the CLI's `--url` input. With it enabled, the renderer resolves each host, validates every resolved address, and pins the connection to the address it checked, refusing loopback, private, link-local, CGNAT, and IPv6 NAT64 targets so a document cannot reach cloud metadata or internal services. Redirects are capped and re-validated per hop. Downloads are bounded in size and total time. Set `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1` to disable the host checks on an intranet, and understand that doing so hands document authors your internal network.
- **Configuration is yours, never the document's.** Frontmatter can only set metadata (title, author, subject, creator, keywords). It cannot reach the `[security]` block, font paths, or the output path. Bundled themes are compiled in and never set security policy.
- **This crate writes PDFs, it does not read them.** There is no untrusted-PDF parsing path, which bounds the impact of advisories in PDF-reading code.

## Known limitations

These are documented rather than fixed, so a report about them is a duplicate rather than a finding. If you can show impact beyond what is described here, that is worth reporting:

- **Hardlinks are not detected** by `image_root` confinement. Path canonicalization resolves symlinks; a hardlink's canonical path is itself. Creating one requires write access inside the root, which is already a stronger position than the image read it would buy.
- **There is a check-to-read window.** A symlink swapped between path validation and the read would defeat confinement. Closing it needs directory-fd traversal.
- **Redirect hops cannot be connection-pinned.** Each hop's host is resolved and validated, but the connection is not pinned to the checked address the way the initial request is, so DNS rebinding on a redirect remains possible.
- **Unmaintained dependencies.** `ttf-parser`, `rustybuzz`, and `bincode` carry RustSec unmaintained advisories with no patched versions available upstream.

## Areas of particular interest

If you are looking for where the sharp edges are:

- **The lexer.** `src/lib/markdown.rs` is a hand-written CommonMark and GFM parser several thousand lines long, reading input the caller did not produce. A panic, a hang, or unbounded memory growth on crafted Markdown is in scope. Nesting is capped at depth 32, so a report should get past that.
- **The layout and math engines.** Adversarial documents reaching a panic through table geometry, line breaking, or the TeX engine. Math parsing is capped at depth 200.
- **The fetch guard.** `src/lib/render/net_guard.rs`. Any way to reach an address the policy should refuse, defeat the redirect re-validation, or exceed the size or time bounds.
- **Image path confinement.** `src/lib/render/image_policy.rs`. Any way to read a file outside a configured `image_root`, beyond the known limitations above.
- **Decoders on document-supplied data.** PNG and JPEG decoding, and SVG rasterization under the `svg` feature. Memory exhaustion is partly bounded by a 4000px dimension cap.
- **Font loading.** Parsing, shaping, and subsetting operate on font files an operator supplies. Less exposed than the document path, but still parsing.

## What is not a vulnerability

- Rendering your own document on your own machine. The threat model is untrusted input, not self-inflicted input.
- The permissive defaults for `image_root` and remote images. They preserve backward compatibility and are documented; deploying without setting them for untrusted input is a misconfiguration, and the fix is the configuration above.
- Reaching a private address after you set `MARKDOWN2PDF_ALLOW_PRIVATE_NETWORK=1`. That is the escape hatch working.
- Wrong, ugly, or unexpected PDF output. That is a rendering bug. Please do open a public issue for it.
