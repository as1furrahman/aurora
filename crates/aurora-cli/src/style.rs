//! Terminal styling. Output goes through `anstream`, which strips colour when stdout is not a
//! terminal or `NO_COLOR` is set, so glyphs and words always carry the meaning on their own.

use anstyle::{AnsiColor, Style};

pub const OK: Style = AnsiColor::Green.on_default().bold();
pub const BAD: Style = AnsiColor::Red.on_default().bold();
pub const WARN: Style = AnsiColor::Yellow.on_default();
pub const DIM: Style = Style::new().dimmed();
pub const HEAD: Style = Style::new().bold();

/// Wrap `text` in `style`.
pub fn paint(style: Style, text: impl std::fmt::Display) -> String {
    format!("{style}{text}{style:#}")
}
