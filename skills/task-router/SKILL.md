---
name: task-router
description: Autonomous task router for coding agents: routes complex programming tasks to targeted, minimal skill specializations without context bloat.
---

# Autonomous Task Router

## 1. Targeted Delegation
- Analyze user requests and decompose them into isolated, minimal scopes.
- Route coding, refactoring, and auditing tasks to specialized subagents or dedicated workflows.
- Keep the parent context lean by streaming summaries rather than raw intermediate logs.

## 2. Dependency Awareness
- Ensure prerequisites and dependencies are satisfied before delegating tasks.
- Respect topological dependency order for skill installations and execution plans.
