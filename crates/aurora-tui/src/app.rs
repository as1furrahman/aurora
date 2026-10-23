//! The TUI model: one `App`, one `Action` enum, pure `update` transitions.
//!
//! Side effects never happen here. A key that needs work (planning, applying) queues a
//! [`Command`]; the event loop runs it on a worker thread and feeds the result back as a [`Msg`].
//! Results from superseded requests are recognised by their generation and ignored.

use std::collections::VecDeque;
use std::path::PathBuf;

use aurora_app::install::{InstallRequest, Planned, WorkflowSource};
use aurora_core::catalog::{Catalog, Tier};
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
}

/// Work for the event loop to run off the UI thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Plan { generation: u64, request: InstallRequest },
}

/// Results coming back from the event loop.
#[derive(Debug)]
pub enum Msg {
    Planned { generation: u64, result: Result<Planned, String> },
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Popup {
    Help { scroll: u16 },
}

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
    generation: u64,
    commands: VecDeque<Command>,
    popups: Vec<Popup>,
    filter: String,
    filter_edit: Option<String>,
}

impl App {
    pub fn new(opts: TuiOptions) -> App {
        let catalog = aurora_app::fetch::load_catalog(opts.catalog.as_deref());
        let mut app = App {
            opts,
            tab: Tab::Skills,
            focus: Focus::List,
            cursor: [0; 4],
            quit: false,
            catalog,
            planned: None,
            plan_error: None,
            loading: false,
            with: Vec::new(),
            ticks: 0,
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
                match result {
                    Ok(planned) => {
                        self.planned = Some(planned);
                        self.plan_error = None;
                    }
                    Err(e) => self.plan_error = Some(e),
                }
                self.clamp();
            }
        }
    }

    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    /// Keep every cursor inside its list.
    fn clamp(&mut self) {
        let len = crate::screens::skills::rows(self).len();
        let c = &mut self.cursor[Tab::Skills.index()];
        *c = (*c).min(len.saturating_sub(1));
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
        } else if self.help_open() {
            Ctx::Help
        } else {
            Ctx::Normal
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
        }
        self.clamp();
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
        if let Some(Popup::Help { scroll }) = self.popups.last_mut() {
            *scroll = match action {
                Action::Down => scroll.saturating_add(1),
                Action::Up => scroll.saturating_sub(1),
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
