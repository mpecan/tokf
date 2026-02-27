use tokf_common::config::types::FilterConfig;
use tokf_common::test_case::{Expectation, TestCase};

use crate::CommandResult;
use crate::filter::{self, FilterOptions};

/// Result of a single test case execution.
#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub passed: bool,
    pub failures: Vec<String>,
}

/// Result of verifying a filter against a suite of test cases.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub cases: Vec<CaseResult>,
}

impl VerifyResult {
    /// Returns true if all cases passed.
    pub fn all_passed(&self) -> bool {
        self.cases.iter().all(|c| c.passed)
    }
}

/// Run a single test case against a filter configuration (in-memory).
///
/// This is the core verification function used by both CLI (`tokf verify`)
/// and server-side publish validation.
pub fn run_case_in_memory(config: &FilterConfig, case: &TestCase) -> CaseResult {
    let inline = case.inline.as_deref().unwrap_or("");
    let fixture = inline.trim_end().to_string();

    let cmd_result = CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: case.exit_code,
        combined: fixture,
    };

    let filtered = filter::apply(config, &cmd_result, &case.args, &FilterOptions::default());

    let mut failures = Vec::new();
    for expect in &case.expects {
        if let Some(msg) = evaluate(expect, &filtered.output) {
            failures.push(msg);
        }
    }

    let passed = failures.is_empty();
    CaseResult {
        name: case.name.clone(),
        passed,
        failures,
    }
}

/// Verify a filter against a set of test cases (all in-memory, no filesystem access).
pub fn verify_filter(config: &FilterConfig, cases: &[TestCase]) -> VerifyResult {
    let cases = cases
        .iter()
        .map(|case| run_case_in_memory(config, case))
        .collect();
    VerifyResult { cases }
}

/// Run a single test case with sandboxed Lua execution (for server-side use).
///
/// Identical to [`run_case_in_memory`] but uses [`filter::apply_sandboxed`] to
/// enforce instruction-count and memory limits on Lua scripts.
#[cfg(feature = "lua")]
pub fn run_case_in_memory_sandboxed(
    config: &FilterConfig,
    case: &TestCase,
    lua_limits: &filter::lua::SandboxLimits,
) -> CaseResult {
    let inline = case.inline.as_deref().unwrap_or("");
    let fixture = inline.trim_end().to_string();

    let cmd_result = CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: case.exit_code,
        combined: fixture,
    };

    let filtered = filter::apply_sandboxed(
        config,
        &cmd_result,
        &case.args,
        &FilterOptions::default(),
        lua_limits,
    );

    let mut failures = Vec::new();
    for expect in &case.expects {
        if let Some(msg) = evaluate(expect, &filtered.output) {
            failures.push(msg);
        }
    }

    let passed = failures.is_empty();
    CaseResult {
        name: case.name.clone(),
        passed,
        failures,
    }
}

/// Verify a filter against test cases with sandboxed Lua execution.
///
/// Server-side variant of [`verify_filter`] that enforces resource limits.
#[cfg(feature = "lua")]
pub fn verify_filter_sandboxed(
    config: &FilterConfig,
    cases: &[TestCase],
    lua_limits: &filter::lua::SandboxLimits,
) -> VerifyResult {
    let cases = cases
        .iter()
        .map(|case| run_case_in_memory_sandboxed(config, case, lua_limits))
        .collect();
    VerifyResult { cases }
}

/// Evaluate a single expectation against filtered output.
///
/// Returns `None` if the assertion passes, or `Some(error_message)` if it fails.
// This function handles all 8 assertion types in a single pass. The length is
// justified by the straightforward pattern repetition; splitting would obscure
// the symmetry between assertion kinds.
#[allow(clippy::too_many_lines)]
pub fn evaluate(expect: &Expectation, output: &str) -> Option<String> {
    if let Some(s) = &expect.contains
        && !output.contains(s.as_str())
    {
        return Some(format!("expected output to contain {s:?}\ngot:\n{output}"));
    }
    if let Some(s) = &expect.not_contains
        && output.contains(s.as_str())
    {
        return Some(format!(
            "expected output NOT to contain {s:?}\ngot:\n{output}"
        ));
    }
    if let Some(s) = &expect.equals
        && output != s.as_str()
    {
        return Some(format!("expected output to equal {s:?}\ngot:\n{output}"));
    }
    if let Some(s) = &expect.starts_with
        && !output.starts_with(s.as_str())
    {
        return Some(format!(
            "expected output to start with {s:?}\ngot:\n{output}"
        ));
    }
    if let Some(s) = &expect.ends_with
        && !output.ends_with(s.as_str())
    {
        return Some(format!("expected output to end with {s:?}\ngot:\n{output}"));
    }
    if let Some(n) = expect.line_count {
        let count = output.lines().filter(|l| !l.trim().is_empty()).count();
        if count != n {
            return Some(format!(
                "expected {n} non-empty lines, got {count}\noutput:\n{output}"
            ));
        }
    }
    if let Some(pattern) = &expect.matches {
        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return Some(format!("invalid regex {pattern:?}: {e}")),
        };
        if !re.is_match(output) {
            return Some(format!(
                "expected output to match regex {pattern:?}\ngot:\n{output}"
            ));
        }
    }
    if let Some(pattern) = &expect.not_matches {
        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(e) => return Some(format!("invalid regex {pattern:?}: {e}")),
        };
        if re.is_match(output) {
            return Some(format!(
                "expected output NOT to match regex {pattern:?}\ngot:\n{output}"
            ));
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_config(toml_str: &str) -> FilterConfig {
        toml::from_str(toml_str).unwrap()
    }

    fn make_case(name: &str, inline: &str, exit_code: i32, expects: Vec<Expectation>) -> TestCase {
        TestCase {
            name: name.to_string(),
            fixture: None,
            inline: Some(inline.to_string()),
            exit_code,
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
    fn verify_filter_passes_with_matching_expectations() {
        let config = make_config(
            r#"
command = "test"
skip = ["^noise"]
"#,
        );
        let case = make_case(
            "basic",
            "noise line\nkeep this",
            0,
            vec![expect_contains("keep this")],
        );
        let result = verify_filter(&config, &[case]);
        assert!(result.all_passed());
        assert_eq!(result.cases.len(), 1);
        assert!(result.cases[0].passed);
    }

    #[test]
    fn verify_filter_fails_with_wrong_expectation() {
        let config = make_config(r#"command = "test""#);
        let case = make_case(
            "fail",
            "hello world",
            0,
            vec![expect_contains("not present")],
        );
        let result = verify_filter(&config, &[case]);
        assert!(!result.all_passed());
        assert!(!result.cases[0].passed);
        assert!(!result.cases[0].failures.is_empty());
    }

    #[test]
    fn verify_filter_multiple_cases() {
        let config = make_config(r#"command = "test""#);
        let cases = vec![
            make_case("pass", "hello", 0, vec![expect_equals("hello")]),
            make_case("fail", "hello", 0, vec![expect_equals("world")]),
        ];
        let result = verify_filter(&config, &cases);
        assert!(!result.all_passed());
        assert!(result.cases[0].passed);
        assert!(!result.cases[1].passed);
    }

    #[test]
    fn evaluate_contains_pass() {
        let e = expect_contains("hello");
        assert!(evaluate(&e, "hello world").is_none());
    }

    #[test]
    fn evaluate_contains_fail() {
        let e = expect_contains("missing");
        assert!(evaluate(&e, "hello world").is_some());
    }

    #[test]
    fn evaluate_equals_pass() {
        let e = expect_equals("exact");
        assert!(evaluate(&e, "exact").is_none());
    }

    #[test]
    fn evaluate_equals_fail() {
        let e = expect_equals("exact");
        assert!(evaluate(&e, "not exact").is_some());
    }

    #[test]
    fn run_case_in_memory_with_exit_code() {
        let config = make_config(
            r#"
command = "test"
[on_failure]
output = "FAILED"
"#,
        );
        let case = make_case("failure branch", "", 1, vec![expect_equals("FAILED")]);
        let result = run_case_in_memory(&config, &case);
        assert!(result.passed);
    }
}
