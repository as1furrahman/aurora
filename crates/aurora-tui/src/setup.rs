//! The guided setup: three short questions on first launch in a project (and on `t` later).
//! 1. Which AI tools do you use?  2. What are you working on?  3. Here's what Aurora will add.
//!
//! The state and its transitions are pure; the app turns the answers into an install request.

use aurora_skills::plan::ActionKind;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::app::App;
use crate::ui::{Theme, centered, hang};
use crate::words;

/// The tools, in the order the default profiles use them (pair = the first two).
pub const TOOL_ORDER: [&str; 4] = ["claude", "codex", "antigravity", "hermes"];

/// Kinds of task, most common first; each is a workflow preset.
pub const TASKS: [&str; 6] = ["bugfix", "feature", "refactor", "docs", "security-review", "spike"];

/// Most tools a profile can use at once.
pub const MAX_TOOLS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Tools,
    Task,
    Preview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub id: String,
    pub installed: bool,
    pub ticked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub step: Step,
    pub tools: Vec<Tool>,
    /// The highlighted tool (step 1).
    pub tool_cursor: usize,
    /// The highlighted kind of task (step 2), an index into `TASKS`.
    pub task: usize,
    /// Why Enter did nothing, shown until the next key.
    pub hint: Option<String>,
}

/// What the app should do after Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Stay,
    /// Plan with these tools and this preset, then show the preview.
    Plan {
        clis: Vec<String>,
        preset: String,
    },
    /// Close the setup and open the Add to project page.
    Finish,
}

impl Setup {
    /// `current` (tools already in use or asked for) wins over `installed` for the ticks; with
    /// neither, nothing is ticked. At most `MAX_TOOLS` start ticked.
    pub fn new(installed: &[String], current: &[String], preset: &str) -> Setup {
        let pre: Vec<&str> = if current.is_empty() {
            TOOL_ORDER.iter().copied().filter(|t| installed.iter().any(|i| i == t)).take(MAX_TOOLS).collect()
        } else {
            current.iter().map(String::as_str).collect()
        };
        let tools = TOOL_ORDER
            .iter()
            .map(|id| Tool {
                id: (*id).to_string(),
                installed: installed.iter().any(|i| i == id),
                ticked: pre.contains(id),
            })
            .collect();
        Setup {
            step: Step::Tools,
            tools,
            tool_cursor: 0,
            task: TASKS.iter().position(|t| *t == preset).unwrap_or(0),
            hint: None,
        }
    }

    /// The ticked tools, in `TOOL_ORDER`.
    pub fn chosen(&self) -> Vec<String> {
        self.tools.iter().filter(|t| t.ticked).map(|t| t.id.clone()).collect()
    }

    pub fn toggle(&mut self) {
        self.hint = None;
        if self.step == Step::Tools
            && let Some(t) = self.tools.get_mut(self.tool_cursor)
        {
            t.ticked = !t.ticked;
        }
    }

    pub fn move_by(&mut self, down: bool) {
        self.hint = None;
        let (c, len) = match self.step {
            Step::Tools => (&mut self.tool_cursor, TOOL_ORDER.len()),
            Step::Task => (&mut self.task, TASKS.len()),
            Step::Preview => return,
        };
        *c = if down { (*c + 1).min(len - 1) } else { c.saturating_sub(1) };
    }

    pub fn advance(&mut self) -> Next {
        self.hint = None;
        match self.step {
            Step::Tools => {
                let n = self.chosen().len();
                if n == 0 {
                    self.hint = Some("Pick at least one tool: move to it and press Space.".into());
                } else if n > MAX_TOOLS {
                    self.hint = Some(format!("Aurora can use up to {MAX_TOOLS} tools at once. Untick one."));
                } else {
                    self.step = Step::Task;
                }
                Next::Stay
            }
            Step::Task => {
                self.step = Step::Preview;
                Next::Plan { clis: self.chosen(), preset: TASKS[self.task].to_string() }
            }
            Step::Preview => Next::Finish,
        }
    }

    pub fn back(&mut self) {
        self.hint = None;
        self.step = match self.step {
            Step::Tools | Step::Task => Step::Tools,
            Step::Preview => Step::Task,
        };
    }
}

/// The setup owns the whole screen: the pages behind it would only distract on a first run.
pub fn render(f: &mut Frame, app: &App, s: &Setup, area: Rect, theme: Theme) {
    let width = area.width.saturating_sub(4).min(90);
    let (n, title) = match s.step {
        Step::Tools => (1, "Which AI tools do you use?"),
        Step::Task => (2, "What are you working on?"),
        Step::Preview => (3, "Here's what Aurora will add"),
    };
    let mut lines = vec![Line::from(Span::styled(title, theme.accent())), Line::raw("")];
    match s.step {
        Step::Tools => tools_lines(s, &mut lines, theme),
        Step::Task => task_lines(s, &mut lines, theme),
        Step::Preview => preview_lines(app, &mut lines, theme),
    }
    lines.push(Line::raw(""));
    if let Some(h) = &s.hint {
        lines.push(Line::from(Span::styled(format!("! {h}"), theme.accent())));
    }
    let keys = match s.step {
        Step::Tools => "Space tick · ↑↓ move · Enter next · Esc not now",
        Step::Task => "↑↓ move · Enter next · Backspace back · Esc not now",
        Step::Preview => "Enter check and add · Backspace back · Esc not now",
    };
    lines.push(Line::from(Span::styled(keys, theme.dim())));
    let lines = hang(lines, width.saturating_sub(2));
    let rect = centered(area, width, (lines.len() as u16 + 2).min(area.height));
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" Set up Aurora · step {n} of 3 "))
                .border_style(theme.focused()),
        ),
        rect,
    );
}

fn pointer(on: bool, theme: Theme) -> (String, ratatui::style::Style) {
    if on {
        ("▶ ".into(), theme.focused().add_modifier(Modifier::REVERSED))
    } else {
        ("  ".into(), ratatui::style::Style::default())
    }
}

fn tools_lines(s: &Setup, lines: &mut Vec<Line>, theme: Theme) {
    lines.push(Line::raw("Aurora gives each tool the skills for its jobs. Pick 1, 2 or 3."));
    lines.push(Line::raw(""));
    for (i, t) in s.tools.iter().enumerate() {
        let (p, style) = pointer(i == s.tool_cursor, theme);
        let mark = if t.ticked { "[x]" } else { "[ ]" };
        let mut spans = vec![Span::styled(format!("{p}{mark} {:<12}", words::tool(&t.id)), style)];
        if t.installed {
            spans.push(Span::styled("  (installed)", theme.dim()));
        }
        lines.push(Line::from(spans));
    }
    if s.tools.iter().all(|t| !t.installed) {
        lines.push(Line::raw(""));
        lines.push(Line::raw("None of these were found on this computer yet. Tick the ones you'll use."));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "With 2 or 3 tools, one builds and another checks the work: a real second opinion.",
        theme.dim(),
    )));
}

fn task_lines(s: &Setup, lines: &mut Vec<Line>, theme: Theme) {
    lines.push(Line::raw("Aurora picks the steps and skills for this kind of task. You can change it later."));
    lines.push(Line::raw(""));
    for (i, t) in TASKS.iter().enumerate() {
        let (p, style) = pointer(i == s.task, theme);
        lines.push(Line::from(Span::styled(format!("{p}{}", words::task_type(t)), style)));
        lines.push(Line::from(Span::styled(format!("    {}", words::task_type_explained(t)), theme.dim())));
    }
}

fn preview_lines(app: &App, lines: &mut Vec<Line>, theme: Theme) {
    if let Some(e) = &app.plan_error {
        lines.push(Line::raw(crate::ui::trouble_hint(e).to_string()));
        lines.push(Line::from(Span::styled(e.clone(), theme.dim())));
        return;
    }
    let (Some(p), false) = (&app.planned, app.loading) else {
        lines.push(Line::from(Span::styled("Working out what your project needs…", theme.dim())));
        return;
    };
    let folder = p.project.file_name().map_or_else(|| p.project.display().to_string(), |n| n.to_string_lossy().into());
    lines.push(Line::raw(format!("For “{}” in {folder}:", words::task_type(&app.opts.preset))));
    lines.push(Line::raw(""));
    let wanted =
        |k: ActionKind| matches!(k, ActionKind::Create | ActionKind::Update | ActionKind::Unchanged | ActionKind::Keep);
    for cli in &p.recommendation.clis {
        let mut skills: Vec<&str> = p
            .plan
            .actions
            .iter()
            .filter(|a| wanted(a.kind) && a.providers.iter().any(|x| x == cli))
            .map(|a| a.entry.as_str())
            .collect();
        skills.dedup();
        let rules = app.catalog.as_ref().ok().and_then(|c| c.target(cli)).map(|t| t.rules_file.clone());
        let jobs: Vec<&str> = p
            .recommendation
            .stages
            .iter()
            .filter(|s| s.provider == *cli)
            .map(|s| words::job(&s.role))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        lines.push(Line::from(vec![
            Span::styled(words::tool(cli).to_string(), theme.accent()),
            Span::raw(format!("  as {}", jobs.join(", "))),
        ]));
        let n = skills.len();
        lines.push(Line::raw(format!("  {n} skill{}: {}", if n == 1 { "" } else { "s" }, skills.join(", "))));
        if let Some(r) = rules.filter(|r| p.plan.actions.iter().any(|a| a.target == *r && wanted(a.kind))) {
            lines.push(Line::raw(format!("  always-on rules in {r}")));
        }
    }
    let check = p.unreviewed(&app.reviewed);
    if !check.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::styled("Check first", theme.accent()),
            Span::raw(format!("  {}: have a quick read before adding (the next page shows them).", check.join(", "))),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::raw(
        "Nothing changes yet. The next page lists every file, and Aurora asks before adding anything.",
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn installed_tools_start_ticked_up_to_three_in_profile_order() {
        let s = Setup::new(&ids(&["hermes", "codex", "claude", "antigravity"]), &[], "bugfix");
        assert_eq!(s.chosen(), ["claude", "codex", "antigravity"]);
        let s = Setup::new(&ids(&["hermes"]), &ids(&["codex", "hermes"]), "docs");
        assert_eq!(s.chosen(), ["codex", "hermes"], "tools in use win");
        assert_eq!(TASKS[s.task], "docs");
    }

    #[test]
    fn enter_moves_through_the_steps() {
        let mut s = Setup::new(&ids(&["claude"]), &[], "feature");
        assert_eq!(s.advance(), Next::Stay);
        assert_eq!(s.step, Step::Task);
        assert_eq!(s.advance(), Next::Plan { clis: ids(&["claude"]), preset: "feature".into() });
        assert_eq!(s.step, Step::Preview);
        assert_eq!(s.advance(), Next::Finish);
        s.back();
        assert_eq!(s.step, Step::Task);
    }

    #[test]
    fn the_cursor_stays_in_range() {
        let mut s = Setup::new(&[], &[], "bugfix");
        for _ in 0..9 {
            s.move_by(true);
        }
        assert_eq!(s.tool_cursor, 3);
        s.advance();
        assert!(s.hint.is_some());
        s.move_by(false);
        assert!(s.hint.is_none(), "any key clears the hint");
    }
}
