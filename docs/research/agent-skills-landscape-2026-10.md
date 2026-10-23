# Agent Skills Landscape: what Aurora should integrate (October 2026)

**Researched:** 2026-10-24, by three parallel research passes over GitHub (repos, API, issues),
Hacker News, skills.sh, vendor docs, surveys and security research.

**Caveats:**

- Reddit refused automated access, so community sentiment comes from HN and GitHub issues.
- Star counts and push dates are from the GitHub API on that day.
- Items marked (?) were not verified against a primary source.
- Benchmark and savings numbers are the projects' own claims unless a third-party source is cited.

## 1. Executive summary

1. **Most people run 1–3 CLIs.** The canonical pair is *Claude Code implements, Codex reviews*.
   Aurora should make `solo`, `pair` and `trio` CLI profiles first-class, with `pair` as the
   flagship default.
2. **Several things people call "skills" are not.**
   - Headroom, rtk and context-mode are proxies, hooks or MCP servers.
   - The Karpathy pack and Ponytail are always-on rules.
   - GSD is a whole lifecycle framework.
   - The Ralph loop is a technique.

   Aurora needs four kinds of entry (§5) and must treat each kind differently.
3. **The best general baseline is small, plain and hook-free:**
   - `karpathy-guidelines` (rules);
   - Superpowers' `verification-before-completion` and `systematic-debugging`, taken
     individually;
   - mattpocock's `handoff` and `grill-me`;
   - `planning-with-files`.
4. **Stage packs come from a few high-quality, portable collections:** obra/superpowers,
   mattpocock/skills, addyosmani/agent-skills, getsentry/skills, trailofbits/skills,
   anthropics/skills, Spec Kit / OpenSpec.
5. **Lifecycle frameworks conflict.** GSD, the Superpowers bootstrap, Spec Kit, BMAD, gstack and
   compound-engineering each want to own planning. Aurora is the orchestrator, so it cherry-picks
   individual skills and never installs their bootstraps or hooks.
6. **The skill supply chain is actively attacked.** About 12% of one registry was malicious, and
   skill scanners are bypassable. Pin to commits, review scripts by hand, use trust tiers, and
   treat scanners as signals rather than gates.
7. **Context budgets are real.**
   - Claude truncates each skill description at 1,536 characters, and the whole listing gets
     about 1% of the context window.
   - Codex caps the listing at 2% of context or 8k characters.

   Fewer, role-scoped skills beat a big global pile.

## 2. How people combine CLIs

| Source | Finding |
|---|---|
| [Pragmatic Engineer, AI Tooling 2026](https://newsletter.pragmaticengineer.com/p/ai-tooling-2026) (906 responses) | 15% use 1 AI tool, 70% use 2–4, 15% use 5+ (all AI tools, not only CLIs). Claude Code is used by 71% of regular agent users and is the most loved (46%). |
| [JetBrains, AI coding agent adoption 2026](https://blog.jetbrains.com/research/2026/08/ai-coding-agent-adoption-2026/) (15k+ developers) | ~90% use agents weekly. Claude Code ~39% worldwide; Codex rose to 16%. |
| [Stack Overflow 2025](https://survey.stackoverflow.co/2025/) | 31% use AI agents; Claude Code at 41% among users of AI assistants. |
| Practitioner write-ups ([zackproser](https://zackproser.com/blog/codex-reviews-claudes-diffs), [knightli](https://knightli.com/en/2026/07/10/claude-code-codex-code-review-workflow/), [claude-code-cross-review](https://github.com/atompilot/claude-code-cross-review)); OpenAI's official `codex-plugin-cc` | **Claude implements, Codex reviews** is the dominant pairing. The rules people follow: never edit the same files, share AGENTS.md, cap review rounds. Second subscriptions are also used to stretch rate limits. |
| [9to5google](https://9to5google.com/2026/06/17/gemini-cli-code-assist-shutting-down/) | Gemini CLI stopped serving individual users on 2026-06-18; Antigravity CLI (`agy`) replaced it. |

**Implication:** design for one to three targets. Claude Code and Codex are tier-1 adapters;
`agy` and Hermes are tier-2, gated on detected capabilities.

## 3. What the skills you named actually are

| Name | Canonical repo | Licence · stars · activity | What it is | Aurora treatment |
|---|---|---|---|---|
| Headroom | [headroomlabs-ai/headroom](https://github.com/headroomlabs-ai/headroom) (moved from chopratejas) | Apache-2.0 · 77.6k · active | Proxy, CLI and MCP server that compresses tool output, logs and RAG chunks. Telemetry is on by default (`HEADROOM_TELEMETRY=off`). `wrap` installs Serena and edits `~/.claude.json`. Its Codex proxy auth bug is open ([#71](https://github.com/headroomlabs-ai/headroom/issues/71)). | **Infra tier.** Opt-in only, kept out of the skill budget, opt-outs documented. |
| Andrej Karpathy skills | [multica-ai/andrej-karpathy-skills](https://github.com/multica-ai/andrej-karpathy-skills) (moved from forrestchang) | MIT per README (no LICENSE file) · 223k · last push 2026-04 | Four behaviour rules: think first, simplicity, surgical changes, goal-driven. Forrest Chang's distillation, not written by Karpathy. | **Rules tier.** Projected into AGENTS.md / CLAUDE.md for every stage. |
| Ponytail | [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) | MIT · 174.7k in about 4 months (popularity unverified) · active | Anti-over-engineering persona ("lazy senior dev", a 7-step ladder before writing code), plus `/ponytail-review`, `/ponytail-audit`, `/ponytail-debt`. **Not a profiler.** Its Node hooks inject into every session. Its baseline benchmark was disputed ([#126](https://github.com/DietrichGebert/ponytail/issues/126)). Fake copies shipping `.exe` files exist. | **Rules tier,** SKILL.md only with no hooks, on implement, refactor and review stages. |
| Ralph loop | Technique by [ghuntley](https://ghuntley.com/ralph); official [ralph-loop plugin](https://github.com/anthropics/claude-plugins-official/tree/main/plugins/ralph-loop); [frankbria/ralph-claude-code](https://github.com/frankbria/ralph-claude-code) | Apache-2.0 / MIT | The official plugin is a Claude-only Stop hook, and `--max-iterations` is unlimited by default. frankbria's version adds a circuit breaker and rate limits. Running it unattended typically needs a permission-bypass flag. | **Not installed.** It becomes Aurora's own bounded loop in the runner milestone (R4). |
| GSD | [open-gsd/gsd-core](https://github.com/open-gsd/gsd-core); [gsd-build/get-shit-done](https://github.com/gsd-build/get-shit-done) is **archived** | MIT · 10.8k · v1.18 (2026-10-19) | Full lifecycle (discuss → plan → execute → verify → ship) with **44 hooks**. One test measured 1.2M tokens against 200k for plain Claude Code ([chaseai](https://chaseai.io/blog/gsd-vs-superpowers-vs-claude-code)). | **Framework tier:** never combined with other frameworks. Its verify/ship ideas inform Aurora's accept and close stages. |

## 4. Recommended skills

### 4.1 General baseline (every stage, every CLI)

| # | Skill | Source | Why | Notes |
|---|---|---|---|---|
| 1 | `karpathy-guidelines` | multica-ai/andrej-karpathy-skills | The cheapest guard against over-engineering and changes nobody asked for | Rules; ~70 lines; no scripts |
| 2 | `verification-before-completion` | [obra/superpowers](https://github.com/obra/superpowers) (MIT, 304k, active) | An honesty gate on every stage exit; matches Aurora's evidence model | Take the individual SKILL.md, **not** the plugin or its `using-superpowers` bootstrap (that hook forces skill use before every reply, and its visual companion has a usage beacon) |
| 3 | `systematic-debugging` | obra/superpowers | Root-cause discipline | Same as #2 |
| 4 | `handoff`, `grill-me` | [mattpocock/skills](https://github.com/mattpocock/skills) (MIT, 302k) | Context transfer between sessions and roles; requirements interrogation. #2 and #12 on [skills.sh](https://skills.sh) | `handoff` writes to `$TMPDIR` |
| 5 | `planning-with-files` | [OthmanAdi/planning-with-files](https://github.com/OthmanAdi/planning-with-files) (MIT, 27.6k) | Plans on disk survive compaction in all four CLIs | Ship with its hooks off |
| 6 (opt-in) | `caveman` (skill only) | [JuliusBrussee/caveman](https://github.com/JuliusBrussee/caveman) (Apache-2.0) | Terse output on implement stages | Its README reports about 3% median gain over plain "be concise" ([HN follow-up](https://news.ycombinator.com/item?id=47954745)). Never the CLI, which has telemetry, or "Auto", which uses the cloud. |

**Infra tier (opt-in, outside the skill budget):**

| Tool | Licence | Caveat |
|---|---|---|
| headroom | Apache-2.0 | telemetry |
| [rtk](https://github.com/rtk-ai/rtk) | BUSL / Apache split | savings disputed ([HN](https://news.ycombinator.com/item?id=49656471)) |
| [context-mode](https://github.com/mksglu/context-mode) | Elastic-2.0, not OSS | 6 hooks |
| [claude-mem](https://github.com/thedotmack/claude-mem) | Apache-2.0 | hosted-observer opt-in |

### 4.2 Stage packs

All of these are portable SKILL.md files unless noted.

| Stage / role | Primary | Secondary | Licence notes |
|---|---|---|---|
| Intake / spec (analyst) | [OpenSpec](https://github.com/Fission-AI/OpenSpec) (MIT, 72.9k; light enough for brownfield) **or** [Spec Kit](https://github.com/github/spec-kit) (MIT, 142.6k; integrations for codex, agy and hermes). **Never both.** | mattpocock `grill-me`, `to-spec` | MIT |
| Design / plan (architect) | superpowers `writing-plans` | mattpocock `improve-codebase-architecture`; [addyosmani](https://github.com/addyosmani/agent-skills) `planning-and-task-breakdown` | MIT |
| Implement | superpowers `test-driven-development` | Ponytail SKILL.md (scope control); addyosmani `incremental-implementation` | MIT |
| Test / verify (tester) | `verification-before-completion` on every exit gate | [microsoft/playwright-cli](https://github.com/microsoft/playwright-cli) skill; anthropics `webapp-testing`; ToB `property-based-testing` | Apache-2.0; ToB is CC-BY-SA |
| Review (reviewer, read-only) | [getsentry/skills](https://github.com/getsentry/skills) `code-review`, `find-bugs` | `/ponytail-review` (reads callers outside the diff); ToB `differential-review` | Apache-2.0 |
| Remediate | superpowers `receiving-code-review` | — | MIT |
| Bugfix (reproduce → root-cause → fix) | superpowers `systematic-debugging` | ToB `regression-hunt`, `patching-bugs`, `post-patch-validation`; mattpocock `diagnosing-bugs` | MIT; CC-BY-SA |
| Security / supply chain | [trailofbits/skills](https://github.com/trailofbits/skills) (`insecure-defaults`, `sharp-edges`, `static-analysis`, `supply-chain-risk-auditor`) | getsentry `security-review`, `gha-security-review`, `skill-scanner` | **CC-BY-SA-4.0** (share-alike if redistributed) |
| Docs / release (documenter) | addyosmani `documentation-and-adrs`, `shipping-and-launch` | getsentry `pr-writer`, `commit` | MIT / Apache-2.0 |
| Accept / close | superpowers `finishing-a-development-branch` plus the verification gate | GSD verify/ship ideas | MIT |
| Spike | mattpocock `prototype`, `research` (lightly verified) | — | MIT |
| Performance | [chrome-devtools-mcp](https://github.com/ChromeDevTools/chrome-devtools-mcp) skills (web only) | **Gap:** no strong backend-profiling skill | Apache-2.0 |

**Claude-only (as packaged):** everything in
[anthropics/claude-plugins-official](https://github.com/anthropics/claude-plugins-official):
`code-review`, `pr-review-toolkit`, `code-simplifier`, `security-guidance` (hooks on every edit),
`ralph-loop`, `commit-commands`.

**Not open source:** anthropics `docx`, `pdf`, `pptx` and `xlsx` are source-available only.

### 4.3 Conflicts: do not combine

1. **More than one lifecycle framework.** Any two of GSD, the Superpowers bootstrap, Spec Kit /
   OpenSpec, BMAD, gstack, compound-engineering and the addyosmani full set conflict over plan
   artifacts and next steps.
2. **The `using-superpowers` bootstrap together with Aurora's per-stage assignment.** The
   bootstrap fights Aurora's assignment.
3. **Stacked hooks:** GSD (44), Ponytail and `security-guidance` each inject into every turn or
   edit.
4. **A Ralph Stop hook together with a "verify before done" gate.** The two can disagree, so the
   loop never ends.
5. **Ponytail's YAGNI ladder alongside exhaustive spec frameworks.** Keep Ponytail off intake and
   design.

## 5. Four kinds of catalog entry

| Kind | Examples | How Aurora integrates it |
|---|---|---|
| `skill` | superpowers `systematic-debugging`, getsentry `code-review` | Installed as SKILL.md into each CLI's skill dir. Counts against the listing budget. |
| `rules` | karpathy-guidelines, Ponytail persona | Projected into a managed block in AGENTS.md / CLAUDE.md. Always-on, no triggering. |
| `framework` | GSD, Spec Kit, BMAD, OpenSpec | At most one per project. Aurora uses only the framework's artifact convention and never its bootstrap or hooks. |
| `infra` | headroom, rtk, context-mode | Out of the skill layer. Documented opt-in only, with telemetry warnings. |

## 6. Ecosystem risk

| Evidence | Source |
|---|---|
| ClawHavoc: 341 of 2,857 ClawHub skills were malicious, using fake "Prerequisites" steps that installed AMOS and backdoors | [The Hacker News](https://thehackernews.com/2026/02/researchers-find-341-malicious-clawhub.html) |
| Snyk ToxicSkills: 13.4% of 3,984 skills had critical issues; 76 confirmed malicious | [Snyk](https://snyk.io/blog/toxicskills-malicious-ai-agent-skills-clawhub/) |
| 31,132 skills analysed: 26.1% vulnerable; skills with scripts are 2.12× more likely to be vulnerable | [arXiv 2601.10338](https://arxiv.org/abs/2601.10338) |
| Skill-Inject: up to 80% attack success against frontier models | [arXiv 2602.20156](https://arxiv.org/abs/2602.20156) |
| Trail of Bits bypassed 5 skill scanners (newline padding, `.pyc`, archives, prompt-injecting the LLM scanner) | [CSA note](https://labs.cloudsecurityalliance.org/research/csa-research-note-ai-agent-skill-scanner-bypass-20260610-csa/) |
| Claude Code: `` !`cmd` `` in a skill runs without asking for permission; skill `hooks` persist for the whole session | [Claude Code docs](https://code.claude.com/docs/en/skills) |
| Mitigations elsewhere: `gh skill install …@SHA --pin`; Hermes `skills_guard` with trust tiers | [GitHub changelog](https://github.blog/changelog/2026-04-17-manage-agent-skills-with-github-cli/), [Hermes docs](https://hermes-agent.nousresearch.com/docs/user-guide/features/skills) |
| Installers misbehave too: vercel `skills` read `gh auth token` without consent (#523) and leaked private-repo telemetry (#699) | vercel-labs/skills issues |

## 7. Context cost

| CLI | Always-loaded listing | Controls |
|---|---|---|
| Claude Code | Description plus `when_to_use`, truncated at 1,536 characters per skill; the listing gets about 1% of context (?) | `skillOverrides` (`name-only`), `disable-model-invocation` |
| Codex | At most 2% of context or 8k characters; descriptions are shortened first, then skills are dropped | `skills.max_context_tokens`, `allow_implicit_invocation: false`, `[[skills.config]] enabled=false` |
| Hermes | About 3k tokens for level-0 metadata | `requires_tools`, bundles |
| Agent Skills spec | About 100 tokens of metadata per skill; body under 5k tokens | — |

Complaints include every description being injected each session regardless of use
([claude-code#43816](https://github.com/anthropics/claude-code/issues/43816)) and silent
truncation breaking routing.

## 8. Design principles Aurora adopts

1. **Pin every source to a commit SHA and verify the content hash.** Updates are explicit diffs.
2. **Trust tiers.** `official`, `curated` and `community`. Community entries need a human review
   of SKILL.md and every script before activation, and a dangerous verdict can't be forced.
3. **Scanners are signals, not gates.** Static rules flag scripts, `` !`cmd` ``, hooks, broad
   `allowed-tools`, padding, `.pyc`, archives, encoded blobs, `curl | sh` and "Prerequisites"
   paste commands.
4. **Least loading.** Skills are activated per role, stage and project, within each CLI's
   measured budget. Manual-only skills are set to name-only or explicit invocation.
5. **Adapt to capabilities, never degrade silently.** If a skill needs Claude-only features,
   other CLIs get a skip with a reason, not a broken copy.
6. **Provenance and no telemetry.** Lock file plus ledger, and credentials are never read
   without consent.
7. **Don't vendor.** Aurora references upstream by pinned SHA. This respects CC-BY-SA,
   source-available and missing-licence terms.
8. **The two-model review gate is also a safety control.** Output from autonomous or looping
   skills passes a read-only review by a different model, with capped rounds.

## 9. Recommended roadmap

- **R2: skill catalog and safe installs.** Curated catalog with pinned SHAs; solo, pair and trio
  profiles; SKILL.md parser, classifier and budget; fetch, hash and scan; projection plus leakage
  matrix plus overlays; lock and ledger; transactional apply and undo; workflow integration.
- **R3: TUI.** Skills × CLIs matrix, roles board, run pipeline with a checkbox submit form,
  per-skill token cost.
- **R4: runner.**
  - one provider headless, behind per-run approval;
  - a built-in bounded Ralph loop;
  - a pair-mode review loop with round caps.
- **R5: retire the Python reference.** Make the protected-path list configurable.
