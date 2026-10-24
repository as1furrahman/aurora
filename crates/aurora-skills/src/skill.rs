//! SKILL.md parsing (R2 Step 1), validated in two layers:
//! 1. the Agent Skills specification (<https://agentskills.io/specification>): `name`,
//!    `description`, `license`, `compatibility`, `metadata`, `allowed-tools`;
//! 2. per-CLI portability: Claude Code extensions that other CLIs ignore, and Claude-only
//!    behaviour (hooks, forked context, `` !`cmd` `` injection) that silently disappears elsewhere.
//!
//! Inputs are untrusted third-party files: size is capped before parsing, YAML parsing is
//! budget-limited (alias bombs fail), and nothing here performs I/O.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::frontmatter::{extract_frontmatter, parse_frontmatter};

/// Largest SKILL.md accepted (the spec recommends bodies under 5k tokens / 500 lines).
pub const MAX_SKILL_MD_BYTES: usize = 256 * 1024;
const MAX_NAME_CHARS: usize = 64;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_COMPATIBILITY_CHARS: usize = 500;
/// Characters a listing spends per skill besides name and description (as in the catalog).
const LISTING_OVERHEAD: usize = 4;

/// Frontmatter keys defined by the Agent Skills specification.
pub const SPEC_KEYS: [&str; 6] = ["name", "description", "license", "compatibility", "metadata", "allowed-tools"];

/// Claude Code extensions (<https://code.claude.com/docs/en/skills>), plus the widely used `version`.
pub const CLAUDE_KEYS: [&str; 15] = [
    "when_to_use",
    "argument-hint",
    "arguments",
    "disable-model-invocation",
    "user-invocable",
    "disallowed-tools",
    "model",
    "effort",
    "context",
    "agent",
    "background",
    "hooks",
    "paths",
    "shell",
    "version",
];

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("SKILL.md is {0} bytes; the limit is {MAX_SKILL_MD_BYTES}")]
    TooLarge(usize),
    #[error("missing YAML frontmatter delimited by ---")]
    MissingFrontmatter,
    #[error("invalid YAML frontmatter: {0}")]
    InvalidYaml(String),
    #[error("missing field `{0}`")]
    MissingField(&'static str),
    #[error("`{0}` must be a string")]
    NotAString(&'static str),
}

/// A parsed SKILL.md.
#[derive(Debug, Clone, PartialEq)]
pub struct Skill {
    /// `name`, or the directory name when absent (as Codex does).
    pub name: String,
    pub name_declared: bool,
    /// `description`, collapsed to one line.
    pub description: String,
    /// Claude Code's `when_to_use`, collapsed to one line.
    pub when_to_use: Option<String>,
    /// Every frontmatter key, as parsed.
    pub frontmatter: BTreeMap<String, Value>,
    pub body: String,
    /// Whether unquoted prose scalars had to be repaired to parse.
    pub repaired: bool,
}

impl Skill {
    fn flag(&self, key: &str) -> bool {
        self.frontmatter.get(key).and_then(Value::as_bool).unwrap_or(false)
    }
}

fn single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn string_field(map: &BTreeMap<String, Value>, key: &'static str) -> Result<Option<String>, ParseError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(single_line(s)).filter(|s| !s.is_empty())),
        Some(_) => Err(ParseError::NotAString(key)),
    }
}

/// Parse SKILL.md `contents` found in directory `dir_name`.
pub fn parse_skill(contents: &str, dir_name: &str) -> Result<Skill, ParseError> {
    if contents.len() > MAX_SKILL_MD_BYTES {
        return Err(ParseError::TooLarge(contents.len()));
    }
    let (frontmatter, body) = extract_frontmatter(contents).ok_or(ParseError::MissingFrontmatter)?;
    let (map, repaired) = parse_frontmatter(&frontmatter).map_err(ParseError::InvalidYaml)?;
    let declared = string_field(&map, "name")?;
    let description = string_field(&map, "description")?.ok_or(ParseError::MissingField("description"))?;
    let when_to_use = string_field(&map, "when_to_use")?;
    Ok(Skill {
        name_declared: declared.is_some(),
        name: declared.unwrap_or_else(|| dir_name.to_string()),
        description,
        when_to_use,
        frontmatter: map,
        body,
        repaired,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub message: String,
}

fn is_spec_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// Check `skill` against the Agent Skills specification.
pub fn check_spec(skill: &Skill, dir_name: &str) -> Vec<Issue> {
    let mut issues = Vec::new();
    let mut error = |message: String| issues.push(Issue { severity: Severity::Error, message });
    if !skill.name_declared {
        error("name is missing (required by the Agent Skills spec)".into());
    }
    if skill.name.chars().count() > MAX_NAME_CHARS {
        error(format!("name exceeds {MAX_NAME_CHARS} characters"));
    }
    if !is_spec_name(&skill.name) {
        error(format!(
            "name '{}' must be lowercase letters, digits and single hyphens, not starting or ending with a hyphen",
            skill.name
        ));
    }
    if skill.name != dir_name {
        error(format!("name '{}' does not match its directory '{dir_name}'", skill.name));
    }
    let description_chars = skill.description.chars().count();
    if description_chars > MAX_DESCRIPTION_CHARS {
        error(format!("description is {description_chars} characters; the limit is {MAX_DESCRIPTION_CHARS}"));
    }
    for key in ["license", "compatibility"] {
        match skill.frontmatter.get(key) {
            None | Some(Value::String(_)) => {}
            Some(_) => error(format!("{key} must be a string")),
        }
    }
    if let Some(Value::String(c)) = skill.frontmatter.get("compatibility")
        && c.chars().count() > MAX_COMPATIBILITY_CHARS
    {
        error(format!("compatibility exceeds {MAX_COMPATIBILITY_CHARS} characters"));
    }

    let mut warning = |message: String| issues.push(Issue { severity: Severity::Warning, message });
    match skill.frontmatter.get("metadata") {
        None => {}
        Some(Value::Object(m)) if m.values().all(Value::is_string) => {}
        Some(_) => warning("metadata should map string keys to string values".into()),
    }
    match skill.frontmatter.get("allowed-tools") {
        None | Some(Value::String(_)) => {}
        Some(_) => warning("allowed-tools should be a space-separated string".into()),
    }
    for key in skill.frontmatter.keys() {
        if !SPEC_KEYS.contains(&key.as_str()) && !CLAUDE_KEYS.contains(&key.as_str()) {
            warning(format!(
                "unknown frontmatter key '{key}' (not in the Agent Skills spec or Claude Code's extensions)"
            ));
        }
    }
    issues
}

/// A Claude Code `` !`cmd` `` context injection: the command runs when the skill loads,
/// without a permission prompt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Injection {
    /// 1-based line within the body.
    pub line: usize,
    pub snippet: String,
}

/// Find `` !`cmd` `` injections and ```` ```! ```` blocks in `body`, using Claude Code's
/// documented form: the `!` starts a token (line start or after whitespace), is outside an open
/// code span, and its command closes with a backtick on the same line. Code spans that merely end
/// in `!` (`` `ensure!` ``, `` `#VALUE!` ``) are prose.
pub fn shell_injections(body: &str) -> Vec<Injection> {
    let mut found = Vec::new();
    for (i, line) in body.lines().enumerate() {
        let hit = if line.trim_start().starts_with("```!") {
            Some(line.trim())
        } else {
            line.match_indices("!`").find_map(|(at, _)| {
                let before = &line[..at];
                let starts_token = before.chars().next_back().is_none_or(char::is_whitespace);
                let outside_span = before.matches('`').count() % 2 == 0;
                let close = line[at + 2..].find('`').filter(|len| *len > 0)?;
                (starts_token && outside_span).then(|| &line[at..at + 2 + close + 1])
            })
        };
        if let Some(snippet) = hit {
            found.push(Injection { line: i + 1, snippet: snippet.chars().take(120).collect() });
        }
    }
    found
}

/// How a skill behaves on one CLI.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase", tag = "support", content = "reasons")]
pub enum Support {
    Native,
    /// Loads, but some behaviour is lost; the reasons say which.
    Degraded(Vec<String>),
}

/// Portability of `skill` on `provider` (an Aurora provider id).
pub fn support(skill: &Skill, provider: &str) -> Support {
    if provider == "claude" {
        return Support::Native;
    }
    let mut reasons = Vec::new();
    if skill.frontmatter.contains_key("hooks") {
        reasons.push(format!("declares hooks, which only Claude Code runs; {provider} loads the skill without them"));
    }
    if skill.frontmatter.get("context").and_then(Value::as_str) == Some("fork") {
        reasons.push("uses `context: fork` (a Claude Code subagent); elsewhere it runs in the main session".into());
    }
    if skill.frontmatter.contains_key("agent") {
        reasons.push("names a Claude Code agent type, which other CLIs ignore".into());
    }
    if !shell_injections(&skill.body).is_empty() {
        reasons.push(
            "uses !`cmd` context injection, which only Claude Code expands; elsewhere the literal text is shown".into(),
        );
    }
    if skill.flag("disable-model-invocation") {
        reasons.push(format!(
            "is manual-only on Claude Code (disable-model-invocation); {provider} may invoke it implicitly unless an \
             overlay disables that"
        ));
    }
    if reasons.is_empty() { Support::Native } else { Support::Degraded(reasons) }
}

/// Characters `skill` adds to `provider`'s always-loaded listing. Claude Code lists the
/// description plus `when_to_use`; other CLIs list the description. `per_skill_cap` is the
/// CLI's per-skill truncation limit, if it has one.
pub fn listing_chars(skill: &Skill, provider: &str, per_skill_cap: Option<usize>) -> usize {
    let mut listed = skill.description.chars().count();
    if provider == "claude"
        && let Some(w) = &skill.when_to_use
    {
        listed += 1 + w.chars().count();
    }
    skill.name.chars().count() + listed.min(per_skill_cap.unwrap_or(usize::MAX)) + LISTING_OVERHEAD
}
