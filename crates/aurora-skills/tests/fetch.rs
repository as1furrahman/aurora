//! R2 Step 2b: fetching a skill at a pinned commit. Network-free: fixtures are local git
//! repositories reached over file://, which only tests may enable.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use aurora_skills::fetch::{FetchError, FetchOptions, FetchRequest, fetch};
use aurora_skills::tree::{Limits, collect};

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
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// An upstream repository with two skills; returns (repo dir, first commit, second commit).
fn upstream(root: &Path) -> (PathBuf, String, String) {
    let repo = root.join("upstream");
    fs::create_dir_all(repo.join("skills/demo")).expect("mkdir");
    fs::create_dir_all(repo.join("skills/other")).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "uploadpack.allowFilter", "true"]);
    git(&repo, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    fs::write(repo.join("skills/demo/SKILL.md"), "---\nname: demo\ndescription: v1\n---\nbody\n").expect("write");
    fs::write(repo.join("skills/other/SKILL.md"), "---\nname: other\ndescription: o\n---\n").expect("write");
    fs::write(repo.join("README.md"), "top-level file outside the skill\n").expect("write");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "v1"]);
    let first = git(&repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("skills/demo/SKILL.md"), "---\nname: demo\ndescription: v2\n---\nbody\n").expect("write");
    git(&repo, &["commit", "-q", "-am", "v2"]);
    let second = git(&repo, &["rev-parse", "HEAD"]);
    (repo, first, second)
}

fn opts() -> FetchOptions {
    FetchOptions { timeout: Duration::from_secs(60), allow_file_urls: true }
}

fn url(repo: &Path) -> String {
    format!("file://{}", repo.display())
}

#[test]
fn fetches_exactly_the_pinned_commit_and_only_the_skill_path() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _second) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let req = FetchRequest { repo: &url(&repo), rev: &first, path: "skills/demo", cache_root: &cache };
    let dir = fetch(&req, &opts()).expect("fetch");
    assert!(dir.ends_with("skills/demo"));
    let text = fs::read_to_string(dir.join("SKILL.md")).expect("read");
    assert!(text.contains("description: v1"), "pinned commit, not the branch tip: {text}");
    let checkout = dir.parent().and_then(Path::parent).expect("checkout root");
    assert!(!checkout.join("README.md").exists(), "only the declared subpath is checked out");
    assert!(!checkout.join("skills/other").exists());
    assert_eq!(collect(&dir, &Limits::default()).expect("collect").files.len(), 1);
}

#[test]
fn the_cache_is_reused_without_contacting_the_remote() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let req = FetchRequest { repo: &u, rev: &first, path: "skills/demo", cache_root: &cache };
    let a = fetch(&req, &opts()).expect("first fetch");
    fs::remove_dir_all(&repo).expect("remote gone");
    let b = fetch(&req, &opts()).expect("served from cache");
    assert_eq!(a, b);
}

#[test]
fn different_revisions_get_different_cache_entries() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, second) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let one =
        fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/demo", cache_root: &cache }, &opts()).expect("v1");
    let two =
        fetch(&FetchRequest { repo: &u, rev: &second, path: "skills/demo", cache_root: &cache }, &opts()).expect("v2");
    assert_ne!(one, two);
    assert!(fs::read_to_string(two.join("SKILL.md")).expect("read").contains("v2"));
}

#[test]
fn requests_are_validated_before_git_runs() {
    let tmp = tempfile::tempdir().expect("tmp");
    let cache = tmp.path().join("cache");
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let bad = |repo: &str, rev: &str, path: &str, o: FetchOptions| {
        fetch(&FetchRequest { repo, rev, path, cache_root: &cache }, &o).expect_err("rejected")
    };
    let no_files = FetchOptions { allow_file_urls: false, ..opts() };
    assert!(matches!(bad("file:///tmp/x", sha, "s", no_files), FetchError::InvalidRequest(_)));
    assert!(matches!(bad("https://github.com/o/r", "main", "s", opts()), FetchError::InvalidRequest(_)));
    assert!(matches!(bad("https://github.com/o/r", sha, "../etc", opts()), FetchError::InvalidRequest(_)));
    assert!(matches!(bad("https://github.com/o/r", sha, "", opts()), FetchError::InvalidRequest(_)));
    assert!(matches!(bad("http://github.com/o/r", sha, "s", opts()), FetchError::InvalidRequest(_)));
    assert!(matches!(bad("https://github.com/o/r --upload-pack=x", sha, "s", opts()), FetchError::InvalidRequest(_)));
    assert!(!cache.exists(), "nothing was created for rejected requests");
}

#[test]
fn unknown_revisions_and_missing_paths_fail_and_leave_no_staging_directories() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let missing_rev = "0123456789abcdef0123456789abcdef01234567";
    let err = fetch(&FetchRequest { repo: &u, rev: missing_rev, path: "skills/demo", cache_root: &cache }, &opts())
        .expect_err("no such commit");
    assert!(matches!(err, FetchError::Git { .. }), "{err:?}");
    let err = fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/nope", cache_root: &cache }, &opts())
        .expect_err("no such path");
    assert!(matches!(err, FetchError::PathMissing { .. }), "{err:?}");
    fn staging(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().contains(".tmp-") {
                found.push(path.clone());
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) && !path.ends_with(".git") {
                staging(&path, found);
            }
        }
    }
    let mut leftovers = Vec::new();
    staging(&cache, &mut leftovers);
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn git_steps_time_out() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let o = FetchOptions { timeout: Duration::ZERO, ..opts() };
    let err = fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/demo", cache_root: &cache }, &o)
        .expect_err("times out");
    assert!(matches!(err, FetchError::Timeout { .. }), "{err:?}");
}

#[test]
fn two_skills_from_the_same_commit_share_a_cache_without_clobbering() {
    // Regression: the cache was keyed by repository and revision only, while each checkout is
    // sparse for one skill path, so the second skill from the same commit was "missing".
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let demo =
        fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/demo", cache_root: &cache }, &opts()).expect("demo");
    let other = fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/other", cache_root: &cache }, &opts())
        .expect("other skill from the same commit");
    assert!(demo.join("SKILL.md").is_file() && other.join("SKILL.md").is_file());
    assert_ne!(demo.parent(), other.parent());
}

#[test]
fn concurrent_fetches_of_the_same_skill_all_succeed() {
    // Regression (R3.1): the TUI plans on worker threads, so two plans in one process fetch the
    // same skill at once. Staging was named by process id only: the second thread failed with
    // "File exists" and its cleanup removed the first thread's checkout mid-fetch.
    let tmp = tempfile::tempdir().expect("tmp");
    let (repo, first, _) = upstream(tmp.path());
    let cache = tmp.path().join("cache");
    let u = url(&repo);
    let results: Vec<Result<PathBuf, FetchError>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                s.spawn(|| {
                    fetch(&FetchRequest { repo: &u, rev: &first, path: "skills/demo", cache_root: &cache }, &opts())
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("thread")).collect()
    });
    for r in &results {
        let dir = r.as_ref().expect("every concurrent fetch succeeds");
        assert!(dir.join("SKILL.md").is_file());
    }
    let parent = cache.join(fs::read_dir(&cache).expect("cache").next().expect("key").expect("entry").file_name());
    let leftovers: Vec<_> = walk(&parent).into_iter().filter(|p| p.to_string_lossy().contains(".tmp-")).collect();
    assert!(leftovers.is_empty(), "no staging directories left behind: {leftovers:?}");
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        out.push(p.clone());
        if p.is_dir() && !p.ends_with(".git") && out.len() < 200 {
            out.extend(walk(&p).into_iter().filter(|q| !q.components().any(|c| c.as_os_str() == ".git")));
        }
    }
    out
}
