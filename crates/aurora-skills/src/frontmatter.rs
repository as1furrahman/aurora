// Modified from openai/codex@970111f6cb62cc02fe36a82ea7d71b37a2889c97
// codex-rs/skills/src/parser.rs (Apache-2.0; Copyright 2025 OpenAI). See THIRD_PARTY_NOTICES.md.
// Changes: YAML is parsed with serde-saphyr (budget-limited, no unsafe-libyaml) into a generic
// map instead of serde_yaml into a fixed struct; extraction also returns the body; name and
// field validation moved to `skill.rs`.

//! SKILL.md frontmatter extraction with Codex's line-oriented repair of unquoted prose scalars.

use std::collections::BTreeMap;

use serde_json::Value;

/// Split `contents` into its `---`-delimited frontmatter and the body that follows.
pub(crate) fn extract_frontmatter(contents: &str) -> Option<(String, String)> {
    let mut lines = contents.split_inclusive('\n');
    if !matches!(lines.next(), Some(line) if line.trim() == "---") {
        return None;
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line.trim_end_matches(['\n', '\r']));
    }

    if frontmatter_lines.is_empty() || !found_closing {
        return None;
    }

    Some((frontmatter_lines.join("\n"), lines.collect()))
}

/// Parse frontmatter YAML into a key map. On failure, retry once with unquoted prose scalars
/// quoted (`description: Build for AWS: ECS`); the flag reports whether that repair was needed.
pub(crate) fn parse_frontmatter(frontmatter: &str) -> Result<(BTreeMap<String, Value>, bool), String> {
    match serde_saphyr::from_str::<BTreeMap<String, Value>>(frontmatter) {
        Ok(map) => Ok((map, false)),
        Err(original_error) => match repair_frontmatter_scalar_fields(frontmatter) {
            // Some third-party skills use prose like `description: Build for AWS: ECS`
            // or `argument-hint: <duration: e.g. 7d>`. Keep the repair line-oriented
            // so unrelated invalid YAML still surfaces.
            Some(repaired) => serde_saphyr::from_str::<BTreeMap<String, Value>>(&repaired)
                .map(|map| (map, true))
                .map_err(|_| original_error.to_string()),
            None => Err(original_error.to_string()),
        },
    }
}

fn repair_frontmatter_scalar_fields(frontmatter: &str) -> Option<String> {
    let mut changed = false;
    let mut block_scalar_indent: Option<usize> = None;
    let mut repaired_lines = Vec::new();
    for line in frontmatter.lines() {
        let indent = line.chars().take_while(|character| *character == ' ').count();
        if let Some(block_indent) = block_scalar_indent {
            if line.trim().is_empty() || indent > block_indent {
                repaired_lines.push(line.to_string());
                continue;
            }
            block_scalar_indent = None;
        }

        let Some((key, value)) = line.split_once(':') else {
            repaired_lines.push(line.to_string());
            continue;
        };
        if key.trim().is_empty() || !value.chars().next().is_none_or(char::is_whitespace) {
            repaired_lines.push(line.to_string());
            continue;
        }

        let trimmed_start = value.trim_start();
        let leading_whitespace = &value[..value.len() - trimmed_start.len()];
        let mut scalar = trimmed_start;
        let mut comment = "";
        for (index, character) in trimmed_start.char_indices() {
            if character == '#'
                && (index == 0 || trimmed_start[..index].chars().next_back().is_some_and(char::is_whitespace))
            {
                let comment_start = trimmed_start[..index].trim_end().len();
                scalar = &trimmed_start[..comment_start];
                comment = &trimmed_start[comment_start..];
                break;
            }
        }

        let scalar = scalar.trim_end();
        let Some(first_char) = scalar.chars().next() else {
            repaired_lines.push(line.to_string());
            continue;
        };
        if matches!(first_char, '|' | '>') {
            block_scalar_indent = Some(indent);
            repaired_lines.push(line.to_string());
            continue;
        }
        if matches!(first_char, '\'' | '"') {
            repaired_lines.push(line.to_string());
            continue;
        }
        let mut has_colon_separator = false;
        let mut chars = scalar.chars().peekable();
        while let Some(character) = chars.next() {
            if character == ':' && matches!(chars.peek(), Some(next_character) if next_character.is_whitespace()) {
                has_colon_separator = true;
                break;
            }
        }
        let invalid_flow_like_scalar =
            matches!(first_char, '[' | '{' | '@' | '`') && serde_saphyr::from_str::<Value>(scalar).is_err();
        if !has_colon_separator && !invalid_flow_like_scalar {
            repaired_lines.push(line.to_string());
            continue;
        }

        let quoted_scalar = format!("'{}'", scalar.replace('\'', "''"));
        repaired_lines.push(format!("{key}:{leading_whitespace}{quoted_scalar}{comment}"));
        changed = true;
    }
    changed.then(|| repaired_lines.join("\n"))
}
