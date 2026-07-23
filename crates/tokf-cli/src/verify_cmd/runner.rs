use std::path::{Path, PathBuf};

use tokf::config;
use tokf::filter;
use tokf::runner::CommandResult;
use tokf_common::richness::{self, Richness};
use tokf_common::safety;

use tokf_common::examples::{self, ExamplesSafety, SafetyWarningDto};

use super::discovery::DiscoveredSuite;
use super::{CaseResult, SuiteResult, TestCase};
use tokf_filter::determinism;

// Delegate assertion evaluation to tokf-filter's verify module.
use tokf_filter::verify::evaluate;

// --- Fixture loading ---

fn read_fixture_file(path: &Path, fixture_name: &str) -> anyhow::Result<String> {
    let content = std::fs::read_to_string(path)?.trim_end().to_string();
    if content.is_empty() {
        anyhow::bail!(
            "fixture file is empty: {fixture_name}\n\
             Hint: use inline = \"\" to test empty/no-output scenarios"
        );
    }
    Ok(content)
}

fn load_fixture(case: &TestCase, case_path: &Path) -> anyhow::Result<String> {
    if let Some(inline) = &case.inline {
        // Inline TOML strings already handle escape sequences (TOML spec)
        return Ok(inline.trim_end().to_string());
    }

    if let Some(fixture) = &case.fixture {
        // Try relative to case file's parent directory first
        let case_dir = case_path.parent().unwrap_or_else(|| Path::new("."));
        let relative_to_case = case_dir.join(fixture);
        if relative_to_case.exists() {
            return read_fixture_file(&relative_to_case, fixture);
        }

        // Try relative to CWD
        let path = Path::new(fixture);
        if path.exists() {
            return read_fixture_file(path, fixture);
        }

        anyhow::bail!("fixture not found: {fixture}");
    }

    anyhow::bail!("test case must specify either 'fixture' or 'inline'")
}

// --- Suite execution ---

fn error_suite(name: &str, error: String) -> SuiteResult {
    SuiteResult {
        filter_name: name.to_string(),
        cases: vec![],
        error: Some(error),
        safety: None,
        total_input_tokens: 0,
        total_output_tokens: 0,
        overall_reduction_pct: 0.0,
    }
}

pub(super) fn run_suite(suite: &DiscoveredSuite, check_safety: bool) -> SuiteResult {
    let cfg = match config::try_load_filter(&suite.filter_path) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return error_suite(
                &suite.filter_name,
                format!("filter not found: {}", suite.filter_path.display()),
            );
        }
        Err(e) => return error_suite(&suite.filter_name, format!("{e:#}")),
    };

    // Validate match_output rules
    for (i, rule) in cfg.match_output.iter().enumerate() {
        if let Err(e) = rule.validate() {
            return error_suite(&suite.filter_name, format!("match_output[{i}]: {e}"));
        }
    }

    let mut case_files: Vec<PathBuf> = match std::fs::read_dir(&suite.suite_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect(),
        Err(e) => {
            return error_suite(&suite.filter_name, format!("cannot read suite dir: {e}"));
        }
    };
    case_files.sort();

    if case_files.is_empty() {
        return error_suite(
            &suite.filter_name,
            format!("suite directory is empty: {}", suite.suite_dir.display()),
        );
    }

    let cases: Vec<CaseResult> = case_files
        .iter()
        .map(|case_path| run_case(&cfg, &suite.filter_name, case_path))
        .collect();

    let total_input_tokens: usize = cases.iter().map(|c| c.input_tokens).sum();
    let total_output_tokens: usize = cases.iter().map(|c| c.output_tokens).sum();
    let overall_reduction_pct = examples::reduction_pct(total_input_tokens, total_output_tokens);

    let safety_result = if check_safety {
        Some(run_safety_checks(&cfg, &case_files))
    } else {
        None
    };

    SuiteResult {
        filter_name: suite.filter_name.clone(),
        cases,
        error: None,
        safety: safety_result,
        total_input_tokens,
        total_output_tokens,
        overall_reduction_pct,
    }
}

fn run_safety_checks(cfg: &config::types::FilterConfig, case_files: &[PathBuf]) -> ExamplesSafety {
    let mut reports = Vec::new();

    // Static config checks (templates, command patterns, etc.)
    reports.push(safety::check_config(cfg));

    // Output pair checks: run each inline test case and check the output
    for case_path in case_files {
        let Ok(case) = load_case(case_path) else {
            continue;
        };
        let Some(inline) = &case.inline else {
            continue;
        };
        let raw = inline.trim_end().to_string();
        let cmd_result = CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: case.exit_code,
            combined: raw.clone(),
        };
        let filtered = filter::apply(
            cfg,
            &cmd_result,
            &case.args,
            &filter::FilterOptions::default(),
        );
        reports.push(safety::check_output_pair(&raw, &filtered.output));
    }

    let merged = safety::merge_reports(reports);
    ExamplesSafety {
        passed: merged.passed,
        warnings: merged.warnings.iter().map(SafetyWarningDto::from).collect(),
    }
}

fn error_case(name: String, failure: String) -> CaseResult {
    CaseResult {
        name,
        passed: false,
        failures: vec![failure],
        input_lines: 0,
        output_lines: 0,
        input_tokens: 0,
        output_tokens: 0,
        reduction_pct: 0.0,
        // An errored case has no meaningful score; 1.0 keeps it from reading
        // as a richness failure.
        richness: Richness {
            atoms: 0,
            kept: 0,
            retained: 1.0,
        },
    }
}

/// Line/token/richness statistics for one raw/filtered pair.
struct CaseStats {
    input_lines: usize,
    output_lines: usize,
    input_tokens: usize,
    output_tokens: usize,
    reduction_pct: f64,
    richness: Richness,
}

fn case_stats(raw: &str, out: &str) -> CaseStats {
    let input_tokens = examples::estimate_tokens(raw);
    let output_tokens = examples::estimate_tokens(out);
    CaseStats {
        input_lines: raw.lines().count(),
        output_lines: out.lines().count(),
        input_tokens,
        output_tokens,
        reduction_pct: examples::reduction_pct(input_tokens, output_tokens),
        richness: richness::score(raw, out),
    }
}

fn run_case(
    cfg: &tokf::config::types::FilterConfig,
    filter_name: &str,
    case_path: &Path,
) -> CaseResult {
    let case = match load_case(case_path) {
        Ok(c) => c,
        Err(e) => {
            let name = case_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            return error_case(name, format!("failed to load case: {e:#}"));
        }
    };

    let fixture = match load_fixture(&case, case_path) {
        Ok(f) => f,
        Err(e) => return error_case(case.name, format!("failed to load fixture: {e:#}")),
    };

    let cmd_result = CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: case.exit_code,
        combined: fixture,
    };

    let (output, mut failures) = apply_and_check(cfg, filter_name, &cmd_result, &case);

    // Scored once against the first run's output — the determinism check
    // above guarantees the second run is byte-identical, so either will do.
    let stats = case_stats(&cmd_result.combined, &output);

    for expect in &case.expects {
        if let Some(msg) = evaluate(expect, &output) {
            failures.push(msg);
        }
    }
    // Opt-in only: a case that declares no min_richness never fails on richness.
    if let Some(msg) = richness::check_min_richness(case.min_richness, stats.richness) {
        failures.push(msg);
    }

    let passed = failures.is_empty();
    CaseResult {
        name: case.name,
        passed,
        failures,
        input_lines: stats.input_lines,
        output_lines: stats.output_lines,
        input_tokens: stats.input_tokens,
        output_tokens: stats.output_tokens,
        reduction_pct: stats.reduction_pct,
        richness: stats.richness,
    }
}

/// Run the filter pipeline for a case, plus a second independent run over
/// the same input to check determinism.
///
/// Returns the (first run's) output and any determinism failure — the
/// `[[expect]]` assertions are evaluated separately by the caller so this
/// function stays focused on one concern.
fn apply_and_check(
    cfg: &tokf::config::types::FilterConfig,
    filter_name: &str,
    cmd_result: &CommandResult,
    case: &TestCase,
) -> (String, Vec<String>) {
    let options = filter::FilterOptions::default();
    let filtered = filter::apply(cfg, cmd_result, &case.args, &options);

    // Determinism check: a filter must be a pure function of its input. Run
    // the pipeline a second, fully independent time against the exact same
    // fixture and assert the output is byte-identical. This is not
    // opt-in — see docs/writing-filters.md#determinism for why output that
    // varies between runs is a correctness bug, not a preference (it
    // silently defeats prompt caching on every later turn).
    let filtered_again = filter::apply(cfg, cmd_result, &case.args, &options);

    let mut failures = Vec::new();
    if let Some(msg) = determinism::check(filter_name, &filtered.output, &filtered_again.output) {
        failures.push(msg);
    }
    (filtered.output, failures)
}

fn load_case(case_path: &Path) -> anyhow::Result<TestCase> {
    let content = std::fs::read_to_string(case_path)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", case_path.display()))?;
    let case: TestCase = toml::from_str(&content)
        .map_err(|e| anyhow::anyhow!("cannot parse {}: {e}", case_path.display()))?;
    if case.expects.is_empty() {
        anyhow::bail!(
            "{}: test case has no [[expect]] blocks",
            case_path.display()
        );
    }
    if let Some(min) = case.min_richness
        && (min.is_nan() || !(0.0..=1.0).contains(&min))
    {
        anyhow::bail!(
            "{}: min_richness must be between 0.0 and 1.0",
            case_path.display()
        );
    }
    Ok(case)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // --- run_case: end-to-end determinism failure via a genuinely
    // nondeterministic filter (Lua's math.random, seeded fresh per VM) ---

    fn nondeterministic_filter_config() -> tokf::config::types::FilterConfig {
        toml::from_str(
            r#"
command = "mytest cmd"

[lua_script]
lang = "luau"
source = "return tostring(math.random(1, 1000000000))"
"#,
        )
        .unwrap()
    }

    fn deterministic_filter_config() -> tokf::config::types::FilterConfig {
        toml::from_str(
            r#"
command = "mytest cmd"

[on_success]
output = "OK"
"#,
        )
        .unwrap()
    }

    fn write_case(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("case.toml");
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn run_case_fails_on_nondeterministic_filter_even_when_expects_pass() {
        let dir = tempfile::tempdir().unwrap();
        let case_path = write_case(
            dir.path(),
            r#"
name = "random output"
inline = "irrelevant input"
exit_code = 0

[[expect]]
matches = "^\\d+$"
"#,
        );
        let cfg = nondeterministic_filter_config();
        let result = run_case(&cfg, "mytest/random", &case_path);

        assert!(
            !result.passed,
            "expected a nondeterministic filter to fail verify"
        );
        assert!(
            result
                .failures
                .iter()
                .any(|f| f.contains("mytest/random") && f.contains("byte-stable")),
            "expected a determinism failure naming the filter, got: {:?}",
            result.failures
        );
    }

    #[test]
    fn run_case_passes_on_deterministic_filter() {
        let dir = tempfile::tempdir().unwrap();
        let case_path = write_case(
            dir.path(),
            r#"
name = "stable output"
inline = "irrelevant input"
exit_code = 0

[[expect]]
equals = "OK"
"#,
        );
        let cfg = deterministic_filter_config();
        let result = run_case(&cfg, "mytest/stable", &case_path);

        assert!(
            result.passed,
            "expected a deterministic filter to pass verify, failures: {:?}",
            result.failures
        );
    }
}
