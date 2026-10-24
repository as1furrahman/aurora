//! Built-in workflow presets (R1 Step 6.3), embedded from `workflows/presets/*.toml`.
//!
//! The stages follow the methodology research recorded in `docs/references/prior-art.md` §4:
//! - testable acceptance criteria with no open `[NEEDS CLARIFICATION]` markers (Spec Kit / Kiro);
//! - red→green implementation evidence (TDD);
//! - two-pass review, spec compliance then code health, with a structured verdict (Superpowers / BMAD);
//! - "unchanged behaviour" for bug fixes (Kiro);
//! - go / needs-clarification / kill spikes (Stage-Gate).
//!
//! Review and verification default to a provider distinct from implementation. Any CLI may
//! still play any role through `--provider` / `--route`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    /// Same text as the definition's `workflow.description`.
    pub summary: &'static str,
    pub toml: &'static str,
}

macro_rules! preset {
    ($name:literal, $summary:literal) => {
        Preset {
            name: $name,
            summary: $summary,
            toml: include_str!(concat!("../../../workflows/presets/", $name, ".toml")),
        }
    };
}

pub const PRESETS: [Preset; 6] = [
    preset!("feature", "Specify, design, build test-first, verify, review independently, document, and accept."),
    preset!(
        "bugfix",
        "Reproduce first, fix the root cause, prove unchanged behaviour still holds, review, and accept."
    ),
    preset!(
        "refactor",
        "Pin behaviour with characterization tests, change structure in small steps, and prove nothing else changed."
    ),
    preset!("docs", "Outline, draft, verify every claim and sample against the code, and review."),
    preset!(
        "security-review",
        "Scope and threat-model, assess with severity-labelled findings, and accept only with no unwaived critical or high findings."
    ),
    preset!(
        "spike",
        "Time-boxed investigation that ends in a go, needs-clarification or kill decision instead of speculative code."
    ),
];

pub fn get_preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}
