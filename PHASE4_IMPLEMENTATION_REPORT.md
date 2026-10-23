# Agent Workbench: Phase 4 Controlled Mutation Implementation Report

**Document Version:** 4.0.0  
**Phase:** Phase 4 Controlled Mutation Implementation (`awb install`, `awb remove`)  
**Primary Implementer:** Google Antigravity CLI  
**Verification Target:** OpenAI Codex CLI Independent Audit  
**Execution Date:** 2026-10-23  
**Status:** **75/75 TESTS PASSED** | **348/348 PROTECTED NODES PRESERVED** | **GLOBAL MUTATIONS DISABLED**

---

## 1. Executive Summary

In accordance with Phase 3 authorization guidelines, Agent Workbench has implemented its first controlled filesystem mutation commands: `awb install` and `awb remove`. 

All mutation operations are executed strictly through `TransactionEngine.execute()`, requiring persistent cross-process locking via `ProcessLock`, pre-mutation snapshot validation, descriptor-relative filesystem primitives via `SafeFS`, and automatic atomic rollback upon any step failure.

### Key Implementation Milestones:
1. **`awb install`**: Supports single-skill selection from canonical registry, deterministic destination routing, pre-mutation snapshots, and installation of canonical skill files alongside a cryptographically sealed `.awb-manifest.json`.
2. **`awb remove`**: Strictly enforces verified ownership before unlinking. Rejects non-AWB directories, unmanaged extra files, content drift (modified files), symlinks, and protected paths.
3. **Dry-Run Engine (`--dry-run`)**: Both commands generate and display full topological mutation plans without acquiring locks or touching the filesystem.
4. **Filesystem Precondition Enforcement**: Rejects world-writable directories lacking sticky bits, directories not owned by the current user UID, paths traversing `.system/`, and unauthorized boundaries.
5. **Phase 4 Global Directory Guard**: Direct mutations targeting real global agent directories (`~/.codex/skills`, `~/.gemini/antigravity-cli/skills`, `~/.gemini/config/skills`) are hard-blocked with `GlobalMutationBlockedError`, confining execution to isolated sandboxes and test fixtures.

---

## 2. Core Architecture & Modules Added

### A. Operations Package (`awb/operations/`)

| Module | Purpose | Security Controls |
| :--- | :--- | :--- |
| `manifest.py` | Model and verify `.awb-manifest.json`. | Hashes all managed files with SHA-256; detects file modification, missing files, extraneous unmanaged files, and symlink injection. |
| `snapshots.py` | `PreMutationSnapshot` capture. | Records exact device, inode, file hashes, and directory structure prior to lock acquisition. |
| `installer.py` | `SkillInstaller` execution. | Enforces ownership check of existing targets; blocks overwriting unmanaged directories; builds directory creation, file write, and manifest write steps. |
| `remover.py` | `SkillRemover` execution. | Re-verifies all hashes before unlinking; rejects content drift; deletes managed files before directory removal; rolls back on failure. |

### B. Canonical Skill Sources (`skills/`)
Created clean canonical skill sources matching `registry/skills.toml`:
- `skills/principles/SKILL.md`
- `skills/task-router/SKILL.md`
- `skills/verification-guard/SKILL.md`
- `skills/token-efficient-workflow/SKILL.md`
- `skills/token-efficient-workflow/rules/efficient-workflow.md`

### C. CLI Subcommands (`awb/cli.py`)
- `awb install <skill_id> --target <path> [--dry-run] [--json]`
- `awb remove <skill_id> --target <path> [--dry-run] [--json]`

---

## 3. Strict Safety & Ownership Invariants

### 1. Insecure Filesystem Configuration Defense
`SkillInstaller.check_security_preconditions`:
- Verifies target parent directory ownership (`st.st_uid == os.getuid()`).
- Rejects world-writable directories lacking the sticky bit (`st_mode & 0o002 != 0 and not (st_mode & 0o1000)`).
- Rejects paths outside declared `allowed_roots`.
- Rejects any target path residing in or pointing to `.system/`.

### 2. Global Directory Hard-Block
- If a target path points to `~/.codex/skills`, `~/.gemini/antigravity-cli/skills`, or `~/.gemini/config/skills`, execution fails closed with `GlobalMutationBlockedError`.
- Prevents accidental modification of live agent environments during testing.

### 3. Ambiguous Ownership & Content-Drift Defense
`SkillRemover.plan_remove`:
- **Missing Manifest:** Directory lacking `.awb-manifest.json` raises `OwnershipError`.
- **Modified Content:** Any file whose current SHA-256 does not match the manifest raises `StateDriftError`, preserving user changes from accidental deletion.
- **Extraneous Files:** Any file added to the directory after installation raises `OwnershipError`.
- **Symlink Infiltration:** Symlinks inside the skill directory raise `SafetyError`.

---

## 4. Test Suite Execution & Verification

Ran full test suite in Python 3.14.8 environment:
```bash
python3 -m unittest discover -s tests -v
```

### Breakdown by Test Module:
- `tests/test_adapters.py`: **6 tests passed**
- `tests/test_cli.py`: **11 tests passed** (including install dry-run, removal lifecycle, destination check, global blocks)
- `tests/test_locking.py`: **6 tests passed**
- `tests/test_registry.py`: **14 tests passed**
- `tests/test_safety.py`: **20 tests passed**
- `tests/test_transactions.py`: **4 tests passed**
- `tests/test_operations.py`: **14 tests passed** (adversarial ownership, drift, rollback, dry-run, multi-file skills)

**Total Test Count:** **75 tests passed, 0 failures, 0 errors, 0 skips in 0.420s (Exit code 0)**.

---

## 5. Protected Assets Baseline Preservation

Cryptographic comparison against `/tmp/full_preservation_baseline.json`:

| Protected Repository Tree | Baseline Nodes | Current Nodes | Added | Removed | Hash Differences |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `dwl/` | 98 | 98 | 0 | 0 | 0 |
| `dwl-native-bar-lab/` | 173 | 173 | 0 | 0 | 0 |
| `token-efficient-workflow/` | 5 | 5 | 0 | 0 | 0 |
| `~/.codex/skills/.system/` | 72 | 72 | 0 | 0 | 0 |
| **Total** | **348** | **348** | **0** | **0** | **0 (Pristine)** |
