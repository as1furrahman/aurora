//! Aurora core: provider-neutral workflow model, strict parser, deterministic planner
//! and evidence-gated stage engine.
//!
//! This crate is pure: it has no terminal dependencies and performs no direct I/O on run
//! directories (that is `aurora-fs`). See `docs/adr/0006-rust-rewrite.md`.

pub mod catalog;
pub mod presets;
pub mod providers;
pub mod registry;
pub mod util;
pub mod workflow;

/// Crate version, shared by the whole workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
