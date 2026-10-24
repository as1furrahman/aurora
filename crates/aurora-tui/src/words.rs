//! Plain-language words for the TUI, in one place, for people new to AI coding tools.
//!
//! The engine and the CLI keep their precise vocabulary (provider, role, stage, run); the TUI
//! speaks of AI tools, jobs, steps and tasks. Technical facts are never hidden, only moved to a
//! "More details" section.

use aurora_core::workflow::StageState;

/// The everyday name of an AI tool.
pub fn tool(id: &str) -> &str {
    match id {
        "claude" => "Claude Code",
        "codex" => "Codex",
        "antigravity" => "Antigravity",
        "hermes" => "Hermes",
        other => other,
    }
}

/// "Claude Code + Codex".
pub fn tools(ids: &[String]) -> String {
    ids.iter().map(|i| tool(i)).collect::<Vec<_>>().join(" + ")
}

/// The job a role does, in plain words.
pub fn job(role: &str) -> &str {
    match role {
        "analyst" => "planner",
        "architect" => "designer",
        "implementer" => "builder",
        "tester" => "checker",
        "reviewer" => "reviewer",
        "security" => "security checker",
        "documenter" => "writer",
        other => other,
    }
}

/// What a job is for, in one line.
pub fn job_explained(role: &str) -> &'static str {
    match role {
        "analyst" => "works out exactly what is wanted before anything is built",
        "architect" => "plans how to build it",
        "implementer" => "writes the code",
        "tester" => "checks that it really works, with tests",
        "reviewer" => "reads the change with fresh eyes and points out problems",
        "security" => "looks for ways the code could be attacked or leak data",
        "documenter" => "writes the explanations other people will read",
        _ => "does this part of the task",
    }
}

/// A workflow preset as a kind of task.
pub fn task_type(preset: &str) -> &str {
    match preset {
        "bugfix" => "Fix a bug",
        "feature" => "Build a new feature",
        "refactor" => "Tidy up code",
        "docs" => "Write or update docs",
        "security-review" => "Check for security problems",
        "spike" => "Explore an idea",
        other => other,
    }
}

/// One line on what a kind of task does.
pub fn task_type_explained(preset: &str) -> &'static str {
    match preset {
        "bugfix" => "show the bug happening, find the cause, fix it, then have it checked",
        "feature" => "pin down what's wanted, plan it, build it test-first, check, review and document it",
        "refactor" => "make the code cleaner without changing what it does, proving nothing broke",
        "docs" => "outline, write, check every example against the code, then review",
        "security-review" => "decide what to look at, look for problems, get a second opinion, write it up",
        "spike" => "a short investigation that ends in a clear go, no-go or more questions",
        _ => "a series of steps",
    }
}

/// A stage id as a step name: "root-cause" -> "Root cause".
pub fn step(id: &str) -> String {
    let words = id.replace('-', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Glyph and words for a step's state.
pub fn step_state(s: StageState) -> (&'static str, &'static str) {
    match s {
        StageState::Pending => ("·", "waiting"),
        StageState::Ready => ("○", "ready"),
        StageState::Running => ("◔", "in progress"),
        StageState::Passed => ("✓", "done"),
        StageState::Failed => ("✗", "not done yet"),
        StageState::Blocked => ("■", "stuck"),
        StageState::Skipped => ("–", "skipped"),
        StageState::NotApplicable => ("–", "not needed"),
    }
}

/// How full a tool's skill list is.
pub fn room(used: usize, budget: usize) -> &'static str {
    if used > budget {
        "too full: some skills may be ignored"
    } else if used * 4 > budget * 3 {
        "getting full"
    } else {
        "plenty of room"
    }
}

/// An install-plan reason, in plain words.
pub fn reason(detail: &str) -> String {
    let d = detail;
    if d.contains("not installed by Aurora") {
        "Something with this name is already here and Aurora didn't add it, so it's left as is.".into()
    } else if d.contains("modified since Aurora installed it") || d.contains("block was edited") {
        "You changed this after Aurora added it, so Aurora won't touch it.".into()
    } else if let Some(rest) = d.strip_prefix("blocked by the scanner") {
        format!("Aurora's safety check blocked this skill{rest}; it won't be added.")
    } else if d.contains("no longer wanted; kept") {
        "Not needed for this task any more. It's kept; nothing is removed unless you ask.".into()
    } else if d == "no longer wanted" {
        "Not needed for this task any more, so it will be removed (you asked to tidy up).".into()
    } else if d.contains("new file with the managed block") {
        "Creates this file with Aurora's always-on rules.".into()
    } else if d.contains("append the managed block") {
        "Adds Aurora's always-on rules at the end of your file; the rest stays as it is.".into()
    } else if d.contains("replace the managed block") {
        "Updates Aurora's block of rules in this file.".into()
    } else if d.contains(" -> ") {
        format!("A newer version is available ({d}).")
    } else {
        d.to_string()
    }
}

/// A recommendation or plan warning, in plain words (unknown ones are shown as they are).
pub fn warning(w: &str) -> String {
    if let Some(rest) = w.strip_prefix("reduced independence: '")
        && let Some((a, rest)) = rest.split_once("' should be independent of '")
        && let Some((b, rest)) = rest.split_once("' but both run on ")
    {
        let t = rest.split('.').next().unwrap_or(rest);
        return format!(
            "{} and {} are both done by {}, so {} isn't a true second opinion. Tip: open a fresh {} session for {}.",
            step(a),
            step(b),
            tool(t),
            step(a),
            tool(t),
            step(a)
        );
    }
    if let Some(rest) = w.strip_prefix('\'')
        && let Some((id, _)) = rest.split_once("' is a community entry")
    {
        return format!("{id} comes from the community. Have a quick read before adding it.");
    }
    if let Some(rest) = w.strip_prefix("other tools that read ")
        && let Some((dir, rest)) = rest.split_once(" (")
    {
        let others = rest.split(')').next().unwrap_or(rest);
        return format!("Other AI apps that look in {dir} ({others}) will see these skills too.");
    }
    if let Some((who, rest)) = w.split_once(" also loads ")
        && let Some((skills, dir)) = rest.split_once(" from the shared ")
    {
        return format!("{} also sees {skills}, because it shares the {dir} folder with another tool.", tool(who));
    }
    if w.starts_with("Hermes loads project skills only after") {
        return "Hermes only uses this project's skills after you run `hermes skills trust` in this folder.".into();
    }
    if let Some(rest) = w.strip_prefix('\'')
        && let Some((id, rest)) = rest.split_once("' declares hooks")
    {
        let t = rest.rsplit("on ").next().unwrap_or("").split(' ').next().unwrap_or("");
        return format!("{id} has Claude-only automatic actions; on {} it works without them.", tool(t));
    }
    if let Some((who, _)) = w.split_once(": the skills it will see need about ") {
        return format!(
            "{} has too many skill descriptions to keep in mind. Turn off some optional skills.",
            tool(who)
        );
    }
    if let Some((who, rest)) = w.split_once(" is not eligible for role '")
        && let Some((role, _)) = rest.split_once('\'')
    {
        return format!(
            "This task type doesn't allow {} to be the {}. Choose another tool with p.",
            tool(who),
            job(role)
        );
    }
    w.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_names() {
        assert_eq!(step("root-cause"), "Root cause");
        assert_eq!(tools(&["claude".into(), "codex".into()]), "Claude Code + Codex");
        assert_eq!(room(1130, 8000), "plenty of room");
        assert_eq!(room(7000, 8000), "getting full");
    }

    #[test]
    fn warnings_become_plain_sentences() {
        let w = warning(
            "reduced independence: 'review' should be independent of 'fix' but both run on claude. Use a fresh session, ideally with a different model.",
        );
        assert_eq!(
            w,
            "Review and Fix are both done by Claude Code, so Review isn't a true second opinion. Tip: open a fresh Claude Code session for Review."
        );
        assert!(
            warning("'ponytail' is a community entry: review its SKILL.md")
                .starts_with("ponytail comes from the community")
        );
        assert!(
            warning("codex also loads finishing (for antigravity) from the shared .agents/skills")
                .starts_with("Codex also sees")
        );
        assert_eq!(warning("something new"), "something new", "unknown warnings pass through");
    }
}
