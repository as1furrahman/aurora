use markdown_table_fixture::render_row;

#[test]
fn pipe_characters_inside_cells_are_escaped() {
    assert_eq!(render_row(&["foo|bar", "1"]), r"| foo\|bar | 1 |");
}

#[test]
fn pipe_characters_inside_headers_are_escaped() {
    assert_eq!(render_row(&["spam|eggs", "n"]), r"| spam\|eggs | n |");
}
