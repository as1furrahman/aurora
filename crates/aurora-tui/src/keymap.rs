//! The single key table. Key handling, the footer and the `?` help overlay are all generated from
//! it, so they cannot drift apart (the television/gitui pattern).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

/// Where a binding applies. `Global` works everywhere, even in popups and while filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Global,
    /// The normal view: tabs, lists and detail panes.
    Normal,
    Help,
    Filter,
}

impl Ctx {
    pub fn title(self) -> &'static str {
        match self {
            Ctx::Global => "Anywhere",
            Ctx::Normal => "Lists and tabs",
            Ctx::Help => "Help",
            Ctx::Filter => "While filtering",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub ctx: Ctx,
    pub keys: &'static [(KeyCode, KeyModifiers)],
    /// Short footer label, e.g. `q quit`.
    pub label: &'static str,
    /// One-line description for the help overlay.
    pub help: &'static str,
    pub action: Action,
    /// Shown in the footer for its context.
    pub footer: bool,
}

const NONE: KeyModifiers = KeyModifiers::NONE;
const CTRL: KeyModifiers = KeyModifiers::CONTROL;

const fn b(
    ctx: Ctx,
    keys: &'static [(KeyCode, KeyModifiers)],
    label: &'static str,
    help: &'static str,
    action: Action,
    footer: bool,
) -> Binding {
    Binding { ctx, keys, label, help, action, footer }
}

static BINDINGS: &[Binding] = &[
    b(Ctx::Global, &[(KeyCode::Char('c'), CTRL)], "^C quit", "Quit Aurora at once", Action::Quit, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('1'), NONE)],
        "1-4 tabs",
        "Switch tab: 1 Skills, 2 Roles, 3 Install, 4 Runs",
        Action::GoTab(0),
        true,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('2'), NONE)],
        "1-4 tabs",
        "Switch tab: 1 Skills, 2 Roles, 3 Install, 4 Runs",
        Action::GoTab(1),
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('3'), NONE)],
        "1-4 tabs",
        "Switch tab: 1 Skills, 2 Roles, 3 Install, 4 Runs",
        Action::GoTab(2),
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('4'), NONE)],
        "1-4 tabs",
        "Switch tab: 1 Skills, 2 Roles, 3 Install, 4 Runs",
        Action::GoTab(3),
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::Char('['), NONE)], "[ ] tab", "Previous tab", Action::PrevTab, false),
    b(Ctx::Normal, &[(KeyCode::Char(']'), NONE)], "[ ] tab", "Next tab", Action::NextTab, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Tab, NONE)],
        "Tab focus",
        "Move focus between the list and the detail pane",
        Action::FocusNext,
        true,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::BackTab, KeyModifiers::SHIFT)],
        "S-Tab focus",
        "Move focus back",
        Action::FocusPrev,
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::Char('j'), NONE), (KeyCode::Down, NONE)], "j/k move", "Move down", Action::Down, true),
    b(Ctx::Normal, &[(KeyCode::Char('k'), NONE), (KeyCode::Up, NONE)], "j/k move", "Move up", Action::Up, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('g'), NONE), (KeyCode::Home, NONE)],
        "g/G ends",
        "Go to the first item",
        Action::Top,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('G'), NONE), (KeyCode::End, NONE)],
        "g/G ends",
        "Go to the last item",
        Action::Bottom,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::PageDown, NONE), (KeyCode::Char('d'), CTRL)],
        "PgDn",
        "Page down",
        Action::PageDown,
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::PageUp, NONE), (KeyCode::Char('u'), CTRL)], "PgUp", "Page up", Action::PageUp, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('/'), NONE)],
        "/ filter",
        "Filter the list (live; Enter keeps, Esc clears)",
        Action::StartFilter,
        true,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Esc, NONE)],
        "Esc back",
        "Back out one level: clear the filter, then return to the list",
        Action::Back,
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::Char('?'), NONE)], "? help", "Show this help", Action::OpenHelp, true),
    b(Ctx::Normal, &[(KeyCode::Char('q'), NONE)], "q quit", "Quit (from the top level)", Action::Quit, true),
    b(
        Ctx::Help,
        &[(KeyCode::Esc, NONE), (KeyCode::Char('q'), NONE), (KeyCode::Char('?'), NONE)],
        "Esc close",
        "Close the popup",
        Action::ClosePopup,
        true,
    ),
    b(Ctx::Help, &[(KeyCode::Char('j'), NONE), (KeyCode::Down, NONE)], "j/k scroll", "Scroll down", Action::Down, true),
    b(Ctx::Help, &[(KeyCode::Char('k'), NONE), (KeyCode::Up, NONE)], "j/k scroll", "Scroll up", Action::Up, false),
    b(
        Ctx::Filter,
        &[(KeyCode::Enter, NONE)],
        "Enter keep",
        "Keep the filter and return to the list",
        Action::AcceptFilter,
        true,
    ),
    b(Ctx::Filter, &[(KeyCode::Esc, NONE)], "Esc clear", "Clear the filter", Action::CancelFilter, true),
];

/// Every binding, in display order.
pub fn bindings() -> &'static [Binding] {
    BINDINGS
}

/// Footer bindings for `ctx`, one per label.
pub fn footer(ctx: Ctx) -> Vec<&'static Binding> {
    let mut out: Vec<&Binding> = Vec::new();
    for bind in BINDINGS.iter().filter(|b| b.ctx == ctx && b.footer) {
        if !out.iter().any(|o| o.label == bind.label) {
            out.push(bind);
        }
    }
    out
}

/// Characters arrive with SHIFT set for upper case and symbols; match them as typed.
fn normalize(key: KeyEvent) -> (KeyCode, KeyModifiers) {
    match key.code {
        KeyCode::Char(_) => (key.code, key.modifiers - KeyModifiers::SHIFT),
        _ => (key.code, key.modifiers),
    }
}

/// The action bound to `key` in `ctx`, falling back to the global bindings.
pub fn resolve(ctx: Ctx, key: KeyEvent) -> Option<Action> {
    let k = normalize(key);
    let find = |c: Ctx| BINDINGS.iter().find(|b| b.ctx == c && b.keys.contains(&k)).map(|b| b.action);
    find(ctx).or_else(|| find(Ctx::Global))
}
