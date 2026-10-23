//! Deterministic, role-specific handoff bundles for manual execution (ADR-0005). Port of
//! `awb/workflow/handoff.py`, byte-identical: no timestamps, so the same run directory, plan
//! and stage always render the same bytes.

use std::path::Path;

use super::model::{NOT_APPLICABLE_FILE, WorkflowDefinition};
use super::planner::PlannedStage;
use crate::providers::get_provider;

fn or_none(items: &[String]) -> String {
    if items.is_empty() { "none".to_string() } else { items.join(", ") }
}

/// Render the bundle for one planned stage. Returns an empty string only if the plan names a
/// stage or provider the definition does not know (impossible for parser-validated input).
pub fn render_handoff(wf: &WorkflowDefinition, stage_plan: &PlannedStage, run_dir: &Path, run_id: &str) -> String {
    let (Some(stage), Ok(provider)) = (wf.stage(&stage_plan.id), get_provider(&stage_plan.provider)) else {
        return String::new();
    };
    let short_sha: String = wf.sha256.chars().take(12).collect();
    let mut lines: Vec<String> = vec![
        format!("# Aurora handoff: {} / stage `{}`", wf.name, stage.id),
        String::new(),
        format!("- Run: {run_id}"),
        format!("- Workflow: {} v{} (sha256 {short_sha})", wf.id, wf.version),
        format!("- Role: **{}**: {}", stage_plan.role, stage_plan.responsibility),
        format!(
            "- Provider: {} (`{}`, assigned by {})",
            provider.display_name, provider.id, stage_plan.assignment_source
        ),
    ];
    for step in &stage_plan.fallback_trail {
        lines.push(format!("- Fallback applied: {} -> {} ({})", step.from, step.to, step.reason));
    }
    for entry in &stage_plan.independence {
        if entry.status == "same-provider" {
            lines.push(format!(
                "- Independence: REDUCED. Stage `{}` also ran on `{}`. Start a fresh session and do not rely on \
                 earlier context; this is still not an independent review.",
                entry.of, entry.other_provider
            ));
        } else {
            lines.push(format!(
                "- Independence: stage `{}` ran on a different provider (`{}`). Do not consult its session.",
                entry.of, entry.other_provider
            ));
        }
    }
    lines.push(format!("- Attempts allowed: {}, then {}", stage.max_attempts, stage.on_exhausted));
    if stage.requires_approval {
        lines.push("- Requires human approval after submission.".to_string());
    }

    lines.extend(["", "## Skills", ""].map(String::from));
    lines.push(format!("- Required: {}", or_none(&stage_plan.required_skills)));
    lines.push(format!("- Recommended: {}", or_none(&stage_plan.recommended_skills)));

    let task = if stage.description.is_empty() { &stage_plan.responsibility } else { &stage.description };
    lines.extend(["", "## Task", ""].map(String::from));
    lines.push(task.clone());

    lines.extend(["", "## Inputs", ""].map(String::from));
    if stage_plan.inputs.is_empty() {
        lines.push("- none".to_string());
    }
    lines.extend(stage_plan.inputs.iter().map(|p| format!("- `{}`", run_dir.join(p).display())));

    lines.extend(
        ["", "## Required outputs", "", "Write each file below. Only these files count as evidence:", ""]
            .map(String::from),
    );
    lines.extend(stage_plan.outputs.iter().map(|p| format!("- `{}`", run_dir.join(p).display())));

    lines.extend(["", "## Acceptance criteria", ""].map(String::from));
    for crit in &stage_plan.acceptance {
        lines.push(format!("- `{}`: {} (evidence: {})", crit.id, crit.description, crit.evidence.join(", ")));
    }

    let met: Vec<String> = stage_plan.acceptance.iter().map(|c| format!("--met {}", c.id)).collect();
    let run = run_dir.display();
    lines.extend(
        [
            "",
            "## Submit",
            "",
            "The stage passes only when every output exists and every criterion is claimed with its evidence present.",
            "A zero exit code, or a statement that the work is done, is not sufficient on its own.",
            "",
            "```",
        ]
        .map(String::from),
    );
    lines.push(format!("aurora workflow submit --run {run} --stage {} {}", stage.id, met.join(" ")));
    lines.push("```".to_string());
    if stage.optional {
        lines.push(String::new());
        lines.push(format!(
            "This stage is optional. If it does not apply, write the justification to `{}` and run:",
            run_dir.join("artifacts").join(&stage.id).join(NOT_APPLICABLE_FILE).display()
        ));
        lines.extend(["", "```"].map(String::from));
        lines.push(format!("aurora workflow submit --run {run} --stage {} --not-applicable", stage.id));
        lines.push("```".to_string());
    }

    provider.build_handoff(&(lines.join("\n") + "\n"))
}
