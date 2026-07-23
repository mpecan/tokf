use tokf_common::config::types::FilterConfig;
use tokf_common::test_case::{Expectation, TestCase};

use crate::CommandResult;
use crate::determinism;
use crate::filter::{self, FilterOptions, FilterResult};

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

/// Enforce a case's declared `min_richness`, if any.
///
/// Deliberately a no-op when the case declares no threshold: richness is an
/// opt-in per-case assertion, never a global gate. tokf is deliberately lossy
/// and a near-zero score is frequently correct.
fn check_richness(case: &TestCase, raw: &str, filtered: &str, failures: &mut Vec<String>) {
    if case.min_richness.is_none() {
        return;
    }
    let richness = tokf_common::richness::score(raw, filtered);
    if let Some(msg) = tokf_common::richness::check_min_richness(case.min_richness, richness) {
        failures.push(msg);
    }
}

/// Run a single in-memory test case, applying `apply_fn` to the fixture.
///
/// Shared body of [`run_case_in_memory`] (non-sandboxed) and
/// [`run_case_in_memory_sandboxed`], parameterised only by how the filter is
/// applied. Returns a failing result when `inline` is `None` (fixture-based
/// cases cannot run in-memory).
///
/// # Determinism check
///
/// `apply_fn` is invoked **twice** over the identical input and the two
/// outputs are compared byte-for-byte via [`determinism::check`]; a divergence
/// is reported as a failure with the same message shape as `tokf verify`. This
/// runs on both the non-sandboxed and sandboxed paths — since they now share
/// this one code path there is nothing to diverge, and it keeps the
/// non-sandboxed `verify_filter` honest for any caller too.
///
/// Cost: this doubles filter execution per case. On the sandboxed/server path
/// the second run is bounded by the same [`filter::lua::SandboxLimits`]
/// (`instruction_limit`/`memory_limit`) as the first, so worst-case CPU per
/// publish doubles rather than becoming unbounded.
///
/// Known limitation (see `docs/writing-filters.md#determinism`): a same-process
/// double run cannot catch nondeterminism that varies *across* processes (e.g.
/// Rust's per-process `HashMap` seed). The mitigation there — avoid unordered
/// iteration reaching output; use `BTreeMap`/explicit ordering — is the real
/// defence for that class of drift.
fn run_case_generic(
    config: &FilterConfig,
    case: &TestCase,
    apply_fn: impl Fn(&CommandResult) -> FilterResult,
) -> CaseResult {
    let Some(inline) = case.inline.as_deref() else {
        return CaseResult {
            name: case.name.clone(),
            passed: false,
            failures: vec![
                "test case has no 'inline' data (fixture-based cases cannot run in-memory)"
                    .to_string(),
            ],
        };
    };
    let fixture = inline.trim_end().to_string();

    let cmd_result = CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: case.exit_code,
        combined: fixture,
    };

    let filtered = apply_fn(&cmd_result);
    let filtered_again = apply_fn(&cmd_result);

    let mut failures = Vec::new();
    let filter_name = config.command.first();
    if let Some(msg) = determinism::check(filter_name, &filtered.output, &filtered_again.output) {
        failures.push(msg);
    }
    for expect in &case.expects {
        if let Some(msg) = evaluate(expect, &filtered.output) {
            failures.push(msg);
        }
    }
    check_richness(case, &cmd_result.combined, &filtered.output, &mut failures);

    let passed = failures.is_empty();
    CaseResult {
        name: case.name.clone(),
        passed,
        failures,
    }
}

/// Run a single test case against a filter configuration (in-memory).
///
/// This is the core verification function used by both CLI (`tokf verify`)
/// and server-side publish validation. Returns a failing result when
/// `inline` is `None` (fixture-based cases cannot run in-memory).
pub fn run_case_in_memory(config: &FilterConfig, case: &TestCase) -> CaseResult {
    run_case_generic(config, case, |cmd_result| {
        filter::apply(config, cmd_result, &case.args, &FilterOptions::default())
    })
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
/// enforce instruction-count and memory limits on Lua scripts. Returns a failing
/// result when `inline` is `None`.
#[cfg(feature = "lua")]
pub fn run_case_in_memory_sandboxed(
    config: &FilterConfig,
    case: &TestCase,
    lua_limits: &filter::lua::SandboxLimits,
) -> CaseResult {
    run_case_generic(config, case, |cmd_result| {
        filter::apply_sandboxed(
            config,
            cmd_result,
            &case.args,
            &FilterOptions::default(),
            lua_limits,
        )
    })
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
            min_richness: None,
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
    fn run_case_in_memory_rejects_missing_inline() {
        let config = make_config(r#"command = "test""#);
        let case = TestCase {
            name: "no-inline".to_string(),
            fixture: Some("some_fixture.txt".to_string()),
            inline: None,
            exit_code: 0,
            args: vec![],
            min_richness: None,
            expects: vec![expect_equals("")],
        };
        let result = run_case_in_memory(&config, &case);
        assert!(!result.passed);
        assert!(result.failures[0].contains("no 'inline' data"));
    }

    fn lossy_config() -> FilterConfig {
        make_config(
            r#"
command = "test"
skip = ["."]
"#,
        )
    }

    const RICH_INPUT: &str = "Compiling tokf-common v0.1.0\n\
        thread 'main' panicked at src/lib/module.rs:42:9\n\
        assertion `left == right` failed";

    #[test]
    fn min_richness_failure_is_reported() {
        let mut case = make_case("lossy", RICH_INPUT, 0, vec![]);
        case.min_richness = Some(0.9);
        let result = run_case_in_memory(&lossy_config(), &case);
        assert!(!result.passed);
        assert!(
            result.failures.iter().any(|f| f.contains("min_richness")),
            "expected min_richness failure, got: {:?}",
            result.failures
        );
    }

    #[test]
    fn min_richness_satisfied_passes() {
        let mut case = make_case("passthrough", RICH_INPUT, 0, vec![]);
        case.min_richness = Some(0.9);
        let result = run_case_in_memory(&make_config(r#"command = "test""#), &case);
        assert!(result.passed, "failures: {:?}", result.failures);
    }

    #[test]
    fn absent_min_richness_never_fails_on_lossiness() {
        // Anti-global-gate regression test: tokf is deliberately lossy, so a
        // case that declares no threshold must never fail on richness grounds.
        let case = make_case("lossy", RICH_INPUT, 0, vec![]);
        assert!(case.min_richness.is_none());
        let result = run_case_in_memory(&lossy_config(), &case);
        assert!(result.passed, "failures: {:?}", result.failures);
    }

    #[cfg(feature = "lua")]
    #[test]
    fn sandboxed_min_richness_failure_is_reported() {
        let limits = filter::lua::SandboxLimits::default();
        let mut case = make_case("lossy", RICH_INPUT, 0, vec![]);
        case.min_richness = Some(0.9);
        let result = run_case_in_memory_sandboxed(&lossy_config(), &case, &limits);
        assert!(!result.passed);
        assert!(result.failures.iter().any(|f| f.contains("min_richness")));
    }

    #[cfg(feature = "lua")]
    #[test]
    fn sandboxed_absent_min_richness_never_fails_on_lossiness() {
        let limits = filter::lua::SandboxLimits::default();
        let case = make_case("lossy", RICH_INPUT, 0, vec![]);
        let result = run_case_in_memory_sandboxed(&lossy_config(), &case, &limits);
        assert!(result.passed, "failures: {:?}", result.failures);
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

    fn expect_matches(pattern: &str) -> Expectation {
        Expectation {
            contains: None,
            not_contains: None,
            equals: None,
            starts_with: None,
            ends_with: None,
            line_count: None,
            matches: Some(pattern.to_string()),
            not_matches: None,
        }
    }

    #[cfg(feature = "lua")]
    const NONDETERMINISTIC_LUA: &str = r#"
command = "test"

[lua_script]
lang = "luau"
source = "return tostring(math.random(1, 1000000000))"
"#;

    #[cfg(feature = "lua")]
    #[test]
    fn run_case_in_memory_sandboxed_rejects_nondeterministic_lua_filter() {
        let limits = filter::lua::SandboxLimits::default();
        let config = make_config(NONDETERMINISTIC_LUA);
        // The expect passes trivially — the failure must come from the
        // byte-stability check, not the assertion.
        let case = make_case("random", "input", 0, vec![expect_matches(r"^\d+$")]);
        let result = run_case_in_memory_sandboxed(&config, &case, &limits);
        assert!(!result.passed, "nondeterministic filter should fail");
        assert!(
            result.failures.iter().any(|f| f.contains("byte-stable")),
            "expected a byte-stability failure, got: {:?}",
            result.failures
        );
    }

    #[cfg(feature = "lua")]
    #[test]
    fn run_case_in_memory_sandboxed_deterministic_filter_still_passes() {
        let limits = filter::lua::SandboxLimits::default();
        let config = make_config(
            r#"
command = "test"

[lua_script]
lang = "luau"
source = 'return "OK"'
"#,
        );
        let case = make_case("stable", "input", 0, vec![expect_equals("OK")]);
        let result = run_case_in_memory_sandboxed(&config, &case, &limits);
        assert!(result.passed, "failures: {:?}", result.failures);
    }

    #[cfg(feature = "lua")]
    #[test]
    fn run_case_in_memory_rejects_nondeterministic_lua_filter() {
        // The non-sandboxed twin: step 2 applies the double-run check here too.
        let config = make_config(NONDETERMINISTIC_LUA);
        let case = make_case("random", "input", 0, vec![expect_matches(r"^\d+$")]);
        let result = run_case_in_memory(&config, &case);
        assert!(!result.passed, "nondeterministic filter should fail");
        assert!(
            result.failures.iter().any(|f| f.contains("byte-stable")),
            "expected a byte-stability failure, got: {:?}",
            result.failures
        );
    }
}
