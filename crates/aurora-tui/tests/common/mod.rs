//! Offline fixture for TUI tests: a local git mirror of a team skill repository, a catalog that
//! pins it, and an empty project. Plans are computed with the real `aurora_app::plan_install`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use aurora_app::install::{InstallRequest, Planned, WorkflowSource, plan_install};
use aurora_skills::tree::{Limits, collect};
use aurora_tui::app::{App, TuiOptions};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub struct Fixture {
    _tmp: tempfile::TempDir,
    pub catalog: PathBuf,
    pub mirror: PathBuf,
    pub cache: PathBuf,
    pub project: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        // Fixed dates make the fixture commit SHA, and so every snapshot, reproducible.
        .env("GIT_AUTHOR_DATE", "2026-10-24T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2026-10-24T00:00:00Z")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

const SKILLS: &[(&str, &str)] = &[
    ("skills/house-rules/SKILL.md", "---\nname: house-rules\ndescription: Team rules.\n---\nKeep changes small.\n"),
    ("skills/demo/SKILL.md", "---\nname: demo\ndescription: Evidence before claims.\n---\nVerify first.\n"),
    (
        "skills/checker/SKILL.md",
        "---\nname: checker\ndescription: Review diffs.\nallowed-tools: Read Bash\n---\nReview.\n",
    ),
    ("skills/scripted/SKILL.md", "---\nname: scripted\ndescription: Has a helper.\n---\nRun run.sh.\n"),
    ("skills/scripted/run.sh", "echo hi\n"),
    ("skills/extra/SKILL.md", "---\nname: extra\ndescription: Optional browser checks.\n---\nCheck pages.\n"),
];

pub fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("mirror/demo-org/team-skills");
    for (path, text) in SKILLS {
        let p = repo.join(path);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, text).expect("write");
    }
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "uploadpack.allowFilter", "true"]);
    git(&repo, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let rev = git(&repo, &["rev-parse", "HEAD"]);
    let h = |name: &str| collect(&repo.join("skills").join(name), &Limits::default()).expect("collect").content_hash();
    let entry = |id: &str, kind: &str, tier: &str, roles: &str, chars: usize| {
        format!(
            "[[entries]]\nid = \"{id}\"\nkind = \"{kind}\"\nsource = \"team\"\npath = \"skills/{id}\"\ntier = \"{tier}\"\n{roles}\
             description_chars = {chars}\nsha256 = \"{}\"\nwhy = \"Why {id}.\"\n\n",
            h(id)
        )
    };
    let catalog = format!(
        r#"schema_version = 1
verified_on = "2026-10-24"

[targets.claude]
listing_chars = 8000
per_skill_chars = 1536
rules_file = "CLAUDE.md"
evidence = "test"

[targets.codex]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "test"

[targets.antigravity]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "test"

[targets.hermes]
listing_chars = 12000
rules_file = "AGENTS.md"
evidence = "test"

[profiles.solo]
description = "one"
default_clis = ["claude"]
roles = {{ analyst = 1, architect = 1, implementer = 1, tester = 1, reviewer = 1, security = 1, documenter = 1 }}

[profiles.pair]
description = "two"
default_clis = ["claude", "codex"]
roles = {{ analyst = 1, architect = 1, implementer = 1, tester = 2, reviewer = 2, security = 1, documenter = 1 }}

[sources.team]
repo = "https://github.com/demo-org/team-skills"
rev = "{rev}"
license = "MIT"
trust = "curated"

{rules}{demo}{checker}{scripted}{extra}"#,
        rules = entry("house-rules", "rules", "baseline", "", 11),
        demo = entry("demo", "skill", "baseline", "", 22),
        checker = entry("checker", "skill", "stage", "roles = [\"reviewer\"]\n", 13),
        scripted = entry("scripted", "skill", "stage", "roles = [\"implementer\"]\n", 14),
        extra = entry("extra", "skill", "opt-in", "roles = [\"tester\"]\n", 23),
    );
    let catalog_path = tmp.path().join("catalog.toml");
    fs::write(&catalog_path, catalog).expect("write catalog");
    let project = tmp.path().join("project");
    fs::create_dir(&project).expect("mkdir");
    let project = fs::canonicalize(project).expect("canonical");
    Fixture {
        catalog: catalog_path,
        mirror: tmp.path().join("mirror"),
        cache: tmp.path().join("cache"),
        project,
        _tmp: tmp,
    }
}

impl Fixture {
    pub fn options(&self) -> TuiOptions {
        TuiOptions {
            project: self.project.clone(),
            catalog: Some(self.catalog.clone()),
            cache: Some(self.cache.clone()),
            mirror: Some(self.mirror.clone()),
            no_color: true,
            ..TuiOptions::default()
        }
    }

    pub fn request(&self, with: &[&str]) -> InstallRequest {
        InstallRequest {
            workflow: WorkflowSource::Preset("bugfix".into()),
            profile: None,
            clis: Vec::new(),
            with: with.iter().map(|s| s.to_string()).collect(),
            project: self.project.clone(),
            keep_hooks: false,
            prune: false,
            catalog: Some(self.catalog.clone()),
            cache: Some(self.cache.clone()),
            mirror: Some(self.mirror.clone()),
        }
    }

    pub fn planned(&self, with: &[&str]) -> Planned {
        plan_install(&self.request(with)).expect("plan")
    }
}

pub fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub fn press(app: &mut App, codes: &[KeyCode]) {
    for c in codes {
        app.press(key(*c));
    }
}

pub fn screen(app: &App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).expect("terminal");
    term.draw(|f| aurora_tui::ui::render(f, app)).expect("draw");
    term.backend().to_string()
}

/// Snapshot `s` with the temporary directory replaced by `[tmp]`.
#[macro_export]
macro_rules! snapshot {
    ($name:expr, $s:expr) => {
        insta::with_settings!({ filters => vec![(r"/[^ ]*?/\.tmp[A-Za-z0-9]+", "[tmp]")] }, {
            insta::assert_snapshot!($name, $s);
        });
    };
}

/// R3.1: engine words may appear only under "More details" (see tests/plain_words.rs).
pub const JARGON: &[&str] = &[
    "CLI",
    "provider",
    "stage",
    "handoff",
    "overlay",
    "sha256",
    "acceptance criteria",
    "listing",
    "budget",
    "journal",
];

/// The visible text, leaving out everything a details pane shows below "More details".
pub fn plain_part(s: &str) -> String {
    let mut out = String::new();
    let mut in_details = false;
    for line in s.lines() {
        let split = ["││", "┐┌", "┘└"].iter().filter_map(|sep| line.find(sep)).min();
        let (left, right) = match split {
            Some(i) => line.split_at(i),
            None => (line, ""),
        };
        out.push_str(left);
        if right.contains("More details") {
            in_details = true;
        }
        if !in_details {
            out.push_str(right);
        }
        out.push('\n');
    }
    out
}

pub fn assert_plain(s: &str, where_: &str) {
    // Product names are fine: "Gemini CLI" is what that app is called.
    let plain = plain_part(s).replace("Gemini CLI", "Gemini");
    for word in JARGON {
        let found = plain.split(|c: char| !c.is_alphanumeric()).any(|w| {
            if word.contains(' ') {
                false
            } else if *word == "CLI" {
                w == "CLI" || w == "CLIs"
            } else {
                w.eq_ignore_ascii_case(word)
            }
        }) || (word.contains(' ') && plain.to_lowercase().contains(word));
        assert!(!found, "{word:?} on the {where_}:\n{s}");
    }
}
