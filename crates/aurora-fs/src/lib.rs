//! Aurora filesystem safety layer — the only crate that writes to run directories.
//!
//! Provides boundary validation (`boundaries`, `paths`), no-follow non-blocking reads (`read`), staged atomic writes
//! (`atomic`), mandatory persistent locking (`lock`), and the run store built on them (`runs`).
//! No `unsafe`: descriptor work goes through `rustix`'s safe API.

pub mod atomic;
pub mod boundaries;
pub mod dirs;
pub mod engine;
mod error;
pub mod lock;
pub mod paths;
pub mod read;
pub mod runs;

pub use error::FsError;
