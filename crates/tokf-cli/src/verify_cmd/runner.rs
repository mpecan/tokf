use std::path::{Path, PathBuf};

use tokf::config;
use tokf::filter;
use tokf::runner::CommandResult;

use super::discovery::DiscoveredSuite;
use super::{CaseResult, SuiteResult, TestCase};

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
