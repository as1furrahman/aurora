//! Strict parser and validator for workflow definitions (schema v1).
//!
//! The parser collects every validation error it finds and reports them together, rather than
//! stopping at the first.

use std::collections::{HashMap, HashSet};

use toml::{Table, Value};

use super::model::{
    ASSURANCE_LEVELS, AcceptanceCriterion, FALLBACK_TRIGGERS, FallbackRule, NOT_APPLICABLE_FILE, ON_EXHAUSTED, Role,
    RoleRouting, RoutingPolicy, SCHEMA_VERSION, Stage, WorkflowDefinition, WorkflowValidationError,
};
use crate::providers::{CAPABILITIES, PROVIDER_IDS, is_known_provider};
use crate::registry::Registry;
use crate::util::{is_kebab_id, is_plain_file_name, is_semver, list_repr, sha256_hex};

const TOP_KEYS: &[&str] = &["schema_version", "workflow", "roles", "stages", "routing"];
const WORKFLOW_KEYS: &[&str] = &["id", "version", "name", "description", "task_types", "assurance"];
const ROLE_KEYS: &[&str] = &["responsibility", "required_skills", "recommended_skills", "requires"];
/// Provider-like keys get a specific message: roles must stay provider-neutral.
const ROLE_PROVIDER_KEYS: &[&str] =
    &["provider", "providers", "preferred_provider", "eligible_providers", "model", "cli", "eligible", "preferred"];
const STAGE_KEYS: &[&str] = &[
    "id",
    "role",
    "description",
    "depends_on",
    "inputs",
    "outputs",
    "optional",
    "max_attempts",
    "on_exhausted",
    "requires_approval",
    "independent_of",
    "verdict",
    "acceptance",
];
const CRITERION_KEYS: &[&str] = &["id", "description", "evidence"];
const ROUTING_KEYS: &[&str] = &["allow_override", "roles", "fallback"];
const ROLE_ROUTING_KEYS: &[&str] = &["eligible", "preferred"];
const FALLBACK_KEYS: &[&str] = &["role", "from", "to", "on"];

/// Parse and validate a definition from TOML text. `sha256` covers the UTF-8 bytes of `text`.
pub fn parse_workflow_str(
    text: &str,
    registry: Option<&Registry>,
    source: &str,
) -> Result<WorkflowDefinition, WorkflowValidationError> {
    parse(text, sha256_hex(text.as_bytes()), registry, source)
}

/// Parse and validate exact definition bytes. `sha256` covers these bytes.
pub fn parse_workflow_bytes(
    raw: &[u8],
    registry: Option<&Registry>,
    source: &str,
) -> Result<WorkflowDefinition, WorkflowValidationError> {
    let text = std::str::from_utf8(raw).map_err(|e| WorkflowValidationError {
        errors: vec![format!("file is not valid UTF-8: {e}")],
        source_name: source.to_string(),
    })?;
    parse(text, sha256_hex(raw), registry, source)
}

/// Accumulates validation errors with location prefixes.
#[derive(Default)]
struct Checker {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl Checker {
    fn error(&mut self, loc: &str, msg: impl AsRef<str>) {
        self.errors.push(format!("{loc}: {}", msg.as_ref()));
    }

    fn keys(&mut self, loc: &str, table: &Table, allowed: &[&str]) {
        for key in table.keys() {
            if !allowed.contains(&key.as_str()) {
                self.error(loc, format!("unknown key '{key}'"));
            }
        }
    }

    /// Absent → empty table; non-table → error and empty table.
    fn table(&mut self, loc: &str, value: Option<&Value>) -> Table {
        match value {
            None => Table::new(),
            Some(Value::Table(t)) => t.clone(),
            Some(_) => {
                self.error(loc, "must be a table");
                Table::new()
            }
        }
    }

    fn string(&mut self, loc: &str, table: &Table, key: &str, required: bool, default: &str) -> String {
        match table.get(key) {
            None => {
                if required {
                    self.error(loc, format!("missing required string '{key}'"));
                }
                default.to_string()
            }
            Some(Value::String(s)) if !(required && s.trim().is_empty()) => s.clone(),
            Some(_) => {
                self.error(loc, format!("'{key}' must be a non-empty string"));
                default.to_string()
            }
        }
    }

    fn boolean(&mut self, loc: &str, table: &Table, key: &str, default: bool) -> bool {
        match table.get(key) {
            None => default,
            Some(Value::Boolean(b)) => *b,
            Some(_) => {
                self.error(loc, format!("'{key}' must be a boolean"));
                default
            }
        }
    }

    fn str_list(&mut self, loc: &str, table: &Table, key: &str) -> Vec<String> {
        let parsed = match table.get(key) {
            None => Some(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            Some(_) => None,
        };
        parsed.unwrap_or_else(|| {
            self.error(loc, format!("'{key}' must be a list of strings"));
            Vec::new()
        })
    }

    fn ident(&mut self, loc: &str, what: &str, value: &str) {
        if !value.is_empty() && !is_kebab_id(value) {
            self.error(loc, format!("{what} '{value}' must be lowercase alphanumeric with hyphens"));
        }
    }
}

/// legacy-compatible `repr` for the few value kinds that appear in messages.
fn scalar_repr(value: Option<&Value>) -> String {
    match value {
        None => "None".into(),
        Some(Value::String(s)) => format!("'{s}'"),
        Some(Value::Integer(i)) => i.to_string(),
        Some(Value::Float(f)) => f.to_string(),
        Some(Value::Boolean(b)) => if *b { "True" } else { "False" }.into(),
        Some(other) => other.to_string(),
    }
}

fn parse(
    text: &str,
    sha256: String,
    registry: Option<&Registry>,
    source: &str,
) -> Result<WorkflowDefinition, WorkflowValidationError> {
    let data: Table = toml::from_str(text).map_err(|e| WorkflowValidationError {
        errors: vec![format!("invalid TOML: {e}")],
        source_name: source.to_string(),
    })?;

    let mut c = Checker::default();
    c.keys("<root>", &data, TOP_KEYS);

    let version = data.get("schema_version");
    if !matches!(version, Some(Value::Integer(v)) if *v == SCHEMA_VERSION) {
        c.error("<root>", format!("unsupported schema_version {}; expected {SCHEMA_VERSION}", scalar_repr(version)));
    }

    let wf = c.table("workflow", data.get("workflow"));
    c.keys("workflow", &wf, WORKFLOW_KEYS);
    let wf_id = c.string("workflow", &wf, "id", true, "");
    c.ident("workflow", "workflow.id", &wf_id);
    let wf_version = c.string("workflow", &wf, "version", true, "");
    if !wf_version.is_empty() && !is_semver(&wf_version) {
        c.error("workflow", format!("version '{wf_version}' is not valid semver"));
    }
    let wf_name = c.string("workflow", &wf, "name", true, "");
    let wf_description = c.string("workflow", &wf, "description", false, "");
    let task_types = c.str_list("workflow", &wf, "task_types");
    let assurance = c.string("workflow", &wf, "assurance", false, "standard");
    if !ASSURANCE_LEVELS.contains(&assurance.as_str()) {
        c.error("workflow", format!("assurance '{assurance}' must be one of {}", list_repr(&ASSURANCE_LEVELS)));
    }

    let roles = parse_roles(&mut c, data.get("roles"), registry);
    let stages = parse_stages(&mut c, data.get("stages"), &roles);
    let routing = parse_routing(&mut c, data.get("routing"), &roles);

    if !c.errors.is_empty() {
        return Err(WorkflowValidationError { errors: c.errors, source_name: source.to_string() });
    }

    Ok(WorkflowDefinition {
        schema_version: SCHEMA_VERSION,
        id: wf_id,
        version: wf_version,
        name: wf_name,
        assurance,
        roles,
        stages,
        routing,
        sha256,
        description: wf_description,
        task_types,
        warnings: c.warnings,
    })
}

fn parse_roles(c: &mut Checker, raw: Option<&Value>, registry: Option<&Registry>) -> Vec<Role> {
    let table = c.table("roles", raw);
    if table.is_empty() {
        c.error("roles", "at least one role is required");
    }
    let mut roles = Vec::new();
    for (role_id, body) in &table {
        let loc = format!("roles.{role_id}");
        c.ident(&loc, "role id", role_id);
        let body = c.table(&loc, Some(body));
        for key in body.keys() {
            if ROLE_PROVIDER_KEYS.contains(&key.as_str()) {
                c.error(
                    &loc,
                    format!(
                        "key '{key}' is not allowed in a role (role != provider; assign providers in \
                         [routing.roles.{role_id}])"
                    ),
                );
            } else if !ROLE_KEYS.contains(&key.as_str()) {
                c.error(&loc, format!("unknown key '{key}'"));
            }
        }
        let required = c.str_list(&loc, &body, "required_skills");
        let recommended = c.str_list(&loc, &body, "recommended_skills");
        let requires = c.str_list(&loc, &body, "requires");
        for cap in &requires {
            if !CAPABILITIES.contains(&cap.as_str()) {
                c.error(&loc, format!("unknown capability '{cap}'; known: {}", list_repr(&CAPABILITIES)));
            }
        }
        if let Some(registry) = registry {
            for skill in &required {
                if registry.get(skill).is_none() {
                    c.error(&loc, format!("required skill '{skill}' is not defined in the skill registry"));
                }
            }
            for skill in &recommended {
                if registry.get(skill).is_none() {
                    c.warnings.push(format!("{loc}: recommended skill '{skill}' is not defined in the skill registry"));
                }
            }
        }
        let responsibility = c.string(&loc, &body, "responsibility", true, "");
        roles.push(Role {
            id: role_id.clone(),
            responsibility,
            required_skills: required,
            recommended_skills: recommended,
            requires,
        });
    }
    roles
}

fn parse_stages(c: &mut Checker, raw: Option<&Value>, roles: &[Role]) -> Vec<Stage> {
    let items = match raw {
        Some(Value::Array(items)) if !items.is_empty() => items,
        _ => {
            c.error("stages", "at least one [[stages]] entry is required");
            return Vec::new();
        }
    };

    let mut stages = Vec::new();
    let mut seen = HashSet::new();
    for (index, body) in items.iter().enumerate() {
        let body = c.table(&format!("stages[{index}]"), Some(body));
        let loc = match body.get("id") {
            Some(Value::String(s)) if !s.is_empty() => format!("stages.{s}"),
            _ => format!("stages[{index}]"),
        };
        c.keys(&loc, &body, STAGE_KEYS);
        let stage_id = c.string(&loc, &body, "id", true, "");
        c.ident(&loc, "stage id", &stage_id);
        if !seen.insert(stage_id.clone()) {
            c.error(&loc, format!("duplicate stage id '{stage_id}'"));
        }

        let role = c.string(&loc, &body, "role", true, "");
        if !role.is_empty() && !roles.iter().any(|r| r.id == role) {
            c.error(&loc, format!("unknown role '{role}'"));
        }

        let outputs = c.str_list(&loc, &body, "outputs");
        if outputs.is_empty() {
            c.error(&loc, "at least one output is required");
        }
        for out in &outputs {
            if !is_plain_file_name(out) || out == NOT_APPLICABLE_FILE {
                c.error(
                    &loc,
                    format!(
                        "output '{out}' must be a plain file name (no path separators, not hidden, not \
                         '{NOT_APPLICABLE_FILE}')"
                    ),
                );
            }
        }
        if outputs.iter().collect::<HashSet<_>>().len() != outputs.len() {
            c.error(&loc, "outputs must be unique");
        }

        let max_attempts = match body.get("max_attempts") {
            None => 2,
            Some(Value::Integer(n)) if (1..=10).contains(n) => *n as u32,
            Some(_) => {
                c.error(&loc, "max_attempts must be an integer between 1 and 10");
                2
            }
        };
        let on_exhausted = c.string(&loc, &body, "on_exhausted", false, "escalate");
        if matches!(body.get("optional"), Some(Value::Boolean(true)))
            && matches!(body.get("requires_approval"), Some(Value::Boolean(true)))
        {
            c.error(
                &loc,
                "an optional stage cannot set requires_approval in schema v1 (NOT_APPLICABLE would bypass \
                 the approval gate)",
            );
        }
        if !ON_EXHAUSTED.contains(&on_exhausted.as_str()) {
            c.error(&loc, format!("on_exhausted '{on_exhausted}' must be one of {}", list_repr(&ON_EXHAUSTED)));
        }

        let acceptance = parse_acceptance(c, &loc, body.get("acceptance"), &outputs);
        let description = c.string(&loc, &body, "description", false, "");
        let depends_on = c.str_list(&loc, &body, "depends_on");
        let inputs = c.str_list(&loc, &body, "inputs");
        let optional = c.boolean(&loc, &body, "optional", false);
        let requires_approval = c.boolean(&loc, &body, "requires_approval", false);
        let independent_of = c.str_list(&loc, &body, "independent_of");
        let verdict = match body.get("verdict") {
            None => None,
            Some(Value::String(v)) => {
                if !outputs.contains(v) {
                    c.error(&loc, format!("verdict '{v}' must be one of the stage outputs {}", list_repr(&outputs)));
                } else if !v.ends_with(".json") {
                    c.error(&loc, format!("verdict '{v}' must be a .json file"));
                }
                Some(v.clone())
            }
            Some(_) => {
                c.error(&loc, "'verdict' must be a non-empty string");
                None
            }
        };
        stages.push(Stage {
            id: stage_id,
            role,
            outputs,
            acceptance,
            description,
            depends_on,
            inputs,
            optional,
            max_attempts,
            on_exhausted,
            requires_approval,
            independent_of,
            verdict,
        });
    }

    check_graph(c, &stages);
    stages
}

fn parse_acceptance(c: &mut Checker, loc: &str, raw: Option<&Value>, outputs: &[String]) -> Vec<AcceptanceCriterion> {
    let items = match raw {
        Some(Value::Array(items)) if !items.is_empty() => items,
        _ => {
            c.error(loc, "at least one [[stages.acceptance]] criterion is required");
            return Vec::new();
        }
    };
    let mut criteria = Vec::new();
    let mut seen = HashSet::new();
    for (index, body) in items.iter().enumerate() {
        let cloc = format!("{loc}.acceptance[{index}]");
        let body = c.table(&cloc, Some(body));
        c.keys(&cloc, &body, CRITERION_KEYS);
        let crit_id = c.string(&cloc, &body, "id", true, "");
        c.ident(&cloc, "criterion id", &crit_id);
        if !seen.insert(crit_id.clone()) {
            c.error(&cloc, format!("duplicate criterion id '{crit_id}'"));
        }
        let evidence = c.str_list(&cloc, &body, "evidence");
        if evidence.is_empty() {
            c.error(&cloc, "evidence must name at least one output");
        }
        for item in &evidence {
            if !outputs.contains(item) {
                c.error(&cloc, format!("evidence '{item}' is not one of the stage outputs {}", list_repr(outputs)));
            }
        }
        let description = c.string(&cloc, &body, "description", true, "");
        criteria.push(AcceptanceCriterion { id: crit_id, description, evidence });
    }
    criteria
}

fn check_graph(c: &mut Checker, stages: &[Stage]) {
    // Later duplicates win; duplicates are already reported.
    let by_id: HashMap<&str, &Stage> = stages.iter().map(|s| (s.id.as_str(), s)).collect();
    for s in stages {
        let loc = format!("stages.{}", s.id);
        for dep in &s.depends_on {
            if !by_id.contains_key(dep.as_str()) {
                c.error(&loc, format!("depends_on references unknown stage '{dep}'"));
            } else if *dep == s.id {
                c.error(&loc, "a stage cannot depend on itself");
            }
        }
        for r in &s.independent_of {
            if !by_id.contains_key(r.as_str()) || *r == s.id {
                c.error(&loc, format!("independent_of '{r}' must name another existing stage"));
            }
        }
    }

    let cycle = find_cycle(stages);
    if !cycle.is_empty() {
        c.error("stages", format!("dependency cycle: {}", cycle.join(" -> ")));
        return;
    }

    for s in stages {
        let ancestors = transitive_dependencies(&s.id, &by_id);
        for r in &s.inputs {
            let (src, name) = r.split_once('/').unwrap_or((r.as_str(), ""));
            let ok = ancestors.contains(src) && by_id.get(src).is_some_and(|d| d.outputs.iter().any(|o| o == name));
            if !ok {
                c.error(
                    &format!("stages.{}", s.id),
                    format!("input '{r}' must be '<stage>/<output>' from a transitive dependency"),
                );
            }
        }
    }
}

/// A dependency cycle as `[a, b, ..., a]` (each item required before the next), or empty.
pub fn find_cycle(stages: &[Stage]) -> Vec<String> {
    let known: HashSet<&str> = stages.iter().map(|s| s.id.as_str()).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = stages.iter().map(|s| (s.id.as_str(), Vec::new())).collect();
    for s in stages {
        for dep in &s.depends_on {
            if known.contains(dep.as_str())
                && *dep != s.id
                && let Some(list) = dependents.get_mut(dep.as_str())
            {
                list.push(s.id.as_str());
            }
        }
    }

    fn visit<'a>(
        node: &'a str,
        dependents: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Vec<String> {
        state.insert(node, 1);
        path.push(node);
        for &next in dependents.get(node).map(Vec::as_slice).unwrap_or(&[]) {
            match state.get(next) {
                Some(1) => {
                    let start = path.iter().position(|p| *p == next).unwrap_or(0);
                    let mut cycle: Vec<String> = path[start..].iter().map(|s| (*s).to_string()).collect();
                    cycle.push(next.to_string());
                    return cycle;
                }
                None => {
                    let found = visit(next, dependents, state, path);
                    if !found.is_empty() {
                        return found;
                    }
                }
                _ => {}
            }
        }
        path.pop();
        state.insert(node, 2);
        Vec::new()
    }

    let mut state = HashMap::new();
    let mut path = Vec::new();
    for s in stages {
        if !state.contains_key(s.id.as_str()) {
            let found = visit(s.id.as_str(), &dependents, &mut state, &mut path);
            if !found.is_empty() {
                return found;
            }
        }
    }
    Vec::new()
}

/// All stages that must complete before `stage_id` (acyclic graph assumed).
fn transitive_dependencies<'a>(stage_id: &str, by_id: &HashMap<&'a str, &'a Stage>) -> HashSet<&'a str> {
    let mut result = HashSet::new();
    let mut stack: Vec<&str> =
        by_id.get(stage_id).map(|s| s.depends_on.iter().map(String::as_str).collect()).unwrap_or_default();
    while let Some(dep) = stack.pop() {
        let Some((&key, stage)) = by_id.get_key_value(dep) else { continue };
        if result.insert(key) {
            stack.extend(stage.depends_on.iter().map(String::as_str));
        }
    }
    result
}

fn parse_routing(c: &mut Checker, raw: Option<&Value>, roles: &[Role]) -> RoutingPolicy {
    let table = c.table("routing", raw);
    c.keys("routing", &table, ROUTING_KEYS);
    let allow_override = c.boolean("routing", &table, "allow_override", true);

    let role_tables = c.table("routing.roles", table.get("roles"));
    for role_id in role_tables.keys() {
        if !roles.iter().any(|r| &r.id == role_id) {
            c.error(&format!("routing.roles.{role_id}"), format!("routing declared for undeclared role '{role_id}'"));
        }
    }

    let mut routed: Vec<RoleRouting> = Vec::new();
    for role in roles {
        let loc = format!("routing.roles.{}", role.id);
        let Some(raw_body) = role_tables.get(&role.id) else {
            c.error(&loc, "missing routing entry; every role needs an eligible provider list");
            continue;
        };
        let body = c.table(&loc, Some(raw_body));
        c.keys(&loc, &body, ROLE_ROUTING_KEYS);
        let eligible = c.str_list(&loc, &body, "eligible");
        if eligible.is_empty() {
            c.error(&loc, "eligible must list at least one provider");
        }
        if eligible.iter().collect::<HashSet<_>>().len() != eligible.len() {
            c.error(&loc, "eligible providers must be unique");
        }
        for pid in &eligible {
            if !is_known_provider(pid) {
                c.error(&loc, format!("unknown provider '{pid}'; known providers: {}", list_repr(&PROVIDER_IDS)));
            }
        }
        let default = eligible.first().cloned().unwrap_or_default();
        let preferred = c.string(&loc, &body, "preferred", false, &default);
        if !eligible.is_empty() && !eligible.contains(&preferred) {
            c.error(&loc, format!("preferred provider '{preferred}' is not in eligible {}", list_repr(&eligible)));
        }
        routed.push(RoleRouting { role: role.id.clone(), eligible, preferred });
    }

    let mut fallback = Vec::new();
    let raw_rules: Vec<Value> = match table.get("fallback") {
        None => Vec::new(),
        Some(Value::Array(items)) => items.clone(),
        Some(_) => {
            c.error("routing.fallback", "must be an array of tables");
            Vec::new()
        }
    };
    for (index, body) in raw_rules.iter().enumerate() {
        let loc = format!("routing.fallback[{index}]");
        let body = c.table(&loc, Some(body));
        c.keys(&loc, &body, FALLBACK_KEYS);
        let role_id = c.string(&loc, &body, "role", true, "");
        let src = c.string(&loc, &body, "from", true, "");
        let dst = c.string(&loc, &body, "to", true, "");
        let on = c.str_list(&loc, &body, "on");
        let eligible: &[String] =
            routed.iter().find(|r| r.role == role_id).map(|r| r.eligible.as_slice()).unwrap_or(&[]);
        if !role_id.is_empty() && !roles.iter().any(|r| r.id == role_id) {
            c.error(&loc, format!("unknown role '{role_id}'"));
        }
        for (pid, label) in [(&src, "from"), (&dst, "to")] {
            if !pid.is_empty() && !eligible.contains(pid) {
                c.error(&loc, format!("'{label}' provider '{pid}' is not eligible for role '{role_id}'"));
            }
        }
        if !src.is_empty() && src == dst {
            c.error(&loc, "'from' and 'to' must not be the same provider");
        }
        if on.is_empty() {
            c.error(&loc, format!("'on' must list at least one trigger from {}", list_repr(&FALLBACK_TRIGGERS)));
        }
        for trigger in &on {
            if !FALLBACK_TRIGGERS.contains(&trigger.as_str()) {
                c.error(
                    &loc,
                    format!("unknown fallback trigger '{trigger}'; allowed: {}", list_repr(&FALLBACK_TRIGGERS)),
                );
            }
        }
        fallback.push(FallbackRule { role: role_id, from_provider: src, to_provider: dst, on });
    }

    RoutingPolicy { allow_override, roles: routed, fallback }
}
