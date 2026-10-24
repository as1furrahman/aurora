//! The TUI model: one `App`, one `Action` enum, pure `update` transitions.
//!
//! Side effects never happen here. A key that needs work (planning, applying) queues a
//! [`Command`]; the event loop runs it on a worker thread and feeds the result back as a [`Msg`].
//! Results from superseded requests are recognised by their generation and ignored.

use std::collections::VecDeque;
use std::path::PathBuf;

use aurora_app::install::{InstallRequest, Planned, WorkflowSource, load_workflow};
use aurora_app::runs::RunSummary;
use aurora_core::catalog::{Catalog, Tier};
use aurora_core::presets::PRESETS;
use aurora_core::providers::PROVIDER_IDS;
use aurora_core::workflow::WorkflowDefinition;
use aurora_fs::engine::{ApproveResult, StatusReport, SubmitResult};
use aurora_skills::apply::UndoReport;
use aurora_skills::plan::ActionKind;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

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
    /// Open the guided setup.
    OpenSetup,
    /// Setup: tick or untick the highlighted tool.
    SetupToggle,
    /// Setup: the next step (or finish).
    SetupNext,
    /// Setup: the previous step.
    SetupBack,
    /// Confirm the open dialog.
    ConfirmYes,
    /// Runs: open the selected run's pipeline, or the selected stage's handoff.
    OpenRun,
    /// Runs: reload the run list.
    ReloadRuns,
    /// Tasks: ask to archive the selected task.
    ArchiveTask,
    /// Runs: open the submit form for the selected stage.
    StartSubmit,
    /// Submit form: check or uncheck the selected criterion.
    ToggleCheck,
    /// Submit form: submit the checked criteria.
    SubmitForm,
    /// Runs: ask who approves the selected stage.
    StartApprove,
    /// Runs: verify the run and re-issue missing handoffs.
    Resume,
    /// Runs: copy the selected stage's handoff path to the clipboard (OSC 52).
    CopyPath,
    /// Tasks: ask to start a new task with the current task type and tools.
    NewTask,
    InputChar(char),
    InputBackspace,
    InputAccept,
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
    LoadRuns {
        root: PathBuf,
    },
    ReadText {
        title: String,
        run_dir: PathBuf,
        rel: String,
    },
    LoadCriteria {
        run_dir: PathBuf,
        stage: String,
    },
    Submit {
        run_dir: PathBuf,
        stage: String,
        met: Vec<String>,
    },
    Approve {
        run_dir: PathBuf,
        stage: String,
        by: String,
    },
    Resume {
        run_dir: PathBuf,
    },
    /// Put text on the terminal clipboard (handled by the event loop).
    Copy {
        text: String,
    },
    StartTask(aurora_app::runs::TaskRequest),
    ArchiveTask {
        run_dir: PathBuf,
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
    /// A new task's run directory, or why it could not start.
    TaskStarted(Result<PathBuf, String>),
    TaskArchived(Result<PathBuf, String>),
    Runs(Result<Vec<RunSummary>, String>),
    Text {
        title: String,
        result: Result<String, String>,
    },
    Criteria {
        run_dir: PathBuf,
        stage: String,
        result: Result<Vec<(String, String)>, String>,
    },
    Submitted {
        stage: String,
        result: Result<SubmitResult, String>,
    },
    Approved {
        stage: String,
        result: Result<ApproveResult, String>,
    },
    /// Boxed: a status report is large.
    Resumed(Box<Result<StatusReport, String>>),
    Notice(String),
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
            Tab::Roles => "Who does what",
            Tab::Install => "Add to project",
            Tab::Runs => "Tasks",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmOp {
    Apply,
    Undo,
    StartTask,
    ArchiveTask(PathBuf),
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
    /// Acceptance criteria as checkboxes: (id, description, checked).
    Submit {
        run_dir: PathBuf,
        stage: String,
        items: Vec<(String, String, bool)>,
        cursor: usize,
    },
    /// The guided setup (first launch, or `t`).
    Setup(crate::setup::Setup),
    /// A one-line text prompt (the approver's name).
    Input {
        prompt: String,
        text: String,
        run_dir: PathBuf,
        stage: String,
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
    /// Bright, bold colours without dimming (`AURORA_THEME=high-contrast`).
    pub high_contrast: bool,
    /// Open the guided setup on start (first launch in a project).
    pub setup: bool,
    /// AI tools found on PATH (provider ids), to pre-tick in the setup.
    pub installed: Vec<String>,
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
            high_contrast: std::env::var("AURORA_THEME").is_ok_and(|v| v == "high-contrast"),
            setup: false,
            installed: Vec::new(),
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
    /// A new task's run directory, opened once the task list reloads.
    open_after_load: Option<PathBuf>,
    /// Runs under the run root.
    pub runs: Vec<RunSummary>,
    pub runs_error: Option<String>,
    /// The run whose pipeline is open.
    pub run_open: Option<usize>,
    /// Cursor in the open pipeline.
    pub stage_cursor: usize,
    toast: Option<(String, u64)>,
    generation: u64,
    commands: VecDeque<Command>,
    popups: Vec<Popup>,
    filter: String,
    filter_edit: Option<String>,
}

impl App {
    pub fn new(opts: TuiOptions) -> App {
        let mut opts = opts;
        // Tasks live in the project unless another folder was asked for.
        if opts.run_root.is_none() {
            opts.run_root = Some(aurora_app::runs::default_run_root(&opts.project));
        }
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
            open_after_load: None,
            runs: Vec::new(),
            runs_error: None,
            run_open: None,
            stage_cursor: 0,
            toast: None,
            generation: 0,
            commands: VecDeque::new(),
            popups: Vec::new(),
            filter: String::new(),
            filter_edit: None,
        };
        if app.opts.setup {
            app.open_setup();
        }
        app.request_plan();
        app
    }

    fn load_runs(&mut self) {
        if let Some(root) = &self.opts.run_root {
            self.commands.push_back(Command::LoadRuns { root: root.clone() });
        }
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
                    Err(e) => self.record(format!("Nothing was added: {e}")),
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
                            format!(" ({} you had changed since were left alone)", r.skipped.len())
                        };
                        self.record(format!("Undone: put back {} things as they were{skipped}.", r.restored.len()));
                    }
                    Err(e) => self.record(format!("Couldn't undo: {e}")),
                }
                self.request_plan();
            }
            other => self.receive_runs(other),
        }
    }

    fn receive_runs(&mut self, msg: Msg) {
        match msg {
            Msg::Runs(Ok(list)) => {
                self.runs = list;
                self.runs_error = None;
                if let Some(dir) = self.open_after_load.take()
                    && let Some(i) = self.runs.iter().position(|r| r.dir == dir)
                {
                    self.tab = Tab::Runs;
                    self.cursor[Tab::Runs.index()] =
                        crate::screens::runs::visible_runs(self).iter().position(|visible| *visible == i).unwrap_or(0);
                    self.run_open = Some(i);
                    self.stage_cursor = 0;
                    self.focus = Focus::List;
                }
            }
            Msg::TaskStarted(result) => {
                self.busy = None;
                match result {
                    Ok(dir) => {
                        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        self.notify(format!("Task {name} is ready: open step 1 with Enter"));
                        self.open_after_load = Some(dir);
                    }
                    Err(e) => self.notify(format!("The task couldn't start: {e}")),
                }
                self.load_runs();
            }
            Msg::TaskArchived(result) => {
                self.busy = None;
                match result {
                    Ok(path) => {
                        self.run_open = None;
                        self.notify(format!("Task archived. It is saved in {}.", path.display()));
                    }
                    Err(e) => self.notify(format!("The task couldn't be archived: {e}")),
                }
                self.load_runs();
            }
            Msg::Runs(Err(e)) => self.runs_error = Some(e),
            Msg::Text { title, result } => match result {
                Ok(text) => self.popups.push(Popup::Viewer { title, text, scroll: 0 }),
                Err(e) => self.notify(e),
            },
            Msg::Criteria { run_dir, stage, result } => match result {
                Ok(c) if c.is_empty() => self.notify(format!("{} has no checklist", crate::words::step(&stage))),
                Ok(c) => {
                    let items = c.into_iter().map(|(id, d)| (id, d, false)).collect();
                    self.popups.push(Popup::Submit { run_dir, stage, items, cursor: 0 });
                }
                Err(e) => self.notify(e),
            },
            Msg::Submitted { stage, result } => {
                match result {
                    Ok(r) => {
                        let (_, state) = crate::words::step_state(r.state);
                        let mut msg = format!("{}: {state}", crate::words::step(&stage));
                        if r.awaiting_approval {
                            msg.push_str(" · waiting for your OK (press A)");
                        }
                        if r.escalated {
                            msg.push_str(" · too many tries, a person needs to look");
                        }
                        if !r.missing.is_empty() {
                            let files: Vec<&str> =
                                r.missing.iter().map(|m| m.rsplit('/').next().unwrap_or(m)).collect();
                            msg.push_str(&format!(" · still missing {}", files.join(", ")));
                        }
                        self.notify(msg);
                    }
                    Err(e) => self.notify(format!("{}: {e}", crate::words::step(&stage))),
                }
                self.load_runs();
            }
            Msg::Approved { stage, result } => {
                match result {
                    Ok(r) => self.notify(format!(
                        "You gave your OK for {}: {}",
                        crate::words::step(&stage),
                        crate::words::step_state(r.state).1
                    )),
                    Err(e) => self.notify(format!("{}: {e}", crate::words::step(&stage))),
                }
                self.load_runs();
            }
            Msg::Resumed(result) => {
                match *result {
                    Ok(r) => self.notify(match r.warnings.len() {
                        0 => format!("Checked task {}: all fine", r.run_id),
                        n => format!("Checked task {}: {n} things to look at (see details)", r.run_id),
                    }),
                    Err(e) => self.notify(e),
                }
                self.load_runs();
            }
            Msg::Notice(m) => self.notify(m),
            _ => {}
        }
        self.clamp();
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
            (
                Tab::Runs,
                if self.run_open.is_some() {
                    crate::screens::runs::visible_stages(self).len()
                } else {
                    crate::screens::runs::visible_runs(self).len()
                },
            ),
        ];
        for (tab, len) in lens {
            let c = &mut self.cursor[tab.index()];
            *c = (*c).min(len.saturating_sub(1));
        }
        if self.run_open.is_some_and(|i| i >= self.runs.len()) {
            self.run_open = None;
        }
        let stages = crate::screens::runs::visible_stages(self).len();
        self.stage_cursor = self.stage_cursor.min(stages.saturating_sub(1));
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
                Some(Popup::Submit { .. }) => Ctx::Form,
                Some(Popup::Input { .. }) => Ctx::Input,
                Some(Popup::Setup(_)) => Ctx::Setup,
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
            KeyCode::Char(c) if ctx == Ctx::Input && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::InputChar(c))
            }
            KeyCode::Backspace if ctx == Ctx::Input => Some(Action::InputBackspace),
            _ if ctx == Ctx::Normal => resolve(&[self.tab.ctx(), Ctx::Normal, Ctx::Global], key),
            _ => resolve(&[ctx, Ctx::Global], key),
        };
        if let Some(a) = action {
            self.update(a);
        }
    }

    pub fn update(&mut self, action: Action) {
        let tabs = Tab::ALL.len();
        let before = self.tab;
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
                if self.tab == Tab::Runs && self.run_open.is_some() && self.filter.is_empty() {
                    self.run_open = None;
                } else if !self.filter.is_empty() {
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
            Action::OpenSetup => self.open_setup(),
            Action::NewTask => self.start_new_task(),
            Action::SetupToggle => {
                if let Some(Popup::Setup(s)) = self.popups.last_mut() {
                    s.toggle();
                }
            }
            Action::SetupBack => {
                if let Some(Popup::Setup(s)) = self.popups.last_mut() {
                    s.back();
                }
            }
            Action::SetupNext => self.setup_next(),
            Action::ConfirmYes => self.confirm_yes(),
            Action::OpenRun => self.open_run(),
            Action::ReloadRuns => self.load_runs(),
            Action::ArchiveTask => self.start_archive_task(),
            Action::StartSubmit => {
                if let Some((dir, stage)) = self.selected_run_stage() {
                    self.commands.push_back(Command::LoadCriteria { run_dir: dir, stage });
                }
            }
            Action::ToggleCheck => {
                if let Some(Popup::Submit { items, cursor, .. }) = self.popups.last_mut()
                    && let Some(item) = items.get_mut(*cursor)
                {
                    item.2 = !item.2;
                }
            }
            Action::SubmitForm => {
                if let Some(Popup::Submit { run_dir, stage, items, .. }) = self.popups.pop() {
                    let met = items.into_iter().filter(|i| i.2).map(|i| i.0).collect();
                    self.commands.push_back(Command::Submit { run_dir, stage, met });
                }
            }
            Action::StartApprove => {
                if let Some((run_dir, stage)) = self.selected_run_stage() {
                    let prompt =
                        format!("Give your OK for {}. Type your name, then press Enter:", crate::words::step(&stage));
                    self.popups.push(Popup::Input { prompt, text: String::new(), run_dir, stage });
                }
            }
            Action::InputChar(c) => {
                if let Some(Popup::Input { text, .. }) = self.popups.last_mut() {
                    text.push(c);
                }
            }
            Action::InputBackspace => {
                if let Some(Popup::Input { text, .. }) = self.popups.last_mut() {
                    text.pop();
                }
            }
            Action::InputAccept => {
                let empty = matches!(self.popups.last(), Some(Popup::Input { text, .. }) if text.trim().is_empty());
                if empty {
                    self.notify("Please type your name first");
                } else if let Some(Popup::Input { text, run_dir, stage, .. }) = self.popups.pop() {
                    self.commands.push_back(Command::Approve { run_dir, stage, by: text.trim().to_string() });
                }
            }
            Action::Resume => {
                if let Some(run) = self.run_open.and_then(|i| self.runs.get(i)) {
                    self.commands.push_back(Command::Resume { run_dir: run.dir.clone() });
                }
            }
            Action::CopyPath => {
                let path = crate::screens::runs::selected_stage(self)
                    .zip(crate::screens::runs::open_run(self))
                    .and_then(|(s, r)| s.handoff.as_ref().map(|h| format!("{}/{h}", r.run_dir)));
                match path {
                    Some(text) => self.commands.push_back(Command::Copy { text }),
                    None => self.notify("This step has no instructions yet"),
                }
            }
        }
        // The task list is read when you open the Tasks page, so it is always current.
        if self.tab == Tab::Runs && before != Tab::Runs {
            self.load_runs();
        }
        self.clamp();
    }

    /// The open run's directory and the selected stage id.
    fn selected_run_stage(&self) -> Option<(PathBuf, String)> {
        let run = self.run_open.and_then(|i| self.runs.get(i))?;
        let stage = crate::screens::runs::selected_stage(self)?;
        Some((run.dir.clone(), stage.id.clone()))
    }

    fn open_run(&mut self) {
        if self.run_open.is_none() {
            if let Some(i) = crate::screens::runs::visible_runs(self).get(self.cursor[Tab::Runs.index()])
                && self.runs.get(*i).is_some_and(|r| r.status.is_ok())
            {
                self.run_open = Some(*i);
                self.stage_cursor = 0;
            }
            return;
        }
        let Some((run_dir, stage)) = self.selected_run_stage() else { return };
        match crate::screens::runs::selected_stage(self).and_then(|s| s.handoff.clone()) {
            Some(rel) => {
                let tool = crate::screens::runs::selected_stage(self)
                    .map(|s| crate::words::tool(&s.provider).to_string())
                    .unwrap_or_default();
                let title =
                    format!(" Instructions for {}: copy them into {tool} (Esc closes) ", crate::words::step(&stage));
                self.commands.push_back(Command::ReadText { title, run_dir, rel });
            }
            None => self.notify(format!(
                "{} has no instructions yet: it waits for the steps before it",
                crate::words::step(&stage)
            )),
        }
    }

    /// Handle a mouse event in a terminal of size `area`: the wheel scrolls (the popup when one
    /// is open), a click on the tab bar switches tab, a click in a pane focuses it, and a click on
    /// a visible list row selects it. Clicks are ignored while a popup is open.
    pub fn mouse(&mut self, ev: MouseEvent, area: Rect) {
        match ev.kind {
            MouseEventKind::ScrollDown => self.update(Action::Down),
            MouseEventKind::ScrollUp => self.update(Action::Up),
            MouseEventKind::Down(MouseButton::Left) if self.popups.is_empty() && self.filter_edit.is_none() => {
                self.click(ev.column, ev.row, area);
            }
            _ => {}
        }
    }

    fn click(&mut self, col: u16, row: u16, area: Rect) {
        // Layout as in `ui::render`: header row, tab row, body, footer row.
        if row == area.y + 1 {
            // Tabs render as " title " separated by a one-column divider.
            let mut x = area.x;
            for (i, tab) in Tab::ALL.iter().enumerate() {
                let width = format!("{} {}", i + 1, tab.title()).chars().count() as u16 + 2;
                if col >= x && col < x + width {
                    self.update(Action::GoTab(i));
                    return;
                }
                x += width + 1;
            }
            return;
        }
        let body_top = area.y + 2;
        let body_bottom = area.y + area.height.saturating_sub(1);
        if row < body_top || row >= body_bottom {
            return;
        }
        if area.width >= crate::ui::SPLIT_WIDTH && col >= area.x + area.width * 55 / 100 {
            self.focus = Focus::Detail;
            return;
        }
        self.focus = Focus::List;
        // Inside the pane border: a table header row, plus the run header in a pipeline.
        let head = if self.tab == Tab::Runs && self.run_open.is_some() { 4 } else { 2 };
        let Some(index) = row.checked_sub(body_top + head).map(usize::from) else { return };
        let visible = usize::from(body_bottom.saturating_sub(body_top + head + 1));
        let current = if self.tab == Tab::Runs && self.run_open.is_some() {
            self.stage_cursor
        } else {
            self.cursor[self.tab.index()]
        };
        // Only while the list is not scrolled, so the row on screen is the row in the list.
        if index < visible && current < visible {
            if self.tab == Tab::Runs && self.run_open.is_some() {
                self.stage_cursor = index;
            } else {
                self.cursor[self.tab.index()] = index;
            }
            self.clamp();
        }
    }

    fn selected_action(&self) -> Option<aurora_skills::plan::Action> {
        crate::screens::install::rows(self).into_iter().nth(self.cursor[Tab::Install.index()])
    }

    fn mark_reviewed(&mut self) {
        let Some(a) = self.selected_action() else { return };
        if !a.needs_review {
            self.notify(format!("{} doesn't need checking", a.entry));
        } else if let Some(i) = self.reviewed.iter().position(|r| *r == a.entry) {
            self.reviewed.remove(i);
            self.notify(format!("{} is no longer marked as checked", a.entry));
        } else {
            self.reviewed.push(a.entry.clone());
            self.notify(format!("{} marked as checked ✓", a.entry));
        }
    }

    fn open_review(&mut self) {
        let (Some(a), Some(p)) = (self.selected_action(), &self.planned) else { return };
        let Some(tree) = p.contents.get(&a.entry) else {
            self.notify(format!("There's nothing to read for {}", a.entry));
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
        let title = format!(" Reading {}: {} (↑↓ scroll · Esc closes) ", a.entry, wanted.join(", "));
        self.popups.push(Popup::Viewer { title, text, scroll: 0 });
    }

    fn start_apply(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("Please wait: {b}…"));
        }
        let Some(p) = &self.planned else { return self.notify("Still working out what to add, one moment") };
        let unreviewed = p.unreviewed(&self.reviewed);
        if !unreviewed.is_empty() {
            return self.notify(format!("Please check {} first: Enter to read it, then r", unreviewed.join(", ")));
        }
        if !p.plan.changed {
            return self.notify("Nothing to add: your project already has everything.");
        }
        let changes: Vec<String> = p
            .plan
            .actions
            .iter()
            .filter(|a| matches!(a.kind, ActionKind::Create | ActionKind::Update | ActionKind::Remove))
            .map(|a| {
                let verb = match a.kind {
                    ActionKind::Create => "add   ",
                    ActionKind::Update => "update",
                    _ => "remove",
                };
                format!("{verb} {}", a.target)
            })
            .collect();
        let mut lines: Vec<String> = changes.iter().take(14).cloned().collect();
        if changes.len() > 14 {
            lines.push(format!("… and {} more", changes.len() - 14));
        }
        lines.push(String::new());
        lines.push("You can undo this at any time with u. Aurora keeps a backup of anything it replaces.".into());
        let title = format!(" Add these {} things to your project? ", p.change_count());
        self.popups.push(Popup::Confirm { op: ConfirmOp::Apply, title, lines });
    }

    fn start_undo(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("Please wait: {b}…"));
        }
        let title = " Undo Aurora's last change? ".to_string();
        let lines = vec![
            "Puts your project back the way it was before Aurora's last change.".to_string(),
            "Anything you edited yourself since then is left alone. Nothing is deleted:".to_string(),
            "removed files are kept as a backup in the .aurora folder.".to_string(),
        ];
        self.popups.push(Popup::Confirm { op: ConfirmOp::Undo, title, lines });
    }

    fn confirm_yes(&mut self) {
        let Some(Popup::Confirm { op, .. }) = self.popups.pop() else { return };
        match op {
            ConfirmOp::Apply => {
                let Some(p) = &self.planned else { return };
                self.busy = Some("adding to your project");
                self.commands
                    .push_back(Command::Apply { planned: Box::new(p.clone()), reviewed: self.reviewed.clone() });
            }
            ConfirmOp::Undo => {
                self.busy = Some("undoing");
                self.commands.push_back(Command::Undo { project: self.opts.project.clone() });
            }
            ConfirmOp::StartTask => {
                let (Some(clis), Some(run_root)) = (self.current_clis(), self.opts.run_root.clone()) else { return };
                self.busy = Some("setting up the task");
                self.commands.push_back(Command::StartTask(aurora_app::runs::TaskRequest {
                    project: self.opts.project.clone(),
                    run_root,
                    preset: self.opts.preset.clone(),
                    profile: Some(self.current_profile()),
                    clis,
                    with: self.with.clone(),
                    catalog: self.opts.catalog.clone(),
                }));
            }
            ConfirmOp::ArchiveTask(run_dir) => {
                if let Some(b) = self.busy {
                    return self.notify(format!("Please wait: {b}…"));
                }
                self.busy = Some("archiving the task");
                self.commands.push_back(Command::ArchiveTask { run_dir, project: self.opts.project.clone() });
            }
        }
    }

    fn start_archive_task(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("Please wait: {b}…"));
        }
        let Some(run_root) = self.opts.run_root.as_deref() else { return };
        if run_root != aurora_app::runs::default_run_root(&self.opts.project) {
            return self.notify("Tasks in a custom folder can't be archived from Aurora yet");
        }
        let index = self.run_open.unwrap_or_else(|| {
            crate::screens::runs::visible_runs(self).get(self.cursor[Tab::Runs.index()]).copied().unwrap_or(usize::MAX)
        });
        let Some(run) = self.runs.get(index) else { return };
        let id = run.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let title = format!(" Archive task {id}? ");
        let lines = vec![
            "Moves this task out of your active task list.".to_string(),
            "You can find its files in .aurora/archive/runs.".to_string(),
            "Nothing is deleted.".to_string(),
        ];
        self.popups.push(Popup::Confirm { op: ConfirmOp::ArchiveTask(run.dir.clone()), title, lines });
    }

    /// Ask to start a task: the steps and who does them, then yes or no.
    fn start_new_task(&mut self) {
        if let Some(b) = self.busy {
            return self.notify(format!("Please wait: {b}…"));
        }
        let Some(p) = self.planned.as_ref().filter(|_| !self.loading) else {
            return self.notify("One moment: still working out your last change");
        };
        let kind = crate::words::task_type(&self.opts.preset).to_string();
        let mut lines = vec!["Aurora sets up these steps, each done by one AI tool:".to_string()];
        for (i, s) in p.recommendation.stages.iter().enumerate() {
            lines.push(format!(
                "  {}. {:<16} the {}, {}",
                i + 1,
                crate::words::step(&s.stage),
                crate::words::job(&s.role),
                crate::words::tool(&s.provider)
            ));
        }
        lines.push(String::new());
        lines.push("They are saved in the .aurora folder in your project. The first step's".into());
        lines.push("instructions will be ready to copy into its AI tool.".into());
        let title = format!(" Start a new task: {kind}? ");
        self.popups.push(Popup::Confirm { op: ConfirmOp::StartTask, title, lines });
    }

    /// Open the guided setup, ticking the tools in use (else those asked for or installed).
    fn open_setup(&mut self) {
        let current = match &self.planned {
            Some(p) => p.recommendation.clis.clone(),
            None => self.opts.clis.clone(),
        };
        let setup = crate::setup::Setup::new(&self.opts.installed, &current, &self.opts.preset);
        self.popups.push(Popup::Setup(setup));
    }

    fn setup_next(&mut self) {
        let Some(Popup::Setup(s)) = self.popups.last_mut() else { return };
        match s.advance() {
            crate::setup::Next::Stay => {}
            crate::setup::Next::Plan { clis, preset } => {
                self.opts.profile = None;
                self.opts.clis = clis;
                self.opts.preset = preset;
                self.workflow = load_workflow(&WorkflowSource::Preset(self.opts.preset.clone()));
                self.cursor = [0; 4];
                self.request_plan();
            }
            crate::setup::Next::Finish => {
                if self.planned.is_none() || self.loading {
                    return;
                }
                self.popups.pop();
                self.tab = Tab::Install;
                self.focus = Focus::List;
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

    /// The profile in effect: the planned one, or the requested one while a plan is on its way
    /// (so a second key press builds on the first, not on the stale plan).
    fn current_profile(&self) -> String {
        match &self.planned {
            Some(p) if !self.loading => p.recommendation.profile.clone(),
            _ => aurora_app::install::default_profile(self.opts.profile.as_deref(), self.opts.clis.len()),
        }
    }

    /// The tools in effect, by the same rule; `None` while a plan is on its way and the request
    /// left the tools to the profile.
    fn current_clis(&self) -> Option<Vec<String>> {
        match &self.planned {
            Some(p) if !self.loading => Some(p.recommendation.clis.clone()),
            _ if !self.opts.clis.is_empty() => Some(self.opts.clis.clone()),
            _ => None,
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
        let Some(mut clis) = self.current_clis() else {
            return self.notify("One moment: still working out your last change");
        };
        let profile = self.current_profile();
        let Ok(cat) = &self.catalog else { return };
        let rows = crate::screens::roles::rows(self);
        let Some(row) = rows.get(self.cursor[Tab::Roles.index()]) else { return };
        let slot = cat.profile(&profile).and_then(|pr| pr.roles.get(&row.role).copied()).unwrap_or(1);
        let Some(current) = clis.get(slot - 1).cloned() else { return };
        let start = PROVIDER_IDS.iter().position(|id| *id == current).unwrap_or(0);
        let next = (1..=PROVIDER_IDS.len())
            .map(|k| PROVIDER_IDS[(start + k) % PROVIDER_IDS.len()])
            .find(|id| !clis.iter().any(|c| c == id));
        let Some(next) = next else { return };
        clis[slot - 1] = next.to_string();
        self.opts.profile = Some(profile);
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
        if let Some(Popup::Setup(s)) = self.popups.last_mut() {
            match action {
                Action::Down => s.move_by(true),
                Action::Up => s.move_by(false),
                _ => {}
            }
            return;
        }
        if let Some(Popup::Submit { cursor, .. }) = self.popups.last_mut() {
            *cursor = match action {
                Action::Down => cursor.saturating_add(1),
                Action::Up => cursor.saturating_sub(1),
                _ => *cursor,
            };
            if let Some(Popup::Submit { items, cursor, .. }) = self.popups.last_mut() {
                *cursor = (*cursor).min(items.len().saturating_sub(1));
            }
            return;
        }
        let c = if self.tab == Tab::Runs && self.run_open.is_some() {
            &mut self.stage_cursor
        } else {
            &mut self.cursor[self.tab.index()]
        };
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
