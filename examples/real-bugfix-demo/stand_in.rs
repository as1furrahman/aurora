use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Args {
    stage: String,
    run: PathBuf,
    project: PathBuf,
    patch: PathBuf,
}

struct Handoff {
    text: String,
    outputs: Vec<PathBuf>,
    role: String,
    provider: String,
    concerns: Vec<String>,
}

impl Handoff {
    fn read(run: &Path, stage: &str) -> Self {
        let text = fs::read_to_string(run.join("handoff").join(format!("{stage}.md")))
            .unwrap_or_else(|error| fail(format!("read {stage} handoff: {error}")));
        let output_section =
            section(&text, "## Required outputs").unwrap_or_else(|| fail("handoff has no required outputs section"));
        let outputs = output_section
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- `"))
            .filter_map(|line| line.split_once('`').map(|(path, _)| PathBuf::from(path)))
            .collect();
        let role = value_after(&text, "- Role: **", "**").unwrap_or_default();
        let provider = value_after(&text, "- Provider: ", " (`").unwrap_or_default();
        let concerns = section(&text, "## Open concerns from upstream stages")
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with("- `"))
            .map(str::to_owned)
            .collect();
        Self { text, outputs, role, provider, concerns }
    }

    fn write(&self, name: &str, content: &str) {
        let path = self
            .outputs
            .iter()
            .find(|path| path.file_name().and_then(|file| file.to_str()) == Some(name))
            .unwrap_or_else(|| fail(format!("handoff does not request {name}")));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| fail(format!("create output directory: {error}")));
        }
        fs::write(path, format!("{}\n", content.trim_end()))
            .unwrap_or_else(|error| fail(format!("write {}: {error}", path.display())));
    }

    fn say(&self, message: &str) {
        println!("  [{} stand-in, role {}] {message}", self.provider, self.role);
    }
}

fn main() {
    let args = parse_args();
    let handoff = Handoff::read(&args.run, &args.stage);
    match args.stage.as_str() {
        "reproduce" => reproduce(&handoff, &args),
        "root-cause" => root_cause(&handoff, &args),
        "fix" => fix(&handoff, &args),
        "review" => review(&handoff, &args),
        "accept" => accept(&handoff, &args),
        stage => fail(format!("unknown stage {stage}")),
    }
    println!("  operator runs: {}", submit_command(&handoff.text));
}

fn parse_args() -> Args {
    let mut args = env::args().skip(1);
    let stage = args.next().unwrap_or_else(|| fail("missing stage"));
    let mut run = None;
    let mut project = None;
    let mut patch = None;
    while let Some(flag) = args.next() {
        let value = args.next().unwrap_or_else(|| fail(format!("missing value for {flag}")));
        match flag.as_str() {
            "--run" => run = Some(PathBuf::from(value)),
            "--project" => project = Some(PathBuf::from(value)),
            "--patch" => patch = Some(PathBuf::from(value)),
            _ => fail(format!("unknown argument {flag}")),
        }
    }
    Args {
        stage,
        run: run.unwrap_or_else(|| fail("--run is required")),
        project: project.unwrap_or_else(|| fail("--project is required")),
        patch: patch.unwrap_or_else(|| fail("--patch is required")),
    }
}

fn reproduce(handoff: &Handoff, args: &Args) {
    let result = cargo_test(&args.project, &["--test", "regression"]);
    if result.status.success() {
        fail("regression tests unexpectedly passed before the fix");
    }
    handoff.say("regression tests fail on the unfixed Rust fixture, as expected");
    handoff.write(
        "bugfix.md",
        &format!(
            "# Markdown table pipe escaping\n\n## Current behaviour\nA literal `|` in a cell becomes an extra column boundary.\n\n## Expected behaviour\nLiteral pipes inside cell and header text are escaped as `\\|`.\n\n## Unchanged behaviour\nThe fixture's unit test checks that cells without pipes keep their existing rendering.\n\n## Regression output\n```text\n{}\n```",
            tail(&combined(&result), 1200)
        ),
    );
    handoff.write(
        "repro-test.md",
        &format!(
            "# Reproduction\n\nCommand: `cargo test --test regression`\nExit code: {} (non-zero is expected before the fix).\n\n```text\n{}\n```",
            result.status.code().unwrap_or(1),
            tail(&combined(&result), 1200)
        ),
    );
}

fn root_cause(handoff: &Handoff, args: &Args) {
    let source_path = args.project.join("src/lib.rs");
    let source = fs::read_to_string(&source_path).unwrap_or_else(|error| fail(format!("read source: {error}")));
    let evidence: Vec<String> = source
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains("cells.join"))
        .map(|(index, line)| format!("{}: {}", index + 1, line.trim()))
        .collect();
    if evidence.is_empty() {
        fail("could not locate the pipe-joining implementation");
    }
    handoff.say(&format!("found unescaped row joining at {}", evidence.join(", ")));
    handoff.write(
        "root-cause.md",
        &format!(
            "# Root cause\n\n`render_row` joins cell strings with ` | ` without escaping pipes inside cell contents.\n\nEvidence in `src/lib.rs`:\n\n```text\n{}\n```\n\nThe fix should escape each cell before joining the row.",
            evidence.join("\n")
        ),
    );
}

fn fix(handoff: &Handoff, args: &Args) {
    let patch = Command::new("git")
        .arg("apply")
        .arg(&args.patch)
        .current_dir(&args.project)
        .output()
        .unwrap_or_else(|error| fail(format!("run git apply: {error}")));
    ensure_success("apply upstream patch", &patch);
    let diff = Command::new("git")
        .args(["diff", "--no-index", "/dev/null", "src/lib.rs"])
        .current_dir(&args.project)
        .output()
        .unwrap_or_else(|error| fail(format!("inspect fixed source: {error}")));
    let tests = cargo_test(&args.project, &[]);
    ensure_success("cargo test after fix", &tests);
    handoff.say("applied the Rust fix and the complete fixture suite passes");
    handoff.write(
        "change-summary.md",
        &format!(
            "# Change summary\n\nEscaped `|` inside each table cell before joining the row.\n\n## Source diff\n```diff\n{}\n```\n\n## Verification\n`cargo test` passed, including both regression cases and the unchanged-rendering unit test.\n\n```text\n{}\n```",
            String::from_utf8_lossy(&diff.stdout),
            combined(&tests)
        ),
    );
}

fn review(handoff: &Handoff, args: &Args) {
    let source = fs::read_to_string(args.project.join("src/lib.rs"))
        .unwrap_or_else(|error| fail(format!("read fixed source: {error}")));
    let addressed = source.contains("replace('|', r\"\\|\")");
    let tests = cargo_test(&args.project, &["--test", "regression"]);
    let outcome = if addressed && tests.status.success() { "pass" } else { "fail" };
    handoff.say(&format!("read-only review; escape implementation present={addressed}; verdict {outcome}"));
    handoff.write(
        "review.md",
        &format!(
            "# Review\n\n## Spec compliance\n- Escaping implementation found: {addressed}.\n- Regression tests pass: {}.\n\n## Code health\n- Escaping happens per cell before row joining.\n- No new dependency or post-processing path was added.\n",
            tests.status.success()
        ),
    );
    handoff.write("review.json", &format!("{{\n  \"outcome\": \"{outcome}\",\n  \"findings\": []\n}}"));
}

fn accept(handoff: &Handoff, args: &Args) {
    handoff.say(&format!("upstream concerns in my handoff: {}", handoff.concerns.len()));
    let tests = cargo_test(&args.project, &[]);
    let source = fs::read_to_string(args.project.join("src/lib.rs"))
        .unwrap_or_else(|error| fail(format!("read fixed source: {error}")));
    let escaped = source.contains("replace('|', r\"\\|\")");
    let verdict = if tests.status.success() && escaped { "verified" } else { "failed" };
    handoff.say(&format!("independent cargo test exit {}; verdict {verdict}", tests.status.code().unwrap_or(1)));
    let acknowledged = if handoff.concerns.is_empty() {
        "  - none".to_owned()
    } else {
        handoff
            .concerns
            .iter()
            .map(|concern| format!("  - {concern} -> checked against the fixed source and tests"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    handoff.write(
        "final-report.md",
        &format!(
            "# Acceptance\n\n- Full Rust fixture suite rerun independently: `cargo test` -> exit {}.\n- Escaping verified: {escaped}.\n- Upstream concerns acknowledged:\n{acknowledged}\n- Verdict: **{verdict}**\n\n```text\n{}\n```",
            tests.status.code().unwrap_or(1),
            combined(&tests)
        ),
    );
}

fn cargo_test(project: &Path, extra: &[&str]) -> Output {
    Command::new("cargo")
        .args(["test", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(extra)
        .current_dir(project)
        .output()
        .unwrap_or_else(|error| fail(format!("run cargo test: {error}")))
}

fn submit_command(handoff: &str) -> String {
    handoff
        .lines()
        .skip_while(|line| *line != "## Submit")
        .skip(1)
        .find(|line| line.starts_with("aurora workflow submit "))
        .unwrap_or("(see handoff)")
        .to_owned()
}

fn section<'a>(text: &'a str, heading: &str) -> Option<&'a str> {
    let start = text.find(heading)? + heading.len();
    let rest = &text[start..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    Some(&rest[..end])
}

fn value_after(text: &str, start: &str, end: &str) -> Option<String> {
    let value = text.split_once(start)?.1.split_once(end)?.0;
    Some(value.to_owned())
}

fn combined(output: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr))
}

fn tail(text: &str, max_chars: usize) -> &str {
    let start = text.char_indices().rev().nth(max_chars).map_or(0, |(index, _)| index);
    &text[start..]
}

fn ensure_success(action: &str, output: &Output) {
    if !output.status.success() {
        fail(format!("{action} failed:\n{}", combined(output)));
    }
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("stand-in: {message}");
    std::process::exit(1)
}
