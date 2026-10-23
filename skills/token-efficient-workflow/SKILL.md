---
name: token-efficient-workflow
description: Universal token-efficient workflow using descriptor pinning, atomic file safety, and minimal tool overhead.
---

# Token-Efficient Hardened Workflow

## 1. Targeted Code & File Exploration
- Search before reading: identify exact function, class, or symbol definitions.
- Line-range reads: inspect only required blocks (typically 20–50 lines).
- Avoid re-reading: maintain state awareness within the conversation trajectory.

## 2. Command Output Compression
- Pipe verbose commands through native filters (`stat`, `grep`, `head`, `tail`).
- Filter build and test outputs while strictly preserving critical failure evidence and error stacks.
