# Third-Party Notices

Aurora's Rust crates depend on third-party crates, each distributed under its own licence. The
licence allow-list is in `deny.toml`.

Code **copied or adapted** from other projects is listed below with its source, revision, licence
and any NOTICE text. Each such file also carries a "Modified from <repo>@<sha> (<licence>)" header.

### openai/codex — SKILL.md frontmatter parsing

- Source: https://github.com/openai/codex, commit `970111f6cb62cc02fe36a82ea7d71b37a2889c97`,
  file `codex-rs/skills/src/parser.rs`
- Licence: Apache License 2.0 (https://www.apache.org/licenses/LICENSE-2.0)
- Used in: `crates/aurora-skills/src/frontmatter.rs` (`extract_frontmatter`,
  `repair_frontmatter_scalar_fields`)
- Changes: YAML is parsed with serde-saphyr into a generic map instead of serde_yaml into a fixed
  struct; extraction also returns the body; name and field validation moved to `skill.rs`.
- NOTICE file of the source project, reproduced as required:

  ```
  OpenAI Codex
  Copyright 2025 OpenAI

  This project includes code derived from [Ratatui](https://github.com/ratatui/ratatui), licensed under the MIT license.
  Copyright (c) 2016-2022 Florian Dehau
  Copyright (c) 2023-2025 The Ratatui Developers
  ```

## Skill catalog references (not distributed)

`catalog/catalog.toml` lists third-party agent skills by repository URL, pinned commit SHA, path
and licence. It contains no third-party skill text: the descriptions and rationales are Aurora's
own. Skills are fetched from their upstream repositories only when a user installs them, under
each upstream licence (including CC-BY-SA-4.0 for trailofbits/skills).
