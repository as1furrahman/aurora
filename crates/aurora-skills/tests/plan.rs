//! R2 Step 3b: the install plan, a pure function from (wanted skills, previous lock, what is on
//! disk) to actions. Aurora only ever touches destinations its lock records and whose content is
//! still exactly what it installed.

use std::collections::BTreeMap;

use aurora_skills::plan::{
    ActionKind, Gate, Lock, LockedSkill, Observed, PlanInput, PreparedRules, PreparedSkill, SourcePin, plan, skill_dir,
};

fn pin(entry: &str) -> SourcePin {
    SourcePin {
        repo: "https://github.com/o/r".into(),
        rev: "0123456789abcdef0123456789abcdef01234567".into(),
        path: format!("skills/{entry}"),
        sha256: format!("up-{entry}"),
    }
}

fn skill(entry: &str, providers: &[&str]) -> PreparedSkill {
    PreparedSkill {
        entry: entry.into(),
        name: entry.into(),
        source: pin(entry),
        installed_sha256: format!("in-{entry}"),
        overlays: Vec::new(),
        gate: Gate::Ready,
        listing_chars: ["claude", "codex", "antigravity", "hermes"].iter().map(|p| (p.to_string(), 100)).collect(),
        providers: providers.iter().map(|p| p.to_string()).collect(),
    }
}

fn budgets() -> BTreeMap<String, usize> {
    [("claude", 8000), ("codex", 8000), ("antigravity", 8000), ("hermes", 12000)]
        .iter()
        .map(|(p, b)| (p.to_string(), *b))
        .collect()
}

fn clis(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

struct Case {
    clis: Vec<String>,
    skills: Vec<PreparedSkill>,
    rules: Vec<PreparedRules>,
    previous: Option<Lock>,
    dirs: BTreeMap<String, Observed>,
    files: BTreeMap<String, Option<String>>,
    prune: bool,
}

impl Case {
    fn new(clis_: &[&str], skills: Vec<PreparedSkill>) -> Case {
        Case {
            clis: clis(clis_),
            skills,
            rules: vec![],
            previous: None,
            dirs: BTreeMap::new(),
            files: BTreeMap::new(),
            prune: false,
        }
    }
    fn run(&self) -> aurora_skills::plan::InstallPlan {
        let b = budgets();
        plan(&PlanInput {
            profile: "pair",
            clis: &self.clis,
            skills: &self.skills,
            rules: &self.rules,
            previous: self.previous.as_ref(),
            observed_dirs: &self.dirs,
            observed_files: &self.files,
            budgets: &b,
            prune: self.prune,
        })
    }
}

fn action<'a>(p: &'a aurora_skills::plan::InstallPlan, target: &str) -> &'a aurora_skills::plan::Action {
    p.actions.iter().find(|a| a.target == target).unwrap_or_else(|| panic!("no action for {target}: {:#?}", p.actions))
}

#[test]
fn project_skill_dirs_follow_each_cli() {
    assert_eq!(skill_dir("claude").expect("claude").rel, ".claude/skills");
    assert_eq!(skill_dir("codex").expect("codex").rel, ".agents/skills");
    assert_eq!(skill_dir("antigravity").expect("agy").rel, ".agents/skills");
    assert_eq!(skill_dir("hermes").expect("hermes").rel, ".hermes/skills");
    assert!(skill_dir("gpt-cli").is_none());
}

#[test]
fn fresh_project_creates_one_copy_per_directory() {
    let case = Case::new(
        &["claude", "codex"],
        vec![skill("tdd", &["claude"]), skill("review", &["codex"]), skill("verify", &["claude", "codex"])],
    );
    let p = case.run();
    assert_eq!(action(&p, ".claude/skills/tdd").kind, ActionKind::Create);
    assert_eq!(action(&p, ".agents/skills/review").kind, ActionKind::Create);
    assert_eq!(action(&p, ".claude/skills/verify").kind, ActionKind::Create);
    assert_eq!(action(&p, ".agents/skills/verify").kind, ActionKind::Create);
    assert_eq!(p.actions.len(), 4);
    // One lock record per installed copy, keyed by destination.
    let keys: Vec<&str> = p.lock.skills.keys().map(String::as_str).collect();
    assert_eq!(keys, [".agents/skills/review", ".agents/skills/verify", ".claude/skills/tdd", ".claude/skills/verify"]);
    let locked: &LockedSkill = &p.lock.skills[".claude/skills/verify"];
    assert_eq!((locked.entry.as_str(), locked.installed_sha256.as_str()), ("verify", "in-verify"));
    assert!(p.changed);
}

#[test]
fn codex_and_antigravity_share_one_copy_and_see_each_others_skills() {
    let case = Case::new(
        &["claude", "codex", "antigravity"],
        vec![skill("review", &["codex"]), skill("verify-ui", &["antigravity"])],
    );
    let p = case.run();
    assert_eq!(action(&p, ".agents/skills/review").providers, ["codex"]);
    let sees = |cli: &str| p.visibility.iter().find(|v| v.provider == cli).expect("visibility").skills.clone();
    assert_eq!(sees("antigravity"), ["review", "verify-ui"], "shared .agents/skills leaks across CLIs");
    assert_eq!(sees("codex"), ["review", "verify-ui"]);
    assert!(sees("claude").is_empty());
    assert!(
        p.warnings.iter().any(|w| w.contains(".agents/skills") && w.contains("antigravity") && w.contains("review")),
        "{:?}",
        p.warnings
    );
}

#[test]
fn hermes_reads_its_own_dir_and_trusted_shared_skills() {
    let case = Case::new(&["codex", "hermes"], vec![skill("review", &["codex"]), skill("plan-it", &["hermes"])]);
    let p = case.run();
    assert_eq!(action(&p, ".hermes/skills/plan-it").kind, ActionKind::Create);
    let hermes = p.visibility.iter().find(|v| v.provider == "hermes").expect("hermes");
    assert_eq!(hermes.skills, ["plan-it", "review"]);
    assert!(p.warnings.iter().any(|w| w.contains("hermes skills trust")), "{:?}", p.warnings);
}

#[test]
fn unchanged_managed_content_is_left_alone() {
    let mut case = Case::new(&["claude"], vec![skill("tdd", &["claude"])]);
    let first = case.run();
    case.previous = Some(first.lock.clone());
    case.dirs.insert(".claude/skills/tdd".into(), Observed::Dir { sha256: "in-tdd".into() });
    let p = case.run();
    assert_eq!(action(&p, ".claude/skills/tdd").kind, ActionKind::Unchanged);
    assert!(!p.changed);
}

#[test]
fn a_new_upstream_version_updates_only_unmodified_copies() {
    let mut case = Case::new(&["claude"], vec![skill("tdd", &["claude"])]);
    case.previous = Some(case.run().lock);
    case.skills[0].installed_sha256 = "in-tdd-v2".into();
    case.dirs.insert(".claude/skills/tdd".into(), Observed::Dir { sha256: "in-tdd".into() });
    assert_eq!(action(&case.run(), ".claude/skills/tdd").kind, ActionKind::Update);

    case.dirs.insert(".claude/skills/tdd".into(), Observed::Dir { sha256: "edited-by-user".into() });
    let p = case.run();
    let a = action(&p, ".claude/skills/tdd");
    assert_eq!(a.kind, ActionKind::Refuse);
    assert!(a.detail.contains("modified"), "{}", a.detail);
    assert_eq!(p.lock.skills[".claude/skills/tdd"].installed_sha256, "in-tdd", "a refused update keeps the old record");
}

#[test]
fn foreign_destinations_are_never_touched() {
    let mut case = Case::new(&["claude"], vec![skill("tdd", &["claude"])]);
    case.dirs.insert(".claude/skills/tdd".into(), Observed::Dir { sha256: "someone-elses".into() });
    let p = case.run();
    let a = action(&p, ".claude/skills/tdd");
    assert_eq!(a.kind, ActionKind::Refuse);
    assert!(a.detail.contains("not installed by Aurora"), "{}", a.detail);
    assert!(!p.lock.skills.contains_key(".claude/skills/tdd"));

    case.dirs.insert(".claude/skills/tdd".into(), Observed::NotADir);
    assert_eq!(action(&case.run(), ".claude/skills/tdd").kind, ActionKind::Refuse);
}

#[test]
fn blocked_skills_are_refused_and_review_is_flagged() {
    let mut blocked = skill("bad", &["claude"]);
    blocked.gate = Gate::Blocked(vec!["archive".into()]);
    let mut review = skill("scripted", &["claude"]);
    review.gate = Gate::NeedsReview(vec!["run.sh".into()]);
    let p = Case::new(&["claude"], vec![blocked, review]).run();
    assert_eq!(action(&p, ".claude/skills/bad").kind, ActionKind::Refuse);
    let a = action(&p, ".claude/skills/scripted");
    assert_eq!(a.kind, ActionKind::Create);
    assert!(a.needs_review);
    assert!(!p.lock.skills.contains_key(".claude/skills/bad"));
}

#[test]
fn stale_managed_copies_are_kept_unless_pruned() {
    let mut case = Case::new(&["claude", "codex"], vec![skill("review", &["codex"])]);
    case.previous = Some(case.run().lock);
    case.dirs.insert(".agents/skills/review".into(), Observed::Dir { sha256: "in-review".into() });
    case.skills.clear();
    let kept = case.run();
    assert_eq!(action(&kept, ".agents/skills/review").kind, ActionKind::Keep);
    assert!(kept.lock.skills.contains_key(".agents/skills/review"), "kept copies stay managed");

    case.prune = true;
    let pruned = case.run();
    assert_eq!(action(&pruned, ".agents/skills/review").kind, ActionKind::Remove);
    assert!(!pruned.lock.skills.contains_key(".agents/skills/review"));

    case.dirs.insert(".agents/skills/review".into(), Observed::Dir { sha256: "edited".into() });
    assert_eq!(action(&case.run(), ".agents/skills/review").kind, ActionKind::Refuse, "never prune edited copies");

    case.dirs.insert(".agents/skills/review".into(), Observed::Absent);
    let gone = case.run();
    assert!(gone.actions.iter().all(|a| a.target != ".agents/skills/review"), "already gone: nothing to do");
    assert!(!gone.lock.skills.contains_key(".agents/skills/review"));
}

#[test]
fn listing_budget_counts_what_each_cli_sees() {
    let mut big = skill("big", &["codex"]);
    big.listing_chars.insert("antigravity".into(), 9000);
    let p = Case::new(&["codex", "antigravity"], vec![big]).run();
    let agy = p.visibility.iter().find(|v| v.provider == "antigravity").expect("agy");
    assert_eq!(agy.listing_chars, 9000);
    assert!(p.warnings.iter().any(|w| w.contains("antigravity") && w.contains("budget")), "{:?}", p.warnings);
}

#[test]
fn rules_become_managed_blocks_in_each_cli_rules_file() {
    let mut case = Case::new(&["claude", "codex"], vec![]);
    case.rules = vec![PreparedRules {
        entry: "karpathy".into(),
        source: pin("karpathy"),
        text: "Think before coding.\n".into(),
        files: vec!["CLAUDE.md".into(), "AGENTS.md".into()],
    }];
    case.files.insert("AGENTS.md".into(), Some("# Project\n".into()));
    let p = case.run();
    assert_eq!(action(&p, "CLAUDE.md").kind, ActionKind::Create);
    assert_eq!(action(&p, "AGENTS.md").kind, ActionKind::Update, "appended to the existing file");
    let block = &p.rule_blocks["karpathy"];
    assert!(block.starts_with("<!-- aurora:begin karpathy -->\n") && block.ends_with("<!-- aurora:end karpathy -->\n"));

    // Once present and unmodified: unchanged. Edited inside the block: refused.
    case.previous = Some(p.lock.clone());
    case.files.insert("AGENTS.md".into(), Some(format!("# Project\n\n{block}")));
    case.files.insert("CLAUDE.md".into(), Some(block.clone()));
    let again = case.run();
    assert_eq!(action(&again, "AGENTS.md").kind, ActionKind::Unchanged);
    case.files.insert("AGENTS.md".into(), Some(format!("# Project\n\n{}", block.replace("Think", "Rush"))));
    assert_eq!(action(&case.run(), "AGENTS.md").kind, ActionKind::Refuse);
}

#[test]
fn the_lock_round_trips_and_is_deterministic() {
    let p = Case::new(&["claude", "codex"], vec![skill("b", &["codex"]), skill("a", &["claude"])]).run();
    let text = p.lock.to_json();
    assert_eq!(
        text,
        Case::new(&["claude", "codex"], vec![skill("a", &["claude"]), skill("b", &["codex"])]).run().lock.to_json()
    );
    let back = Lock::from_json(&text).expect("parses");
    assert_eq!(back, p.lock);
    assert!(!text.contains("/home/") && !text.contains("time"), "no absolute paths or timestamps: {text}");
    let mut bad: serde_json::Value = serde_json::from_str(&text).expect("json");
    bad["version"] = 99.into();
    assert!(Lock::from_json(&bad.to_string()).is_err());
}

#[test]
fn rules_files_are_rendered_without_touching_other_text() {
    use aurora_skills::plan::{render_rules_file, rule_block};
    let block = rule_block("k", "Rule one.\n");
    assert_eq!(render_rules_file(None, "k", &block).expect("new"), block);
    assert_eq!(render_rules_file(Some("# Notes\n"), "k", &block).expect("append"), format!("# Notes\n\n{block}"));
    assert_eq!(render_rules_file(Some("# Notes"), "k", &block).expect("append"), format!("# Notes\n\n{block}"));
    let old = rule_block("k", "Old rule.\n");
    let current = format!("# Notes\n\n{old}\n## More\n");
    assert_eq!(
        render_rules_file(Some(&current), "k", &block).expect("replace"),
        format!("# Notes\n\n{block}\n## More\n")
    );
    assert!(render_rules_file(Some("<!-- aurora:begin k -->\nno end"), "k", &block).is_err());
}
