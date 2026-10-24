# Rust table regression fixture

This standalone crate is the target project for the real bugfix workflow demo. Its initial
implementation intentionally fails the regression tests because it does not escape `|` inside
Markdown table cells. The fix stage will apply the demo's upstream patch and run `cargo test`.

The ordinary unit test records unchanged rendering for cells without pipe characters.
