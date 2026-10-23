//! `aurora workflow start|status|submit|approve|resume` (port of the run commands in
//! `awb/workflow/commands.py`). Exit codes: 0 ok, 1 error / gate not satisfied, 3 blocked.

use std::path::PathBuf;

use anstream::{eprintln, println};
use aurora_core::workflow::{PlanOptions, StageState};
use aurora_fs::engine::{Engine, StatusReport};
use aurora_fs::runs::RunError;
use clap::Args;

use crate::style::{BAD, HEAD, OK, WARN, paint};
use crate::workflow::{EXIT_BLOCKED, parse_routes, print_plan};
use crate::{DefinitionArgs, RoutingArgs, providers, registry};

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Run directory
    #[arg(long)]
    pub run: PathBuf,
    /// Output in JSON format
    #[arg(long)]
    pub json: bool,
}

fn error(message: impl std::fmt::Display) -> u8 {
    eprintln!("{} {message}", paint(BAD, "Error:"));
    1
}

fn print_json(value: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

pub fn start(def: &DefinitionArgs, routing: &RoutingArgs, run_root: &std::path::Path, run_id: Option<&str>) -> u8 {
    let outcome = (|| -> Result<_, String> {
        let routes = parse_routes(&routing.route).map_err(|e| e.to_string())?;
        let reg = registry::load(def.registry.as_deref())?;
        let detected = (!routing.no_detect).then(providers::detect_all);
        let opts = PlanOptions { provider_override: routing.provider.as_deref(), routes, detected: detected.as_ref() };
        Engine::host().start_run(&def.file, run_root, Some(&reg), &opts, run_id, None).map_err(|e| e.to_string())
    })();
    let started = match outcome {
        Ok(started) => started,
        Err(e) => return error(e),
    };
    let record = &started.record;
    let issued: Vec<&str> =
        record.stages.iter().filter(|(_, st)| st.handoff.is_some()).map(|(id, _)| id.as_str()).collect();
    let blocked: Vec<&str> =
        record.stages.iter().filter(|(_, st)| st.state == StageState::Blocked).map(|(id, _)| id.as_str()).collect();
    if def.json {
        let warnings: Vec<&String> = started.warnings.iter().chain(&record.plan.warnings).collect();
        print_json(&serde_json::json!({
            "run_dir": started.run_dir.display().to_string(),
            "run_id": record.run_id,
            "issued": issued,
            "blocked": blocked,
            "warnings": warnings,
        }));
    } else {
        print_plan(&record.plan);
        println!("\nRun created: {}", started.run_dir.display());
        for sid in &issued {
            if let Some(handoff) = &record.stages[*sid].handoff {
                println!("  handoff issued: {}", started.run_dir.join(&handoff.path).display());
            }
        }
        for sid in &blocked {
            println!("  {} {sid}", paint(BAD, "[BLOCKED]"));
        }
    }
    if blocked.is_empty() { 0 } else { EXIT_BLOCKED }
}

pub fn print_status(report: &StatusReport) {
    let done = if report.complete { paint(OK, "COMPLETE") } else { paint(WARN, "INCOMPLETE") };
    println!(
        "{}{done}",
        paint(
            HEAD,
            format!(
                "Aurora — Run {} [{} v{}] (mode: {}): ",
                report.run_id, report.workflow.id, report.workflow.version, report.mode
            )
        )
    );
    println!("{}", "-".repeat(65));
    for s in &report.stages {
        let mut extra = Vec::new();
        if s.awaiting_approval {
            extra.push("awaiting approval".to_string());
        }
        if s.escalated {
            extra.push("ESCALATED".to_string());
        }
        if !s.waiting_on.is_empty() {
            extra.push(format!("waiting on {}", s.waiting_on.join(", ")));
        }
        let suffix = if extra.is_empty() { String::new() } else { format!("  ({})", extra.join("; ")) };
        println!(
            "  {:<14} {:<15} {:<12} attempts {}/{}{suffix}",
            s.id,
            s.state.as_str(),
            s.provider,
            s.attempts,
            s.max_attempts
        );
        for m in &s.missing {
            println!("      missing: {m}");
        }
        for r in &s.blocked_reasons {
            println!("      blocked: {r}");
        }
    }
    for d in &report.drift {
        println!("  {} {}: {} ({})", paint(BAD, "[DRIFT]"), d.stage, d.path, d.problem);
    }
    if !report.concerns.is_empty() {
        println!("  {} passed with concerns: {}", paint(WARN, "[concerns]"), report.concerns.join(", "));
    }
    if !report.waived.is_empty() {
        println!("  {} accepted by recorded waiver: {}", paint(WARN, "[waived]"), report.waived.join(", "));
    }
    if !report.reduced_independence.is_empty() {
        println!("  [independence] reduced (same provider): {}", report.reduced_independence.join(", "));
    }
    for w in &report.warnings {
        println!("  {} {w}", paint(WARN, "[warning]"));
    }
}

fn report(args: &RunArgs, result: Result<StatusReport, RunError>) -> u8 {
    match result {
        Err(e) => error(e),
        Ok(report) => {
            if args.json {
                print_json(&report);
            } else {
                print_status(&report);
            }
            0
        }
    }
}

pub fn status(args: &RunArgs) -> u8 {
    report(args, Engine::host().status(&args.run))
}

pub fn resume(args: &RunArgs) -> u8 {
    report(args, Engine::host().resume(&args.run, None))
}

pub fn submit(args: &RunArgs, stage: &str, met: &[String], not_applicable: bool) -> u8 {
    let result = match Engine::host().submit(&args.run, stage, met, not_applicable, None) {
        Ok(result) => result,
        Err(e) => return error(e),
    };
    if args.json {
        print_json(&result);
    } else {
        let label = if result.awaiting_approval {
            format!("{} (gate satisfied; awaiting approval)", result.state)
        } else {
            result.state.to_string()
        };
        let label = match result.state {
            StageState::Passed | StageState::NotApplicable => paint(OK, label),
            _ if result.awaiting_approval => paint(WARN, label),
            _ => paint(BAD, label),
        };
        println!("Stage {}: {label} [attempt {}]", result.stage, result.attempts);
        for m in &result.missing {
            println!("  missing: {m}");
        }
        if result.escalated {
            println!("  {} attempts exhausted; human decision required", paint(BAD, "ESCALATED:"));
        }
        for w in &result.warnings {
            println!("  {} {w}", paint(WARN, "[warning]"));
        }
    }
    let ok = matches!(result.state, StageState::Passed | StageState::NotApplicable) || result.awaiting_approval;
    if ok { 0 } else { 1 }
}

pub fn approve(args: &RunArgs, stage: &str, by: &str) -> u8 {
    match Engine::host().approve(&args.run, stage, by, None) {
        Err(e) => error(e),
        Ok(result) => {
            if args.json {
                print_json(&result);
            } else {
                println!("Stage {}: {} (approved by {by})", result.stage, paint(OK, result.state));
            }
            0
        }
    }
}

pub fn waive(args: &RunArgs, stage: &str, reason: &str, by: &str) -> u8 {
    match Engine::host().waive(&args.run, stage, reason, by, None) {
        Err(e) => error(e),
        Ok(result) => {
            if args.json {
                print_json(&result);
            } else {
                println!("Stage {}: {} (gate waived by {by}: {reason})", result.stage, paint(WARN, result.state));
            }
            0
        }
    }
}
