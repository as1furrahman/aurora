---
name: verification-guard
description: "Verification and audit guard: independent pre- and post-mutation verification, cryptographic preservation auditing, and test assertions."
---

# Verification and Audit Guard

## 1. Pre-Mutation Verification
- Compute cryptographic baseline hashes of all relevant filesystem trees before altering any file.
- Verify target ownership, permissions, and active locks prior to performing mutations.

## 2. Post-Mutation Verification
- Re-run full test suites and confirm exit code 0.
- Verify that only intended target files were modified, and all protected assets remain 100% matched to baseline.
- Audit for orphaned staging artifacts or lock remnants.
