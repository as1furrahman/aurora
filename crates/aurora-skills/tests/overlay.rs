//! R2 Step 3a: install overlays. Aurora installs upstream files unchanged except for removing
//! frontmatter keys that bypass a CLI's permission prompts (`allowed-tools`, and `hooks` unless
//! the user keeps them). Everything else stays byte-identical, and the result is re-parsed to
//! prove that only the named keys changed.

use aurora_skills::overlay::{OverlayError, strip_frontmatter_keys};
use aurora_skills::skill::parse_skill;

const HEAVY: &str = "---\nname: heavy\ndescription: >\n  Plans on disk.\n  Use for long tasks.\nallowed-tools: Read Write Bash\n\
hooks:\n  Stop:\n    - hooks:\n        - type: command\n          command: \"sh x.sh\"\nmetadata:\n  version: \"1\"\n---\n# Body\n\nallowed-tools: this line is body text, not frontmatter\n";

#[test]
fn removes_only_the_named_top_level_keys() {
    let (out, removed) = strip_frontmatter_keys(HEAVY, &["allowed-tools", "hooks"]).expect("strips");
    assert_eq!(removed, ["allowed-tools", "hooks"]);
    let before = parse_skill(HEAVY, "heavy").expect("parses");
    let after = parse_skill(&out, "heavy").expect("still parses");
    assert!(!after.frontmatter.contains_key("allowed-tools") && !after.frontmatter.contains_key("hooks"));
    assert_eq!(after.description, before.description);
    assert_eq!(after.frontmatter.get("metadata"), before.frontmatter.get("metadata"));
    assert_eq!(after.body, before.body, "the body, including look-alike lines, is untouched");
}

#[test]
fn other_lines_are_byte_identical() {
    let (out, _) = strip_frontmatter_keys(HEAVY, &["allowed-tools"]).expect("strips");
    let expected = HEAVY.replace("allowed-tools: Read Write Bash\n", "");
    assert_eq!(out, expected);
}

#[test]
fn absent_keys_leave_the_file_unchanged() {
    let plain = "---\nname: p\ndescription: d\n---\nbody\n";
    let (out, removed) = strip_frontmatter_keys(plain, &["allowed-tools", "hooks"]).expect("no-op");
    assert_eq!(out, plain);
    assert!(removed.is_empty());
}

#[test]
fn list_and_block_values_are_removed_whole() {
    let text = "---\nname: p\ndescription: d\nallowed-tools:\n  - Read\n  - Bash\n\nlicense: MIT\n---\nbody\n";
    let (out, removed) = strip_frontmatter_keys(text, &["allowed-tools"]).expect("strips");
    assert_eq!(removed, ["allowed-tools"]);
    assert_eq!(out, "---\nname: p\ndescription: d\nlicense: MIT\n---\nbody\n");
}

#[test]
fn crlf_files_keep_their_line_endings() {
    let text = "---\r\nname: p\r\ndescription: d\r\nallowed-tools: Bash\r\n---\r\nbody\r\n";
    let (out, _) = strip_frontmatter_keys(text, &["allowed-tools"]).expect("strips");
    assert_eq!(out, "---\r\nname: p\r\ndescription: d\r\n---\r\nbody\r\n");
}

#[test]
fn refuses_files_it_cannot_edit_safely() {
    assert!(matches!(strip_frontmatter_keys("no frontmatter\n", &["hooks"]), Err(OverlayError::NoFrontmatter)));
    // A flow mapping hides `hooks` inside a single line the line editor cannot split.
    let flow = "---\n{name: p, description: d, hooks: {Stop: []}}\n---\n";
    assert!(matches!(strip_frontmatter_keys(flow, &["hooks"]), Err(OverlayError::Unverified(_))));
}
