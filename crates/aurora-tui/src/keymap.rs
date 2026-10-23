//! The single key table. Key handling, the footer and the `?` help overlay are all generated from
//! it, so they cannot drift apart (the television/gitui pattern). Labels and help are written for
//! people new to the terminal: plain verbs, no jargon.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::Action;

/// Where a binding applies. `Global` works everywhere, even in popups and while searching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctx {
    Global,
    /// The normal view: pages, lists and detail panes.
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
            Ctx::Normal => "Moving around",
            Ctx::Help => "In this help",
            Ctx::Filter => "While searching",
            Ctx::Confirm => "When Aurora asks yes or no",
            Ctx::Viewer => "While reading a file",
            Ctx::Form => "In a checklist",
            Ctx::Input => "While typing a name",
            Ctx::Skills => "On the Skills page",
            Ctx::Roles => "On the Who does what page",
            Ctx::Install => "On the Add to project page",
            Ctx::Runs => "On the Tasks page",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Binding {
    pub ctx: Ctx,
    pub keys: &'static [(KeyCode, KeyModifiers)],
    /// Short footer label: the key, a space, then a plain verb (`q quit`).
    pub label: &'static str,
    /// One plain sentence for the help overlay.
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

const UP: &[(KeyCode, KeyModifiers)] = &[(KeyCode::Up, NONE), (KeyCode::Char('k'), NONE)];
const DOWN: &[(KeyCode, KeyModifiers)] = &[(KeyCode::Down, NONE), (KeyCode::Char('j'), NONE)];

static BINDINGS: &[Binding] = &[
    // Skills page
    b(
        Ctx::Skills,
        &[(KeyCode::Char('i'), NONE)],
        "i add/remove optional",
        "Add or remove the selected optional skill",
        Action::ToggleOptIn,
        true,
    ),
    b(
        Ctx::Skills,
        &[(KeyCode::Char('r'), NONE)],
        "r refresh",
        "Work out the list again (for example after changing files yourself)",
        Action::Refresh,
        false,
    ),
    b(Ctx::Skills, &[(KeyCode::Enter, NONE)], "Enter details", "Show the details pane", Action::FocusNext, false),
    // Who does what page
    b(
        Ctx::Roles,
        &[(KeyCode::Char('p'), NONE)],
        "p change tool",
        "Give the selected job to a different AI tool",
        Action::CycleCli,
        true,
    ),
    b(
        Ctx::Roles,
        &[(KeyCode::Char('P'), NONE)],
        "P how many tools",
        "Switch between using 1, 2 or 3 AI tools",
        Action::CycleProfile,
        true,
    ),
    b(
        Ctx::Roles,
        &[(KeyCode::Char('w'), NONE)],
        "w task type",
        "Switch the kind of task (fix a bug, build a feature, ...)",
        Action::CyclePreset,
        true,
    ),
    b(Ctx::Roles, &[(KeyCode::Char('r'), NONE)], "r refresh", "Work out the list again", Action::Refresh, false),
    // Add to project page
    b(
        Ctx::Install,
        &[(KeyCode::Enter, NONE)],
        "Enter read it",
        "Read the selected skill before adding it",
        Action::OpenReview,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('r'), NONE)],
        "r mark as checked",
        "Mark the selected skill as checked by you (needed for community skills)",
        Action::MarkReviewed,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('a'), NONE)],
        "a add to project",
        "Add everything listed to this project (Aurora asks first)",
        Action::StartApply,
        true,
    ),
    b(
        Ctx::Install,
        &[(KeyCode::Char('u'), NONE)],
        "u undo",
        "Undo the last change Aurora made (Aurora asks first)",
        Action::StartUndo,
        true,
    ),
    b(Ctx::Install, &[(KeyCode::Char('R'), NONE)], "R refresh", "Work out the list again", Action::Refresh, false),
    // Tasks page
    b(
        Ctx::Runs,
        &[(KeyCode::Enter, NONE)],
        "Enter open",
        "Open a task; on a step, show its instructions",
        Action::OpenRun,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('s'), NONE)],
        "s checklist",
        "Tick off the step's checklist when you've done it",
        Action::StartSubmit,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('A'), NONE)],
        "A give OK",
        "Give your OK where a step waits for a person",
        Action::StartApprove,
        true,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('y'), NONE)],
        "y copy path",
        "Copy where the step's instructions are saved",
        Action::CopyPath,
        false,
    ),
    b(
        Ctx::Runs,
        &[(KeyCode::Char('R'), NONE)],
        "R check task",
        "Check the task's files and recreate missing instructions",
        Action::Resume,
        false,
    ),
    b(Ctx::Runs, &[(KeyCode::Char('r'), NONE)], "r refresh", "Load the tasks again", Action::ReloadRuns, false),
    // Anywhere
    b(Ctx::Global, &[(KeyCode::Char('c'), CTRL)], "^C quit", "Quit Aurora right away", Action::Quit, false),
    // Moving around
    b(
        Ctx::Normal,
        &[(KeyCode::Char('1'), NONE)],
        "1-4 pages",
        "Go to a page: 1 Skills, 2 Who does what, 3 Add to project, 4 Tasks",
        Action::GoTab(0),
        true,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('2'), NONE)],
        "1-4 pages",
        "Go to a page: 1 Skills, 2 Who does what, 3 Add to project, 4 Tasks",
        Action::GoTab(1),
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('3'), NONE)],
        "1-4 pages",
        "Go to a page: 1 Skills, 2 Who does what, 3 Add to project, 4 Tasks",
        Action::GoTab(2),
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('4'), NONE)],
        "1-4 pages",
        "Go to a page: 1 Skills, 2 Who does what, 3 Add to project, 4 Tasks",
        Action::GoTab(3),
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::Char('['), NONE)], "[ ] pages", "Previous page", Action::PrevTab, false),
    b(Ctx::Normal, &[(KeyCode::Char(']'), NONE)], "[ ] pages", "Next page", Action::NextTab, false),
    b(Ctx::Normal, DOWN, "↑↓ move", "Move down the list", Action::Down, true),
    b(Ctx::Normal, UP, "↑↓ move", "Move up the list", Action::Up, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Tab, NONE)],
        "Tab details",
        "Switch between the list and the details pane",
        Action::FocusNext,
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::BackTab, KeyModifiers::SHIFT)], "S-Tab back", "Switch back", Action::FocusPrev, false),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('g'), NONE), (KeyCode::Home, NONE)],
        "Home top",
        "Jump to the top of the list",
        Action::Top,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('G'), NONE), (KeyCode::End, NONE)],
        "End bottom",
        "Jump to the bottom of the list",
        Action::Bottom,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::PageDown, NONE), (KeyCode::Char('d'), CTRL)],
        "PgDn page",
        "Move down a page",
        Action::PageDown,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::PageUp, NONE), (KeyCode::Char('u'), CTRL)],
        "PgUp page",
        "Move up a page",
        Action::PageUp,
        false,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Char('/'), NONE)],
        "/ search",
        "Search the list as you type (Enter keeps it, Esc clears it)",
        Action::StartFilter,
        true,
    ),
    b(
        Ctx::Normal,
        &[(KeyCode::Esc, NONE)],
        "Esc back",
        "Go back one step: clear the search, close details, leave a task",
        Action::Back,
        false,
    ),
    b(Ctx::Normal, &[(KeyCode::Char('?'), NONE)], "? help", "Show this help", Action::OpenHelp, true),
    b(Ctx::Normal, &[(KeyCode::Char('q'), NONE)], "q quit", "Quit Aurora", Action::Quit, true),
    // Help
    b(
        Ctx::Help,
        &[(KeyCode::Esc, NONE), (KeyCode::Char('q'), NONE), (KeyCode::Char('?'), NONE)],
        "Esc close",
        "Close the help",
        Action::ClosePopup,
        true,
    ),
    b(Ctx::Help, DOWN, "↑↓ scroll", "Scroll down", Action::Down, true),
    b(Ctx::Help, UP, "↑↓ scroll", "Scroll up", Action::Up, false),
    // Searching
    b(
        Ctx::Filter,
        &[(KeyCode::Enter, NONE)],
        "Enter keep",
        "Keep the search and go back to the list",
        Action::AcceptFilter,
        true,
    ),
    b(Ctx::Filter, &[(KeyCode::Esc, NONE)], "Esc clear", "Clear the search", Action::CancelFilter, true),
    // Yes or no
    b(Ctx::Confirm, &[(KeyCode::Char('y'), NONE)], "y yes", "Yes, go ahead", Action::ConfirmYes, true),
    b(
        Ctx::Confirm,
        &[(KeyCode::Char('n'), NONE), (KeyCode::Esc, NONE), (KeyCode::Enter, NONE), (KeyCode::Char('q'), NONE)],
        "n no",
        "No (Esc and Enter also mean no; nothing happens unless you press y)",
        Action::ClosePopup,
        true,
    ),
    // Reading a file
    b(
        Ctx::Viewer,
        &[(KeyCode::Esc, NONE), (KeyCode::Char('q'), NONE)],
        "Esc close",
        "Close the file",
        Action::ClosePopup,
        true,
    ),
    b(Ctx::Viewer, DOWN, "↑↓ scroll", "Scroll down", Action::Down, true),
    b(Ctx::Viewer, UP, "↑↓ scroll", "Scroll up", Action::Up, false),
    b(
        Ctx::Viewer,
        &[(KeyCode::PageDown, NONE), (KeyCode::Char(' '), NONE)],
        "Space page",
        "Scroll down a page",
        Action::PageDown,
        false,
    ),
    b(Ctx::Viewer, &[(KeyCode::PageUp, NONE)], "PgUp page", "Scroll up a page", Action::PageUp, false),
    // Checklist
    b(
        Ctx::Form,
        &[(KeyCode::Char(' '), NONE), (KeyCode::Char('x'), NONE)],
        "Space tick",
        "Tick or untick the selected item",
        Action::ToggleCheck,
        true,
    ),
    b(Ctx::Form, DOWN, "↑↓ move", "Next item", Action::Down, true),
    b(Ctx::Form, UP, "↑↓ move", "Previous item", Action::Up, false),
    b(
        Ctx::Form,
        &[(KeyCode::Enter, NONE)],
        "Enter done",
        "Send the ticked items; Aurora then checks the files really exist",
        Action::SubmitForm,
        true,
    ),
    b(Ctx::Form, &[(KeyCode::Esc, NONE)], "Esc cancel", "Close without sending", Action::ClosePopup, true),
    // Typing a name
    b(Ctx::Input, &[(KeyCode::Enter, NONE)], "Enter OK", "Use what you typed", Action::InputAccept, true),
    b(Ctx::Input, &[(KeyCode::Esc, NONE)], "Esc cancel", "Cancel", Action::ClosePopup, true),
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
