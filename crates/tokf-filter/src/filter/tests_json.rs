use tokf_common::config::types::{
    ChunkConfig, CommandPattern, FilterConfig, JsonConfig, JsonExtractRule, JsonFieldExtract,
    OutputBranch, Section,
};

use crate::CommandResult;

use super::{FilterOptions, apply};

fn default_config() -> FilterConfig {
    FilterConfig {
        command: CommandPattern::Single("test".to_string()),
        run: None,
        skip: vec![],
        keep: vec![],
        step: vec![],
        extract: None,
        match_output: vec![],
        section: vec![],
        on_success: None,
        on_failure: None,
        parse: None,
        tree: None,
        output: None,
        fallback: None,
        replace: vec![],
        dedup: false,
        dedup_window: None,
        strip_ansi: false,
        trim_lines: false,
        strip_empty_lines: false,
        collapse_empty_lines: false,
        lua_script: None,
        chunk: vec![],
        json: None,
        variant: vec![],
        show_history_hint: false,
        inject_path: false,
        passthrough_args: vec![],
        description: None,
        truncate_lines_at: None,
        on_empty: None,
        head: None,
        tail: None,
        max_lines: None,
    }
}

#[test]
fn json_scalar_in_template() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.version".to_string(),
            as_name: "ver".to_string(),
            fields: vec![],
        }],
    });
    config.on_success = Some(OutputBranch {
        output: Some("Version: {ver}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: r#"{"version": "1.2.3"}"#.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(result.output, "Version: 1.2.3");
}

#[test]
fn json_array_with_each_pipe() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.items[*]".to_string(),
            as_name: "pods".to_string(),
            fields: vec![
                JsonFieldExtract {
                    field: "name".to_string(),
                    as_name: "name".to_string(),
                },
                JsonFieldExtract {
                    field: "status".to_string(),
                    as_name: "status".to_string(),
                },
            ],
        }],
    });
    config.on_success = Some(OutputBranch {
        output: Some(
            "Pods ({pods_count}):\n{pods | each: \"  {name}: {status}\" | join: \"\\n\"}"
                .to_string(),
        ),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let json_input = r#"{"items": [
        {"name": "web-1", "status": "Running"},
        {"name": "db-1", "status": "Pending"}
    ]}"#;

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: json_input.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(
        result.output,
        "Pods (2):\n  web-1: Running\n  db-1: Pending"
    );
}

#[test]
fn json_skips_parse_when_configured() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.msg".to_string(),
            as_name: "msg".to_string(),
            fields: vec![],
        }],
    });
    // Parse would normally intercept, but JSON should cause it to be skipped.
    config.parse = Some(tokf_common::config::types::ParseConfig {
        branch: None,
        group: None,
    });
    config.on_success = Some(OutputBranch {
        output: Some("{msg}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: r#"{"msg": "hello"}"#.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(result.output, "hello");
}

#[test]
fn json_invalid_input_falls_through() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.foo".to_string(),
            as_name: "foo".to_string(),
            fields: vec![],
        }],
    });
    config.on_success = Some(OutputBranch {
        output: Some("got: {foo}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: "not json".to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    // JSON parsing failed → fallback to raw output
    assert_eq!(result.output, "not json");
}

#[test]
fn json_on_failure_branch() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.error".to_string(),
            as_name: "err".to_string(),
            fields: vec![],
        }],
    });
    config.on_failure = Some(OutputBranch {
        output: Some("Error: {err}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 1,
            combined: r#"{"error": "not found"}"#.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(result.output, "Error: not found");
}

#[test]
fn json_skips_sections_when_configured() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.msg".to_string(),
            as_name: "msg".to_string(),
            fields: vec![],
        }],
    });
    // Sections would normally collect lines, but JSON should cause them to be skipped.
    config.section = vec![Section {
        name: Some("errors".to_string()),
        enter: Some("^ERROR".to_string()),
        exit: Some("^$".to_string()),
        match_pattern: None,
        split_on: None,
        collect_as: Some("errors".to_string()),
    }];
    config.on_success = Some(OutputBranch {
        output: Some("{msg}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: r#"{"msg": "json wins"}"#.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(result.output, "json wins");
}

#[test]
fn json_skips_chunks_when_configured() {
    let mut config = default_config();
    config.json = Some(JsonConfig {
        extract: vec![JsonExtractRule {
            path: "$.status".to_string(),
            as_name: "status".to_string(),
            fields: vec![],
        }],
    });
    // Chunks would normally split output, but JSON should cause them to be skipped.
    config.chunk = vec![ChunkConfig {
        split_on: "^---".to_string(),
        include_split_line: true,
        collect_as: "blocks".to_string(),
        extract: None,
        body_extract: vec![],
        aggregate: vec![],
        group_by: None,
        children_as: None,
    }];
    config.on_success = Some(OutputBranch {
        output: Some("Status: {status}".to_string()),
        aggregate: None,
        tail: None,
        head: None,
        skip: vec![],
        extract: None,
        aggregates: vec![],
    });

    let result = apply(
        &config,
        &CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            combined: r#"{"status": "ok"}"#.to_string(),
        },
        &[],
        &FilterOptions::default(),
    );

    assert_eq!(result.output, "Status: ok");
}
