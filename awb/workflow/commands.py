"""
`aurora workflow ...` subcommands. Exit codes: 0 ok, 1 error, 3 plan/run has blocked stages.
"""

import argparse
import json
import sys
from pathlib import Path
from typing import Dict, Tuple

from awb.config import Config
from awb.errors import AWBError
from awb.providers import PROVIDERS
from awb.registry.parser import load_registry
from awb.registry.schema import RegistryManifest

from .model import WorkflowDefinition, WorkflowValidationError
from .parser import load_workflow
from .planner import PlanError, build_plan

EXIT_BLOCKED = 3


def add_workflow_parser(subparsers) -> None:
    wf_parser = subparsers.add_parser("workflow", help="Validate, plan, and track role-based workflows (manual handoff)")
    wf_sub = wf_parser.add_subparsers(dest="workflow_command")

    def with_definition(p):
        p.add_argument("file", type=Path, help="Workflow definition (.toml)")
        p.add_argument("--registry", type=Path, default=None, help="Path to custom skills.toml")
        p.add_argument("--json", action="store_true", help="Output in JSON format")

    def with_routing(p):
        group = p.add_mutually_exclusive_group()
        group.add_argument("--provider", default=None,
                           help="Single-provider mode: run every eligible stage through this provider")
        group.add_argument("--route", action="append", default=[], metavar="STAGE=PROVIDER",
                           help="Route one stage to a provider (repeatable)")
        p.add_argument("--no-detect", action="store_true", help="Skip PATH detection of provider binaries")

    with_definition(wf_sub.add_parser("validate", help="Validate a workflow definition"))
    with_definition(wf_sub.add_parser("show", help="Show roles, stages, skills, routing and acceptance gates"))
    plan_parser = wf_sub.add_parser("plan", help="Produce a deterministic execution plan (writes nothing)")
    with_definition(plan_parser)
    with_routing(plan_parser)

    start_parser = wf_sub.add_parser("start", help="Create a run, pin the definition, and issue handoff bundles")
    with_definition(start_parser)
    with_routing(start_parser)
    start_parser.add_argument("--run-root", type=Path, required=True,
                              help="Existing directory that will contain the run directory")
    start_parser.add_argument("--run-id", default=None, help="Run directory name (default: <workflow-id>-<UTC time>)")

    def with_run(p, stage=False):
        p.add_argument("--run", type=Path, required=True, help="Run directory")
        if stage:
            p.add_argument("--stage", required=True, help="Stage id")
        p.add_argument("--json", action="store_true", help="Output in JSON format")

    with_run(wf_sub.add_parser("status", help="Show run state, completion, escalations and evidence drift"))
    with_run(wf_sub.add_parser("resume", help="Verify integrity and re-issue missing handoffs (never re-runs PASSED stages)"))
    submit_parser = wf_sub.add_parser("submit", help="Submit a stage's evidence and evaluate its acceptance gate")
    with_run(submit_parser, stage=True)
    submit_parser.add_argument("--met", action="append", default=[], metavar="CRITERION",
                               help="Claim an acceptance criterion as met (repeatable)")
    submit_parser.add_argument("--not-applicable", action="store_true",
                               help="Mark an optional stage NOT_APPLICABLE (requires NOT_APPLICABLE.md)")
    approve_parser = wf_sub.add_parser("approve", help="Record human approval for a stage awaiting approval")
    with_run(approve_parser, stage=True)
    approve_parser.add_argument("--by", required=True, help="Name of the approving person")


def run_workflow_command(args: argparse.Namespace, config: Config) -> int:
    handlers = {"validate": _cmd_validate, "show": _cmd_show, "plan": _cmd_plan, "start": _cmd_start,
                "status": _cmd_status, "resume": _cmd_resume, "submit": _cmd_submit, "approve": _cmd_approve}
    handler = handlers.get(args.workflow_command)
    if handler is None:
        print("Error: specify a workflow subcommand: " + ", ".join(handlers), file=sys.stderr)
        return 1
    return handler(args, config)


def _load(args, config: Config) -> Tuple[WorkflowDefinition, RegistryManifest]:
    registry = load_registry(args.registry or config.registry_path)
    return load_workflow(args.file, registry=registry), registry


def _cmd_validate(args, config: Config) -> int:
    try:
        wf, _ = _load(args, config)
    except WorkflowValidationError as e:
        if args.json:
            print(json.dumps({"valid": False, "source": e.source, "errors": e.errors}, indent=2))
        else:
            print(f"INVALID: {e.source}")
            for err in e.errors:
                print(f"  - {err}")
        return 1
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps({"valid": True, "id": wf.id, "version": wf.version, "sha256": wf.sha256,
                          "warnings": list(wf.warnings)}, indent=2))
    else:
        print(f"VALID: {wf.id} v{wf.version} ({len(wf.roles)} roles, {len(wf.stages)} stages)")
        print(f"  sha256: {wf.sha256}")
        for w in wf.warnings:
            print(f"  [warning] {w}")
    return 0


def describe(wf: WorkflowDefinition) -> dict:
    """Provider-neutral description of a definition, plus its separate routing policy."""
    return {
        "workflow": {"id": wf.id, "version": wf.version, "name": wf.name, "description": wf.description,
                     "task_types": list(wf.task_types), "assurance": wf.assurance, "sha256": wf.sha256},
        "roles": {r.id: {"responsibility": r.responsibility, "required_skills": list(r.required_skills),
                         "recommended_skills": list(r.recommended_skills)} for r in wf.roles.values()},
        "stages": [
            {"id": s.id, "role": s.role, "description": s.description, "depends_on": list(s.depends_on),
             "inputs": list(s.inputs), "outputs": list(s.outputs), "optional": s.optional,
             "max_attempts": s.max_attempts, "on_exhausted": s.on_exhausted,
             "requires_approval": s.requires_approval, "independent_of": list(s.independent_of),
             "acceptance": [{"id": c.id, "description": c.description, "evidence": list(c.evidence)}
                            for c in s.acceptance]}
            for s in wf.stages
        ],
        "routing": {
            "allow_override": wf.routing.allow_override,
            "roles": {rid: {"eligible": list(rr.eligible), "preferred": rr.preferred}
                      for rid, rr in wf.routing.roles.items()},
            "fallback": [{"role": f.role, "from": f.from_provider, "to": f.to_provider, "on": list(f.on)}
                         for f in wf.routing.fallback],
        },
        "warnings": list(wf.warnings),
    }


def _cmd_show(args, config: Config) -> int:
    try:
        wf, _ = _load(args, config)
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    data = describe(wf)
    if args.json:
        print(json.dumps(data, indent=2))
        return 0
    print(f"Aurora — Workflow: {wf.name} [{wf.id} v{wf.version}] (assurance: {wf.assurance})")
    print("-" * 65)
    print("Roles (provider-neutral):")
    for rid, r in data["roles"].items():
        print(f"  {rid}: {r['responsibility']}")
        print(f"    required skills: {', '.join(r['required_skills']) or 'none'}"
              f"; recommended: {', '.join(r['recommended_skills']) or 'none'}")
    print("\nStages:")
    for s in data["stages"]:
        flags = [f for f, on in (("optional", s["optional"]), ("requires_approval", s["requires_approval"])) if on]
        print(f"  {s['id']} [role: {s['role']}] depends_on: {', '.join(s['depends_on']) or '-'}"
              + (f" ({', '.join(flags)})" if flags else ""))
        if s["independent_of"]:
            print(f"    independent_of: {', '.join(s['independent_of'])}")
        print(f"    outputs: {', '.join(s['outputs'])}; max_attempts: {s['max_attempts']} -> {s['on_exhausted']}")
        for c in s["acceptance"]:
            print(f"    gate {c['id']}: {c['description']} [evidence: {', '.join(c['evidence'])}]")
    print("\nRouting (separate from roles):")
    for rid, rr in data["routing"]["roles"].items():
        print(f"  {rid}: eligible {rr['eligible']}, preferred {rr['preferred']}")
    for f in data["routing"]["fallback"]:
        print(f"  fallback {f['role']}: {f['from']} -> {f['to']} on {f['on']}")
    return 0


def parse_routes(values) -> Dict[str, str]:
    routes: Dict[str, str] = {}
    for item in values:
        stage, sep, provider = item.partition("=")
        if not sep or not stage or not provider:
            raise PlanError(f"--route expects STAGE=PROVIDER, got '{item}'")
        if stage in routes:
            raise PlanError(f"--route given twice for stage '{stage}'")
        routes[stage] = provider
    return routes


def detect_providers() -> Dict[str, bool]:
    return {pid: PROVIDERS[pid].detect().installed for pid in sorted(PROVIDERS)}


def plan_from_args(args, config: Config):
    wf, registry = _load(args, config)
    plan = build_plan(
        wf,
        registry=registry,
        provider_override=args.provider,
        routes=parse_routes(args.route),
        detected=None if args.no_detect else detect_providers(),
    )
    return wf, plan


def print_plan(plan: dict) -> None:
    wf = plan["workflow"]
    mode = plan["mode"] + (f", provider={plan['provider_override']}" if plan["provider_override"] else "")
    print(f"Aurora — Workflow Plan: {wf['id']} v{wf['version']} (mode: {mode})")
    print("-" * 65)
    print(f" {'#':>2}  {'stage':<14} {'role':<12} {'provider':<12} {'source':<10} status")
    for i, s in enumerate(plan["stages"], 1):
        status = "BLOCKED" if s["status"] == "blocked" else "assignable"
        print(f" {i:>2}  {s['id']:<14} {s['role']:<12} {s['provider']:<12} {s['assignment_source']:<10} {status}")
    for s in plan["stages"]:
        for step in s["fallback_trail"]:
            print(f"  [fallback] {s['id']}: {step['from']} -> {step['to']} ({step['reason']})")
        for entry in s["independence"]:
            print(f"  [independence] {s['id']} vs {entry['of']}: {entry['status']}")
        for reason in s["blocked_reasons"]:
            print(f"  [BLOCKED] {s['id']}: {reason}")
        for w in s["warnings"]:
            print(f"  [warning] {s['id']}: {w}")
    for w in plan["warnings"]:
        print(f"  [warning] {w}")


def _cmd_plan(args, config: Config) -> int:
    try:
        _, plan = plan_from_args(args, config)
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(plan, indent=2))
    else:
        print_plan(plan)
    return EXIT_BLOCKED if plan["blocked"] else 0


def _cmd_start(args, config: Config) -> int:
    from .engine import start_run

    try:
        routes = parse_routes(args.route)
        registry = load_registry(args.registry or config.registry_path)
        run_dir, record = start_run(
            args.file, args.run_root, registry=registry, provider_override=args.provider, routes=routes,
            detected=None if args.no_detect else detect_providers(), run_id=args.run_id,
        )
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    issued = [sid for sid, st in record["stages"].items() if st["handoff"]]
    blocked = [sid for sid, st in record["stages"].items() if st["state"] == "BLOCKED"]
    if args.json:
        print(json.dumps({"run_dir": str(run_dir), "run_id": record["run_id"], "issued": issued,
                          "blocked": blocked, "warnings": record["_warnings"] + record["plan"]["warnings"]}, indent=2))
    else:
        print_plan(record["plan"])
        print(f"\nRun created: {run_dir}")
        for sid in issued:
            print(f"  handoff issued: {run_dir / record['stages'][sid]['handoff']['path']}")
        for sid in blocked:
            print(f"  [BLOCKED] {sid}")
    return EXIT_BLOCKED if blocked else 0


def print_status(report: dict) -> None:
    done = "COMPLETE" if report["complete"] else "INCOMPLETE"
    print(f"Aurora — Run {report['run_id']} [{report['workflow']['id']} v{report['workflow']['version']}] "
          f"(mode: {report['mode']}): {done}")
    print("-" * 65)
    for s in report["stages"]:
        extra = []
        if s["awaiting_approval"]:
            extra.append("awaiting approval")
        if s["escalated"]:
            extra.append("ESCALATED")
        if s["waiting_on"]:
            extra.append(f"waiting on {', '.join(s['waiting_on'])}")
        print(f"  {s['id']:<14} {s['state']:<15} {s['provider']:<12} attempts {s['attempts']}/{s['max_attempts']}"
              + (f"  ({'; '.join(extra)})" if extra else ""))
        for m in s["missing"]:
            print(f"      missing: {m}")
        for r in s["blocked_reasons"]:
            print(f"      blocked: {r}")
    for d in report["drift"]:
        print(f"  [DRIFT] {d['stage']}: {d['path']} ({d['problem']})")
    if report["reduced_independence"]:
        print(f"  [independence] reduced (same provider): {', '.join(report['reduced_independence'])}")
    for w in report["warnings"]:
        print(f"  [warning] {w}")


def _report(args, fn) -> int:
    try:
        report = fn()
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print_status(report)
    return 0


def _cmd_status(args, config: Config) -> int:
    from .engine import status
    return _report(args, lambda: status(args.run))


def _cmd_resume(args, config: Config) -> int:
    from .engine import resume
    return _report(args, lambda: resume(args.run))


def _cmd_submit(args, config: Config) -> int:
    from .engine import submit

    try:
        result = submit(args.run, args.stage, met=args.met, not_applicable=args.not_applicable)
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        label = result["state"] + (" (gate satisfied; awaiting approval)" if result["awaiting_approval"] else "")
        print(f"Stage {result['stage']}: {label} [attempt {result['attempts']}]")
        for m in result["missing"]:
            print(f"  missing: {m}")
        if result["escalated"]:
            print("  ESCALATED: attempts exhausted; human decision required")
        for w in result["warnings"]:
            print(f"  [warning] {w}")
    return 0 if result["state"] in ("PASSED", "NOT_APPLICABLE") or result["awaiting_approval"] else 1


def _cmd_approve(args, config: Config) -> int:
    from .engine import approve

    try:
        result = approve(args.run, args.stage, by=args.by)
    except AWBError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(result, indent=2))
    else:
        print(f"Stage {result['stage']}: {result['state']} (approved by {args.by})")
    return 0
