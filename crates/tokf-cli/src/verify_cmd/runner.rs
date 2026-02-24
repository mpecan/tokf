use std::path::{Path, PathBuf};

use tokf::config;
use tokf::filter;
use tokf::runner::CommandResult;

use super::discovery::DiscoveredSuite;
use super::{CaseResult, Expectation, SuiteResult, TestCase};

// --- Fixture loading ---

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
            return Ok(std::fs::read_to_string(relative_to_case)?
                .trim_end()
                .to_string());
        }

        // Try relative to CWD
        let path = Path::new(fixture);
        if path.exists() {
            return Ok(std::fs::read_to_string(path)?.trim_end().to_string());
        }

        anyhow::bail!("fixture not found: {fixture}");
    }

    anyhow::bail!("test case must specify either 'fixture' or 'inline'")
}

// --- Assertions ---

// This function handles all 8 assertion types in a single pass. The length is
// justified by the straightforward pattern repetition; splitting would obscure
// the symmetry between assertion kinds.
#[allow(clippy::too_many_lines)]
pub(super) fn evaluate(expect: &Expectation, output: &str) -> Option<String> {
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

// --- Suite execution ---

pub(super) fn run_suite(suite: &DiscoveredSuite) -> SuiteResult {
    let cfg = match config::try_load_filter(&suite.filter_path) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return SuiteResult {
                filter_name: suite.filter_name.clone(),
                cases: vec![],
                error: Some(format!("filter not found: {}", suite.filter_path.display())),
            };
        }
        Err(e) => {
            return SuiteResult {
                filter_name: suite.filter_name.clone(),
                cases: vec![],
                error: Some(format!("{e:#}")),
            };
        }
    };

    let mut case_files: Vec<PathBuf> = match std::fs::read_dir(&suite.suite_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect(),
        Err(e) => {
            return SuiteResult {
                filter_name: suite.filter_name.clone(),
                cases: vec![],
                error: Some(format!("cannot read suite dir: {e}")),
            };
        }
    };
    case_files.sort();

    if case_files.is_empty() {
        return SuiteResult {
            filter_name: suite.filter_name.clone(),
            cases: vec![],
            error: Some(format!(
                "suite directory is empty: {}",
                suite.suite_dir.display()
            )),
        };
    }

    let cases: Vec<CaseResult> = case_files
        .iter()
        .map(|case_path| run_case(&cfg, case_path))
        .collect();

    SuiteResult {
        filter_name: suite.filter_name.clone(),
        cases,
        error: None,
    }
}

fn run_case(cfg: &tokf::config::types::FilterConfig, case_path: &Path) -> CaseResult {
    let case = match load_case(case_path) {
        Ok(c) => c,
        Err(e) => {
            return CaseResult {
                name: case_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                passed: false,
                failures: vec![format!("failed to load case: {e:#}")],
            };
        }
    };

    let fixture = match load_fixture(&case, case_path) {
        Ok(f) => f,
        Err(e) => {
            return CaseResult {
                name: case.name,
                passed: false,
                failures: vec![format!("failed to load fixture: {e:#}")],
            };
        }
    };

    let cmd_result = CommandResult {
        stdout: String::new(),
        stderr: String::new(),
        exit_code: case.exit_code,
        combined: fixture,
    };

    let filtered = filter::apply(
        cfg,
        &cmd_result,
        &case.args,
        &filter::FilterOptions::default(),
    );

    let mut failures = Vec::new();
    for expect in &case.expects {
        if let Some(msg) = evaluate(expect, &filtered.output) {
            failures.push(msg);
        }
    }

    let passed = failures.is_empty();
    CaseResult {
        name: case.name,
        passed,
        failures,
    }
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
    Ok(case)
}
