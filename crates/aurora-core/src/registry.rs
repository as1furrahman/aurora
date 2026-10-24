//! Read-only canonical skill registry (`registry/skills.toml`).
//!
//! Registry parsing and validation. The first validation error is returned to the caller.

use std::collections::{HashMap, HashSet};

use toml::{Table, Value};

use crate::util::{is_kebab_id, is_semver, list_repr};

/// Skill-storage adapter ids a skill may declare in `compatible_agents`.
pub const ALLOWED_AGENTS: &[&str] =
    &["antigravity-cli", "antigravity-workspace", "antigravity-ide", "codex-cli", "claude-code", "hermes", "universal"];

/// A skill entry from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub compatible_agents: Vec<String>,
    pub triggers: Vec<String>,
    pub files: Vec<String>,
    pub category: Option<String>,
}

/// Validated registry, skills kept in declaration order.
#[derive(Debug, Clone)]
pub struct Registry {
    pub schema_version: String,
    pub updated_at: String,
    skills: Vec<SkillDefinition>,
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&SkillDefinition> {
        self.skills.iter().find(|s| s.id == id)
    }

    pub fn skills(&self) -> &[SkillDefinition] {
        &self.skills
    }

    pub fn skill_count(&self) -> usize {
        self.skills.len()
    }
}

/// Registry parse or validation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    Dependency(String),
}

use RegistryError::{Dependency, Parse, Validation};

/// Parse and validate registry TOML text.
pub fn parse_registry_str(content: &str) -> Result<Registry, RegistryError> {
    if content.trim().is_empty() {
        return Err(Parse("Registry content is empty; cannot parse empty registry".into()));
    }
    let data: Table = toml::from_str(content).map_err(|e| Parse(format!("Failed to parse registry TOML: {e}")))?;

    let meta = match data.get("registry") {
        None => return Err(Validation("Missing mandatory [registry] section".into())),
        Some(Value::Table(t)) => t,
        Some(_) => return Err(Validation("[registry] section must be a table".into())),
    };
    let schema_version = match meta.get("version") {
        Some(Value::String(s)) => s.clone(),
        _ => return Err(Validation("[registry] section must declare a string 'version'".into())),
    };
    if !is_semver(&schema_version) {
        return Err(Validation(format!(
            "[registry] version '{schema_version}' is not valid semantic versioning (SemVer)"
        )));
    }
    let updated_at = match meta.get("updated_at") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return Err(Validation("[registry] section must declare a non-empty string 'updated_at'".into())),
    };

    let raw_skills = match data.get("skills") {
        None => return Err(Validation("Missing mandatory [[skills]] array in registry".into())),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(Validation("[[skills]] must be a list of skill tables".into())),
    };

    let mut raw_ids = Vec::new();
    let mut skills: Vec<SkillDefinition> = Vec::new();
    for (idx, item) in raw_skills.iter().enumerate() {
        let Value::Table(t) = item else {
            return Err(Validation(format!("Skill entry #{idx} is not a valid table")));
        };
        let id = str_field(t, "id");
        raw_ids.push(id.clone());
        let skill = SkillDefinition {
            name: str_field(t, "name"),
            version: str_field(t, "version"),
            description: str_field(t, "description"),
            dependencies: list_field(t, "dependencies")
                .ok_or_else(|| Validation(format!("Skill '{id}' dependencies must be a list of string skill IDs")))?,
            compatible_agents: list_field(t, "compatible_agents")
                .ok_or_else(|| Validation(format!("Skill '{id}' compatible_agents must be a list of strings")))?,
            triggers: list_field(t, "triggers").unwrap_or_default(),
            files: list_field(t, "files").unwrap_or_default(),
            category: match t.get("category") {
                Some(Value::String(s)) => Some(s.clone()),
                _ => None,
            },
            id,
        };
        // Later duplicates replace earlier entries; duplicates are rejected below.
        skills.retain(|s| s.id != skill.id);
        skills.push(skill);
    }

    let mut seen = HashSet::new();
    for id in &raw_ids {
        if !seen.insert(id.as_str()) {
            return Err(Validation(format!("Duplicate skill ID detected in registry: '{id}'")));
        }
    }
    for skill in &skills {
        validate_skill(skill)?;
    }
    let ids: HashSet<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    for skill in &skills {
        for dep in &skill.dependencies {
            if !ids.contains(dep.as_str()) {
                return Err(Dependency(format!(
                    "Skill '{}' declares dependency '{dep}' which does not exist in registry",
                    skill.id
                )));
            }
        }
    }
    detect_cycles(&skills)?;

    Ok(Registry { schema_version, updated_at, skills })
}

fn str_field(t: &Table, key: &str) -> String {
    match t.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// `Some(list)` when absent (empty) or a list of strings; `None` when malformed.
fn list_field(t: &Table, key: &str) -> Option<Vec<String>> {
    match t.get(key) {
        None => Some(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        Some(_) => None,
    }
}

fn validate_skill(skill: &SkillDefinition) -> Result<(), RegistryError> {
    let id = &skill.id;
    if id.is_empty() {
        return Err(Validation("Skill 'id' must be a non-empty string".into()));
    }
    if !is_kebab_id(id) {
        return Err(Validation(format!(
            "Skill id '{id}' must be lowercase alphanumeric with hyphens (e.g. 'token-efficient-workflow')"
        )));
    }
    if skill.name.is_empty() {
        return Err(Validation(format!("Skill '{id}' must have a non-empty 'name' string")));
    }
    if skill.version.is_empty() {
        return Err(Validation(format!("Skill '{id}' must have a 'version' string")));
    }
    if !is_semver(&skill.version) {
        return Err(Validation(format!(
            "Skill '{id}' version '{}' is not valid semantic versioning (SemVer)",
            skill.version
        )));
    }
    if skill.description.is_empty() {
        return Err(Validation(format!("Skill '{id}' must have a non-empty 'description' string")));
    }
    if skill.dependencies.iter().any(|d| d == id) {
        return Err(Dependency(format!("Skill '{id}' cannot depend on itself")));
    }
    for agent in &skill.compatible_agents {
        if !ALLOWED_AGENTS.contains(&agent.as_str()) {
            let mut allowed: Vec<&str> = ALLOWED_AGENTS.to_vec();
            allowed.sort_unstable();
            return Err(Validation(format!(
                "Skill '{id}' declares unrecognized agent '{agent}'. Allowed: {}",
                list_repr(&allowed)
            )));
        }
    }
    Ok(())
}

fn detect_cycles(skills: &[SkillDefinition]) -> Result<(), RegistryError> {
    let by_id: HashMap<&str, &SkillDefinition> = skills.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut state: HashMap<&str, u8> = HashMap::new();
    let mut path: Vec<&str> = Vec::new();

    fn dfs<'a>(
        node: &'a str,
        by_id: &HashMap<&'a str, &'a SkillDefinition>,
        state: &mut HashMap<&'a str, u8>,
        path: &mut Vec<&'a str>,
    ) -> Result<(), RegistryError> {
        state.insert(node, 1);
        path.push(node);
        for dep in &by_id[node].dependencies {
            let Some((&dep, _)) = by_id.get_key_value(dep.as_str()) else { continue };
            match state.get(dep) {
                Some(1) => {
                    let start = path.iter().position(|p| *p == dep).unwrap_or(0);
                    let mut cycle: Vec<&str> = path[start..].to_vec();
                    cycle.push(dep);
                    return Err(Dependency(format!("Dependency cycle detected in registry: {}", cycle.join(" -> "))));
                }
                None => dfs(dep, by_id, state, path)?,
                _ => {}
            }
        }
        path.pop();
        state.insert(node, 2);
        Ok(())
    }

    for skill in skills {
        if !state.contains_key(skill.id.as_str()) {
            dfs(skill.id.as_str(), &by_id, &mut state, &mut path)?;
        }
    }
    Ok(())
}
