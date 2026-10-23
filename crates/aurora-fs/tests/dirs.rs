//! R2 Step 4a: fd-relative directory primitives for installing skills. Every path is relative
//! to an opened root; no component is ever followed through a symlink; renames never replace.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use aurora_fs::dirs::{Entry, NewFile, Root};

fn files() -> Vec<NewFile> {
    vec![
        NewFile {
            path: "SKILL.md".into(),
            content: b"---\nname: x\ndescription: d\n---\n".to_vec(),
            executable: false,
        },
        NewFile { path: "scripts/run.sh".into(), content: b"echo hi\n".to_vec(), executable: true },
    ]
}

#[test]
fn ensure_dir_creates_missing_components() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    root.ensure_dir(".claude/skills").expect("mkdir -p");
    assert!(tmp.path().join(".claude/skills").is_dir());
    root.ensure_dir(".claude/skills").expect("idempotent");
}

#[test]
fn ensure_dir_refuses_symlinked_components() {
    let tmp = tempfile::tempdir().expect("tmp");
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    std::os::unix::fs::symlink(elsewhere.path(), tmp.path().join(".claude")).expect("symlink");
    let root = Root::open(tmp.path()).expect("open");
    assert!(root.ensure_dir(".claude/skills").is_err());
    assert!(fs::read_dir(elsewhere.path()).expect("read").next().is_none(), "nothing written through the link");
}

#[test]
fn write_tree_places_a_complete_directory_with_modes() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    root.ensure_dir(".agents/skills").expect("mkdir");
    root.write_tree(".agents/skills", "x", &files()).expect("write");
    let dir = tmp.path().join(".agents/skills/x");
    assert_eq!(fs::read(dir.join("scripts/run.sh")).expect("read"), b"echo hi\n");
    let mode = |p: &str| fs::metadata(dir.join(p)).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode("scripts/run.sh") & 0o111, 0o111);
    assert_eq!(mode("SKILL.md") & 0o111, 0);
    let leftovers: Vec<_> = fs::read_dir(tmp.path().join(".agents/skills")).expect("read").flatten().collect();
    assert_eq!(leftovers.len(), 1, "no staging directory left behind");
}

#[test]
fn write_tree_never_replaces_an_existing_destination() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    fs::create_dir_all(tmp.path().join("skills/x")).expect("mkdir");
    fs::write(tmp.path().join("skills/x/mine.md"), b"keep").expect("write");
    assert!(root.write_tree("skills", "x", &files()).is_err());
    assert_eq!(fs::read(tmp.path().join("skills/x/mine.md")).expect("read"), b"keep");
    assert_eq!(fs::read_dir(tmp.path().join("skills")).expect("read").count(), 1, "staging cleaned up");
}

#[test]
fn write_tree_rejects_escaping_paths() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    let bad = vec![NewFile { path: "../evil".into(), content: b"x".to_vec(), executable: false }];
    assert!(root.write_tree("", "x", &bad).is_err());
    assert!(!tmp.path().parent().expect("parent").join("evil").exists());
    assert!(root.write_tree("", "../x", &files()).is_err());
    assert!(root.ensure_dir("/etc").is_err() && root.ensure_dir("a/../../b").is_err());
}

#[test]
fn rename_moves_within_the_root_without_replacing() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    root.ensure_dir("a").expect("mkdir");
    root.ensure_dir("backup").expect("mkdir");
    root.write_tree("a", "x", &files()).expect("write");
    root.rename("a/x", "backup/x").expect("move");
    assert!(tmp.path().join("backup/x/SKILL.md").is_file() && !tmp.path().join("a/x").exists());
    root.write_tree("a", "x", &files()).expect("write again");
    assert!(root.rename("a/x", "backup/x").is_err(), "destination exists: refused");
}

#[test]
fn entry_reports_what_is_there_without_following() {
    let tmp = tempfile::tempdir().expect("tmp");
    let root = Root::open(tmp.path()).expect("open");
    fs::create_dir(tmp.path().join("d")).expect("mkdir");
    fs::write(tmp.path().join("f"), b"x").expect("write");
    std::os::unix::fs::symlink("d", tmp.path().join("l")).expect("symlink");
    assert_eq!(root.entry("d").expect("d"), Entry::Dir);
    assert_eq!(root.entry("f").expect("f"), Entry::File);
    assert_eq!(root.entry("l").expect("l"), Entry::Symlink);
    assert_eq!(root.entry("missing").expect("missing"), Entry::Absent);
    assert_eq!(root.entry("missing/deeper").expect("missing parent"), Entry::Absent);
}

#[test]
fn opening_a_symlinked_root_is_refused() {
    let tmp = tempfile::tempdir().expect("tmp");
    fs::create_dir(tmp.path().join("real")).expect("mkdir");
    std::os::unix::fs::symlink("real", tmp.path().join("link")).expect("symlink");
    assert!(Root::open(&tmp.path().join("link")).is_err());
}
