//! The TUI model: one `App`, one `Action` enum, pure `update` transitions.

use std::path::PathBuf;

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

#[derive(Debug, Clone)]
pub struct App {
    pub opts: TuiOptions,
    pub tab: Tab,
    pub focus: Focus,
    /// Cursor per tab.
    pub cursor: [usize; 4],
    pub quit: bool,
    popups: Vec<Popup>,
    filter: String,
    filter_edit: Option<String>,
}

impl App {
    pub fn new(opts: TuiOptions) -> App {
        App {
            opts,
            tab: Tab::Skills,
            focus: Focus::List,
            cursor: [0; 4],
            quit: false,
            popups: Vec::new(),
            filter: String::new(),
            filter_edit: None,
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
            _ => resolve(ctx, key),
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
        }
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
