use super::*;
use tokf_common::config::types::{AggregateRule, ExtractRule};

fn minimal_config() -> FilterConfig {
    toml::from_str(r#"command = "test""#).unwrap()
}

// --- select_branch ---

#[test]
fn select_branch_success() {
    let mut config = minimal_config();
    config.on_success = Some(OutputBranch {
        output: Some("success".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });
    assert!(select_branch(&config, 0).is_some());
    assert!(select_branch(&config, 1).is_none());
}

#[test]
fn select_branch_failure() {
    let mut config = minimal_config();
    config.on_failure = Some(OutputBranch {
        output: Some("failure".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });
    assert!(select_branch(&config, 0).is_none());
    assert!(select_branch(&config, 1).is_some());
    assert!(select_branch(&config, 127).is_some());
}

// --- apply_branch ---

/// Helper: call `apply_branch` with empty sections and chunks (non-section path).
fn branch_apply(branch: &OutputBranch, combined: &str) -> String {
    apply_branch(
        branch,
        combined,
        &SectionMap::new(),
        &template::ChunkMap::new(),
        false,
    )
    .unwrap()
}

#[test]
fn branch_fixed_output() {
    let branch = OutputBranch {
        output: Some("ok \u{2713}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "anything"), "ok \u{2713}");
}

#[test]
fn branch_output_template_resolves_output_var() {
    let branch = OutputBranch {
        output: Some("{output}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "hello world"), "hello world");
}

#[test]
fn branch_output_template_with_surrounding_text() {
    let branch = OutputBranch {
        output: Some("Result: {output}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(
        branch_apply(&branch, "line1\nline2"),
        "Result: line1\nline2"
    );
}

#[test]
fn branch_tail_truncation() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(2),
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "c\nd");
}

#[test]
fn branch_head_truncation() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: Some(2),
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "a\nb");
}

#[test]
fn branch_tail_then_head() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(3),
        head: Some(2),
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    // tail 3 of [a,b,c,d] → [b,c,d], then head 2 → [b,c]
    assert_eq!(branch_apply(&branch, "a\nb\nc\nd"), "b\nc");
}

#[test]
fn branch_skip_then_join() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec!["^noise".to_string()],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(
        branch_apply(&branch, "noise line\nkeep me\nnoise again"),
        "keep me"
    );
}

#[test]
fn branch_extract() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: Some(ExtractRule {
            pattern: r"(\S+)\s*->\s*(\S+)".to_string(),
            output: "ok {2}".to_string(),
        }),
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "main -> main"), "ok main");
}

#[test]
fn branch_tail_less_than_lines() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(10),
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    // Only 3 lines, tail 10 → all lines kept
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "a\nb\nc");
}

#[test]
fn branch_empty_string_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, ""), "");
}

#[test]
fn branch_single_line_no_newline() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "only-line"), "only-line");
}

#[test]
fn branch_tail_zero_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: Some(0),
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "");
}

#[test]
fn branch_head_zero_returns_empty() {
    let branch = OutputBranch {
        output: None,
        aggregate: None,
        tail: None,
        head: Some(0),
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    assert_eq!(branch_apply(&branch, "a\nb\nc"), "");
}

// --- apply_branch fallback when has_sections=true ---

#[test]
fn branch_with_sections_expected_but_empty_returns_none() {
    // has_sections=true, but SectionMap has empty data → returns None (triggers fallback)
    let mut sections = SectionMap::new();
    sections.insert(
        "summary_lines".to_string(),
        section::SectionData {
            lines: vec![],
            blocks: vec![],
        },
    );
    let branch = OutputBranch {
        output: Some("{passed} passed ({suites} suites)".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![AggregateRule {
            from: "summary_lines".to_string(),
            pattern: r"ok\. (\d+) passed".to_string(),
            sum: Some("passed".to_string()),
            count_as: Some("suites".to_string()),
        }],
    };
    let result = apply_branch(
        &branch,
        "irrelevant",
        &sections,
        &template::ChunkMap::new(),
        true,
    );
    assert!(result.is_none(), "empty sections should trigger fallback");
}

#[test]
fn branch_with_sections_populated_renders_template() {
    let mut sections = SectionMap::new();
    sections.insert(
        "summary_lines".to_string(),
        section::SectionData {
            lines: vec![
                "test result: ok. 12 passed; 0 failed".to_string(),
                "test result: ok. 8 passed; 0 failed".to_string(),
            ],
            blocks: vec![],
        },
    );
    let branch = OutputBranch {
        output: Some("{passed} passed ({suites} suites)".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![AggregateRule {
            from: "summary_lines".to_string(),
            pattern: r"ok\. (\d+) passed".to_string(),
            sum: Some("passed".to_string()),
            count_as: Some("suites".to_string()),
        }],
    };
    let result = apply_branch(
        &branch,
        "irrelevant",
        &sections,
        &template::ChunkMap::new(),
        true,
    );
    assert_eq!(result.unwrap(), "20 passed (2 suites)");
}

#[test]
fn branch_without_sections_ignores_has_sections_flag() {
    // has_sections=false, empty SectionMap → should NOT trigger fallback
    let branch = OutputBranch {
        output: Some("ok".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    };
    let result = apply_branch(
        &branch,
        "anything",
        &SectionMap::new(),
        &template::ChunkMap::new(),
        false,
    );
    assert_eq!(result.unwrap(), "ok");
}
