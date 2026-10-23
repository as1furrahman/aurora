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
    Confirm,
    Viewer,
    Form,
    Input,
    Skills,
    Roles,
    Install,
    Runs,
}

impl Ctx {
    pub fn title(self) -> &'static str {
        match self {
            Ctx::Global => "Anywhere",
            Ctx::Normal => "Lists and tabs",
            Ctx::Help => "Help",
            Ctx::Filter => "While filtering",
            Ctx::Confirm => "Confirm dialog",
            Ctx::Viewer => "File viewer",
            Ctx::Form => "Submit form",
            Ctx::Input => "Text prompt",
            Ctx::Skills => "Skills tab",
            Ctx::Roles => "Roles tab",
            Ctx::Install => "Install tab",
            Ctx::Runs => "Runs tab",
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
    b(
        Ctx::Skills,
        &[(KeyCode::Char('r'), NONE)],
        "r refresh",
        "Re-run the install plan (aurora skills plan)",
        Action::Refresh,
        true,
    ),
    b(
        Ctx::Skills,
        &[(KeyCode::Char('i'), NONE)],
        "i opt-in",
        "Include or drop the selected opt-in skill (--with)",
        Action::ToggleOptIn,
        true,
    ),
    b(Ctx::Skills, &[(KeyCode::Enter, NONE)], "Enter detail", "Focus the detail pane", Action::FocusNext, false),
    b(
        Ctx::Roles,
        &[(KeyCode::Char('p'), NONE)],
        "p cli",
        "Give the selected role to the next CLI not yet in the profile",
        Action::CycleCli,
        true,
    ),
    b(
        Ctx::Roles,
        &[(KeyCode::Char('P'), NONE)],
        "P profile",
        "Switch the CLI profile (solo, pair, trio)",
        Action::CycleProfile,
        true,
    ),
    b(Ctx::Roles, &[(KeyCode::Char('w'), NONE)], "w workflow", "Switch the workflow preset", Action::CyclePreset, true),
    b(Ctx::Roles, &[(KeyCode::Char('r'), NONE)], "r refresh", "Re-run the plan", Action::Refresh, false),
    b(
        Ctx::Install,
        &[(KeyCode::Enter, NONE)],
        "Enter read",
        "Read the selected skill's flagged files (or its SKILL.md)",
        Action::OpenReview,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('r'), NONE)],
        "r reviewed",
        "Mark the selected skill reviewed for this session (--reviewed)",
        Action::MarkReviewed,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('a'), NONE)],
        "a apply",
        "Apply the plan (asks first; aurora skills apply)",
        Action::StartApply,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('u'), NONE)],
        "u undo",
        "Undo the last operation (asks first; aurora skills undo)",
        Action::StartUndo,
        true,
    ),
    b(Ctx::Install, &[(KeyCode::Char('R'), NONE)], "R refresh", "Re-run the plan", Action::Refresh, false),
    b(
        Ctx::Runs,
        &[(KeyCode::Enter, NONE)],
        "Enter open",
        "Open the run's pipeline; on a stage, read its handoff",
        Action::OpenRun,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('s'), NONE)],
        "s submit",
        "Submit the stage's evidence (checkbox form of its criteria)",
        Action::StartSubmit,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('A'), NONE)],
        "A approve",
        "Record human approval for the stage",
        Action::StartApprove,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('y'), NONE)],
        "y copy",
        "Copy the stage's handoff path (OSC 52 clipboard)",
        Action::CopyPath,
        false,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('R'), NONE)],
        "R resume",
        "Verify the run and re-issue missing handoffs",
        Action::Resume,
        false,
    ),
    b(Ctx::Runs, &[(KeyCode::Char('r'), NONE)], "r reload", "Reload the runs", Action::ReloadRuns, true),
    b(
        Ctx::Form,
        &[(KeyCode::Char(' '), NONE), (KeyCode::Char('x'), NONE)],
        "Space check",
        "Check or uncheck the criterion",
        Action::ToggleCheck,
        true,
    ),
    b(
        Ctx::Form,
        &[(KeyCode::Char('j'), NONE), (KeyCode::Down, NONE)],
        "j/k move",
        "Next criterion",
        Action::Down,
        true,
    ),
    b(
        Ctx::Form,
        &[(KeyCode::Char('k'), NONE), (KeyCode::Up, NONE)],
        "j/k move",
        "Previous criterion",
        Action::Up,
        false,
    ),
    b(
        Ctx::Form,
        &[(KeyCode::Enter, NONE)],
        "Enter submit",
        "Submit the checked criteria (the gate decides)",
        Action::SubmitForm,
        true,
    ),
    b(Ctx::Form, &[(KeyCode::Esc, NONE)], "Esc cancel", "Cancel without submitting", Action::ClosePopup, true),
    b(Ctx::Input, &[(KeyCode::Enter, NONE)], "Enter ok", "Accept the text", Action::InputAccept, true),
    b(Ctx::Input, &[(KeyCode::Esc, NONE)], "Esc cancel", "Cancel", Action::ClosePopup, true),
    b(Ctx::Confirm, &[(KeyCode::Char('y'), NONE)], "y apply", "Yes: go ahead", Action::ConfirmYes, true),
    b(
        Ctx::Confirm,
        &[(KeyCode::Char('n'), NONE), (KeyCode::Esc, NONE), (KeyCode::Enter, NONE), (KeyCode::Char('q'), NONE)],
        "n cancel",
        "No (also Esc and Enter: the default is No)",
        Action::ClosePopup,
        true,
    ),
    b(
        Ctx::Viewer,
        &[(KeyCode::Esc, NONE), (KeyCode::Char('q'), NONE)],
        "Esc close",
        "Close the viewer",
        Action::ClosePopup,
        true,
    ),
    b(
        Ctx::Viewer,
        &[(KeyCode::Char('j'), NONE), (KeyCode::Down, NONE)],
        "j/k scroll",
        "Scroll down",
        Action::Down,
        true,
    ),
    b(Ctx::Viewer, &[(KeyCode::Char('k'), NONE), (KeyCode::Up, NONE)], "j/k scroll", "Scroll up", Action::Up, false),
    b(
        Ctx::Viewer,
        &[(KeyCode::PageDown, NONE), (KeyCode::Char(' '), NONE)],
        "PgDn page",
        "Page down",
        Action::PageDown,
        false,
    ),
    b(Ctx::Viewer, &[(KeyCode::PageUp, NONE)], "PgUp page", "Page up", Action::PageUp, false),
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
    footer_for(&[ctx])
}

/// Footer bindings for several contexts, in order, one per label.
pub fn footer_for(ctxs: &[Ctx]) -> Vec<&'static Binding> {
    let mut out: Vec<&Binding> = Vec::new();
    for ctx in ctxs {
        for bind in BINDINGS.iter().filter(|b| b.ctx == *ctx && b.footer) {
            if !out.iter().any(|o| o.label == bind.label) {
                out.push(bind);
            }
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

/// The action bound to `key` in the first of `ctxs` that binds it.
pub fn resolve(ctxs: &[Ctx], key: KeyEvent) -> Option<Action> {
    let k = normalize(key);
    ctxs.iter().find_map(|c| BINDINGS.iter().find(|b| b.ctx == *c && b.keys.contains(&k)).map(|b| b.action))
}
