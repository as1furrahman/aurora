/// Render one row in a pipe-delimited Markdown table.
///
/// This fixture starts with the issue demonstrated by the workflow: literal pipes in cell
/// content are emitted as column separators.
pub fn render_row(cells: &[&str]) -> String {
    format!("| {} |", cells.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::render_row;

    #[test]
    fn tables_without_pipes_keep_their_existing_rendering() {
        assert_eq!(render_row(&["name", "value"]), "| name | value |");
    }
}
