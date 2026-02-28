use tokf_common::config::types::FilterConfig;
use tokf_common::test_case::TestCase;
use tokf_filter::filter::lua::SandboxLimits;
use tokf_filter::verify::{VerifyResult, verify_filter_sandboxed};

/// Validate that a filter config is safe for server-side execution.
///
/// Rejects filters with `lua_script.file` — only inline `source` is supported.
fn validate_filter_for_server(config: &FilterConfig) -> Result<(), String> {
    if let Some(ref script) = config.lua_script
        && script.file.is_some()
    {
        return Err(
            "lua_script.file is not supported for published filters; use inline 'source' instead"
                .to_string(),
        );
    }
    Ok(())
}

/// Validate that all test cases use inline data (no fixture file references).
fn validate_cases_for_server(cases: &[TestCase]) -> Result<(), String> {
    for case in cases {
        if case.fixture.is_some() {
            return Err(format!(
                "test case '{}': 'fixture' is not supported for published filters; use 'inline' instead",
                case.name
            ));
        }
    }
    Ok(())
}

/// Run server-side filter verification with sandboxed Lua execution.
///
/// Validates that the filter and test cases are safe for server-side execution,
/// then runs each test case against the filter with resource limits.
///
/// # Errors
///
/// Returns `Err` if:
/// - The filter uses `lua_script.file` (only inline `source` supported)
/// - Any test case uses `fixture` (only `inline` supported)
pub fn verify_filter_server(
    config: &FilterConfig,
    cases: &[TestCase],
) -> Result<VerifyResult, String> {
    validate_filter_for_server(config)?;
    validate_cases_for_server(cases)?;

    let limits = SandboxLimits::default();
    Ok(verify_filter_sandboxed(config, cases, &limits))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tokf_common::test_case::Expectation;

    fn make_config(toml_str: &str) -> FilterConfig {
        toml::from_str(toml_str).unwrap()
    }

    fn make_case(name: &str, inline: &str, expects: Vec<Expectation>) -> TestCase {
        TestCase {
            name: name.to_string(),
            fixture: None,
            inline: Some(inline.to_string()),
            exit_code: 0,
            args: vec![],
            expects,
        }
    }

    fn expect_contains(s: &str) -> Expectation {
        Expectation {
            contains: Some(s.to_string()),
            not_contains: None,
            equals: None,
            starts_with: None,
            ends_with: None,
            line_count: None,
            matches: None,
            not_matches: None,
        }
    }

    fn expect_equals(s: &str) -> Expectation {
        Expectation {
            contains: None,
            not_contains: None,
            equals: Some(s.to_string()),
            starts_with: None,
            ends_with: None,
            line_count: None,
            matches: None,
            not_matches: None,
        }
    }

    #[test]
    fn server_verify_passes_with_correct_expectations() {
        let config = make_config(
            r#"
command = "test"
skip = ["^noise"]
"#,
        );
        let cases = vec![make_case(
            "basic",
            "noise line\nkeep this",
            vec![expect_contains("keep this")],
        )];
        let result = verify_filter_server(&config, &cases).unwrap();
        assert!(result.all_passed());
    }

    #[test]
    fn server_verify_fails_with_wrong_expectations() {
        let config = make_config(r#"command = "test""#);
        let cases = vec![make_case(
            "fail",
            "hello world",
            vec![expect_contains("not present")],
        )];
        let result = verify_filter_server(&config, &cases).unwrap();
        assert!(!result.all_passed());
        assert!(!result.cases[0].failures.is_empty());
    }

    #[test]
    fn server_verify_rejects_fixture_references() {
        let config = make_config(r#"command = "test""#);
        let cases = vec![TestCase {
            name: "fixture_case".to_string(),
            fixture: Some("output.txt".to_string()),
            inline: None,
            exit_code: 0,
            args: vec![],
            expects: vec![expect_contains("x")],
        }];
        let err = verify_filter_server(&config, &cases).unwrap_err();
        assert!(
            err.contains("fixture"),
            "expected fixture rejection, got: {err}"
        );
    }

    #[test]
    fn server_verify_rejects_lua_file_reference() {
        let config = make_config(
            r#"
command = "test"

[lua_script]
lang = "luau"
file = "/some/path/script.luau"
"#,
        );
        let cases = vec![make_case("basic", "hello", vec![expect_equals("hello")])];
        let err = verify_filter_server(&config, &cases).unwrap_err();
        assert!(
            err.contains("lua_script.file"),
            "expected lua_script.file rejection, got: {err}"
        );
    }

    #[test]
    fn server_verify_allows_inline_lua_source() {
        let config = make_config(
            r#"
command = "test"

[lua_script]
lang = "luau"
source = 'return "filtered"'
"#,
        );
        let cases = vec![make_case("lua", "input", vec![expect_equals("filtered")])];
        let result = verify_filter_server(&config, &cases).unwrap();
        assert!(result.all_passed());
    }

    #[test]
    fn server_verify_multiple_cases_mixed_results() {
        let config = make_config(r#"command = "test""#);
        let cases = vec![
            make_case("pass", "hello", vec![expect_equals("hello")]),
            make_case("fail", "hello", vec![expect_equals("world")]),
        ];
        let result = verify_filter_server(&config, &cases).unwrap();
        assert!(!result.all_passed());
        assert!(result.cases[0].passed);
        assert!(!result.cases[1].passed);
    }

    #[test]
    fn server_verify_exit_code_branch() {
        let config = make_config(
            r#"
command = "test"
[on_failure]
output = "FAILED"
"#,
        );
        let cases = vec![TestCase {
            name: "failure branch".to_string(),
            fixture: None,
            inline: Some(String::new()),
            exit_code: 1,
            args: vec![],
            expects: vec![expect_equals("FAILED")],
        }];
        let result = verify_filter_server(&config, &cases).unwrap();
        assert!(result.all_passed());
    }
}
