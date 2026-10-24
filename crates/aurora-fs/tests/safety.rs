//! Filesystem safety gates and boundary rules. All fixtures live in temporary directories;
//! protected/global locations are injected through `BoundaryPolicy`, never the real home.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use aurora_fs::FsError;
use aurora_fs::atomic::{Fault, atomic_write, atomic_write_with_fault};
use aurora_fs::boundaries::{BoundaryPolicy, validate_security_preconditions};
use aurora_fs::paths::validate_path;
use aurora_fs::read::safe_read;

fn tmp() -> (tempfile::TempDir, PathBuf) {
    let t = tempfile::tempdir().expect("tempdir");
    let root = t.path().canonicalize().expect("canonical");
    (t, root)
}

fn policy(home: &Path) -> BoundaryPolicy {
    BoundaryPolicy::for_home(home)
}

// --- gate 1: symlinked ancestry ------------------------------------------------

#[test]
fn gate1_rejects_symlinked_ancestor_directory() {
    let (_t, root) = tmp();
    fs::create_dir(root.join("real")).expect("mkdir");
    symlink(root.join("real"), root.join("link")).expect("symlink");
    let err = validate_path(&root.join("link/file.txt"), &[&root], false).expect_err("symlinked ancestor");
    assert!(matches!(err, FsError::SymlinkAncestor(_)), "{err}");
}

#[test]
fn gate1_rejects_symlinked_leaf_target() {
    let (_t, root) = tmp();
    fs::write(root.join("real.txt"), "x").expect("write");
    symlink(root.join("real.txt"), root.join("leaf.txt")).expect("symlink");
    let err = validate_path(&root.join("leaf.txt"), &[&root], false).expect_err("symlinked leaf");
    assert!(matches!(err, FsError::SymlinkAncestor(_)), "{err}");
}

#[test]
fn gate1_positive_control_regular_ancestry() {
    let (_t, root) = tmp();
    fs::create_dir_all(root.join("a/b")).expect("mkdir");
    let canon = validate_path(&root.join("a/b/new.txt"), &[&root], false).expect("regular path");
    assert_eq!(canon, root.join("a/b/new.txt"));
}

// --- gate 2: traversal -----------------------------------------------------------

#[test]
fn gate2_rejects_dot_dot_traversal_escape() {
    let (_t, root) = tmp();
    fs::create_dir(root.join("inner")).expect("mkdir");
    let err = validate_path(&root.join("inner/../../escape.txt"), &[&root.join("inner")], false).expect_err("escape");
    assert!(matches!(err, FsError::PathTraversal(_)), "{err}");
}

#[test]
fn gate2_rejects_unauthorized_absolute_path() {
    let (_t, root) = tmp();
    let err = validate_path(Path::new("/etc/passwd"), &[&root], false).expect_err("outside");
    assert!(matches!(err, FsError::PathTraversal(_)));
}

#[test]
fn gate2_rejects_sibling_prefix_confusion() {
    let (_t, root) = tmp();
    fs::create_dir(root.join("run")).expect("mkdir");
    // "/x/run-evil" must not count as inside "/x/run".
    let err = validate_path(&root.join("run-evil/f"), &[&root.join("run")], false).expect_err("prefix");
    assert!(matches!(err, FsError::PathTraversal(_)));
}

#[test]
fn gate2_positive_control_nested_boundary() {
    let (_t, root) = tmp();
    assert!(validate_path(&root.join("x/y/z.txt"), &[&root], false).is_ok());
}

// --- gate 3: non-blocking reads --------------------------------------------------

#[test]
fn gate3_safe_read_rejects_fifo_without_blocking() {
    let (_t, root) = tmp();
    let fifo = root.join("pipe");
    // The mkfifo command, not mknodat: rustix has no mknodat on macOS.
    let made = std::process::Command::new("mkfifo").arg(&fifo).status().expect("mkfifo runs");
    assert!(made.success(), "mkfifo");
    let err = safe_read(&fifo, &[&root], 1024).expect_err("fifo refused");
    assert!(matches!(err, FsError::Fifo(_)), "{err}");
}

#[test]
fn gate3_positive_control_regular_read() {
    let (_t, root) = tmp();
    fs::write(root.join("f.txt"), "hello").expect("write");
    let info = safe_read(&root.join("f.txt"), &[&root], 1024).expect("read");
    assert_eq!(info.content, b"hello");
    assert_eq!(info.size, 5);
    assert_eq!(info.sha256, "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824");
}

#[test]
fn gate3_read_rejects_symlink_and_oversize() {
    let (_t, root) = tmp();
    fs::write(root.join("big.txt"), vec![b'x'; 2048]).expect("write");
    assert!(matches!(safe_read(&root.join("big.txt"), &[&root], 1024), Err(FsError::Descriptor(_))));
    symlink(root.join("big.txt"), root.join("alias.txt")).expect("symlink");
    assert!(matches!(safe_read(&root.join("alias.txt"), &[&root], 4096), Err(FsError::SymlinkAncestor(_))));
}

// --- gate 5: atomic writes -------------------------------------------------------

fn staging_files(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .expect("dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".awb_stage_"))
        .collect()
}

#[test]
fn gate5_failure_injection_preserves_original_and_cleans_up() {
    for fault in [Fault::AfterCreate, Fault::DuringWrite, Fault::BeforeFsync, Fault::BeforeReplace] {
        let (_t, root) = tmp();
        let dest = root.join("record.json");
        fs::write(&dest, "original").expect("seed");
        let err = atomic_write_with_fault(&dest, b"replacement", &[&root], 0o600, Some(fault));
        assert!(err.is_err(), "{fault:?} must fail");
        assert_eq!(fs::read_to_string(&dest).expect("read"), "original", "{fault:?}");
        assert!(staging_files(&root).is_empty(), "{fault:?} left staging files");
    }
}

#[test]
fn gate5_clean_write_is_complete_private_and_leaves_no_staging() {
    let (_t, root) = tmp();
    let dest = root.join("record.json");
    let info = atomic_write(&dest, b"{}\n", &[&root], 0o600).expect("write");
    assert_eq!(fs::read(&dest).expect("read"), b"{}\n");
    assert_eq!(fs::metadata(&dest).expect("meta").permissions().mode() & 0o777, 0o600);
    assert_eq!(info.size, 3);
    assert!(staging_files(&root).is_empty());
}

#[test]
fn gate5_atomic_write_rejects_symlinked_destination() {
    let (_t, root) = tmp();
    fs::write(root.join("real"), "keep").expect("write");
    symlink(root.join("real"), root.join("dest")).expect("symlink");
    assert!(atomic_write(&root.join("dest"), b"x", &[&root], 0o600).is_err());
    assert_eq!(fs::read_to_string(root.join("real")).expect("read"), "keep");
}

// --- boundary policy -------------------------------------------------------------

#[test]
fn boundaries_accept_private_sandbox() {
    let (_t, root) = tmp();
    let home = root.join("home");
    fs::create_dir(&home).expect("home");
    let sandbox = root.join("sandbox");
    fs::create_dir(&sandbox).expect("sandbox");
    let ok = validate_security_preconditions(&sandbox.join("run"), &[&sandbox], false, &policy(&home)).expect("ok");
    assert_eq!(ok, sandbox.join("run"));
}

#[test]
fn boundaries_block_global_agent_dirs_unless_allowed() {
    let (_t, root) = tmp();
    let home = root.join("home");
    let claude = home.join(".claude/skills");
    fs::create_dir_all(&claude).expect("mkdir");
    let p = policy(&home);
    let err =
        validate_security_preconditions(&claude.join("x"), &[&home.join(".claude")], false, &p).expect_err("global");
    assert!(matches!(err, FsError::GlobalMutationBlocked(_)), "{err}");
    for dir in [".agents/skills", ".codex/skills", ".gemini/antigravity-cli/skills", ".gemini/config/skills"] {
        assert!(p.global_agent_dirs.contains(&home.join(dir)), "{dir} must be guarded");
    }
}

#[test]
fn boundaries_reject_system_dirs_sensitive_paths_and_broad_roots() {
    let (_t, root) = tmp();
    let home = root.join("home");
    fs::create_dir(&home).expect("home");
    let p = policy(&home);
    let sandbox = root.join("sb");
    fs::create_dir(&sandbox).expect("sb");
    assert!(matches!(
        validate_security_preconditions(&sandbox.join(".system/x"), &[&sandbox], false, &p),
        Err(FsError::Safety(_))
    ));
    assert!(matches!(
        validate_security_preconditions(Path::new("/etc/x"), &[&sandbox], false, &p),
        Err(FsError::Safety(_))
    ));
    for broad in [Path::new("/"), Path::new("/home"), home.as_path()] {
        assert!(
            matches!(validate_security_preconditions(&sandbox.join("x"), &[broad], false, &p), Err(FsError::Safety(_))),
            "{}",
            broad.display()
        );
    }
    assert!(matches!(
        validate_security_preconditions(&sandbox.join("x"), &[] as &[&Path], false, &p),
        Err(FsError::SecurityPrecondition(_))
    ));
}

#[test]
fn boundaries_reject_protected_projects_in_both_directions() {
    let (_t, root) = tmp();
    let home = root.join("home");
    let project = root.join("projects/precious");
    fs::create_dir_all(&project).expect("mkdir");
    let mut p = policy(&home);
    p.protected_projects = vec![project.clone()];
    let parent = root.join("projects");
    for (target, roots) in [(project.join("x"), vec![parent.clone()]), (parent.join("other"), vec![parent.clone()])] {
        let refs: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        assert!(matches!(validate_security_preconditions(&target, &refs, false, &p), Err(FsError::Safety(_))));
    }
}

#[test]
fn boundaries_reject_group_or_world_writable_parent_without_sticky_bit() {
    let (_t, root) = tmp();
    let home = root.join("home");
    let p = policy(&home);
    let shared = root.join("shared");
    fs::create_dir(&shared).expect("mkdir");
    for mode in [0o777, 0o770] {
        fs::set_permissions(&shared, fs::Permissions::from_mode(mode)).expect("chmod");
        let err = validate_security_preconditions(&shared.join("run"), &[&shared], false, &p).expect_err("insecure");
        assert!(matches!(err, FsError::SecurityPrecondition(_)), "{mode:o}: {err}");
    }
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o1777)).expect("chmod sticky");
    assert!(validate_security_preconditions(&shared.join("run"), &[&shared], false, &p).is_ok());
}
