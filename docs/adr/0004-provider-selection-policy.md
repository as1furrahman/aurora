# ADR-0004 — Provider selection: routing, single-provider override, declared fallback

**Status:** Accepted (2026-10-23)

## Context
Users want both "route each role to the best CLI" and "run everything through the one CLI I have".
Neither choice may change role responsibilities or acceptance gates, and no provider switch may be
invisible.

## Decision
- **Syntax:** `--provider <id>` (single-provider override, applied to all stages) and
  `--route <stage>=<id>` (per-stage, repeatable). The two are mutually exclusive. The same flags are
  used on `workflow plan` and `workflow start`.
- **Precedence:** override > route > the workflow's `preferred` > the first `eligible` provider.
- **Eligibility:** the role's `[routing.roles.<role>].eligible` list. Under `--provider`, a stage
  whose role does not permit that provider becomes `BLOCKED (not-eligible)`. Its role is not
  rewritten and no other provider is substituted. An ineligible `--route` is a hard error.
- **Fallback:** only through `[[routing.fallback]]` rules with an explicit trigger (`incompatible`,
  `unavailable`). Rules are evaluated in declaration order and every step is recorded. Fallback is
  disabled for stages assigned by `--provider` or `--route`, so an explicit user choice is never
  overridden.
- **Independence:** stages declare `independent_of`. A pair with the same provider is reported as
  `same-provider`. Aurora never labels such a pair as independent verification.

## Consequences
- Plans are deterministic for a given (definition, registry, options, detected set).
- Single-provider runs keep all role prompts and gates, and their reduced independence is visible.
