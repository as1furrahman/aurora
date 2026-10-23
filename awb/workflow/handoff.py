"""
Deterministic, role-specific handoff bundles for manual execution (ADR-0005).
No timestamps: the same run directory, plan and stage always render the same bytes.
"""

from pathlib import Path

from awb.providers import get_provider

from .model import WorkflowDefinition


def render_handoff(wf: WorkflowDefinition, plan: dict, stage_plan: dict, run_dir: Path, run_id: str) -> str:
    stage = wf.stage(stage_plan["id"])
    provider = get_provider(stage_plan["provider"])

    lines = [
        f"# Aurora handoff: {wf.name} / stage `{stage.id}`",
        "",
        f"- Run: {run_id}",
        f"- Workflow: {wf.id} v{wf.version} (sha256 {wf.sha256[:12]})",
        f"- Role: **{stage_plan['role']}**: {stage_plan['responsibility']}",
        f"- Provider: {provider.spec.display_name} (`{provider.id}`, assigned by {stage_plan['assignment_source']})",
    ]
    for step in stage_plan["fallback_trail"]:
        lines.append(f"- Fallback applied: {step['from']} -> {step['to']} ({step['reason']})")
    for entry in stage_plan["independence"]:
        if entry["status"] == "same-provider":
            lines.append(f"- Independence: REDUCED. Stage `{entry['of']}` also ran on `{entry['other_provider']}`. "
                         "Start a fresh session and do not rely on earlier context; this is still not an "
                         "independent review.")
        else:
            lines.append(f"- Independence: stage `{entry['of']}` ran on a different provider "
                         f"(`{entry['other_provider']}`). Do not consult its session.")
    lines.append(f"- Attempts allowed: {stage.max_attempts}, then {stage.on_exhausted}")
    if stage.requires_approval:
        lines.append("- Requires human approval after submission.")

    lines += ["", "## Skills", ""]
    lines.append(f"- Required: {', '.join(stage_plan['required_skills']) or 'none'}")
    lines.append(f"- Recommended: {', '.join(stage_plan['recommended_skills']) or 'none'}")

    lines += ["", "## Task", "", stage.description or stage_plan["responsibility"]]

    lines += ["", "## Inputs", ""]
    lines += [f"- `{run_dir / p}`" for p in stage_plan["inputs"]] or ["- none"]

    lines += ["", "## Required outputs", "", "Write each file below. Only these files count as evidence:", ""]
    lines += [f"- `{run_dir / p}`" for p in stage_plan["outputs"]]

    lines += ["", "## Acceptance criteria", ""]
    for crit in stage_plan["acceptance"]:
        lines.append(f"- `{crit['id']}`: {crit['description']} (evidence: {', '.join(crit['evidence'])})")

    met = " ".join(f"--met {c['id']}" for c in stage_plan["acceptance"])
    lines += [
        "",
        "## Submit",
        "",
        "The stage passes only when every output exists and every criterion is claimed with its evidence present.",
        "A zero exit code, or a statement that the work is done, is not sufficient on its own.",
        "",
        "```",
        f"aurora workflow submit --run {run_dir} --stage {stage.id} {met}",
        "```",
    ]
    if stage.optional:
        lines += [
            "",
            f"This stage is optional. If it does not apply, write the justification to "
            f"`{run_dir / 'artifacts' / stage.id / 'NOT_APPLICABLE.md'}` and run:",
            "",
            "```",
            f"aurora workflow submit --run {run_dir} --stage {stage.id} --not-applicable",
            "```",
        ]

    return provider.build_handoff("\n".join(lines) + "\n")
