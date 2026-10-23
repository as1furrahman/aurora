//! Curated skill catalog (R2 Step 0), embedded from `catalog/catalog.toml`.
//!
//! The catalog vendors no third-party content: each entry names an upstream repository pinned to
//! a full commit SHA, the roles and stages it serves, and the measured length of its SKILL.md
//! description (what every CLI injects into its always-loaded skill listing). [`Catalog::recommend`]
//! maps a workflow onto a 1-3 CLI profile and picks each stage's skills within each CLI's listing
//! budget. Research: `docs/research/agent-skills-landscape-2026-10.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::providers::{PROVIDER_IDS, is_known_provider};
use crate::util::{is_kebab_id, is_plain_file_name};
use crate::workflow::WorkflowDefinition;

/// The built-in catalog text.
pub const CATALOG_TOML: &str = include_str!("../../../catalog/catalog.toml");

/// Characters a listing spends per skill besides its id and description (separator, newline).
const LISTING_OVERHEAD: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// SKILL.md installed into a CLI's skill directory.
    Skill,
    /// Always-on instructions projected into CLAUDE.md / AGENTS.md.
    Rules,
    /// A whole lifecycle methodology; never installed by Aurora.
    Framework,
    /// A proxy, hook or MCP server; never installed by Aurora.
    Infra,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    Baseline,
    Stage,
    OptIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    Official,
    Curated,
    Community,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Flag {
    Hooks,
    Scripts,
    AllowedTools,
    ManualOnly,
    ShareAlike,
    PopularityUnverified,
    NoLicenseFile,
    Telemetry,
    Network,
    NonOsiLicense,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub repo: String,
    pub rev: String,
    pub license: String,
    pub trust: Trust,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Per-CLI facts the recommendation needs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Always-loaded skill listing budget, in characters.
    pub listing_chars: usize,
    /// Per-skill description cap, if the CLI truncates descriptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_skill_chars: Option<usize>,
    /// File the CLI reads always-on project instructions from.
    pub rules_file: String,
    pub evidence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub description: String,
    pub default_clis: Vec<String>,
    /// Role id -> 1-based CLI slot.
    pub roles: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    pub source: String,
    /// Directory inside the source repository (skill and rules entries).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub tier: Tier,
    /// Roles served; empty means every role.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Stage ids served within those roles; empty means every stage.
    #[serde(default)]
    pub stages: Vec<String>,
    #[serde(default)]
    pub description_chars: usize,
    #[serde(default)]
    pub flags: Vec<Flag>,
    #[serde(default)]
    pub conflicts_with: Vec<String>,
    /// Overrides the source licence for this entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// Content hash of the skill directory at `rev` (`aurora skills scan` / `fetch`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Record of a human review of exactly that content: who/when and what was read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed: Option<String>,
    pub why: String,
}

impl Entry {
    pub fn has(&self, flag: Flag) -> bool {
        self.flags.contains(&flag)
    }

    /// Whether this entry serves `stage` (played by `role`).
    pub fn serves(&self, role: &str, stage: &str) -> bool {
        (self.roles.is_empty() || self.roles.iter().any(|r| r == role))
            && (self.stages.is_empty() || self.stages.iter().any(|s| s == stage))
    }

    /// Characters this skill adds to `target`'s always-loaded listing.
    pub fn listing_cost(&self, target: &Target) -> usize {
        self.id.len() + self.description_chars.min(target.per_skill_chars.unwrap_or(usize::MAX)) + LISTING_OVERHEAD
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    schema_version: i64,
    verified_on: String,
    targets: IndexMap<String, Target>,
    profiles: IndexMap<String, Profile>,
    sources: IndexMap<String, Source>,
    #[serde(default)]
    entries: Vec<Entry>,
}

/// A validated catalog.
#[derive(Debug, Clone)]
pub struct Catalog {
    verified_on: String,
    targets: IndexMap<String, Target>,
    profiles: IndexMap<String, Profile>,
    sources: IndexMap<String, Source>,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    #[error("cannot parse catalog: {0}")]
    Parse(String),
    #[error("catalog is invalid:\n  - {}", .0.join("\n  - "))]
    Validation(Vec<String>),
    #[error("{0}")]
    Request(String),
}

/// Parse and validate the built-in catalog.
pub fn builtin_catalog() -> Result<Catalog, CatalogError> {
    parse_catalog_str(CATALOG_TOML)
}

/// Parse and validate catalog TOML text. Unknown keys are parse errors; every validation
/// problem is reported at once.
pub fn parse_catalog_str(text: &str) -> Result<Catalog, CatalogError> {
    let raw: RawCatalog = toml::from_str(text).map_err(|e| CatalogError::Parse(e.to_string()))?;
    let errors = validate(&raw);
    if !errors.is_empty() {
        return Err(CatalogError::Validation(errors));
    }
    Ok(Catalog {
        verified_on: raw.verified_on,
        targets: raw.targets,
        profiles: raw.profiles,
        sources: raw.sources,
        entries: raw.entries,
    })
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b.iter().enumerate().all(|(i, c)| if i == 4 || i == 7 { *c == b'-' } else { c.is_ascii_digit() })
}

fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_github_repo(s: &str) -> bool {
    s.strip_prefix("https://github.com/").is_some_and(|rest| {
        let parts: Vec<&str> = rest.split('/').collect();
        parts.len() == 2
            && parts.iter().all(|p| {
                !p.is_empty()
                    && !p.starts_with('.')
                    && p.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
    })
}

fn is_repo_subpath(s: &str) -> bool {
    !s.is_empty()
        && !s.contains('\\')
        && s.split('/').all(|part| !part.is_empty() && part != "." && part != ".." && !part.starts_with('-'))
}

fn validate(raw: &RawCatalog) -> Vec<String> {
    let mut errors = Vec::new();
    if raw.schema_version != 1 {
        errors.push(format!("schema_version must be 1, got {}", raw.schema_version));
    }
    if !is_iso_date(&raw.verified_on) {
        errors.push(format!("verified_on '{}' must be a YYYY-MM-DD date", raw.verified_on));
    }

    for provider in PROVIDER_IDS {
        if !raw.targets.contains_key(provider) {
            errors.push(format!("[targets.{provider}] is missing"));
        }
    }
    for (id, t) in &raw.targets {
        if !is_known_provider(id) {
            errors.push(format!("[targets.{id}]: unknown provider"));
        }
        if t.listing_chars == 0 || t.per_skill_chars == Some(0) {
            errors.push(format!("[targets.{id}]: budgets must be positive"));
        }
        if !is_plain_file_name(&t.rules_file) {
            errors.push(format!("[targets.{id}]: rules_file '{}' must be a plain file name", t.rules_file));
        }
    }

    for (id, p) in &raw.profiles {
        if !is_kebab_id(id) {
            errors.push(format!("profile '{id}': id must be kebab-case"));
        }
        if p.default_clis.is_empty() {
            errors.push(format!("profile '{id}': default_clis is empty"));
        }
        let mut seen = BTreeSet::new();
        for cli in &p.default_clis {
            if !is_known_provider(cli) {
                errors.push(format!("profile '{id}': unknown provider '{cli}'"));
            }
            if !seen.insert(cli) {
                errors.push(format!("profile '{id}': provider '{cli}' is listed twice"));
            }
        }
        for (role, slot) in &p.roles {
            if !(1..=p.default_clis.len()).contains(slot) {
                errors
                    .push(format!("profile '{id}': role '{role}' slot {slot} is outside 1..={}", p.default_clis.len()));
            }
        }
    }

    for (id, s) in &raw.sources {
        if !is_kebab_id(id) {
            errors.push(format!("source '{id}': id must be kebab-case"));
        }
        if !is_github_repo(&s.repo) {
            errors.push(format!("source '{id}': repo '{}' must be https://github.com/<owner>/<name>", s.repo));
        }
        if !is_full_sha(&s.rev) {
            errors.push(format!("source '{id}': rev '{}' must be a full 40-character lowercase commit SHA", s.rev));
        }
        if s.license.trim().is_empty() {
            errors.push(format!("source '{id}': license is empty"));
        }
    }

    let mut ids = BTreeSet::new();
    for e in &raw.entries {
        let id = &e.id;
        if !is_kebab_id(id) {
            errors.push(format!("entry '{id}': id must be kebab-case"));
        }
        if !ids.insert(id.as_str()) {
            errors.push(format!("duplicate entry id '{id}'"));
        }
        if !raw.sources.contains_key(&e.source) {
            errors.push(format!("entry '{id}': unknown source '{}'", e.source));
        }
        match (e.kind, &e.path) {
            (Kind::Skill | Kind::Rules, None) => {
                errors.push(format!("entry '{id}': a {:?} entry needs a path", e.kind))
            }
            (_, Some(path)) if !is_repo_subpath(path) => {
                errors.push(format!("entry '{id}': path '{path}' must be a relative path inside the repository"));
            }
            _ => {}
        }
        if matches!(e.kind, Kind::Skill | Kind::Rules) {
            if e.description_chars == 0 {
                errors.push(format!("entry '{id}': description_chars must be measured (> 0)"));
            }
            if !e
                .sha256
                .as_deref()
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
            {
                errors.push(format!(
                    "entry '{id}': sha256 must be the 64-character content hash at rev (`aurora skills fetch {id}` prints it)"
                ));
            }
        } else if e.sha256.is_some() || e.reviewed.is_some() {
            errors
                .push(format!("entry '{id}': a {:?} entry is never installed, so it has no sha256 or review", e.kind));
        }
        if matches!(e.kind, Kind::Framework | Kind::Infra) && e.tier != Tier::OptIn {
            errors.push(format!("entry '{id}': {:?} entries must be opt-in", e.kind));
        }
        if e.tier == Tier::Stage && e.roles.is_empty() {
            errors.push(format!("entry '{id}': a stage entry must name its roles"));
        }
        if !e.stages.is_empty() && e.roles.is_empty() {
            errors.push(format!("entry '{id}': stages need roles"));
        }
        for name in e.roles.iter().chain(&e.stages) {
            if !is_kebab_id(name) {
                errors.push(format!("entry '{id}': '{name}' is not a kebab-case role or stage id"));
            }
        }
        if e.why.trim().is_empty() {
            errors.push(format!("entry '{id}': why is empty"));
        }
    }
    for e in &raw.entries {
        for other in &e.conflicts_with {
            match raw.entries.iter().find(|o| &o.id == other) {
                None => errors.push(format!("entry '{}': conflicts with unknown entry '{other}'", e.id)),
                Some(o) if !o.conflicts_with.contains(&e.id) => {
                    errors.push(format!("conflict '{}' -> '{other}' is not symmetric", e.id));
                }
                Some(_) => {}
            }
        }
    }
    errors
}

/// What to recommend for.
#[derive(Debug, Clone, Default)]
pub struct RecommendRequest {
    /// Profile name (`solo`, `pair`, `trio`).
    pub profile: String,
    /// CLIs filling the profile's slots in order; empty means the profile's `default_clis`.
    pub clis: Vec<String>,
    /// Opt-in entry ids to include.
    pub with: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RulesProjection {
    pub entry: String,
    /// Provider id -> the file the rules are projected into.
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StageRecommendation {
    pub stage: String,
    pub role: String,
    pub provider: String,
    /// Skill entry ids, in catalog order.
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderLoad {
    pub provider: String,
    /// Every skill this CLI needs across its stages, in catalog order.
    pub skills: Vec<String>,
    pub listing_chars: usize,
    pub budget_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    pub workflow: String,
    pub profile: String,
    pub clis: Vec<String>,
    pub rules: Vec<RulesProjection>,
    pub stages: Vec<StageRecommendation>,
    pub providers: Vec<ProviderLoad>,
    pub warnings: Vec<String>,
}

impl Catalog {
    pub fn verified_on(&self) -> &str {
        &self.verified_on
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn sources(&self) -> &IndexMap<String, Source> {
        &self.sources
    }

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.get(id)
    }

    pub fn profiles(&self) -> &IndexMap<String, Profile> {
        &self.profiles
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.get(name)
    }

    pub fn target(&self, provider: &str) -> Option<&Target> {
        self.targets.get(provider)
    }

    /// The licence that applies to `entry` (its own override, else its source's).
    pub fn license_of<'a>(&'a self, entry: &'a Entry) -> &'a str {
        entry.license.as_deref().or_else(|| self.source(&entry.source).map(|s| s.license.as_str())).unwrap_or("")
    }

    /// Assign each stage of `wf` to a CLI through the requested profile and pick its skills.
    /// Request mistakes are errors; budget, eligibility, independence and portability problems
    /// are warnings, so the caller can show everything at once.
    pub fn recommend(&self, wf: &WorkflowDefinition, req: &RecommendRequest) -> Result<Recommendation, CatalogError> {
        let request_error = |msg: String| Err(CatalogError::Request(msg));
        let Some(profile) = self.profile(&req.profile) else {
            let known: Vec<&str> = self.profiles.keys().map(String::as_str).collect();
            return request_error(format!("unknown profile '{}'; expected one of: {}", req.profile, known.join(", ")));
        };
        let clis = if req.clis.is_empty() { profile.default_clis.clone() } else { req.clis.clone() };
        if clis.len() != profile.default_clis.len() {
            return request_error(format!(
                "profile '{}' needs {} CLIs, got {} ({})",
                req.profile,
                profile.default_clis.len(),
                clis.len(),
                clis.join(", ")
            ));
        }
        for (i, cli) in clis.iter().enumerate() {
            if !is_known_provider(cli) {
                return request_error(format!("unknown CLI '{cli}'; expected one of: {}", PROVIDER_IDS.join(", ")));
            }
            if clis[..i].contains(cli) {
                return request_error(format!("CLI '{cli}' is listed twice; use a smaller profile instead"));
            }
        }
        let mut with = BTreeSet::new();
        for id in &req.with {
            let Some(entry) = self.get(id) else {
                return request_error(format!("unknown catalog entry '{id}' (see `aurora catalog list`)"));
            };
            if matches!(entry.kind, Kind::Framework | Kind::Infra) {
                let repo = self.source(&entry.source).map(|s| s.repo.as_str()).unwrap_or_default();
                return request_error(format!(
                    "'{id}' is {} and is not installed by Aurora; set it up from {repo} if you adopt it",
                    if entry.kind == Kind::Framework { "a framework" } else { "infrastructure" }
                ));
            }
            with.insert(id.as_str());
        }
        let selected = |e: &Entry| e.tier != Tier::OptIn || with.contains(e.id.as_str());

        let mut warnings = Vec::new();
        let mut note = |w: String| {
            if !warnings.contains(&w) {
                warnings.push(w);
            }
        };

        let rules = self
            .entries
            .iter()
            .filter(|e| e.kind == Kind::Rules && selected(e))
            .map(|e| RulesProjection {
                entry: e.id.clone(),
                files: clis.iter().filter_map(|c| self.target(c).map(|t| (c.clone(), t.rules_file.clone()))).collect(),
            })
            .collect();

        let mut stages = Vec::new();
        for stage in &wf.stages {
            let slot = match profile.roles.get(&stage.role) {
                Some(slot) => *slot,
                None => {
                    note(format!(
                        "role '{}' is not in profile '{}'; its stages run on the first CLI ({})",
                        stage.role, req.profile, clis[0]
                    ));
                    1
                }
            };
            let provider = clis[slot - 1].clone();
            if let Some(routing) = wf.routing.role(&stage.role)
                && !routing.eligible.is_empty()
                && !routing.eligible.contains(&provider)
            {
                note(format!(
                    "{provider} is not eligible for role '{}' in workflow '{}' (eligible: {}). Add it to \
                     [routing.roles.{}].eligible in the workflow, or pick other --clis.",
                    stage.role,
                    wf.id,
                    routing.eligible.join(", "),
                    stage.role
                ));
            }
            let skills = self
                .entries
                .iter()
                .filter(|e| e.kind == Kind::Skill && selected(e) && e.serves(&stage.role, &stage.id))
                .map(|e| e.id.clone())
                .collect();
            stages.push(StageRecommendation { stage: stage.id.clone(), role: stage.role.clone(), provider, skills });
        }

        for (stage, rec) in wf.stages.iter().zip(&stages) {
            for other in &stage.independent_of {
                if let Some(o) = stages.iter().find(|s| &s.stage == other)
                    && o.provider == rec.provider
                {
                    note(format!(
                        "reduced independence: '{}' should be independent of '{other}' but both run on {}. \
                         Use a fresh session, ideally with a different model.",
                        rec.stage, rec.provider
                    ));
                }
            }
        }

        let mut providers = Vec::new();
        for cli in &clis {
            let wanted: BTreeSet<&str> = stages
                .iter()
                .filter(|s| &s.provider == cli)
                .flat_map(|s| s.skills.iter().map(String::as_str))
                .collect();
            let skills: Vec<&Entry> = self.entries.iter().filter(|e| wanted.contains(e.id.as_str())).collect();
            let target = self.target(cli).ok_or_else(|| CatalogError::Request(format!("no target for {cli}")))?;
            let listing_chars = skills.iter().map(|e| e.listing_cost(target)).sum();
            if listing_chars > target.listing_chars {
                note(format!(
                    "{cli}: these skills need about {listing_chars} listing characters, over its budget of {}; \
                     drop opt-in skills or split roles across more CLIs",
                    target.listing_chars
                ));
            }
            for e in &skills {
                for other in &e.conflicts_with {
                    if wanted.contains(other.as_str()) {
                        note(format!("{cli}: '{}' conflicts with '{other}'; keep one", e.id));
                    }
                }
                if e.has(Flag::Hooks) && cli != "claude" {
                    note(format!(
                        "'{}' declares hooks, which only Claude Code runs; on {cli} it loads without them",
                        e.id
                    ));
                }
            }
            providers.push(ProviderLoad {
                provider: cli.clone(),
                skills: skills.iter().map(|e| e.id.clone()).collect(),
                listing_chars,
                budget_chars: target.listing_chars,
            });
        }

        let used: BTreeSet<&str> = providers.iter().flat_map(|p| p.skills.iter().map(String::as_str)).collect();
        for e in self.entries.iter().filter(|e| used.contains(e.id.as_str())) {
            if self.source(&e.source).is_some_and(|s| s.trust == Trust::Community) {
                note(format!(
                    "'{}' is a community entry: review its SKILL.md and every bundled file before installing",
                    e.id
                ));
            }
        }

        Ok(Recommendation {
            workflow: wf.id.clone(),
            profile: req.profile.clone(),
            clis,
            rules,
            stages,
            providers,
            warnings,
        })
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Kind::Skill => "skill",
            Kind::Rules => "rules",
            Kind::Framework => "framework",
            Kind::Infra => "infra",
        })
    }
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Tier::Baseline => "baseline",
            Tier::Stage => "stage",
            Tier::OptIn => "opt-in",
        })
    }
}

impl fmt::Display for Trust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Trust::Official => "official",
            Trust::Curated => "curated",
            Trust::Community => "community",
        })
    }
}

impl fmt::Display for Flag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Flag::Hooks => "hooks",
            Flag::Scripts => "scripts",
            Flag::AllowedTools => "allowed-tools",
            Flag::ManualOnly => "manual-only",
            Flag::ShareAlike => "share-alike",
            Flag::PopularityUnverified => "popularity-unverified",
            Flag::NoLicenseFile => "no-license-file",
            Flag::Telemetry => "telemetry",
            Flag::Network => "network",
            Flag::NonOsiLicense => "non-osi-license",
        })
    }
}
