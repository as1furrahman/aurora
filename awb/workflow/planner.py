"""
Deterministic, side-effect-free execution planning (ADR-0004).
Same definition + registry + options + detected set => identical plan.
"""

import heapq
from typing import Dict, List, Mapping, Optional, Tuple

from awb.providers import PROVIDERS
from awb.registry.schema import RegistryManifest

from .model import Role, Stage, WorkflowDefinition, WorkflowError

PLAN_SCHEMA_VERSION = 1


class PlanError(WorkflowError):
    """Raised for invalid planning options (unknown or ineligible provider, forbidden override)."""
    pass


def build_plan(
    wf: WorkflowDefinition,
    registry: Optional[RegistryManifest] = None,
    provider_override: Optional[str] = None,
    routes: Optional[Mapping[str, str]] = None,
    detected: Optional[Mapping[str, bool]] = None,
) -> dict:
    """
    Build a JSON-serializable plan. Never performs I/O.
    detected: provider id -> installed flag, or None when availability is not checked.
    """
    routes = dict(routes or {})
    _check_options(wf, provider_override, routes)

    warnings: List[str] = []
    if registry is None:
        warnings.append("No skill registry supplied: skill compatibility was not checked.")

    assigned: Dict[str, dict] = {}
    for stage in _topological(wf.stages):
        assigned[stage.id] = _plan_stage(wf, stage, registry, provider_override, routes, detected)

    for stage in wf.stages:
        entry = assigned[stage.id]
        entry["independence"] = [
            {
                "of": ref,
                "stage_provider": entry["provider"],
                "other_provider": assigned[ref]["provider"],
                "status": "same-provider" if entry["provider"] == assigned[ref]["provider"] else "distinct-provider",
            }
            for ref in stage.independent_of
        ]

    if any(e["status"] == "same-provider" for s in assigned.values() for e in s["independence"]):
        warnings.append(
            "Stages declaring independent_of share a provider with the stage they should be independent of: "
            "reduced independence. Do not treat these reviews as independent verification. "
            "Running them in a fresh-session context is a weaker control and is not verified by Aurora."
        )

    order = [s.id for s in _topological(wf.stages)]
    stages = [assigned[sid] for sid in order]
    return {
        "plan_schema_version": PLAN_SCHEMA_VERSION,
        "workflow": {"id": wf.id, "version": wf.version, "sha256": wf.sha256},
        "mode": "single" if provider_override else "multi",
        "provider_override": provider_override,
        "routes": routes,
        "detection": dict(detected) if detected is not None else None,
        "order": order,
        "stages": stages,
        "blocked": any(s["status"] == "blocked" for s in stages),
        "warnings": warnings + list(wf.warnings),
    }


def _check_options(wf: WorkflowDefinition, override: Optional[str], routes: Dict[str, str]) -> None:
    if override and routes:
        raise PlanError("--provider (single-provider mode) and --route are mutually exclusive")
    if (override or routes) and not wf.routing.allow_override:
        raise PlanError(f"Workflow '{wf.id}' sets allow_override = false; --provider/--route are not permitted")
    for pid in ([override] if override else []) + list(routes.values()):
        if pid not in PROVIDERS:
            raise PlanError(f"Unknown provider '{pid}'. Known providers: {sorted(PROVIDERS)}")
    for stage_id, pid in routes.items():
        stage = wf.find_stage(stage_id)
        if stage is None:
            raise PlanError(f"--route names unknown stage '{stage_id}'. Stages: {[s.id for s in wf.stages]}")
        eligible = wf.routing.roles[stage.role].eligible
        if pid not in eligible:
            raise PlanError(
                f"--route {stage_id}={pid}: provider '{pid}' is not eligible for role '{stage.role}' "
                f"(eligible: {list(eligible)})"
            )


def _topological(stages: Tuple[Stage, ...]) -> List[Stage]:
    """Kahn's algorithm; ties broken by declaration order."""
    index = {s.id: i for i, s in enumerate(stages)}
    remaining = {s.id: len(set(s.depends_on)) for s in stages}
    dependents: Dict[str, List[str]] = {s.id: [] for s in stages}
    for s in stages:
        for dep in set(s.depends_on):
            dependents[dep].append(s.id)
    heap = [index[sid] for sid, n in remaining.items() if n == 0]
    heapq.heapify(heap)
    ordered: List[Stage] = []
    while heap:
        stage = stages[heapq.heappop(heap)]
        ordered.append(stage)
        for nxt in dependents[stage.id]:
            remaining[nxt] -= 1
            if remaining[nxt] == 0:
                heapq.heappush(heap, index[nxt])
    return ordered


def _incompatibilities(role: Role, provider: str, registry: Optional[RegistryManifest],
                       skills: Tuple[str, ...]) -> List[dict]:
    if registry is None:
        return []
    adapters = PROVIDERS[provider].spec.skill_adapters
    found = []
    for skill_id in skills:
        skill = registry.get_skill(skill_id)
        if skill is None:
            continue
        agents = list(skill.compatible_agents)
        if "universal" in agents or set(agents) & set(adapters):
            continue
        found.append({"skill": skill_id, "provider": provider,
                      "checked_adapters": list(adapters), "compatible_agents": agents})
    return found


def _problems(role: Role, provider: str, registry, detected) -> List[str]:
    reasons = []
    if _incompatibilities(role, provider, registry, role.required_skills):
        reasons.append("incompatible")
    if detected is not None and not detected.get(provider, False):
        reasons.append("unavailable")
    return reasons


def _plan_stage(wf: WorkflowDefinition, stage: Stage, registry, override, routes, detected) -> dict:
    role = wf.roles[stage.role]
    rr = wf.routing.roles[stage.role]
    blocked: List[str] = []
    warnings: List[str] = []

    if override:
        provider, source, fallback_allowed = override, "override", False
        if provider not in rr.eligible:
            blocked.append(f"not-eligible: provider '{provider}' is not eligible for role '{role.id}' "
                           f"(eligible: {list(rr.eligible)}); single-provider mode never substitutes another")
    elif stage.id in routes:
        provider, source, fallback_allowed = routes[stage.id], "route", False
    else:
        provider, source, fallback_allowed = rr.preferred, "preferred", True

    trail: List[dict] = []
    visited = {provider}
    while fallback_allowed:
        reasons = _problems(role, provider, registry, detected)

        def resolved(rule) -> List[str]:
            # A step is taken only if the target does not share the triggering problem.
            target_problems = _problems(role, rule.to_provider, registry, detected)
            return [r for r in reasons if r in rule.on and r not in target_problems]

        rule = next(
            (r for r in wf.routing.fallback
             if r.role == role.id and r.from_provider == provider and r.to_provider not in visited and resolved(r)),
            None,
        )
        if rule is None:
            break
        trail.append({"from": provider, "to": rule.to_provider, "reason": resolved(rule)[0]})
        provider, source = rule.to_provider, "fallback"
        visited.add(provider)

    incompat = _incompatibilities(role, provider, registry, role.required_skills)
    if incompat:
        blocked.append(f"incompatible: required skill(s) {[i['skill'] for i in incompat]} cannot be loaded by "
                       f"provider '{provider}'; no lossy conversion is performed")
    for item in _incompatibilities(role, provider, registry, role.recommended_skills):
        warnings.append(f"recommended skill '{item['skill']}' is not compatible with provider '{provider}'")
    if detected is not None and not detected.get(provider, False):
        warnings.append(f"provider '{provider}' is not installed on this host; the handoff can still be run elsewhere")

    def artifact(stage_id: str, name: str) -> str:
        return f"artifacts/{stage_id}/{name}"

    return {
        "id": stage.id,
        "role": role.id,
        "responsibility": role.responsibility,
        "description": stage.description,
        "depends_on": list(stage.depends_on),
        "provider": provider,
        "assignment_source": source,
        "fallback_trail": trail,
        "status": "blocked" if blocked else "assignable",
        "blocked_reasons": blocked,
        "warnings": warnings,
        "incompatibilities": incompat,
        "required_skills": list(role.required_skills),
        "recommended_skills": list(role.recommended_skills),
        "independence": [],
        "handoff_path": f"handoff/{stage.id}.md",
        "inputs": [artifact(*ref.split("/", 1)) for ref in stage.inputs],
        "outputs": [artifact(stage.id, o) for o in stage.outputs],
        "acceptance": [
            {"id": c.id, "description": c.description, "evidence": [artifact(stage.id, e) for e in c.evidence]}
            for c in stage.acceptance
        ],
        "optional": stage.optional,
        "requires_approval": stage.requires_approval,
        "max_attempts": stage.max_attempts,
        "on_exhausted": stage.on_exhausted,
    }
