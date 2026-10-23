//! `aurora` — role-based agent skill manager and workflow orchestrator.

use clap::Parser;

/// Aurora: assign skills and roles to coding CLIs and track evidence-gated workflows.
#[derive(Debug, Parser)]
#[command(name = "aurora", version = aurora_core::VERSION, about, long_about = None)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
