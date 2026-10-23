//! The TUI model: one `App`, one `Action` enum, pure `update` transitions.
//!
//! Side effects never happen here. A key that needs work (planning, applying) queues a
//! [`Command`]; the event loop runs it on a worker thread and feeds the result back as a [`Msg`].
//! Results from superseded requests are recognised by their generation and ignored.

use std::collections::VecDeque;
use std::path::PathBuf;

use aurora_app::install::{InstallRequest, Planned, WorkflowSource, load_workflow};
use aurora_core::catalog::{Catalog, Tier};
use aurora_core::presets::PRESETS;
use aurora_core::providers::PROVIDER_IDS;
use aurora_core::workflow::WorkflowDefinition;
use aurora_skills::apply::UndoReport;
use aurora_skills::plan::ActionKind;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::keymap::{Ctx, resolve};

/// Everything a key or a background job can ask the model to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    GoTab(usize),
    NextTab,
    PrevTab,
    FocusNext,
    FocusPrev,
    Up,
    Down,
    Top,
    Bottom,
    PageUp,
    PageDown,
    StartFilter,
    AcceptFilter,
    CancelFilter,
    FilterBackspace,
    FilterChar(char),
    OpenHelp,
    ClosePopup,
    Back,
    /// Re-run the install plan.
    Refresh,
    /// Include or drop the selected opt-in catalog entry (`--with`).
    ToggleOptIn,
    /// Next workflow preset.
    CyclePreset,
    /// Next CLI profile (solo, pair, trio), with its own CLIs.
    CycleProfile,
    /// Give the selected stage's role slot to the next CLI not already in the profile.
    CycleCli,
    /// Mark the selected entry reviewed for this session (`--reviewed`).
    MarkReviewed,
    /// Read the selected entry's flagged files (or its SKILL.md).
    OpenReview,
    /// Ask to apply the plan.
    StartApply,
    /// Ask to undo the last operation.
    StartUndo,
    /// Confirm the open dialog.
    ConfirmYes,
}

/// Work for the event loop to run off the UI thread.
#[derive(Debug, Clone)]
pub enum Command {
    Plan {
        generation: u64,
        request: InstallRequest,
    },
    /// Apply exactly this plan; `reviewed` must cover every entry that needs review.
    Apply {
        planned: Box<Planned>,
        reviewed: Vec<String>,
    },
    Undo {
        project: PathBuf,
    },
}

/// Results coming back from the event loop.
#[derive(Debug)]
pub enum Msg {
    Planned {
        generation: u64,
        /// Boxed: a plan is large and the other messages are small.
        result: Box<Result<Planned, String>>,
    },
    /// A one-line outcome, or why nothing was applied.
    Applied(Result<String, String>),
    Undone(Result<UndoReport, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Skills,
    Roles,
    Install,
    Runs,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Skills, Tab::Roles, Tab::Install, Tab::Runs];

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// Key context for tab-specific bindings.
    pub fn ctx(self) -> Ctx {
        match self {
            Tab::Skills => Ctx::Skills,
            Tab::Roles => Ctx::Roles,
            Tab::Install => Ctx::Install,
            Tab::Runs => Ctx::Runs,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Skills => "Skills",
            Tab::Roles => "Roles",
            Tab::Install => "Install",
            Tab::Runs => "Runs",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmOp {
    Apply,
    Undo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    Help {
        scroll: u16,
    },
    /// A yes/no question; only `y` confirms.
    Confirm {
        op: ConfirmOp,
        title: String,
        lines: Vec<String>,
    },
    /// Read-only text (flagged files, handoffs).
    Viewer {
        title: String,
        text: String,
        scroll: u16,
    },
}

/// Event-loop ticks a toast stays visible (100 ms each).
const TOAST_TICKS: u64 = 60;

/// What the TUI works on; the same inputs as the `aurora skills` commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiOptions {
    pub project: PathBuf,
    pub run_root: Option<PathBuf>,
    pub preset: String,
    pub profile: Option<String>,
    pub clis: Vec<String>,
    pub catalog: Option<PathBuf>,
    pub cache: Option<PathBuf>,
    pub mirror: Option<PathBuf>,
    /// Glyphs and emphasis only, no colour (`NO_COLOR`).
    pub no_color: bool,
}

impl Default for TuiOptions {
    fn default() -> Self {
        TuiOptions {
            project: PathBuf::from("."),
            run_root: None,
            preset: "bugfix".into(),
            profile: None,
            clis: Vec::new(),
            catalog: None,
            cache: None,
            mirror: None,
            no_color: std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
        }
    }
}

#[derive(Debug)]
pub struct App {
    pub opts: TuiOptions,
    pub tab: Tab,
    pub focus: Focus,
    /// Cursor per tab.
    pub cursor: [usize; 4],
    pub quit: bool,
    /// The catalog the plan is made from (built-in unless `--catalog`).
    pub catalog: Result<Catalog, String>,
    /// The workflow being planned.
    pub workflow: Result<WorkflowDefinition, String>,
    /// The latest install plan, if one finished.
    pub planned: Option<Planned>,
    /// Why the latest plan failed, if it did.
    pub plan_error: Option<String>,
    /// A plan is running.
    pub loading: bool,
    /// Opt-in entries included (`--with`).
    pub with: Vec<String>,
    /// Event-loop iterations, for the spinner.
    pub ticks: u64,
    /// Entries the user marked reviewed this session.
    pub reviewed: Vec<String>,
    /// An apply or undo is running.
    pub busy: Option<&'static str>,
    /// The outcome of the last apply or undo, kept after its toast fades.
    pub last_result: Option<String>,
    toast: Option<(String, u64)>,
    generation: u64,
    commands: VecDeque<Command>,
    popups: Vec<Popup>,
    filter: String,
    filter_edit: Option<String>,
}

impl App {
    pub fn new(opts: TuiOptions) -> App {
        let catalog = aurora_app::fetch::load_catalog(opts.catalog.as_deref());
        let workflow = load_workflow(&WorkflowSource::Preset(opts.preset.clone()));
        let mut app = App {
            opts,
            tab: Tab::Skills,
            focus: Focus::List,
            cursor: [0; 4],
            quit: false,
            catalog,
            workflow,
            planned: None,
            plan_error: None,
            loading: false,
            with: Vec::new(),
            ticks: 0,
            reviewed: Vec::new(),
            busy: None,
            last_result: None,
            toast: None,
            generation: 0,
            commands: VecDeque::new(),
            popups: Vec::new(),
            filter: String::new(),
            filter_edit: None,
        };
        app.request_plan();
        app
    }

    /// The `aurora skills plan` request for the current settings.
    pub fn install_request(&self) -> InstallRequest {
        InstallRequest {
            workflow: WorkflowSource::Preset(self.opts.preset.clone()),
            profile: self.opts.profile.clone(),
            clis: self.opts.clis.clone(),
            with: self.with.clone(),
            project: self.opts.project.clone(),
            keep_hooks: false,
            prune: false,
            catalog: self.opts.catalog.clone(),
            cache: self.opts.cache.clone(),
            mirror: self.opts.mirror.clone(),
        }
    }

    fn request_plan(&mut self) {
        self.generation += 1;
        self.loading = true;
        self.commands.retain(|c| !matches!(c, Command::Plan { .. }));
        self.commands.push_back(Command::Plan { generation: self.generation, request: self.install_request() });
    }

    /// The next piece of work for the event loop.
    pub fn take_command(&mut self) -> Option<Command> {
        self.commands.pop_front()
    }

    /// A result from the event loop.
    pub fn receive(&mut self, msg: Msg) {
        match msg {
            Msg::Planned { generation, result } => {
                if generation != self.generation {
                    return;
                }
                self.loading = false;
                match *result {
                    Ok(planned) => {
                        self.planned = Some(planned);
                        self.plan_error = None;
                    }
                    Err(e) => self.plan_error = Some(e),
                }
                self.clamp();
            }
            Msg::Applied(result) => {
                self.busy = None;
                match result {
                    Ok(msg) => {
                        self.reviewed.clear();
                        self.record(msg);
                    }
                    Err(e) => self.record(format!("not applied: {e}")),
                }
                self.request_plan();
            }
            Msg::Undone(result) => {
                self.busy = None;
                match result {
                    Ok(r) => {
                        let skipped = if r.skipped.is_empty() {
                            String::new()
                        } else {
                            format!("; {} skipped (changed since)", r.skipped.len())
                        };
                        self.record(format!("Restored {} items from operation {}{skipped}", r.restored.len(), r.op_id));
                    }
                    Err(e) => self.record(format!("not undone: {e}")),
                }
                self.request_plan();
            }
        }
    }

    fn record(&mut self, msg: String) {
        self.last_result = Some(msg.clone());
        self.notify(msg);
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), self.ticks + TOAST_TICKS));
    }

    /// The current toast, if it has not expired.
    pub fn toast(&self) -> Option<&str> {
        self.toast.as_ref().filter(|(_, until)| self.ticks < *until).map(|(m, _)| m.as_str())
    }

    /// The topmost popup.
    pub fn popup(&self) -> Option<&Popup> {
        self.popups.last()
    }

    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// Keep every cursor inside its list.
    fn clamp(&mut self) {
        let lens = [
            (Tab::Skills, crate::screens::skills::rows(self).len()),
            (Tab::Roles, crate::screens::roles::rows(self).len()),
            (Tab::Install, crate::screens::install::rows(self).len()),
        ];
        for (tab, len) in lens {
            let c = &mut self.cursor[tab.index()];
            *c = (*c).min(len.saturating_sub(1));
        }
    }

    pub fn help_open(&self) -> bool {
        matches!(self.popups.last(), Some(Popup::Help { .. }))
    }

    pub fn help_scroll(&self) -> u16 {
        match self.popups.last() {
            Some(Popup::Help { scroll }) => *scroll,
            _ => 0,
        }
    }

    pub fn filter_editing(&self) -> bool {
        self.filter_edit.is_some()
    }

    /// The active filter: the text being typed, else the kept filter.
    pub fn filter(&self) -> &str {
        self.filter_edit.as_deref().unwrap_or(&self.filter)
    }

    /// The key context right now.
    pub fn ctx(&self) -> Ctx {
        if self.filter_edit.is_some() {
            Ctx::Filter
        } else {
            match self.popups.last() {
                Some(Popup::Help { .. }) => Ctx::Help,
                Some(Popup::Confirm { .. }) => Ctx::Confirm,
                Some(Popup::Viewer { .. }) => Ctx::Viewer,
                None => Ctx::Normal,
            }
        }
    }

    /// Handle one key press.
    pub fn press(&mut self, key: KeyEvent) {
        let ctx = self.ctx();
        let typed = |c| ctx == Ctx::Filter && !key.modifiers.contains(KeyModifiers::CONTROL) && c != '\0';
        let action = match key.code {
            KeyCode::Char(c) if typed(c) => Some(Action::FilterChar(c)),
            KeyCode::Backspace if ctx == Ctx::Filter => Some(Action::FilterBackspace),
            _ if ctx == Ctx::Normal => resolve(&[self.tab.ctx(), Ctx::Normal, Ctx::Global], key),
            _ => resolve(&[ctx, Ctx::Global], key),
        };
        if let Some(a) = action {
            self.update(a);
        }
    }

    pub fn update(&mut self, action: Action) {
        let tabs = Tab::ALL.len();
        match action {
            Action::Quit => self.quit = true,
            Action::GoTab(i) => self.tab = Tab::ALL[i.min(tabs - 1)],
            Action::NextTab => self.tab = Tab::ALL[(self.tab.index() + 1) % tabs],
            Action::PrevTab => self.tab = Tab::ALL[(self.tab.index() + tabs - 1) % tabs],
            Action::FocusNext | Action::FocusPrev => {
                self.focus = if self.focus == Focus::List { Focus::Detail } else { Focus::List };
            }
            Action::Down | Action::Up | Action::PageDown | Action::PageUp | Action::Top | Action::Bottom => {
                self.scroll(action);
            }
            Action::StartFilter => self.filter_edit = Some(self.filter.clone()),
            Action::FilterChar(c) => {
                if let Some(edit) = &mut self.filter_edit {
                    edit.push(c);
                }
            }
            Action::FilterBackspace => {
                if let Some(edit) = &mut self.filter_edit {
                    edit.pop();
                }
            }
            Action::AcceptFilter => {
                if let Some(edit) = self.filter_edit.take() {
                    self.filter = edit;
                }
            }
            Action::CancelFilter => {
                self.filter_edit = None;
                self.filter.clear();
            }
            Action::OpenHelp => self.popups.push(Popup::Help { scroll: 0 }),
            Action::ClosePopup => {
                self.popups.pop();
            }
            Action::Back => {
                if !self.filter.is_empty() {
                    self.filter.clear();
                } else if self.focus == Focus::Detail {
                    self.focus = Focus::List;
                }
            }
            Action::Refresh => self.request_plan(),
            Action::ToggleOptIn => self.toggle_opt_in(),
            Action::CyclePreset => self.cycle_preset(),
            Action::CycleProfile => self.cycle_profile(),
            Action::CycleCli => self.cycle_cli(),
            Action::MarkReviewed => self.mark_reviewed(),
            Action::OpenReview => self.open_review(),
            Action::StartApply => self.start_apply(),
            Action::StartUndo => self.start_undo(),
            Action::ConfirmYes => self.confirm_yes(),
        }
        self.clamp();
    }

    fn selected_action(&self) -> Option<aurora_skills::plan::Action> {
        crate::screens::install::rows(self).into_iter().nth(self.cursor[Tab::Install.index()])
    }

    fn mark_reviewed(&mut self) {
        let Some(a) = self.selected_action() else { return };
        if !a.needs_review {
            self.notify(format!("{} needs no review", a.entry));
        } else if let Some(i) = self.reviewed.iter().position(|r| *r == a.entry) {
            self.reviewed.remove(i);
            self.notify(format!("{} no longer marked reviewed", a.entry));
        } else {
            self.reviewed.push(a.entry.clone());
            self.notify(format!("{} marked reviewed for this session", a.entry));
        }
    }

    fn open_review(&mut self) {
        let (Some(a), Some(p)) = (self.selected_action(), &self.planned) else { return };
        let Some(tree) = p.contents.get(&a.entry) else {
            self.notify(format!("{} has no files to read here", a.target));
            return;
        };
        let wanted: Vec<String> = match p.review_files.get(&a.entry) {
            Some(files) => files.clone(),
            None => vec!["SKILL.md".to_string()],
        };
        let mut text = String::new();
        for path in &wanted {
            let body = tree
                .file(path)
                .and_then(|f| f.content.as_deref())
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .unwrap_or_else(|| "(not readable)".into());
            text.push_str(&format!("── {path} ──\n{body}\n"));
        }
        let title = format!(" {} — {} (verified copy; Esc closes) ", a.entry, wanted.join(", "));
        self.popups.push(Popup::Viewer { title, text, scroll: 0 });
    }

    fn start_apply(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("busy: {b}…"));
        }
        let Some(p) = &self.planned else { return self.notify("no plan yet") };
        let unreviewed = p.unreviewed(&self.reviewed);
        if !unreviewed.is_empty() {
            return self
                .notify(format!("review {} first (Enter shows the files, r marks reviewed)", unreviewed.join(", ")));
        }
        if !p.plan.changed {
            return self.notify("Nothing to do: the project already matches the plan.");
        }
        let changes: Vec<String> = p
            .plan
            .actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Create | ActionKind::Update | ActionKind::Remove))
            .map(|a| format!("{} {}", format!("{:?}", a.kind).to_lowercase(), a.target))
            .collect();
        let mut lines: Vec<String> = changes.iter().take(14).cloned().collect();
        if changes.len() > 14 {
            lines.push(format!("… and {} more", changes.len() - 14));
        }
        lines.push(String::new());
        lines.push("Journaled under .aurora/ops/; undo with u. Nothing is ever deleted.".into());
        let title = format!(" Apply {} changes to {}? ", p.change_count(), p.project.display());
        self.popups.push(Popup::Confirm { op: ConfirmOp::Apply, title, lines });
    }

    fn start_undo(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("busy: {b}…"));
        }
        let title = format!(" Undo the last Aurora operation in {}? ", self.opts.project.display());
        let lines = vec![
            "Restores what the last apply changed, where it is unchanged since.".to_string(),
            "Removed copies are moved aside under .aurora/ops/, not deleted.".to_string(),
        ];
        self.popups.push(Popup::Confirm { op: ConfirmOp::Undo, title, lines });
    }

    fn confirm_yes(&mut self) {
        let Some(Popup::Confirm { op, .. }) = self.popups.pop() else { return };
        match op {
            ConfirmOp::Apply => {
                let Some(p) = &self.planned else { return };
                self.busy = Some("applying");
                self.commands
                    .push_back(Command::Apply { planned: Box::new(p.clone()), reviewed: self.reviewed.clone() });
            }
            ConfirmOp::Undo => {
                self.busy = Some("undoing");
                self.commands.push_back(Command::Undo { project: self.opts.project.clone() });
            }
        }
    }

    fn cycle_preset(&mut self) {
        let i = PRESETS.iter().position(|p| p.name == self.opts.preset).map_or(0, |i| (i + 1) % PRESETS.len());
        self.opts.preset = PRESETS[i].name.to_string();
        self.workflow = load_workflow(&WorkflowSource::Preset(self.opts.preset.clone()));
        self.cursor[Tab::Roles.index()] = 0;
        self.request_plan();
    }

    /// The profile in effect: the planned one, else the requested one.
    fn current_profile(&self) -> String {
        match &self.planned {
            Some(p) => p.recommendation.profile.clone(),
            None => aurora_app::install::default_profile(self.opts.profile.as_deref(), self.opts.clis.len()),
        }
    }

    fn cycle_profile(&mut self) {
        let Ok(cat) = &self.catalog else { return };
        let names: Vec<&String> = cat.profiles().keys().collect();
        if names.is_empty() {
            return;
        }
        let current = self.current_profile();
        let i = names.iter().position(|n| **n == current).map_or(0, |i| (i + 1) % names.len());
        self.opts.profile = Some(names[i].clone());
        self.opts.clis.clear();
        self.request_plan();
    }

    fn cycle_cli(&mut self) {
        let (Some(p), Ok(cat)) = (&self.planned, &self.catalog) else { return };
        let rows = crate::screens::roles::rows(self);
        let Some(row) = rows.get(self.cursor[Tab::Roles.index()]) else { return };
        let mut clis = p.recommendation.clis.clone();
        let slot = cat.profile(&p.recommendation.profile).and_then(|pr| pr.roles.get(&row.role).copied()).unwrap_or(1);
        let Some(current) = clis.get(slot - 1).cloned() else { return };
        let start = PROVIDER_IDS.iter().position(|id| *id == current).unwrap_or(0);
        let next = (1..=PROVIDER_IDS.len())
            .map(|k| PROVIDER_IDS[(start + k) % PROVIDER_IDS.len()])
            .find(|id| !clis.iter().any(|c| c == id));
        let Some(next) = next else { return };
        clis[slot - 1] = next.to_string();
        self.opts.profile = Some(p.recommendation.profile.clone());
        self.opts.clis = clis;
        self.request_plan();
    }

    fn toggle_opt_in(&mut self) {
        let rows = crate::screens::skills::rows(self);
        let Some(row) = rows.get(self.cursor[Tab::Skills.index()]) else { return };
        if row.tier != Tier::OptIn {
            return;
        }
        let id = row.id.clone();
        if let Some(i) = self.with.iter().position(|w| *w == id) {
            self.with.remove(i);
        } else {
            self.with.push(id);
        }
        self.request_plan();
    }

    fn scroll(&mut self, action: Action) {
        if let Some(Popup::Help { scroll } | Popup::Viewer { scroll, .. }) = self.popups.last_mut() {
            *scroll = match action {
                Action::Down => scroll.saturating_add(1),
                Action::Up => scroll.saturating_sub(1),
                Action::PageDown => scroll.saturating_add(15),
                Action::PageUp => scroll.saturating_sub(15),
                Action::Top => 0,
                _ => *scroll,
            };
            return;
        }
        let c = &mut self.cursor[self.tab.index()];
        *c = match action {
            Action::Down => c.saturating_add(1),
            Action::Up => c.saturating_sub(1),
            Action::PageDown => c.saturating_add(10),
            Action::PageUp => c.saturating_sub(10),
            Action::Top => 0,
            _ => usize::MAX,
        };
    }
}
