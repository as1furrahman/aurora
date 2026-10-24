---
name: principles
description: "Core baseline engineering principles: correctness outranks token savings, deterministic safety, and test-first verification."
---

# Baseline Engineering Principles

## 1. Safety and Correctness First
- Correctness and data safety strictly outrank execution speed and token conservation.
- Never execute unchecked or unverified filesystem mutations.
- Always verify path ancestry, reject symlinks, and avoid TOCTOU races using descriptor-relative operations.

## 2. Deterministic Testing
- Every safety primitive must have deterministic adversarial test gates.
- Verify both positive controls and negative failure injection modes.
- Never mock away core filesystem boundaries or kernel advisory locks.

## 3. Preservation Invariant
- Existing repositories, external user code, and system-managed agent skills must remain pristine.
- No global mutations may be executed without explicit authorization and verified pre-mutation snapshots.
