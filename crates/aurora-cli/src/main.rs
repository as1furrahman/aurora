//! `aurora` — role-based agent skill manager and workflow orchestrator.
//!
//! Exit codes: 0 ok, 1 error, 2 usage error (clap), 3 plan/run has blocked stages.

mod providers;
mod registry;
mod runs;
mod style;
mod workflow;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};

/// Aurora: assign skills and roles to coding CLIs and track evidence-gated workflows.
#[derive(Debug, Parser)]
#[command(name = "aurora", version = aurora_core::VERSION, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List execution providers (coding CLIs) and PATH detection; no binary is run
    Providers {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },
    /// Validate, plan and track role-based workflows (manual handoff)
    Workflow {
        #[command(subcommand)]
        command: WorkflowCommand,
    },
}

#[derive(Debug, Subcommand)]
enum WorkflowCommand {
    /// Validate a workflow definition
    Validate(DefinitionArgs),
    /// Show roles, stages, skills, routing and acceptance gates
    Show(DefinitionArgs),
    /// Produce a deterministic execution plan (writes nothing)
    Plan {
        #[command(flatten)]
        definition: DefinitionArgs,
        #[command(flatten)]
        routing: RoutingArgs,
    },
    /// List the built-in workflow presets
    Presets {
        /// Output in JSON format
        #[arg(long)]
        json: bool,
    },
    /// Write a built-in preset to a file you can edit (never overwrites)
    Init {
        /// Preset name (see `aurora workflow presets`)
        preset: String,
        /// Output file (default: ./<preset>.toml)
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Create a run, pin the definition, and issue handoff bundles
    Start {
        #[command(flatten)]
        definition: DefinitionArgs,
        #[command(flatten)]
        routing: RoutingArgs,
        /// Existing directory that will contain the run directory
        #[arg(long)]
        run_root: PathBuf,
        /// Run directory name (default: <workflow-id>-<UTC time>)
        #[arg(long)]
        run_id: Option<String>,
    },
    /// Show run state, completion, escalations and evidence drift
    Status(runs::RunArgs),
    /// Verify integrity and re-issue missing handoffs (never re-runs PASSED stages)
    Resume(runs::RunArgs),
    /// Submit a stage's evidence and evaluate its acceptance gate
    Submit {
        #[command(flatten)]
        run: runs::RunArgs,
        /// Stage id
        #[arg(long)]
        stage: String,
        /// Claim an acceptance criterion as met (repeatable)
        #[arg(long, value_name = "CRITERION")]
        met: Vec<String>,
        /// Mark an optional stage NOT_APPLICABLE (requires NOT_APPLICABLE.md)
        #[arg(long)]
        not_applicable: bool,
    },
    /// Accept a FAILED or BLOCKED stage by recorded decision (reason and approver are kept)
    Waive {
        #[command(flatten)]
        run: runs::RunArgs,
        /// Stage id
        #[arg(long)]
        stage: String,
        /// Why the gate is waived (shown to every downstream role)
        #[arg(long)]
        reason: String,
        /// Name of the person waiving the gate
        #[arg(long)]
        by: String,
    },
    /// Record human approval for a stage awaiting approval
    Approve {
        #[command(flatten)]
        run: runs::RunArgs,
        /// Stage id
        #[arg(long)]
        stage: String,
        /// Name of the approving person
        #[arg(long)]
        by: String,
    },
}

#[derive(Debug, Args)]
struct DefinitionArgs {
    /// Workflow definition (.toml)
    file: PathBuf,
    /// Path to a custom skills.toml (default: $AURORA_REGISTRY_PATH, else the built-in registry)
    #[arg(long)]
    registry: Option<PathBuf>,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RoutingArgs {
    /// Single-provider mode: run every eligible stage through this provider
    #[arg(long, conflicts_with = "route")]
    provider: Option<String>,
    /// Route one stage to a provider (repeatable)
    #[arg(long, value_name = "STAGE=PROVIDER")]
    route: Vec<String>,
    /// Skip PATH detection of provider binaries
    #[arg(long)]
    no_detect: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        // The TUI will launch here (docs/design/tui-vision.md); until then, show help.
        None => {
            let _ = Cli::command().print_help();
            println!();
            0
        }
        Some(Command::Providers { json }) => providers::run(json),
        Some(Command::Workflow { command }) => match command {
            WorkflowCommand::Validate(def) => workflow::validate(&def),
            WorkflowCommand::Show(def) => workflow::show(&def),
            WorkflowCommand::Plan { definition, routing } => workflow::plan(&definition, &routing),
            WorkflowCommand::Start { definition, routing, run_root, run_id } => {
                runs::start(&definition, &routing, &run_root, run_id.as_deref())
            }
            WorkflowCommand::Status(args) => runs::status(&args),
            WorkflowCommand::Presets { json } => workflow::presets(json),
            WorkflowCommand::Init { preset, output } => workflow::init(&preset, output.as_deref()),
            WorkflowCommand::Resume(args) => runs::resume(&args),
            WorkflowCommand::Submit { run, stage, met, not_applicable } => {
                runs::submit(&run, &stage, &met, not_applicable)
            }
            WorkflowCommand::Approve { run, stage, by } => runs::approve(&run, &stage, &by),
            WorkflowCommand::Waive { run, stage, reason, by } => runs::waive(&run, &stage, &reason, &by),
        },
    };
    ExitCode::from(code)
}
