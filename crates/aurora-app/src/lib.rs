//! Aurora application layer: the orchestration shared by the CLI and the TUI.
//!
//! `fetch` resolves catalog entries to pins and fetches, collects, scans and checks them;
//! `install` turns a workflow, a CLI profile and a project into an install plan and applies or
//! undoes it. Everything here composes the tested engines in `aurora-core`, `aurora-skills` and
//! `aurora-fs`; it adds no mutation logic of its own.

pub mod fetch;
pub mod install;
