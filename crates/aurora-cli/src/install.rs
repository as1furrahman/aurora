//! `aurora skills plan`: what installing a workflow's recommended skills into a project would do.
//! A dry run: it fetches into the cache and reads the project without following symlinks, but
//! writes nothing to the project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anstream::println;
use aurora_app::install::{
    InstallRequest, Planned, WorkflowSource, apply_planned, check_project, new_op_id, plan_install, undo_project,
};
use aurora_core::catalog::Recommendation;
use aurora_skills::apply::ApplyError;
use aurora_skills::plan::{ActionKind, InstallPlan};
use clap::Args;
use serde_json::json;

use crate::skills::error;
use crate::style::{BAD, DIM, HEAD, OK, WARN, paint};

pub use aurora_app::install::LOCK_FILE;

#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Workflow definition (.toml); or use --preset
    #[arg(required_unless_present = "preset", conflicts_with = "preset")]
    file: Option<PathBuf>,
    /// Built-in preset name (see `aurora workflow presets`)
    #[arg(long)]
    preset: Option<String>,
    /// CLI profile: solo, pair or trio (default: from the number of --clis, else pair)
    #[arg(long)]
    profile: Option<String>,
    /// CLIs filling the profile's slots, comma-separated (e.g. claude,codex)
    #[arg(long, value_delimiter = ',')]
    clis: Vec<String>,
    /// Include an opt-in entry (repeatable)
    #[arg(long, value_name = "ENTRY")]
    with: Vec<String>,
    /// Project directory to install into (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Keep skill hooks instead of removing them (they run commands without asking)
    #[arg(long)]
    keep_hooks: bool,
    /// Remove Aurora-installed skills that are no longer wanted
    #[arg(long)]
    prune: bool,
    /// Custom catalog file (default: the built-in catalog)
    #[arg(long, value_name = "FILE")]
    catalog: Option<PathBuf>,
    /// Cache directory (default: $AURORA_CACHE_DIR, else $XDG_CACHE_HOME/aurora/skills, else ~/.cache/aurora/skills)
    #[arg(long, value_name = "DIR")]
    cache: Option<PathBuf>,
    /// Serve https://github.com/<owner>/<name> from DIR/<owner>/<name> (an offline mirror)
    #[arg(long, value_name = "DIR")]
    mirror: Option<PathBuf>,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

impl PlanArgs {
    /// The application-layer request for these arguments.
    pub(crate) fn request(&self) -> InstallRequest {
        InstallRequest {
            workflow: match (&self.preset, &self.file) {
                (Some(name), _) => WorkflowSource::Preset(name.clone()),
                (None, file) => WorkflowSource::File(file.clone().unwrap_or_default()),
            },
            profile: self.profile.clone(),
            clis: self.clis.clone(),
            with: self.with.clone(),
            project: self.project.clone(),
            keep_hooks: self.keep_hooks,
            prune: self.prune,
            catalog: self.catalog.clone(),
            cache: self.cache.clone(),
            mirror: self.mirror.clone(),
        }
    }
}

pub fn run_plan(args: &PlanArgs) -> u8 {
    let planned = match plan_install(&args.request()) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let Planned { project, recommendation: rec, plan: p, .. } = &planned;
    let warnings = planned.all_warnings();
    if args.json {
        let out = json!({
            "project": project.display().to_string(),
            "profile": rec.profile,
            "clis": rec.clis,
            "actions": p.actions,
            "visibility": p.visibility,
            "warnings": warnings,
            "lock": p.lock,
            "changed": p.changed,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        return 0;
    }
    print_plan(project, rec, p, &warnings);
    println!("\n{}", paint(DIM, "Dry run: nothing was written to the project."));
    0
}

pub(crate) fn print_plan(project: &Path, rec: &Recommendation, p: &InstallPlan, warnings: &[String]) {
    println!(
        "{}",
        paint(
            HEAD,
            format!("Aurora — Install plan: {} ({}: {})", project.display(), rec.profile, rec.clis.join(" + "))
        )
    );
    println!("{}", "-".repeat(65));
    for a in &p.actions {
        let kind = match a.kind {
            ActionKind::Create => paint(OK, "create   "),
            ActionKind::Update => paint(OK, "update   "),
            ActionKind::Unchanged => paint(DIM, "unchanged"),
            ActionKind::Keep => paint(DIM, "keep     "),
            ActionKind::Remove => paint(WARN, "remove   "),
            ActionKind::Refuse => paint(BAD, "refuse   "),
        };
        let who = if a.providers.is_empty() { String::new() } else { format!(" [{}]", a.providers.join(", ")) };
        let review = if a.needs_review { paint(WARN, "  needs your review") } else { String::new() };
        let detail = if a.detail.is_empty() { String::new() } else { format!("  {}", a.detail) };
        println!("  {kind} {:<46} {}{who}{review}{detail}", a.target, a.entry);
    }
    let overlays: Vec<(&String, &Vec<String>)> =
        p.lock.skills.values().filter(|s| !s.overlays.is_empty()).map(|s| (&s.entry, &s.overlays)).collect();
    if !overlays.is_empty() {
        println!("\nOverlays (the only edits Aurora makes to upstream files):");
        let mut seen = Vec::new();
        for (entry, keys) in overlays {
            if seen.contains(&entry) {
                continue;
            }
            seen.push(entry);
            for key in keys {
                let why = if key == "hooks" {
                    "hooks run commands without asking; --keep-hooks keeps them"
                } else {
                    "pre-approved tools; the CLI asks you as usual"
                };
                println!("  {entry}: {key} removed ({why})");
            }
        }
    }
    println!("\nWhat each CLI will load from this project:");
    for v in &p.visibility {
        let line = format!(
            "  {:<12} {:>2} skills, ~{} of {} chars: {}",
            v.provider,
            v.skills.len(),
            v.listing_chars,
            v.budget_chars,
            v.skills.join(", ")
        );
        println!("{}", if v.listing_chars > v.budget_chars { paint(BAD, line) } else { line });
    }
    if !warnings.is_empty() {
        println!("\nWarnings:");
        for w in warnings {
            println!("  {} {w}", paint(WARN, "!"));
        }
    }
    let lock_state = if !p.changed { "unchanged" } else { "would be written" };
    println!(
        "\n{LOCK_FILE}: {lock_state} ({} installed copies, {} rule blocks)",
        p.lock.skills.len(),
        p.lock.rules.values().map(BTreeMap::len).sum::<usize>()
    );
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    #[command(flatten)]
    plan: PlanArgs,
    /// Apply without asking (required when not running in a terminal)
    #[arg(long)]
    yes: bool,
    /// Confirm you read the flagged files of this entry (repeatable)
    #[arg(long, value_name = "ENTRY")]
    reviewed: Vec<String>,
}

fn confirm(question: &str) -> bool {
    use std::io::{BufRead, IsTerminal, Write};
    if !std::io::stdin().is_terminal() {
        return false;
    }
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).is_ok() && matches!(answer.trim(), "y" | "Y" | "yes")
}

pub fn run_apply(args: &ApplyArgs) -> u8 {
    let planned = match plan_install(&args.plan.request()) {
        Ok(p) => p,
        Err(e) => return error(e),
    };
    let Planned { project, recommendation: rec, plan: p, review_files, .. } = &planned;
    if let Err(e) = check_project(project) {
        return error(e);
    }
    let warnings = planned.all_warnings();
    if !args.plan.json {
        print_plan(project, rec, p, &warnings);
    }
    let unreviewed = planned.unreviewed(&args.reviewed);
    if !unreviewed.is_empty() {
        let list: Vec<String> = unreviewed
            .iter()
            .map(|e| format!("{e} (read: {})", review_files.get(e).map(|f| f.join(", ")).unwrap_or_default()))
            .collect();
        let flags: Vec<String> = unreviewed.iter().map(|e| format!("--reviewed {e}")).collect();
        return error(format!(
            "these skills need your review before they are installed: {}. Read the files in the cache \
             (`aurora skills fetch <entry>` shows where), then rerun with {}",
            list.join("; "),
            flags.join(" ")
        ));
    }
    if !p.changed {
        println!("\nNothing to do: the project already matches the plan.");
        return 0;
    }
    let changes = planned.change_count();
    if !args.yes && !confirm(&format!("\nApply {changes} changes to {}?", project.display())) {
        return error("not applied: confirm in a terminal, or pass --yes");
    }
    let id = new_op_id();
    match apply_planned(&planned, &id) {
        Ok(record) => {
            if args.plan.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &json!({"project": project.display().to_string(), "operation": record})
                    )
                    .unwrap_or_default()
                );
            } else {
                println!("\n{} {changes} changes (operation {id}).", paint(OK, "Applied"));
                println!("  Undo: aurora skills undo --project {}", project.display());
            }
            0
        }
        Err(e @ ApplyError::Pending(_)) => error(e),
        Err(e) => error(format!("not applied: {e}")),
    }
}

#[derive(Debug, Args)]
pub struct UndoArgs {
    /// Project directory (default: the current directory)
    #[arg(long, default_value = ".")]
    project: PathBuf,
    /// Operation id (default: the most recent one)
    #[arg(long, value_name = "ID")]
    op: Option<String>,
    /// Confirm a non-interactive rollback (accepted for script symmetry; undo never prompts)
    #[arg(long)]
    yes: bool,
    /// Output in JSON format
    #[arg(long)]
    json: bool,
}

pub fn run_undo(args: &UndoArgs) -> u8 {
    let project = match std::fs::canonicalize(&args.project) {
        Ok(p) => p,
        Err(e) => return error(format!("project {}: {e}", args.project.display())),
    };
    match undo_project(&project, args.op.as_deref()) {
        Ok(report) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
                return 0;
            }
            println!("{} {} items from operation {}:", paint(OK, "Restored"), report.restored.len(), report.op_id);
            for r in &report.restored {
                println!("  {r}");
            }
            if !report.skipped.is_empty() {
                println!("\nSkipped (changed since the operation; left as they are):");
                for s in &report.skipped {
                    println!("  {} {s}", paint(WARN, "!"));
                }
            }
            println!(
                "\n{}",
                paint(
                    DIM,
                    format!(
                        "Removed items were moved aside to {}/.aurora/ops/{}/, not deleted.",
                        project.display(),
                        report.op_id
                    )
                )
            );
            0
        }
        Err(e) => error(e),
    }
}
