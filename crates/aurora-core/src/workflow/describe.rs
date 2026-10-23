//! Provider-neutral description of a definition plus its separate routing policy
//! (the `workflow show --json` shape; port of `describe()` in `awb/workflow/commands.py`).

use indexmap::IndexMap;
use serde::Serialize;

use super::model::WorkflowDefinition;

#[derive(Debug, Clone, Serialize)]
pub struct Description {
    pub workflow: DescribedWorkflow,
    pub roles: IndexMap<String, DescribedRole>,
    pub stages: Vec<DescribedStage>,
    pub routing: DescribedRouting,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedWorkflow {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub task_types: Vec<String>,
    pub assurance: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedRole {
    pub responsibility: String,
    pub required_skills: Vec<String>,
    pub recommended_skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedStage {
    pub id: String,
    pub role: String,
    pub description: String,
    pub depends_on: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub optional: bool,
    pub max_attempts: u32,
    pub on_exhausted: String,
    pub requires_approval: bool,
    pub independent_of: Vec<String>,
    pub acceptance: Vec<DescribedCriterion>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedCriterion {
    pub id: String,
    pub description: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedRouting {
    pub allow_override: bool,
    pub roles: IndexMap<String, DescribedRoleRouting>,
    pub fallback: Vec<DescribedFallback>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedRoleRouting {
    pub eligible: Vec<String>,
    pub preferred: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DescribedFallback {
    pub role: String,
    pub from: String,
    pub to: String,
    pub on: Vec<String>,
}

pub fn describe(wf: &WorkflowDefinition) -> Description {
    Description {
        workflow: DescribedWorkflow {
            id: wf.id.clone(),
            version: wf.version.clone(),
            name: wf.name.clone(),
            description: wf.description.clone(),
            task_types: wf.task_types.clone(),
            assurance: wf.assurance.clone(),
            sha256: wf.sha256.clone(),
        },
        roles: wf
            .roles
            .iter()
            .map(|r| {
                let described = DescribedRole {
                    responsibility: r.responsibility.clone(),
                    required_skills: r.required_skills.clone(),
                    recommended_skills: r.recommended_skills.clone(),
                };
                (r.id.clone(), described)
            })
            .collect(),
        stages: wf
            .stages
            .iter()
            .map(|s| DescribedStage {
                id: s.id.clone(),
                role: s.role.clone(),
                description: s.description.clone(),
                depends_on: s.depends_on.clone(),
                inputs: s.inputs.clone(),
                outputs: s.outputs.clone(),
                optional: s.optional,
                max_attempts: s.max_attempts,
                on_exhausted: s.on_exhausted.clone(),
                requires_approval: s.requires_approval,
                independent_of: s.independent_of.clone(),
                acceptance: s
                    .acceptance
                    .iter()
                    .map(|c| DescribedCriterion {
                        id: c.id.clone(),
                        description: c.description.clone(),
                        evidence: c.evidence.clone(),
                    })
                    .collect(),
            })
            .collect(),
        routing: DescribedRouting {
            allow_override: wf.routing.allow_override,
            roles: wf
                .routing
                .roles
                .iter()
                .map(|r| {
                    (
                        r.role.clone(),
                        DescribedRoleRouting { eligible: r.eligible.clone(), preferred: r.preferred.clone() },
                    )
                })
                .collect(),
            fallback: wf
                .routing
                .fallback
                .iter()
                .map(|f| DescribedFallback {
                    role: f.role.clone(),
                    from: f.from_provider.clone(),
                    to: f.to_provider.clone(),
                    on: f.on.clone(),
                })
                .collect(),
        },
        warnings: wf.warnings.clone(),
    }
}
