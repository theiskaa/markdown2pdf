//! Styling module — config schema, theme presets, merge / resolve
//! pipeline, and the concrete `ResolvedStyle` consumed by the renderer.
//!
//! Public surface:
//! - [`DocumentConfig`] — the user-facing TOML schema; see
//!   `docs/config.toml` in the repo for the annotated reference.
//! - [`ResolvedStyle`] — what the renderer reads.
//! - [`resolve`] — produces a `ResolvedStyle` from a user
//!   `DocumentConfig`, applying preset + defaults cascade.
//! - [`load_theme_preset`] — fetch one of the bundled presets.
//! - [`ResolveError`] — every failure mode of the config pipeline.
//!
//! See `themes/default.toml` for the exhaustive preset that all other
//! themes inherit from.

pub mod error;
pub mod merge;
pub mod resolved;
pub mod schema;
pub mod themes;

pub use error::ResolveError;
pub use merge::{merge_documents, resolve};
pub use resolved::*;
pub use schema::*;
pub use themes::{available_theme_names, load_theme_preset};
