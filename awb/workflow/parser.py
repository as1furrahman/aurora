"""
Strict parser and validator for workflow definitions (schema v1, TOML via tomllib).
Collects every error it finds and raises a single WorkflowValidationError.
"""

import hashlib
import re
import tomllib
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Tuple, Union

from awb.providers import PROVIDERS
from awb.registry.schema import RegistryManifest
from awb.registry.validator import SEMVER_PATTERN

from .model import (
    ASSURANCE_LEVELS,
    FALLBACK_TRIGGERS,
    NOT_APPLICABLE_FILE,
    ON_EXHAUSTED,
    SCHEMA_VERSION,
    AcceptanceCriterion,
    FallbackRule,
    Role,
    RoleRouting,
    RoutingPolicy,
    Stage,
    WorkflowDefinition,
    WorkflowValidationError,
)

ID_PATTERN = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
OUTPUT_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]*$")

TOP_KEYS = {"schema_version", "workflow", "roles", "stages", "routing"}
WORKFLOW_KEYS = {"id", "version", "name", "description", "task_types", "assurance"}
ROLE_KEYS = {"responsibility", "required_skills", "recommended_skills"}
# Provider-like keys get a specific message: roles must stay provider-neutral.
ROLE_PROVIDER_KEYS = {"provider", "providers", "preferred_provider", "eligible_providers",
                      "model", "cli", "eligible", "preferred"}
STAGE_KEYS = {"id", "role", "description", "depends_on", "inputs", "outputs", "optional",
              "max_attempts", "on_exhausted", "requires_approval", "independent_of", "acceptance"}
CRITERION_KEYS = {"id", "description", "evidence"}
ROUTING_KEYS = {"allow_override", "roles", "fallback"}
ROLE_ROUTING_KEYS = {"eligible", "preferred"}
FALLBACK_KEYS = {"role", "from", "to", "on"}


class _Checker:
    """Accumulates validation errors with location prefixes."""

    def __init__(self):
        self.errors: List[str] = []
        self.warnings: List[str] = []

    def error(self, loc: str, msg: str) -> None:
        self.errors.append(f"{loc}: {msg}")

    def keys(self, loc: str, table: Dict[str, Any], allowed: Set[str]) -> None:
        for key in table:
            if key not in allowed:
                self.error(loc, f"unknown key '{key}'")

    def table(self, loc: str, value: Any) -> Dict[str, Any]:
        if value is None:
            return {}
        if not isinstance(value, dict):
            self.error(loc, "must be a table")
            return {}
        return value

    def string(self, loc: str, table: Dict[str, Any], key: str, required: bool = True, default: str = "") -> str:
        value = table.get(key)
        if value is None:
            if required:
                self.error(loc, f"missing required string '{key}'")
            return default
        if not isinstance(value, str) or (required and not value.strip()):
            self.error(loc, f"'{key}' must be a non-empty string")
            return default
        return value

    def boolean(self, loc: str, table: Dict[str, Any], key: str, default: bool) -> bool:
        value = table.get(key, default)
        if not isinstance(value, bool):
            self.error(loc, f"'{key}' must be a boolean")
            return default
        return value

    def str_list(self, loc: str, table: Dict[str, Any], key: str) -> Tuple[str, ...]:
        value = table.get(key, [])
        if not isinstance(value, list) or not all(isinstance(v, str) for v in value):
            self.error(loc, f"'{key}' must be a list of strings")
            return ()
        return tuple(value)

    def ident(self, loc: str, what: str, value: str) -> None:
        if value and not ID_PATTERN.match(value):
            self.error(loc, f"{what} '{value}' must be lowercase alphanumeric with hyphens")


def parse_workflow_string(
    text: str,
    registry: Optional[RegistryManifest] = None,
    source: str = "<string>",
) -> WorkflowDefinition:
    """Parse and validate a workflow definition from TOML text."""
    return _parse(text, hashlib.sha256(text.encode("utf-8")).hexdigest(), registry, source)


def load_workflow(path: Union[str, Path], registry: Optional[RegistryManifest] = None) -> WorkflowDefinition:
    """Load and validate a workflow definition file. sha256 covers the exact file bytes."""
    p = Path(path)
    try:
        raw = p.read_bytes()
    except OSError as e:
        raise WorkflowValidationError([f"cannot read file: {e}"], source=str(p)) from e
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError as e:
        raise WorkflowValidationError([f"file is not valid UTF-8: {e}"], source=str(p)) from e
    return _parse(text, hashlib.sha256(raw).hexdigest(), registry, str(p))


def _parse(text: str, sha256: str, registry: Optional[RegistryManifest], source: str) -> WorkflowDefinition:
    try:
        data = tomllib.loads(text)
    except tomllib.TOMLDecodeError as e:
        raise WorkflowValidationError([f"invalid TOML: {e}"], source=source) from e

    c = _Checker()
    c.keys("<root>", data, TOP_KEYS)

    version = data.get("schema_version")
    if version != SCHEMA_VERSION or isinstance(version, bool):
        c.error("<root>", f"unsupported schema_version {version!r}; expected {SCHEMA_VERSION}")

    wf = c.table("workflow", data.get("workflow"))
    c.keys("workflow", wf, WORKFLOW_KEYS)
    wf_id = c.string("workflow", wf, "id")
    c.ident("workflow", "workflow.id", wf_id)
    wf_version = c.string("workflow", wf, "version")
    if wf_version and not SEMVER_PATTERN.match(wf_version):
        c.error("workflow", f"version '{wf_version}' is not valid semver")
    wf_name = c.string("workflow", wf, "name")
    wf_description = c.string("workflow", wf, "description", required=False)
    task_types = c.str_list("workflow", wf, "task_types")
    assurance = c.string("workflow", wf, "assurance", required=False, default="standard")
    if assurance not in ASSURANCE_LEVELS:
        c.error("workflow", f"assurance '{assurance}' must be one of {list(ASSURANCE_LEVELS)}")

    roles = _parse_roles(c, data.get("roles"), registry)
    stages = _parse_stages(c, data.get("stages"), roles)
    routing = _parse_routing(c, data.get("routing"), roles)

    if c.errors:
        raise WorkflowValidationError(c.errors, source=source)

    return WorkflowDefinition(
        schema_version=SCHEMA_VERSION,
        id=wf_id,
        version=wf_version,
        name=wf_name,
        assurance=assurance,
        roles=roles,
        stages=stages,
        routing=routing,
        sha256=sha256,
        description=wf_description,
        task_types=task_types,
        warnings=tuple(c.warnings),
    )


def _parse_roles(c: _Checker, raw: Any, registry: Optional[RegistryManifest]) -> Dict[str, Role]:
    table = c.table("roles", raw)
    if not table:
        c.error("roles", "at least one role is required")
    roles: Dict[str, Role] = {}
    for role_id, body in table.items():
        loc = f"roles.{role_id}"
        c.ident(loc, "role id", role_id)
        body = c.table(loc, body)
        for key in body:
            if key in ROLE_PROVIDER_KEYS:
                c.error(loc, f"key '{key}' is not allowed in a role (role != provider; "
                             f"assign providers in [routing.roles.{role_id}])")
            elif key not in ROLE_KEYS:
                c.error(loc, f"unknown key '{key}'")
        required = c.str_list(loc, body, "required_skills")
        recommended = c.str_list(loc, body, "recommended_skills")
        if registry is not None:
            for skill in required:
                if registry.get_skill(skill) is None:
                    c.error(loc, f"required skill '{skill}' is not defined in the skill registry")
            for skill in recommended:
                if registry.get_skill(skill) is None:
                    c.warnings.append(f"{loc}: recommended skill '{skill}' is not defined in the skill registry")
        roles[role_id] = Role(
            id=role_id,
            responsibility=c.string(loc, body, "responsibility"),
            required_skills=required,
            recommended_skills=recommended,
        )
    return roles


def _parse_stages(c: _Checker, raw: Any, roles: Dict[str, Role]) -> Tuple[Stage, ...]:
    if not isinstance(raw, list) or not raw:
        c.error("stages", "at least one [[stages]] entry is required")
        return ()

    stages: List[Stage] = []
    seen: Set[str] = set()
    for index, body in enumerate(raw):
        body = c.table(f"stages[{index}]", body)
        stage_id = body.get("id") if isinstance(body.get("id"), str) else ""
        loc = f"stages.{stage_id}" if stage_id else f"stages[{index}]"
        c.keys(loc, body, STAGE_KEYS)
        stage_id = c.string(loc, body, "id")
        c.ident(loc, "stage id", stage_id)
        if stage_id in seen:
            c.error(loc, f"duplicate stage id '{stage_id}'")
        seen.add(stage_id)

        role = c.string(loc, body, "role")
        if role and role not in roles:
            c.error(loc, f"unknown role '{role}'")

        outputs = c.str_list(loc, body, "outputs")
        if not outputs:
            c.error(loc, "at least one output is required")
        for out in outputs:
            if not OUTPUT_PATTERN.match(out) or out == NOT_APPLICABLE_FILE:
                c.error(loc, f"output '{out}' must be a plain file name (no path separators, not hidden, "
                             f"not '{NOT_APPLICABLE_FILE}')")
        if len(set(outputs)) != len(outputs):
            c.error(loc, "outputs must be unique")

        max_attempts = body.get("max_attempts", 2)
        if not isinstance(max_attempts, int) or isinstance(max_attempts, bool) or not 1 <= max_attempts <= 10:
            c.error(loc, "max_attempts must be an integer between 1 and 10")
            max_attempts = 2
        on_exhausted = c.string(loc, body, "on_exhausted", required=False, default="escalate")
        if on_exhausted not in ON_EXHAUSTED:
            c.error(loc, f"on_exhausted '{on_exhausted}' must be one of {list(ON_EXHAUSTED)}")

        stages.append(Stage(
            id=stage_id,
            role=role,
            outputs=outputs,
            acceptance=_parse_acceptance(c, loc, body.get("acceptance"), outputs),
            description=c.string(loc, body, "description", required=False),
            depends_on=c.str_list(loc, body, "depends_on"),
            inputs=c.str_list(loc, body, "inputs"),
            optional=c.boolean(loc, body, "optional", False),
            max_attempts=max_attempts,
            on_exhausted=on_exhausted,
            requires_approval=c.boolean(loc, body, "requires_approval", False),
            independent_of=c.str_list(loc, body, "independent_of"),
        ))

    _check_graph(c, stages)
    return tuple(stages)


def _parse_acceptance(c: _Checker, loc: str, raw: Any, outputs: Tuple[str, ...]) -> Tuple[AcceptanceCriterion, ...]:
    if not isinstance(raw, list) or not raw:
        c.error(loc, "at least one [[stages.acceptance]] criterion is required")
        return ()
    criteria: List[AcceptanceCriterion] = []
    seen: Set[str] = set()
    for index, body in enumerate(raw):
        cloc = f"{loc}.acceptance[{index}]"
        body = c.table(cloc, body)
        c.keys(cloc, body, CRITERION_KEYS)
        crit_id = c.string(cloc, body, "id")
        c.ident(cloc, "criterion id", crit_id)
        if crit_id in seen:
            c.error(cloc, f"duplicate criterion id '{crit_id}'")
        seen.add(crit_id)
        evidence = c.str_list(cloc, body, "evidence")
        if not evidence:
            c.error(cloc, "evidence must name at least one output")
        for item in evidence:
            if item not in outputs:
                c.error(cloc, f"evidence '{item}' is not one of the stage outputs {list(outputs)}")
        criteria.append(AcceptanceCriterion(id=crit_id, description=c.string(cloc, body, "description"),
                                            evidence=evidence))
    return tuple(criteria)


def _check_graph(c: _Checker, stages: List[Stage]) -> None:
    by_id = {s.id: s for s in stages}
    for s in stages:
        for dep in s.depends_on:
            if dep not in by_id:
                c.error(f"stages.{s.id}", f"depends_on references unknown stage '{dep}'")
            elif dep == s.id:
                c.error(f"stages.{s.id}", "a stage cannot depend on itself")
        for ref in s.independent_of:
            if ref not in by_id or ref == s.id:
                c.error(f"stages.{s.id}", f"independent_of '{ref}' must name another existing stage")

    cycle = find_cycle(stages)
    if cycle:
        c.error("stages", "dependency cycle: " + " -> ".join(cycle))
        return

    for s in stages:
        ancestors = transitive_dependencies(s.id, by_id)
        for ref in s.inputs:
            src, _, name = ref.partition("/")
            if src not in ancestors or name not in by_id[src].outputs:
                c.error(f"stages.{s.id}", f"input '{ref}' must be '<stage>/<output>' from a transitive dependency")


def find_cycle(stages) -> List[str]:
    """Return a dependency cycle as [a, b, ..., a] (each item required before the next), or []."""
    known = {s.id for s in stages}
    dependents: Dict[str, List[str]] = {s.id: [] for s in stages}
    for s in stages:
        for dep in s.depends_on:
            if dep in known and dep != s.id:
                dependents[dep].append(s.id)

    state: Dict[str, int] = {}
    path: List[str] = []

    def visit(node: str) -> List[str]:
        state[node] = 1
        path.append(node)
        for nxt in dependents[node]:
            if state.get(nxt) == 1:
                return path[path.index(nxt):] + [nxt]
            if nxt not in state:
                found = visit(nxt)
                if found:
                    return found
        path.pop()
        state[node] = 2
        return []

    for s in stages:
        if s.id not in state:
            found = visit(s.id)
            if found:
                return found
    return []


def transitive_dependencies(stage_id: str, by_id: Dict[str, Stage]) -> Set[str]:
    """All stages that must complete before stage_id (acyclic graph assumed)."""
    result: Set[str] = set()
    stack = list(by_id[stage_id].depends_on)
    while stack:
        dep = stack.pop()
        if dep in result or dep not in by_id:
            continue
        result.add(dep)
        stack.extend(by_id[dep].depends_on)
    return result


def _parse_routing(c: _Checker, raw: Any, roles: Dict[str, Role]) -> RoutingPolicy:
    table = c.table("routing", raw)
    c.keys("routing", table, ROUTING_KEYS)
    allow_override = c.boolean("routing", table, "allow_override", True)

    role_tables = c.table("routing.roles", table.get("roles"))
    routed: Dict[str, RoleRouting] = {}
    for role_id in role_tables:
        if role_id not in roles:
            c.error(f"routing.roles.{role_id}", f"routing declared for undeclared role '{role_id}'")
    for role_id in roles:
        loc = f"routing.roles.{role_id}"
        if role_id not in role_tables:
            c.error(loc, "missing routing entry; every role needs an eligible provider list")
            continue
        body = c.table(loc, role_tables[role_id])
        c.keys(loc, body, ROLE_ROUTING_KEYS)
        eligible = c.str_list(loc, body, "eligible")
        if not eligible:
            c.error(loc, "eligible must list at least one provider")
        if len(set(eligible)) != len(eligible):
            c.error(loc, "eligible providers must be unique")
        for pid in eligible:
            if pid not in PROVIDERS:
                c.error(loc, f"unknown provider '{pid}'; known providers: {sorted(PROVIDERS)}")
        preferred = c.string(loc, body, "preferred", required=False, default=eligible[0] if eligible else "")
        if eligible and preferred not in eligible:
            c.error(loc, f"preferred provider '{preferred}' is not in eligible {list(eligible)}")
        routed[role_id] = RoleRouting(role=role_id, eligible=eligible, preferred=preferred)

    fallback: List[FallbackRule] = []
    raw_fallback = table.get("fallback", [])
    if not isinstance(raw_fallback, list):
        c.error("routing.fallback", "must be an array of tables")
        raw_fallback = []
    for index, body in enumerate(raw_fallback):
        loc = f"routing.fallback[{index}]"
        body = c.table(loc, body)
        c.keys(loc, body, FALLBACK_KEYS)
        role_id = c.string(loc, body, "role")
        src = c.string(loc, body, "from")
        dst = c.string(loc, body, "to")
        on = c.str_list(loc, body, "on")
        eligible = routed[role_id].eligible if role_id in routed else ()
        if role_id and role_id not in roles:
            c.error(loc, f"unknown role '{role_id}'")
        for pid, label in ((src, "from"), (dst, "to")):
            if pid and pid not in eligible:
                c.error(loc, f"'{label}' provider '{pid}' is not eligible for role '{role_id}'")
        if src and src == dst:
            c.error(loc, "'from' and 'to' must not be the same provider")
        if not on:
            c.error(loc, f"'on' must list at least one trigger from {list(FALLBACK_TRIGGERS)}")
        for trigger in on:
            if trigger not in FALLBACK_TRIGGERS:
                c.error(loc, f"unknown fallback trigger '{trigger}'; allowed: {list(FALLBACK_TRIGGERS)}")
        fallback.append(FallbackRule(role=role_id, from_provider=src, to_provider=dst, on=on))

    return RoutingPolicy(allow_override=allow_override, roles=routed, fallback=tuple(fallback))
