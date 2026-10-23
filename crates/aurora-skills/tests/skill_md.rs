//! R2 Step 1: SKILL.md parsing, Agent Skills spec checks, per-CLI portability and listing cost.
//! Inputs are untrusted third-party files, so malformed YAML must fail cleanly, never panic.

use aurora_skills::skill::{
    MAX_SKILL_MD_BYTES, ParseError, Severity, Support, check_spec, listing_chars, parse_skill, shell_injections,
    support,
};

fn skill(frontmatter: &str, body: &str) -> String {
    format!("---\n{frontmatter}\n---\n{body}")
}

fn errors(text: &str, dir: &str) -> Vec<String> {
    let s = parse_skill(text, dir).expect("parses");
    check_spec(&s, dir).into_iter().filter(|i| i.severity == Severity::Error).map(|i| i.message).collect()
}

const PLAIN: &str = "name: tidy\ndescription: Keep changes small. Use when editing code.";

#[test]
fn parses_name_description_and_body() {
    let s = parse_skill(&skill(PLAIN, "# Tidy\nBe surgical.\n"), "tidy").expect("parses");
    assert_eq!(s.name, "tidy");
    assert_eq!(s.description, "Keep changes small. Use when editing code.");
    assert!(s.body.contains("Be surgical."));
    assert!(!s.repaired);
    assert!(check_spec(&s, "tidy").is_empty());
}

#[test]
fn multi_line_descriptions_are_normalised_to_one_line() {
    let s = parse_skill(&skill("name: tidy\ndescription: >\n  Keep changes\n  small.", ""), "tidy").expect("parses");
    assert_eq!(s.description, "Keep changes small.");
}

#[test]
fn frontmatter_must_be_delimited() {
    assert!(matches!(parse_skill("name: x\n", "x"), Err(ParseError::MissingFrontmatter)));
    assert!(matches!(parse_skill("---\nname: x\ndescription: y\n", "x"), Err(ParseError::MissingFrontmatter)));
    assert!(matches!(parse_skill("---\n---\nbody", "x"), Err(ParseError::MissingFrontmatter)));
}

#[test]
fn unquoted_colons_in_prose_are_repaired() {
    let s = parse_skill(&skill("name: aws\ndescription: Build for AWS: ECS and Lambda", ""), "aws").expect("repaired");
    assert!(s.repaired);
    assert_eq!(s.description, "Build for AWS: ECS and Lambda");
}

#[test]
fn genuinely_invalid_yaml_is_still_an_error() {
    // A stray sequence item inside the mapping: no line the repair could quote.
    let text = skill("name: x\ndescription: ok\n- stray item", "");
    assert!(matches!(parse_skill(&text, "x"), Err(ParseError::InvalidYaml(_))));
}

#[test]
fn alias_bombs_fail_without_expanding() {
    let mut fm = String::from("name: x\ndescription: y\na0: &a0 [z,z,z,z,z,z,z,z,z]\n");
    for i in 1..8 {
        let refs = vec![format!("*a{}", i - 1); 9].join(",");
        fm.push_str(&format!("a{i}: &a{i} [{refs}]\n"));
    }
    assert!(matches!(parse_skill(&skill(&fm, ""), "x"), Err(ParseError::InvalidYaml(_))));
}

#[test]
fn oversized_files_are_rejected_before_parsing() {
    let text = skill(PLAIN, &"x".repeat(MAX_SKILL_MD_BYTES));
    assert!(matches!(parse_skill(&text, "tidy"), Err(ParseError::TooLarge(_))));
}

#[test]
fn description_is_required() {
    assert!(matches!(parse_skill(&skill("name: x", ""), "x"), Err(ParseError::MissingField("description"))));
}

#[test]
fn spec_rules_for_name() {
    assert!(errors(&skill(PLAIN, ""), "other").iter().any(|e| e.contains("directory")), "name must match its dir");
    for bad in ["Tidy", "-tidy", "tidy-", "ti--dy", "ti_dy"] {
        let text = skill(&format!("name: {bad}\ndescription: d"), "");
        assert!(errors(&text, bad).iter().any(|e| e.contains("name")), "{bad}");
    }
    let long = "a".repeat(65);
    assert!(errors(&skill(&format!("name: {long}\ndescription: d"), ""), &long).iter().any(|e| e.contains("64")));
}

#[test]
fn spec_rules_for_lengths_and_types() {
    let long = format!("name: x\ndescription: {}", "d".repeat(1025));
    assert!(errors(&skill(&long, ""), "x").iter().any(|e| e.contains("1024")));
    let compat = format!("name: x\ndescription: d\ncompatibility: {}", "c".repeat(501));
    assert!(errors(&skill(&compat, ""), "x").iter().any(|e| e.contains("compatibility")));
    let meta = parse_skill(&skill("name: x\ndescription: d\nmetadata:\n  n: 3", ""), "x").expect("parses");
    assert!(check_spec(&meta, "x").iter().any(|i| i.message.contains("metadata")));
    let tools = parse_skill(&skill("name: x\ndescription: d\nallowed-tools: [Read]", ""), "x").expect("parses");
    assert!(check_spec(&tools, "x").iter().any(|i| i.message.contains("allowed-tools")));
}

#[test]
fn unknown_keys_are_warnings() {
    let s = parse_skill(&skill("name: x\ndescription: d\nfrobnicate: true", ""), "x").expect("parses");
    let issues = check_spec(&s, "x");
    assert!(issues.iter().any(|i| i.severity == Severity::Warning && i.message.contains("frobnicate")), "{issues:?}");
    let claude = parse_skill(&skill("name: x\ndescription: d\nargument-hint: \"[file]\"", ""), "x").expect("parses");
    assert!(check_spec(&claude, "x").is_empty(), "known Claude Code extensions are not unknown keys");
}

#[test]
fn shell_injection_syntax_is_found_but_prose_is_not() {
    let body = "Context: !`git status --short`\nUse `!` before `:` for a breaking change.\n```!\nls\n```\n";
    let found = shell_injections(body);
    assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [1, 3], "{found:?}");
    assert!(found[0].snippet.contains("git status"));
}

#[test]
fn portable_skills_are_native_everywhere() {
    let s = parse_skill(&skill(PLAIN, "Plain instructions."), "tidy").expect("parses");
    for cli in ["claude", "codex", "antigravity", "hermes"] {
        assert_eq!(support(&s, cli), Support::Native, "{cli}");
    }
}

#[test]
fn claude_only_behaviour_degrades_elsewhere_with_reasons() {
    let fm = "name: x\ndescription: d\nhooks:\n  Stop:\n    - hooks: [{type: command, command: echo}]\ncontext: fork";
    let s = parse_skill(&skill(fm, "Run !`date` first."), "x").expect("parses");
    assert_eq!(support(&s, "claude"), Support::Native);
    let Support::Degraded(reasons) = support(&s, "codex") else { panic!("codex should degrade") };
    let all = reasons.join(" | ");
    assert!(all.contains("hooks") && all.contains("context: fork") && all.contains("!`cmd`"), "{all}");
}

#[test]
fn manual_only_skills_become_implicit_without_an_overlay() {
    let s = parse_skill(&skill("name: x\ndescription: d\ndisable-model-invocation: true", ""), "x").expect("parses");
    assert_eq!(support(&s, "claude"), Support::Native);
    let Support::Degraded(reasons) = support(&s, "codex") else { panic!("codex should degrade") };
    assert!(reasons.iter().any(|r| r.contains("manual-only")), "{reasons:?}");
}

#[test]
fn listing_cost_follows_each_cli() {
    let fm = format!("name: x\ndescription: {}\nwhen_to_use: {}", "d".repeat(1000), "w".repeat(1000));
    let s = parse_skill(&skill(&fm, ""), "x").expect("parses");
    // Claude lists description + when_to_use, capped at its per-skill limit; Codex lists the description.
    assert_eq!(listing_chars(&s, "claude", Some(1536)), "x".len() + 1536 + 4);
    assert_eq!(listing_chars(&s, "codex", None), "x".len() + 1000 + 4);
}

#[test]
fn code_spans_ending_in_a_bang_are_not_injections() {
    // Real lines from trailofbits/skills and anthropics/skills that a naive `!`` match flags.
    let body = "- All user input must be validated with `ensure!`\n\
                | Use `fuzz_target!` macro | Write custom main function |\n\
                Unquoted, it evaluates to `#VALUE!`.\n\
                - PR diff: !`gh pr diff`\n\
                Unclosed !`never ends\n";
    let found = shell_injections(body);
    assert_eq!(found.iter().map(|f| f.line).collect::<Vec<_>>(), [4], "{found:?}");
    assert_eq!(found[0].snippet, "!`gh pr diff`");
}
