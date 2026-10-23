//! Deterministic, side-effect-free execution planning (ADR-0004). Port of
//! `awb/workflow/planner.py`: same definition + registry + options + detected set ⇒ identical
//! plan, with JSON field names identical to the Python plan.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::model::{Role, Stage, WorkflowDefinition};
use crate::providers::{PROVIDER_IDS, Strength, get_provider, is_known_provider};
use crate::registry::Registry;
use crate::util::py_list;

pub const PLAN_SCHEMA_VERSION: u32 = 1;

pub const REDUCED_INDEPENDENCE_WARNING: &str = "Stages declaring independent_of share a provider with the stage they \
     should be independent of: reduced independence. Do not treat these reviews as independent verification. Running \
     them in a fresh-session context is a weaker control and is not verified by Aurora.";

/// Invalid planning options (unknown or ineligible provider, forbidden override).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PlanError(pub String);

/// Provider id → installed flag, or `None` when availability is not checked.
pub type Detection = BTreeMap<String, bool>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub plan_schema_version: u32,
    pub workflow: PlanWorkflow,
    pub mode: String,
    pub provider_override: Option<String>,
    pub routes: IndexMap<String, String>,
    pub detection: Option<Detection>,
    pub order: Vec<String>,
    pub stages: Vec<PlannedStage>,
    pub blocked: bool,
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn stage(&self, id: &str) -> Option<&PlannedStage> {
        self.stages.iter().find(|s| s.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanWorkflow {
    pub id: String,
    pub version: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedStage {
    pub id: String,
    pub role: String,
    pub responsibility: String,
    pub description: String,
    pub depends_on: Vec<String>,
    pub provider: String,
    pub assignment_source: String,
    pub fallback_trail: Vec<FallbackStep>,
    pub status: String,
    pub blocked_reasons: Vec<String>,
    pub warnings: Vec<String>,
    pub incompatibilities: Vec<Incompatibility>,
    pub required_skills: Vec<String>,
    pub recommended_skills: Vec<String>,
    /// Required capabilities and how strongly the assigned provider enforces them (omitted
    /// when the role requires none, keeping plans identical to the Python plan).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enforcement: Vec<PlannedEnforcement>,
    pub independence: Vec<Independence>,
    pub handoff_path: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub acceptance: Vec<PlannedCriterion>,
    pub optional: bool,
    pub requires_approval: bool,
    pub max_attempts: u32,
    pub on_exhausted: String,
    /// Run-relative path of the structured verdict, when the stage declares one (omitted
    /// otherwise, so plans without verdicts stay identical to the Python plan).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// Catalog skills to use in this stage (from a CLI profile; omitted when none, keeping plans
    /// identical to the Python plan).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stage_skills: Vec<String>,
}

impl PlannedStage {
    pub fn is_blocked(&self) -> bool {
        self.status == "blocked"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackStep {
    pub from: String,
    pub to: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Incompatibility {
    pub skill: String,
    pub provider: String,
    pub checked_adapters: Vec<String>,
    pub compatible_agents: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Independence {
    pub of: String,
    pub stage_provider: String,
    pub other_provider: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedEnforcement {
    pub capability: String,
    pub strength: Strength,
    pub how: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedCriterion {
    pub id: String,
    pub description: String,
    pub evidence: Vec<String>,
}

/// Planning options. `routes` keeps the order in which they were given.
#[derive(Debug, Clone, Default)]
pub struct PlanOptions<'a> {
    pub provider_override: Option<&'a str>,
    pub routes: IndexMap<String, String>,
    pub detected: Option<&'a Detection>,
    /// Stage id -> catalog skills to name in its handoff.
    pub stage_skills: IndexMap<String, Vec<String>>,
}

/// Build a plan. Never performs I/O.
pub fn build_plan(wf: &WorkflowDefinition, registry: Option<&Registry>, opts: &PlanOptions) -> Result<Plan, PlanError> {
    let override_ = opts.provider_override.filter(|p| !p.is_empty());
    check_options(wf, override_, &opts.routes)?;
    if let Some(unknown) = opts.stage_skills.keys().find(|s| wf.stage(s).is_none()) {
        return Err(PlanError(format!("stage skills name unknown stage '{unknown}'")));
    }

    let mut warnings = Vec::new();
    if registry.is_none() {
        warnings.push("No skill registry supplied: skill compatibility was not checked.".to_string());
    }

    let order = topological(&wf.stages);
    let mut assigned: HashMap<&str, PlannedStage> = HashMap::new();
    for stage in &order {
        let mut planned = plan_stage(wf, stage, registry, override_, &opts.routes, opts.detected);
        planned.stage_skills = opts.stage_skills.get(&stage.id).cloned().unwrap_or_default();
        assigned.insert(stage.id.as_str(), planned);
    }

    for stage in &wf.stages {
        let provider = assigned[stage.id.as_str()].provider.clone();
        let independence = stage
            .independent_of
            .iter()
            .map(|r| {
                let other = assigned[r.as_str()].provider.clone();
                Independence {
                    of: r.clone(),
                    status: if provider == other { "same-provider" } else { "distinct-provider" }.to_string(),
                    stage_provider: provider.clone(),
                    other_provider: other,
                }
            })
            .collect();
        if let Some(entry) = assigned.get_mut(stage.id.as_str()) {
            entry.independence = independence;
        }
    }

    if assigned.values().any(|s| s.independence.iter().any(|e| e.status == "same-provider")) {
        warnings.push(REDUCED_INDEPENDENCE_WARNING.to_string());
    }
    warnings.extend(wf.warnings.iter().cloned());

    let stages: Vec<PlannedStage> = order.iter().filter_map(|s| assigned.remove(s.id.as_str())).collect();
    Ok(Plan {
        plan_schema_version: PLAN_SCHEMA_VERSION,
        workflow: PlanWorkflow { id: wf.id.clone(), version: wf.version.clone(), sha256: wf.sha256.clone() },
        mode: if override_.is_some() { "single" } else { "multi" }.to_string(),
        provider_override: override_.map(str::to_string),
        routes: opts.routes.clone(),
        detection: opts.detected.cloned(),
        order: order.iter().map(|s| s.id.clone()).collect(),
        blocked: stages.iter().any(PlannedStage::is_blocked),
        stages,
        warnings,
    })
}

fn check_options(
    wf: &WorkflowDefinition,
    override_: Option<&str>,
    routes: &IndexMap<String, String>,
) -> Result<(), PlanError> {
    if override_.is_some() && !routes.is_empty() {
        return Err(PlanError("--provider (single-provider mode) and --route are mutually exclusive".into()));
    }
    if (override_.is_some() || !routes.is_empty()) && !wf.routing.allow_override {
        return Err(PlanError(format!(
            "Workflow '{}' sets allow_override = false; --provider/--route are not permitted",
            wf.id
        )));
    }
    for pid in override_.into_iter().chain(routes.values().map(String::as_str)) {
        if !is_known_provider(pid) {
            return Err(PlanError(format!("Unknown provider '{pid}'. Known providers: {}", py_list(&PROVIDER_IDS))));
        }
    }
    for (stage_id, pid) in routes {
        let Some(stage) = wf.stage(stage_id) else {
            let ids: Vec<&str> = wf.stages.iter().map(|s| s.id.as_str()).collect();
            return Err(PlanError(format!("--route names unknown stage '{stage_id}'. Stages: {}", py_list(&ids))));
        };
        let eligible = wf.routing.role(&stage.role).map(|r| r.eligible.as_slice()).unwrap_or(&[]);
        if !eligible.contains(pid) {
            return Err(PlanError(format!(
                "--route {stage_id}={pid}: provider '{pid}' is not eligible for role '{}' (eligible: {})",
                stage.role,
                py_list(eligible)
            )));
        }
    }
    Ok(())
}

/// Kahn's algorithm; ties broken by declaration order.
pub fn topological(stages: &[Stage]) -> Vec<&Stage> {
    let index: HashMap<&str, usize> = stages.iter().enumerate().map(|(i, s)| (s.id.as_str(), i)).collect();
    let mut remaining: HashMap<&str, usize> =
        stages.iter().map(|s| (s.id.as_str(), s.depends_on.iter().collect::<HashSet<_>>().len())).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = stages.iter().map(|s| (s.id.as_str(), Vec::new())).collect();
    for s in stages {
        let mut seen = HashSet::new();
        for dep in &s.depends_on {
            if seen.insert(dep.as_str())
                && let Some(list) = dependents.get_mut(dep.as_str())
            {
                list.push(s.id.as_str());
            }
        }
    }
    let mut heap: BinaryHeap<Reverse<usize>> =
        stages.iter().filter(|s| remaining[s.id.as_str()] == 0).map(|s| Reverse(index[s.id.as_str()])).collect();
    let mut ordered = Vec::with_capacity(stages.len());
    while let Some(Reverse(i)) = heap.pop() {
        let stage = &stages[i];
        ordered.push(stage);
        for &next in &dependents[stage.id.as_str()] {
            if let Some(n) = remaining.get_mut(next) {
                *n -= 1;
                if *n == 0 {
                    heap.push(Reverse(index[next]));
                }
            }
        }
    }
    ordered
}

fn incompatibilities(provider: &str, registry: Option<&Registry>, skills: &[String]) -> Vec<Incompatibility> {
    let Some(registry) = registry else { return Vec::new() };
    let Ok(spec) = get_provider(provider) else { return Vec::new() };
    let mut found = Vec::new();
    for skill_id in skills {
        let Some(skill) = registry.get(skill_id) else { continue };
        let agents = &skill.compatible_agents;
        if agents.iter().any(|a| a == "universal" || spec.skill_adapters.contains(&a.as_str())) {
            continue;
        }
        found.push(Incompatibility {
            skill: skill_id.clone(),
            provider: provider.to_string(),
            checked_adapters: spec.skill_adapters.iter().map(|s| (*s).to_string()).collect(),
            compatible_agents: agents.clone(),
        });
    }
    found
}

fn problems(
    role: &Role,
    provider: &str,
    registry: Option<&Registry>,
    detected: Option<&Detection>,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if !incompatibilities(provider, registry, &role.required_skills).is_empty() {
        reasons.push("incompatible");
    }
    if let Some(detected) = detected
        && !detected.get(provider).copied().unwrap_or(false)
    {
        reasons.push("unavailable");
    }
    reasons
}

fn plan_stage(
    wf: &WorkflowDefinition,
    stage: &Stage,
    registry: Option<&Registry>,
    override_: Option<&str>,
    routes: &IndexMap<String, String>,
    detected: Option<&Detection>,
) -> PlannedStage {
    // The parser guarantees every stage's role and routing entry exist.
    let role = wf.role(&stage.role).cloned().unwrap_or_else(|| Role {
        id: stage.role.clone(),
        responsibility: String::new(),
        required_skills: Vec::new(),
        recommended_skills: Vec::new(),
        requires: Vec::new(),
    });
    let eligible: Vec<String> = wf.routing.role(&stage.role).map(|r| r.eligible.clone()).unwrap_or_default();
    let preferred = wf.routing.role(&stage.role).map(|r| r.preferred.clone()).unwrap_or_default();
    let mut blocked = Vec::new();
    let mut warnings = Vec::new();

    let (mut provider, mut source, fallback_allowed) = if let Some(p) = override_ {
        if !eligible.iter().any(|e| e == p) {
            blocked.push(format!(
                "not-eligible: provider '{p}' is not eligible for role '{}' (eligible: {}); single-provider mode \
                 never substitutes another",
                role.id,
                py_list(&eligible)
            ));
        }
        (p.to_string(), "override", false)
    } else if let Some(p) = routes.get(&stage.id) {
        (p.clone(), "route", false)
    } else {
        (preferred, "preferred", true)
    };

    let mut trail = Vec::new();
    let mut visited: HashSet<String> = HashSet::from([provider.clone()]);
    // Fallback only for the preferred assignment; never under --provider or --route.
    loop {
        if !fallback_allowed {
            break;
        }
        let reasons = problems(&role, &provider, registry, detected);
        // A step is taken only if the target does not share the triggering problem.
        let resolved = |to: &str, on: &[String]| -> Vec<&'static str> {
            let target = problems(&role, to, registry, detected);
            reasons.iter().copied().filter(|r| on.iter().any(|o| o == r) && !target.contains(r)).collect()
        };
        let step = wf.routing.fallback.iter().find_map(|rule| {
            if rule.role != role.id || rule.from_provider != provider || visited.contains(&rule.to_provider) {
                return None;
            }
            resolved(&rule.to_provider, &rule.on).first().map(|reason| (rule.to_provider.clone(), *reason))
        });
        let Some((to, reason)) = step else { break };
        trail.push(FallbackStep { from: provider.clone(), to: to.clone(), reason: reason.to_string() });
        provider = to;
        source = "fallback";
        visited.insert(provider.clone());
    }

    let incompat = incompatibilities(&provider, registry, &role.required_skills);
    if !incompat.is_empty() {
        let skills: Vec<&str> = incompat.iter().map(|i| i.skill.as_str()).collect();
        blocked.push(format!(
            "incompatible: required skill(s) {} cannot be loaded by provider '{provider}'; no lossy conversion is \
             performed",
            py_list(&skills)
        ));
    }
    for item in incompatibilities(&provider, registry, &role.recommended_skills) {
        warnings.push(format!("recommended skill '{}' is not compatible with provider '{provider}'", item.skill));
    }
    let mut enforcement = Vec::new();
    if let Ok(spec) = get_provider(&provider) {
        for cap in &role.requires {
            let Some(e) = spec.enforcement(cap) else { continue };
            if e.strength == Strength::Soft {
                warnings.push(format!(
                    "provider '{provider}' enforces '{cap}' only softly (a prompt instruction, not a CLI control); \
                     prefer a provider with hard enforcement for role '{}'",
                    role.id
                ));
            }
            enforcement.push(PlannedEnforcement {
                capability: cap.clone(),
                strength: e.strength,
                how: e.how.to_string(),
                source: e.source.to_string(),
            });
        }
    }
    if let Some(detected) = detected
        && !detected.get(&provider).copied().unwrap_or(false)
    {
        warnings.push(format!(
            "provider '{provider}' is not installed on this host; the handoff can still be run elsewhere"
        ));
    }

    let artifact = |stage_id: &str, name: &str| format!("artifacts/{stage_id}/{name}");
    PlannedStage {
        id: stage.id.clone(),
        role: role.id.clone(),
        responsibility: role.responsibility.clone(),
        description: stage.description.clone(),
        depends_on: stage.depends_on.clone(),
        provider,
        assignment_source: source.to_string(),
        fallback_trail: trail,
        status: if blocked.is_empty() { "assignable" } else { "blocked" }.to_string(),
        blocked_reasons: blocked,
        warnings,
        incompatibilities: incompat,
        required_skills: role.required_skills.clone(),
        recommended_skills: role.recommended_skills.clone(),
        enforcement,
        independence: Vec::new(),
        handoff_path: format!("handoff/{}.md", stage.id),
        inputs: stage
            .inputs
            .iter()
            .map(|r| {
                let (src, name) = r.split_once('/').unwrap_or((r.as_str(), ""));
                artifact(src, name)
            })
            .collect(),
        outputs: stage.outputs.iter().map(|o| artifact(&stage.id, o)).collect(),
        acceptance: stage
            .acceptance
            .iter()
            .map(|c| PlannedCriterion {
                id: c.id.clone(),
                description: c.description.clone(),
                evidence: c.evidence.iter().map(|e| artifact(&stage.id, e)).collect(),
            })
            .collect(),
        optional: stage.optional,
        requires_approval: stage.requires_approval,
        max_attempts: stage.max_attempts,
        on_exhausted: stage.on_exhausted.clone(),
        verdict: stage.verdict.as_ref().map(|v| artifact(&stage.id, v)),
        stage_skills: Vec::new(),
    }
}
