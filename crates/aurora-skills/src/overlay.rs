//! Install overlays (R2 Step 3a): the only changes Aurora makes to upstream skill files.
//!
//! Top-level frontmatter keys that bypass a CLI's permission prompts are removed line by line
//! (`allowed-tools` always; `hooks` unless the user keeps them), so every other byte of the file
//! is unchanged. The result is re-parsed and compared with the original: only the named keys may
//! differ, otherwise the overlay is refused.

use crate::skill::parse_skill;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OverlayError {
    #[error("SKILL.md has no `---` frontmatter block")]
    NoFrontmatter,
    #[error("overlay could not be verified: {0}")]
    Unverified(String),
}

fn starts_key(line: &str, key: &str) -> bool {
    line.strip_prefix(key)
        .and_then(|rest| rest.strip_prefix(':'))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r', '\n']))
}

/// Remove the top-level frontmatter `keys` (and their nested or list values) from SKILL.md
/// `text`. Returns the new text and the keys actually removed, in file order.
pub fn strip_frontmatter_keys(text: &str, keys: &[&str]) -> Result<(String, Vec<String>), OverlayError> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if !lines.first().is_some_and(|l| l.trim() == "---") {
        return Err(OverlayError::NoFrontmatter);
    }
    let close =
        lines.iter().skip(1).position(|l| l.trim() == "---").map(|i| i + 1).ok_or(OverlayError::NoFrontmatter)?;

    let mut out = String::with_capacity(text.len());
    let mut removed: Vec<String> = Vec::new();
    let mut removing = false;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 && i < close {
            // A removed key's value continues over blank, indented and indentation-less list lines.
            if removing && (line.trim().is_empty() || line.starts_with([' ', '\t', '-'])) {
                continue;
            }
            removing = false;
            if let Some(key) = keys.iter().find(|k| starts_key(line, k)) {
                removing = true;
                if !removed.iter().any(|r| r == key) {
                    removed.push((*key).to_string());
                }
                continue;
            }
        }
        out.push_str(line);
    }

    let before =
        parse_skill(text, "").map_err(|e| OverlayError::Unverified(format!("original does not parse: {e}")))?;
    let after = parse_skill(&out, "").map_err(|e| OverlayError::Unverified(format!("result does not parse: {e}")))?;
    for key in keys {
        if after.frontmatter.contains_key(*key) {
            return Err(OverlayError::Unverified(format!("`{key}` is still present (not a plain top-level key)")));
        }
    }
    let kept = |m: &std::collections::BTreeMap<String, serde_json::Value>| {
        m.iter().filter(|(k, _)| !keys.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect::<Vec<_>>()
    };
    if kept(&before.frontmatter) != kept(&after.frontmatter) || before.body != after.body {
        return Err(OverlayError::Unverified("other frontmatter or the body changed".into()));
    }
    Ok((out, removed))
}
