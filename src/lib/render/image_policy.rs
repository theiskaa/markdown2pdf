//! Image-path policy: is a document-referenced image reference a URL,
//! and (for local paths) is it allowed to be read under the
//! operator's `[security]` confinement settings? Pure decision logic
//! with zero dependency on `Engine`, layout state, or PDF drawing —
//! `layout::Engine::decode_image_file` calls straight into this.
//!
//! Unlike `net_guard`, this is NOT gated behind the `fetch` feature:
//! local-path confinement (`resolve_image_path`) applies regardless
//! of whether remote fetching is compiled in, and `is_http_url` has
//! to be evaluated unconditionally to decide which branch
//! `decode_image_file` even takes.

/// Case-insensitive check: does `path_str` look like an `http(s)://`
/// URL? Markdown authors occasionally write `HTTP://…` or mixed-case
/// schemes; a case-*sensitive* comparison here would silently route
/// such a reference down the local-file branch instead of the URL
/// branch — not because the URL-side guards (`allow_remote_images`,
/// the host allow-list, the redirect cap, size/time bounds) were
/// defeated, but because they were never consulted at all. Fails
/// closed either way (an unrecognized-as-URL string just fails to
/// resolve as a local file too), but a legitimately-formed
/// uppercase-scheme URL deserves to actually be fetched (and be
/// subject to those guards), not to silently degrade to alt text.
pub(crate) fn is_http_url(path_str: &str) -> bool {
    let lower = path_str.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Why [`resolve_image_path`] refused a path — distinguishes a
/// genuine `[security]` policy decision from a plain I/O failure
/// (missing file, bad permissions) so the caller can phrase the two
/// very differently. Folding both into one "refused" message would
/// send an operator debugging a typo'd or moved image link hunting
/// through their security config for a problem that isn't there.
#[derive(Debug)]
pub(crate) enum ImagePathRefusal {
    /// A deliberate refusal: absolute paths are disabled, or the
    /// resolved path escapes `image_root`. This is the policy working
    /// as intended.
    Policy(String),
    /// `canonicalize` failed — the path (or `image_root` itself)
    /// doesn't exist, or isn't readable. An everyday authoring
    /// mistake, not a security event.
    NotFound(String),
}

/// Decide whether a document-referenced local image path may be read
/// under the operator's `[security]` policy, and if so, resolve it to
/// the concrete path to read. `path` is the raw, document-controlled
/// path parsed out of the markdown source; it never comes from
/// configuration.
///
/// - `!allow_absolute` refuses any absolute `path` outright, before
///   `root` is even considered.
/// - `root = None` preserves the historical, unconfined behavior: the
///   path is returned as-is (relative paths later resolve against the
///   process CWD via `std::fs::read`).
/// - `root = Some(dir)` confines every local read to `dir`: a relative
///   `path` joins onto it, an absolute `path` is used as-is (subject
///   to `allow_absolute` above), and both the root and the candidate
///   are canonicalized with `std::fs::canonicalize` before the
///   containment check. Canonicalizing — rather than stripping `..`
///   components by hand — is what makes the check hold against a
///   symlink planted inside `dir` that points outside it; a purely
///   textual prefix check would pass a document straight through such
///   a symlink.
///
/// # Known limitations
///
/// This is a containment check, not a full sandbox. Three gaps are
/// worth knowing about rather than discovering:
///
/// - **Hardlinks are not detected.** `canonicalize` resolves
///   symlinks, but a hardlink's canonical path *is itself* — a
///   hardlink planted inside `root` whose inode is shared with a file
///   outside it sails through this check. Creating that hardlink,
///   though, already requires write access inside `root`, which is a
///   strictly stronger primitive than the image read it would buy —
///   so this is a documented limitation, not a hole worth plugging.
/// - **TOCTOU.** There is a window between this function's
///   `canonicalize` and the caller's subsequent `fs::read` in which
///   the resolved path could be swapped out from under the check
///   (e.g. a symlink repointed mid-race). Closing it needs
///   `openat`-style directory-fd traversal, which is out of scope
///   here.
/// - **`allow_absolute = false` is checked *before* root
///   confinement.** An absolute path is refused even when it points
///   at a file genuinely inside `root`. That's deliberate — it fails
///   closed on the simpler check first — but it surprises operators
///   who expect `image_root` alone to be the deciding factor once
///   it's set.
pub(crate) fn resolve_image_path(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
    allow_absolute: bool,
) -> Result<std::path::PathBuf, ImagePathRefusal> {
    if path.is_absolute() && !allow_absolute {
        return Err(ImagePathRefusal::Policy(format!(
            "absolute image paths are disabled (allow_absolute_image_paths = false): {:?}",
            path
        )));
    }
    let Some(root) = root else {
        return Ok(path.to_path_buf());
    };
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let canonical_root = std::fs::canonicalize(root).map_err(|e| {
        ImagePathRefusal::NotFound(format!(
            "image_root {:?} could not be resolved: {}",
            root, e
        ))
    })?;
    let canonical_candidate = std::fs::canonicalize(&candidate).map_err(|e| {
        ImagePathRefusal::NotFound(format!(
            "image path {:?} not found under image_root {:?}: {}",
            candidate, root, e
        ))
    })?;
    if canonical_candidate.starts_with(&canonical_root) {
        Ok(canonical_candidate)
    } else {
        Err(ImagePathRefusal::Policy(format!(
            "image path {:?} escapes the configured image_root {:?}",
            candidate, root
        )))
    }
}

// Unit tests for `resolve_image_path`, the local-image containment
// guard backing `[security]`. Behavioral (render-degrades-to-alt-
// text) coverage lives in `tests/render/image_pipeline.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn is_http_url_is_case_insensitive() {
        assert!(is_http_url("http://example.com/x.png"));
        assert!(is_http_url("https://example.com/x.png"));
        assert!(is_http_url("HTTP://example.com/x.png"));
        assert!(is_http_url("HTTPS://example.com/x.png"));
        assert!(is_http_url("HttP://Example.COM/x.PNG"));
        assert!(!is_http_url("ftp://example.com/x.png"));
        assert!(!is_http_url("relative/path.png"));
        assert!(!is_http_url("/abs/local/path.png"));
    }

    mod image_path_policy {
        use super::*;

        /// A fresh temp directory for one test, auto-removed on drop.
        struct TempDir(PathBuf);
        impl TempDir {
            fn new(name: &str) -> Self {
                use std::sync::atomic::{AtomicU64, Ordering};
                static SEQ: AtomicU64 = AtomicU64::new(0);
                let n = SEQ.fetch_add(1, Ordering::Relaxed);
                let dir = std::env::temp_dir().join(format!(
                    "m2p_imgroot_{}_{}_{}",
                    std::process::id(),
                    name,
                    n
                ));
                std::fs::create_dir_all(&dir).expect("create temp dir");
                Self(dir)
            }
            fn path(&self) -> &std::path::Path {
                &self.0
            }
        }
        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn no_root_absolute_path_allowed_when_flag_set() {
            let abs = std::env::temp_dir().join("m2p_no_root_case.png");
            let result = resolve_image_path(&abs, None, true);
            assert_eq!(result.unwrap(), abs);
        }

        #[test]
        fn no_root_absolute_path_refused_when_flag_unset() {
            let abs = std::env::temp_dir().join("m2p_no_root_case2.png");
            assert!(resolve_image_path(&abs, None, false).is_err());
        }

        #[test]
        fn root_relative_path_inside_root_allowed() {
            let dir = TempDir::new("rel_inside");
            let file = dir.path().join("pic.png");
            std::fs::write(&file, b"fake").unwrap();
            let rel = std::path::Path::new("pic.png");
            let resolved = resolve_image_path(rel, Some(dir.path()), true).unwrap();
            let canonical_root = std::fs::canonicalize(dir.path()).unwrap();
            assert!(
                resolved.starts_with(&canonical_root),
                "resolved path {:?} must live under root {:?}",
                resolved,
                canonical_root
            );
        }

        #[test]
        fn root_dot_dot_escape_refused() {
            let dir = TempDir::new("dotdot");
            // A real file just outside `dir` so the escape attempt
            // would succeed with a naive (non-canonicalizing) guard.
            let outside = dir.path().parent().unwrap().join(format!(
                "m2p_outside_{}.png",
                std::process::id()
            ));
            std::fs::write(&outside, b"fake").unwrap();
            let rel = std::path::Path::new("..").join(outside.file_name().unwrap());
            let result = resolve_image_path(&rel, Some(dir.path()), true);
            assert!(result.is_err(), "`..` escape must be refused");
            let _ = std::fs::remove_file(&outside);
        }

        #[test]
        fn root_absolute_path_outside_root_refused() {
            let dir = TempDir::new("abs_outside");
            let outside = std::env::temp_dir().join(format!(
                "m2p_abs_outside_{}.png",
                std::process::id()
            ));
            std::fs::write(&outside, b"fake").unwrap();
            let result = resolve_image_path(&outside, Some(dir.path()), true);
            assert!(result.is_err(), "absolute path outside root must be refused");
            let _ = std::fs::remove_file(&outside);
        }

        #[test]
        fn root_absolute_path_inside_root_allowed() {
            let dir = TempDir::new("abs_inside");
            let file = dir.path().join("inside.png");
            std::fs::write(&file, b"fake").unwrap();
            let result = resolve_image_path(&file, Some(dir.path()), true);
            assert!(result.is_ok(), "absolute path inside root must be allowed");
        }

        // The symlink case: the real proof this is a containment check
        // and not naive string-prefixing. A symlink physically inside
        // `root` whose target resolves outside it must be refused —
        // canonicalization (not textual `..` stripping) is what makes
        // this hold.
        #[cfg(unix)]
        #[test]
        fn root_symlink_inside_root_pointing_outside_refused() {
            let dir = TempDir::new("symlink_escape");
            let outside_target = std::env::temp_dir().join(format!(
                "m2p_symlink_target_{}.png",
                std::process::id()
            ));
            std::fs::write(&outside_target, b"fake").unwrap();
            let link = dir.path().join("evil_link.png");
            std::os::unix::fs::symlink(&outside_target, &link).expect("create symlink");
            let rel = std::path::Path::new("evil_link.png");
            let result = resolve_image_path(rel, Some(dir.path()), true);
            assert!(
                result.is_err(),
                "symlink inside root pointing outside must be refused, got {:?}",
                result
            );
            let _ = std::fs::remove_file(&outside_target);
        }

        #[test]
        fn root_nonexistent_file_refused() {
            let dir = TempDir::new("missing");
            let rel = std::path::Path::new("does-not-exist.png");
            let result = resolve_image_path(rel, Some(dir.path()), true);
            assert!(result.is_err(), "nonexistent file must be refused");
        }
    }
}
