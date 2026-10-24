//! R2 Step 4b: applying an install plan and undoing it. Temporary projects only.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use aurora_skills::apply::{ApplyError, ApplyInput, OpStatus, apply, pending_op, undo};
use aurora_skills::plan::{
    ActionKind, Gate, InstallPlan, Lock, Observed, PlanInput, PreparedRules, PreparedSkill, SourcePin, plan,
};
use aurora_skills::tree::{FileEntry, Limits, SkillTree, collect};

fn tree(name: &str, version: &str) -> SkillTree {
    let md = format!("---\nname: {name}\ndescription: {version}\n---\nbody {version}\n");
    SkillTree {
        files: vec![
            FileEntry {
                path: "SKILL.md".into(),
                size: md.len() as u64,
                executable: false,
                content: Some(md.into_bytes()),
            },
            FileEntry { path: "scripts/run.sh".into(), size: 3, executable: true, content: Some(b"ok\n".to_vec()) },
        ],
        max_file_bytes: 1 << 20,
        ..SkillTree::default()
    }
}

fn prepared(name: &str, version: &str, providers: &[&str]) -> (PreparedSkill, SkillTree) {
    let t = tree(name, version);
    let s = PreparedSkill {
        entry: name.into(),
        name: name.into(),
        source: SourcePin {
            repo: "https://github.com/o/r".into(),
            rev: format!("{:0<40}", version.len()),
            path: format!("skills/{name}"),
            sha256: format!("up-{version}"),
        },
        installed_sha256: t.content_hash(),
        overlays: vec![],
        gate: Gate::Ready,
        listing_chars: BTreeMap::new(),
        providers: providers.iter().map(|p| p.to_string()).collect(),
    };
    (s, t)
}

fn rules() -> PreparedRules {
    PreparedRules {
        entry: "house".into(),
        source: SourcePin {
            repo: "https://github.com/o/r".into(),
            rev: "a".repeat(40),
            path: "skills/house".into(),
            sha256: "h".into(),
        },
        text: "Keep changes small.\n".into(),
        files: vec!["CLAUDE.md".into(), "AGENTS.md".into()],
    }
}

/// Plan against what is really on disk now.
fn plan_now(project: &Path, skills: &[PreparedSkill], prune: bool) -> InstallPlan {
    let previous = fs::read_to_string(project.join("aurora.lock")).ok().map(|t| Lock::from_json(&t).expect("lock"));
    let mut dirs = BTreeMap::new();
    let mut targets: Vec<String> = previous.iter().flat_map(|l| l.skills.keys().cloned()).collect();
    for s in skills {
        for p in &s.providers {
            targets.push(format!("{}/{}", aurora_skills::plan::skill_dir(p).expect("dir").rel, s.name));
        }
    }
    for t in targets {
        let path = project.join(&t);
        let seen = match fs::symlink_metadata(&path) {
            Err(_) => Observed::Absent,
            Ok(m) if m.is_dir() => {
                Observed::Dir { sha256: collect(&path, &Limits::default()).expect("collect").content_hash() }
            }
            Ok(_) => Observed::NotADir,
        };
        dirs.insert(t, seen);
    }
    let files =
        ["CLAUDE.md", "AGENTS.md"].iter().map(|f| (f.to_string(), fs::read_to_string(project.join(f)).ok())).collect();
    let budgets = [("claude".to_string(), 8000), ("codex".to_string(), 8000)].into_iter().collect();
    let clis = vec!["claude".to_string(), "codex".to_string()];
    plan(&PlanInput {
        profile: "pair",
        clis: &clis,
        skills,
        rules: &[rules()],
        previous: previous.as_ref(),
        observed_dirs: &dirs,
        observed_files: &files,
        budgets: &budgets,
        prune,
    })
}

fn run(
    project: &Path,
    p: &InstallPlan,
    contents: &BTreeMap<String, SkillTree>,
    op: &str,
    fault: Option<usize>,
) -> Result<aurora_skills::apply::OpRecord, ApplyError> {
    apply(&ApplyInput { project, plan: p, contents, op_id: op, fault_before_step: fault })
}

/// Every file under `dir` except Aurora's own `.aurora/` state, with contents.
fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(dir).expect("read_dir").flatten() {
            let p = e.path();
            let rel = p.strip_prefix(root).expect("rel").display().to_string();
            if rel == ".aurora" {
                continue;
            }
            if p.is_dir() {
                out.push((format!("{rel}/"), Vec::new()));
                walk(root, &p, out);
            } else {
                out.push((rel, fs::read(&p).expect("read")));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

struct Project {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
}

fn project() -> Project {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = tmp.path().join("project");
    fs::create_dir(&dir).expect("mkdir");
    fs::write(dir.join("AGENTS.md"), "# Team notes\n").expect("write");
    fs::write(dir.join("main.rs"), "fn main() {}\n").expect("write");
    let dir = fs::canonicalize(dir).expect("canonical");
    Project { _tmp: tmp, dir }
}

fn contents(items: &[(PreparedSkill, SkillTree)]) -> BTreeMap<String, SkillTree> {
    items.iter().map(|(s, t)| (s.entry.clone(), t.clone())).collect()
}

#[test]
fn apply_installs_rules_and_lock_then_replans_as_unchanged() {
    let pr = project();
    let items = [prepared("tdd", "v1", &["claude"]), prepared("review", "v1", &["codex"])];
    let skills: Vec<PreparedSkill> = items.iter().map(|(s, _)| s.clone()).collect();
    let p = plan_now(&pr.dir, &skills, false);
    let op = run(&pr.dir, &p, &contents(&items), "op1", None).expect("apply");
    assert_eq!(op.status, OpStatus::Committed);

    let run_sh = pr.dir.join(".claude/skills/tdd/scripts/run.sh");
    assert_eq!(fs::read(&run_sh).expect("read"), b"ok\n");
    assert!(pr.dir.join(".agents/skills/review/SKILL.md").is_file());
    let agents = fs::read_to_string(pr.dir.join("AGENTS.md")).expect("read");
    assert!(agents.starts_with("# Team notes\n") && agents.contains("<!-- aurora:begin house -->"), "{agents}");
    assert!(fs::read_to_string(pr.dir.join("CLAUDE.md")).expect("read").contains("Keep changes small."));
    assert_eq!(fs::read_to_string(pr.dir.join("aurora.lock")).expect("lock"), p.lock.to_json());

    let again = plan_now(&pr.dir, &skills, false);
    assert!(again.actions.iter().all(|a| a.kind == ActionKind::Unchanged), "{:#?}", again.actions);
    assert!(!again.changed);
}

#[test]
fn undo_restores_the_project_exactly_and_deletes_nothing() {
    let pr = project();
    let before = snapshot(&pr.dir);
    let items = [prepared("tdd", "v1", &["claude", "codex"])];
    let skills: Vec<PreparedSkill> = items.iter().map(|(s, _)| s.clone()).collect();
    run(&pr.dir, &plan_now(&pr.dir, &skills, false), &contents(&items), "op1", None).expect("apply");
    assert_ne!(snapshot(&pr.dir), before);

    let report = undo(&pr.dir, None).expect("undo");
    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let after: Vec<_> = snapshot(&pr.dir)
        .into_iter()
        .filter(|(p, _)| !matches!(p.as_str(), ".agents/" | ".claude/" | ".agents/skills/" | ".claude/skills/"))
        .collect();
    assert_eq!(after, before, "project content restored");
    let kept = pr.dir.join(".aurora/ops/op1");
    let moved: Vec<_> = walkdir(&kept).into_iter().filter(|p| p.ends_with("SKILL.md")).collect();
    assert_eq!(moved.len(), 2, "both installed copies were moved aside, not deleted: {moved:?}");
    assert!(matches!(undo(&pr.dir, Some("op1")), Err(ApplyError::AlreadyUndone(_))));
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() { out.extend(walkdir(&p)) } else { out.push(p) }
        }
    }
    out
}

#[test]
fn updates_move_the_old_copy_aside_and_undo_brings_it_back() {
    let pr = project();
    let v1 = [prepared("tdd", "v1", &["claude"])];
    run(&pr.dir, &plan_now(&pr.dir, &[v1[0].0.clone()], false), &contents(&v1), "op1", None).expect("v1");
    let v2 = [prepared("tdd", "v2", &["claude"])];
    let p = plan_now(&pr.dir, &[v2[0].0.clone()], false);
    assert!(p.actions.iter().any(|a| a.target == ".claude/skills/tdd" && a.kind == ActionKind::Update));
    run(&pr.dir, &p, &contents(&v2), "op2", None).expect("v2");
    let md = || fs::read_to_string(pr.dir.join(".claude/skills/tdd/SKILL.md")).expect("read");
    assert!(md().contains("body v2"));
    undo(&pr.dir, None).expect("undo v2");
    assert!(md().contains("body v1"));
    let lock = Lock::from_json(&fs::read_to_string(pr.dir.join("aurora.lock")).expect("lock")).expect("parse");
    assert_eq!(lock.skills[".claude/skills/tdd"].installed_sha256, v1[0].1.content_hash(), "the lock is restored too");
}

#[test]
fn prune_moves_copies_aside_and_undo_restores_them() {
    let pr = project();
    let items = [prepared("review", "v1", &["codex"])];
    run(&pr.dir, &plan_now(&pr.dir, &[items[0].0.clone()], false), &contents(&items), "op1", None).expect("apply");
    let p = plan_now(&pr.dir, &[], true);
    assert!(p.actions.iter().any(|a| a.kind == ActionKind::Remove));
    run(&pr.dir, &p, &BTreeMap::new(), "op2", None).expect("prune");
    assert!(!pr.dir.join(".agents/skills/review").exists());
    undo(&pr.dir, None).expect("undo");
    assert!(pr.dir.join(".agents/skills/review/SKILL.md").is_file());
}

#[test]
fn a_crash_leaves_a_pending_operation_that_blocks_until_undone() {
    let pr = project();
    let before = snapshot(&pr.dir);
    let items = [prepared("tdd", "v1", &["claude"]), prepared("review", "v1", &["codex"])];
    let skills: Vec<PreparedSkill> = items.iter().map(|(s, _)| s.clone()).collect();
    let p = plan_now(&pr.dir, &skills, false);
    assert!(matches!(run(&pr.dir, &p, &contents(&items), "op1", Some(2)), Err(ApplyError::InjectedFault(2))));
    assert_eq!(pending_op(&pr.dir).expect("scan").as_deref(), Some("op1"));
    assert!(matches!(run(&pr.dir, &p, &contents(&items), "op2", None), Err(ApplyError::Pending(_))));

    undo(&pr.dir, None).expect("recover");
    assert!(pending_op(&pr.dir).expect("scan").is_none());
    let after: Vec<_> = snapshot(&pr.dir).into_iter().filter(|(p, _)| !p.ends_with('/')).collect();
    let before: Vec<_> = before.into_iter().filter(|(p, _)| !p.ends_with('/')).collect();
    assert_eq!(after, before);
}

#[test]
fn undo_skips_copies_the_user_changed() {
    let pr = project();
    let items = [prepared("tdd", "v1", &["claude"])];
    run(&pr.dir, &plan_now(&pr.dir, &[items[0].0.clone()], false), &contents(&items), "op1", None).expect("apply");
    fs::write(pr.dir.join(".claude/skills/tdd/notes.md"), "my notes\n").expect("edit");
    let report = undo(&pr.dir, None).expect("undo");
    assert_eq!(report.skipped.len(), 1, "{:?}", report.skipped);
    assert!(report.skipped[0].contains(".claude/skills/tdd"));
    assert_eq!(fs::read_to_string(pr.dir.join(".claude/skills/tdd/notes.md")).expect("kept"), "my notes\n");
}

#[test]
fn a_destination_appearing_after_planning_aborts_and_rolls_back() {
    let pr = project();
    let before = snapshot(&pr.dir);
    let items = [prepared("a-first", "v1", &["claude"]), prepared("z-last", "v1", &["claude"])];
    let skills: Vec<PreparedSkill> = items.iter().map(|(s, _)| s.clone()).collect();
    let p = plan_now(&pr.dir, &skills, false);
    fs::create_dir_all(pr.dir.join(".claude/skills/z-last")).expect("someone else");
    let err = run(&pr.dir, &p, &contents(&items), "op1", None).expect_err("refused");
    assert!(matches!(err, ApplyError::Precondition { .. }), "{err:?}");
    assert!(pending_op(&pr.dir).expect("scan").is_none(), "rolled back, nothing pending");
    let now: Vec<_> = snapshot(&pr.dir).into_iter().filter(|(p, _)| !p.ends_with('/')).collect();
    let before: Vec<_> = before.into_iter().filter(|(p, _)| !p.ends_with('/')).collect();
    assert_eq!(now, before);
    assert!(pr.dir.join(".claude/skills/z-last").is_dir(), "the other process's directory is untouched");
}

#[test]
fn refused_and_unchanged_actions_are_not_executed() {
    let pr = project();
    fs::create_dir_all(pr.dir.join(".claude/skills/tdd")).expect("mkdir");
    fs::write(pr.dir.join(".claude/skills/tdd/SKILL.md"), "mine").expect("write");
    let items = [prepared("tdd", "v1", &["claude"])];
    let p = plan_now(&pr.dir, &[items[0].0.clone()], false);
    assert!(p.actions.iter().any(|a| a.kind == ActionKind::Refuse));
    run(&pr.dir, &p, &contents(&items), "op1", None).expect("applies the rest");
    assert_eq!(fs::read_to_string(pr.dir.join(".claude/skills/tdd/SKILL.md")).expect("read"), "mine");
}
