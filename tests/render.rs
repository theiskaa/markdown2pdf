//! Integration tests for the renderer. Each `mod` here is a file in
//! `tests/render/`; collectively they exercise every renderer feature
//! through the public `parse_into_bytes` API.
//!
//! `tests/render.rs` is treated by Cargo as the crate root for this
//! integration-test target, so `mod foo;` would resolve relative to
//! `tests/` rather than `tests/render/`. We use `#[path = ...]` to keep
//! the test files grouped under `tests/render/`.
//!
//! Shared helper code (the `render` helper used by nearly every file)
//! lives in `tests/render/common.rs`, mirroring the lexer tests'
//! `tests/markdown/common.rs` layout.

#[path = "render/common.rs"]
mod common;

#[path = "render/fonts.rs"]
mod fonts;

#[path = "render/styling.rs"]
mod styling;

#[path = "render/golden.rs"]
mod golden;
