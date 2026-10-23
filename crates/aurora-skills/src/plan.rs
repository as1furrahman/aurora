//! Install planning (R2 Step 3b): a pure function from what is wanted, the previous lock and
//! what is on disk to a list of actions, a visibility matrix and the next lock. It performs no
//! I/O; the CLI observes the project and Step 4 applies the plan.
//!
//! Ownership comes from the lock alone: a destination is Aurora's only if the lock records it,
//! and it may only be changed or removed while its content still has the recorded hash.
//! Foreign destinations, edited copies and edited rule blocks are reported, never touched.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A project skill directory and who reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillDir {
    pub rel: &'static str,
    /// Other tools known to read this directory (not managed by Aurora).
    pub other_tools: &'static [&'static str],
}

const CLAUDE_DIR: SkillDir = SkillDir { rel: ".claude/skills", other_tools: &["GitHub Copilot", "Cursor", "OpenCode"] };
const SHARED_DIR: SkillDir =
    SkillDir { rel: ".agents/skills", other_tools: &["Cursor", "GitHub Copilot", "Gemini CLI", "OpenCode"] };
const HERMES_DIR: SkillDir = SkillDir { rel: ".hermes/skills", other_tools: &[] };

/// Where Aurora installs project skills for `provider`. Claude Code reads only `.claude/skills`;
/// Codex and Antigravity share `.agents/skills`; Hermes gets its own `.hermes/skills`, which only
/// it reads (Hermes v0.22.5 `PROJECT_SKILLS_SUBDIRS`).
pub fn skill_dir(provider: &str) -> Option<SkillDir> {
    match provider {
        "claude" => Some(CLAUDE_DIR),
        "codex" | "antigravity" => Some(SHARED_DIR),
        "hermes" => Some(HERMES_DIR),
        _ => None,
    }
}

/// Project skill directories `provider` loads skills from.
fn reads(provider: &str) -> &'static [&'static str] {
    match provider {
        "claude" => &[".claude/skills"],
        "codex" | "antigravity" => &[".agents/skills"],
        // Both only in repositories trusted with `hermes skills trust`.
        "hermes" => &[".hermes/skills", ".agents/skills"],
        _ => &[],
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePin {
    pub repo: String,
    pub rev: String,
    pub path: String,
    /// Upstream content hash.
    pub sha256: String,
}

/// Install gate from the scanner and the catalog review (integrity failures never reach a plan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case", tag = "gate", content = "detail")]
pub enum Gate {
    Ready,
    Reviewed(String),
    NeedsReview(Vec<String>),
    Blocked(Vec<String>),
}

/// A fetched, verified skill with overlays applied, ready to place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSkill {
    pub entry: String,
    /// Directory name to install as (the skill's name).
    pub name: String,
    pub source: SourcePin,
    /// Content hash of the installed copy (after overlays).
    pub installed_sha256: String,
    /// Frontmatter keys removed by overlays.
    pub overlays: Vec<String>,
    pub gate: Gate,
    /// Provider -> characters the skill adds to that CLI's listing.
    pub listing_chars: BTreeMap<String, usize>,
    /// CLIs that want this skill.
    pub providers: Vec<String>,
}

/// An always-on rules entry projected into rules files as a managed block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedRules {
    pub entry: String,
    pub source: SourcePin,
    /// Block body (the SKILL.md body).
    pub text: String,
    /// Project files to place the block in (`CLAUDE.md`, `AGENTS.md`).
    pub files: Vec<String>,
}

/// What is at a skill destination now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observed {
    Absent,
    /// A real directory with this content hash.
    Dir {
        sha256: String,
    },
    /// A file, symlink or anything that is not a plain directory.
    NotADir,
}

#[derive(Debug, Clone, Copy)]
pub struct PlanInput<'a> {
    pub profile: &'a str,
    pub clis: &'a [String],
    pub skills: &'a [PreparedSkill],
    pub rules: &'a [PreparedRules],
    pub previous: Option<&'a Lock>,
    /// Destination (relative to the project) -> what is there.
    pub observed_dirs: &'a BTreeMap<String, Observed>,
    /// Rules file -> its content, if it exists.
    pub observed_files: &'a BTreeMap<String, Option<String>>,
    /// Provider -> listing budget in characters.
    pub budgets: &'a BTreeMap<String, usize>,
    /// Remove managed copies that are no longer wanted.
    pub prune: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    Create,
    Update,
    Unchanged,
    /// Managed but no longer wanted; kept because `prune` is off.
    Keep,
    Remove,
    Refuse,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Action {
    pub kind: ActionKind,
    /// Destination relative to the project (a skill directory or a rules file).
    pub target: String,
    pub entry: String,
    pub providers: Vec<String>,
    /// The user must confirm they reviewed the skill before this is applied.
    pub needs_review: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Visibility {
    pub provider: String,
    /// Skill names this CLI will load from the project, sorted.
    pub skills: Vec<String>,
    pub listing_chars: usize,
    pub budget_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSkill {
    pub entry: String,
    pub repo: String,
    pub rev: String,
    pub path: String,
    pub sha256: String,
    pub installed_sha256: String,
    #[serde(default)]
    pub overlays: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedRules {
    pub repo: String,
    pub rev: String,
    pub sha256: String,
    pub block_sha256: String,
}

/// `aurora.lock`: what Aurora installed where. Sorted, timestamp-free, project-relative.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub version: u32,
    pub profile: String,
    pub clis: Vec<String>,
    /// Destination -> installed copy.
    pub skills: BTreeMap<String, LockedSkill>,
    /// Rules file -> entry -> managed block.
    pub rules: BTreeMap<String, BTreeMap<String, LockedRules>>,
}

pub const LOCK_VERSION: u32 = 1;

impl Lock {
    pub fn to_json(&self) -> String {
        let mut text = serde_json::to_string_pretty(self).unwrap_or_default();
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<Lock, String> {
        let lock: Lock = serde_json::from_str(text).map_err(|e| format!("invalid aurora.lock: {e}"))?;
        if lock.version != LOCK_VERSION {
            return Err(format!("aurora.lock version {} is not supported (expected {LOCK_VERSION})", lock.version));
        }
        Ok(lock)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstallPlan {
    pub actions: Vec<Action>,
    pub visibility: Vec<Visibility>,
    pub warnings: Vec<String>,
    /// Entry -> the exact managed block text.
    pub rule_blocks: BTreeMap<String, String>,
    pub lock: Lock,
    /// Applying would change the project or the lock.
    pub changed: bool,
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn begin_marker(entry: &str) -> String {
    format!("<!-- aurora:begin {entry} -->")
}

fn end_marker(entry: &str) -> String {
    format!("<!-- aurora:end {entry} -->")
}

/// The managed block for a rules entry, markers included.
pub fn rule_block(entry: &str, text: &str) -> String {
    let body = if text.ends_with('\n') { text.to_string() } else { format!("{text}\n") };
    format!("{}\n{body}{}\n", begin_marker(entry), end_marker(entry))
}

/// The managed block for `entry` inside `content`: `Ok(None)` when absent, `Err` when malformed.
pub fn find_block<'a>(content: &'a str, entry: &str) -> Result<Option<&'a str>, String> {
    let begin = begin_marker(entry);
    let end = end_marker(entry);
    let Some(start) = content.find(&begin) else { return Ok(None) };
    if content[start + begin.len()..].contains(&begin) || content[..start].contains(&end) {
        return Err(format!("the Aurora block for '{entry}' appears more than once or out of order"));
    }
    let Some(offset) = content[start..].find(&end) else {
        return Err(format!("the Aurora block for '{entry}' has no end marker"));
    };
    let mut stop = start + offset + end.len();
    if content[stop..].starts_with('\n') {
        stop += 1;
    }
    Ok(Some(&content[start..stop]))
}

/// Build the install plan.
pub fn plan(input: &PlanInput<'_>) -> InstallPlan {
    let mut actions = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let previous = input.previous.cloned().unwrap_or_default();
    let mut lock = Lock {
        version: LOCK_VERSION,
        profile: input.profile.to_string(),
        clis: input.clis.to_vec(),
        skills: BTreeMap::new(),
        rules: BTreeMap::new(),
    };

    // Destination -> (skill, providers that want it there).
    let mut wanted: BTreeMap<String, (&PreparedSkill, Vec<String>)> = BTreeMap::new();
    for s in input.skills {
        for p in &s.providers {
            if let Some(dir) = skill_dir(p) {
                let slot = wanted.entry(format!("{}/{}", dir.rel, s.name)).or_insert((s, Vec::new()));
                if !slot.1.contains(p) {
                    slot.1.push(p.clone());
                }
            }
        }
    }

    let observed = |target: &str| input.observed_dirs.get(target).cloned().unwrap_or(Observed::Absent);
    let record = |s: &PreparedSkill| LockedSkill {
        entry: s.entry.clone(),
        repo: s.source.repo.clone(),
        rev: s.source.rev.clone(),
        path: s.source.path.clone(),
        sha256: s.source.sha256.clone(),
        installed_sha256: s.installed_sha256.clone(),
        overlays: s.overlays.clone(),
    };

    let mut placed: Vec<(&str, &PreparedSkill, Vec<String>)> = Vec::new();
    for (target, (s, providers)) in &wanted {
        let owned = previous.skills.get(target);
        let act = |kind, detail: String| Action {
            kind,
            target: target.clone(),
            entry: s.entry.clone(),
            providers: providers.clone(),
            needs_review: matches!(s.gate, Gate::NeedsReview(_))
                && matches!(kind, ActionKind::Create | ActionKind::Update),
            detail,
        };
        if let Gate::Blocked(rules) = &s.gate {
            actions.push(act(ActionKind::Refuse, format!("blocked by the scanner ({})", rules.join(", "))));
            if let Some(o) = owned {
                lock.skills.insert(target.clone(), o.clone());
            }
            continue;
        }
        let action = match (observed(target), owned) {
            (Observed::Absent, _) => act(ActionKind::Create, String::new()),
            (Observed::Dir { sha256 }, Some(o)) if sha256 == o.installed_sha256 => {
                if o.installed_sha256 == s.installed_sha256 && o.entry == s.entry {
                    act(ActionKind::Unchanged, String::new())
                } else {
                    act(ActionKind::Update, format!("{} -> {}", short(&o.rev), short(&s.source.rev)))
                }
            }
            (Observed::Dir { .. }, Some(o)) => {
                lock.skills.insert(target.clone(), o.clone());
                actions.push(act(
                    ActionKind::Refuse,
                    "modified since Aurora installed it; Aurora will not overwrite it (restore or remove it yourself)"
                        .into(),
                ));
                continue;
            }
            (Observed::Dir { .. }, None) => {
                actions
                    .push(act(ActionKind::Refuse, "exists and was not installed by Aurora; it is left alone".into()));
                continue;
            }
            (Observed::NotADir, owned) => {
                if let Some(o) = owned {
                    lock.skills.insert(target.clone(), o.clone());
                }
                actions.push(act(
                    ActionKind::Refuse,
                    "exists and is not a plain directory; not installed by Aurora".into(),
                ));
                continue;
            }
        };
        lock.skills.insert(target.clone(), record(s));
        placed.push((target.as_str(), *s, providers.clone()));
        actions.push(action);
    }

    // Managed copies that are no longer wanted.
    for (target, o) in &previous.skills {
        if wanted.contains_key(target) {
            continue;
        }
        let act = |kind, detail: &str| Action {
            kind,
            target: target.clone(),
            entry: o.entry.clone(),
            providers: Vec::new(),
            needs_review: false,
            detail: detail.to_string(),
        };
        match observed(target) {
            Observed::Absent => {}
            Observed::Dir { sha256 } if sha256 == o.installed_sha256 => {
                if input.prune {
                    actions.push(act(ActionKind::Remove, "no longer wanted"));
                } else {
                    lock.skills.insert(target.clone(), o.clone());
                    actions.push(act(ActionKind::Keep, "no longer wanted; kept (prune removes it)"));
                }
            }
            _ => {
                lock.skills.insert(target.clone(), o.clone());
                actions.push(act(
                    ActionKind::Refuse,
                    "no longer wanted but modified since Aurora installed it; not removed",
                ));
            }
        }
    }

    // Always-on rules as managed blocks.
    let mut rule_blocks = BTreeMap::new();
    for r in input.rules {
        let block = rule_block(&r.entry, &r.text);
        let block_sha = sha256_hex(&block);
        let mut files: Vec<&String> = r.files.iter().collect();
        files.sort();
        files.dedup();
        for file in files {
            let prev = previous.rules.get(file).and_then(|m| m.get(&r.entry));
            let act = |kind, detail: String| Action {
                kind,
                target: file.clone(),
                entry: r.entry.clone(),
                providers: Vec::new(),
                needs_review: false,
                detail,
            };
            let content = input.observed_files.get(file).cloned().flatten();
            let action = match content.as_deref().map(|c| find_block(c, &r.entry)) {
                None => act(ActionKind::Create, "new file with the managed block".into()),
                Some(Ok(None)) => act(ActionKind::Update, "append the managed block".into()),
                Some(Ok(Some(found))) if found == block => act(ActionKind::Unchanged, String::new()),
                Some(Ok(Some(found))) if prev.is_some_and(|p| p.block_sha256 == sha256_hex(found)) => {
                    act(ActionKind::Update, "replace the managed block".into())
                }
                Some(Ok(Some(_))) => act(ActionKind::Refuse, "the Aurora block was edited; it is left alone".into()),
                Some(Err(e)) => act(ActionKind::Refuse, e),
            };
            let entry_locks = lock.rules.entry(file.clone()).or_default();
            if action.kind == ActionKind::Refuse {
                if let Some(p) = prev {
                    entry_locks.insert(r.entry.clone(), p.clone());
                }
            } else {
                entry_locks.insert(
                    r.entry.clone(),
                    LockedRules {
                        repo: r.source.repo.clone(),
                        rev: r.source.rev.clone(),
                        sha256: r.source.sha256.clone(),
                        block_sha256: block_sha.clone(),
                    },
                );
            }
            actions.push(action);
        }
        rule_blocks.insert(r.entry.clone(), block);
    }
    lock.rules.retain(|_, m| !m.is_empty());

    // Visibility: what each CLI will load, including skills other CLIs placed in shared dirs.
    let mut visibility = Vec::new();
    for cli in input.clis {
        let dirs = reads(cli);
        let mut names = BTreeSet::new();
        let mut listing = 0;
        for (target, s, _) in &placed {
            if dirs.iter().any(|d| target.starts_with(&format!("{d}/"))) && names.insert(s.name.clone()) {
                listing += s.listing_chars.get(cli).copied().unwrap_or(0);
            }
        }
        let budget = input.budgets.get(cli).copied().unwrap_or(0);
        if listing > budget {
            warnings.push(format!(
                "{cli}: the skills it will see need about {listing} listing characters, over its budget of {budget}"
            ));
        }
        visibility.push(Visibility {
            provider: cli.clone(),
            skills: names.into_iter().collect(),
            listing_chars: listing,
            budget_chars: budget,
        });
    }
    for cli in input.clis {
        let mut leaked: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for (target, s, providers) in &placed {
            let dir = target.rsplit_once('/').map(|(d, _)| d).unwrap_or_default();
            if reads(cli).contains(&dir) && !providers.contains(cli) {
                leaked.entry(dir).or_default().insert(format!("{} (for {})", s.name, providers.join(", ")));
            }
        }
        for (dir, names) in leaked {
            warnings.push(format!(
                "{cli} also loads {} from the shared {dir}",
                names.into_iter().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    let uses = |dir: &str| placed.iter().any(|(t, _, _)| t.starts_with(&format!("{dir}/")));
    if input.clis.iter().any(|c| c == "hermes") && (uses(HERMES_DIR.rel) || uses(SHARED_DIR.rel)) {
        warnings.push(
            "Hermes loads project skills only after `hermes skills trust` in this repository, and scans them itself"
                .into(),
        );
    }
    for dir in [CLAUDE_DIR, SHARED_DIR] {
        if uses(dir.rel) && !dir.other_tools.is_empty() {
            warnings.push(format!(
                "other tools that read {} ({}) will load these skills too",
                dir.rel,
                dir.other_tools.join(", ")
            ));
        }
    }

    let changed =
        actions.iter().any(|a| matches!(a.kind, ActionKind::Create | ActionKind::Update | ActionKind::Remove))
            || input.previous != Some(&lock);
    InstallPlan { actions, visibility, warnings, rule_blocks, lock, changed }
}

fn short(rev: &str) -> &str {
    &rev[..rev.len().min(12)]
}
