//! Aurora filesystem safety layer. The only crate allowed to write to run directories.
//!
//! Ports the guarantees of the Python `awb.safety` package (boundary validation,
//! no-follow reads, atomic writes, mandatory locking). See `docs/adr/0006-rust-rewrite.md`.
