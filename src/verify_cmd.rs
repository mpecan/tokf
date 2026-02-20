use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;

use tokf::config;
use tokf::filter;
use tokf::runner::CommandResult;

// --- Types ---

#[derive(Deserialize)]
struct TestCase {
    name: String,
    #[serde(default)]
    fixture: Option<String>,
    #[serde(default)]
    inline: Option<String>,
    #[serde(default)]
    exit_code: i32,
    #[serde(default)]
    args: Vec<String>,
    #[serde(rename = "expect", default)]
    expects: Vec<Expectation>,
}

#[derive(Deserialize)]
struct Expectation {
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    not_contains: Option<String>,
    #[serde(default)]
    equals: Option<String>,
    #[serde(default)]
    starts_with: Option<String>,
    #[serde(default)]
    ends_with: Option<String>,
    #[serde(default)]
    line_count: Option<usize>,
    #[serde(default)]
    matches: Option<String>,
    #[serde(default)]
    not_matches: Option<String>,
}

#[derive(Serialize)]
struct CaseResult {
    name: String,
    passed: bool,
    failures: Vec<String>,
}

#[derive(Serialize)]
struct SuiteResult {
    filter_name: String,
    cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

// --- Search dirs for verify ---

fn verify_search_dirs() -> Vec<PathBuf> {
    // Priority order (highest first):
    //   1. filters/ in CWD — catches the stdlib during repo development
    //   2. .tokf/filters/ in CWD — repo-local custom filters
    //   3. {config_dir}/tokf/filters/ — user-level custom filters
    // When the same filter name appears in multiple dirs, the first wins.
    let mut dirs = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("filters"));
        dirs.push(cwd.join(".tokf/filters"));
    }

    if let Some(config) = dirs::config_dir() {
        dirs.push(config.join("tokf/filters"));
    }

    dirs
}

// --- Discovery ---

/// A discovered suite: filter TOML path, suite directory, and filter name.
struct DiscoveredSuite {
    filter_path: PathBuf,
    suite_dir: PathBuf,
    filter_name: String,
}

fn discover_suites(search_dirs: &[PathBuf], filter_arg: Option<&str>) -> Vec<DiscoveredSuite> {
    let mut result = Vec::new();

    for dir in search_dirs {
        if !dir.exists() {
            continue;
        }
        collect_suites(dir, dir, &mut result);
    }

    // Remove duplicates: prefer first occurrence (higher priority dir).
    // HashSet tracks seen names; retain() preserves insertion order.
    let mut seen = HashSet::new();
    result.retain(|s| seen.insert(s.filter_name.clone()));

    if let Some(name) = filter_arg {
        result.retain(|s| s.filter_name == name);
    }

    result
}

fn collect_suites(root: &Path, dir: &Path, result: &mut Vec<DiscoveredSuite>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in &entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.starts_with('.') {
            continue;
        }

        if path.is_file() && path.extension().is_some_and(|e| e == "toml") {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // Suite directories use the convention <stem>_test/ adjacent to <stem>.toml.
            let suite_dir = path.parent().unwrap_or(dir).join(format!("{stem}_test"));
            if suite_dir.is_dir() {
                let filter_name = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .with_extension("")
                    .to_string_lossy()
                    .into_owned();
                // Normalize path separators on Windows so filter names are always "foo/bar".
                #[cfg(windows)]
                let filter_name = filter_name.replace('\\', "/");
                result.push(DiscoveredSuite {
                    filter_path: path,
                    suite_dir,
                    filter_name,
                });
            }
        } else if path.is_dir() {
            // Skip _test directories — they are suite dirs, not filter category dirs.
            if name_str.ends_with("_test") {
                continue;
            }
            collect_suites(root, &path, result);
        }
    }
}

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
fn evaluate(expect: &Expectation, output: &str) -> Option<String> {
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

fn run_suite(suite: &DiscoveredSuite) -> SuiteResult {
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

    let filtered = filter::apply(cfg, &cmd_result, &case.args);

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

// --- Output formatting ---

fn case_count_in_dir(dir: &Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |rd| {
        rd.filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
            .count()
    })
}

fn print_list(suites: &[DiscoveredSuite]) {
    for suite in suites {
        let count = case_count_in_dir(&suite.suite_dir);
        let noun = if count == 1 { "case" } else { "cases" };
        println!("{} ({count} {noun})", suite.filter_name);
    }
}

fn print_results(results: &[SuiteResult]) {
    let mut total_cases = 0;
    let mut total_passed = 0;

    for suite in results {
        if let Some(err) = &suite.error {
            println!("\u{2717} {} \u{2014} error: {err}", suite.filter_name);
            continue;
        }

        let suite_passed = suite.cases.iter().all(|c| c.passed);
        let icon = if suite_passed { "\u{2713}" } else { "\u{2717}" };
        println!("{icon} {}", suite.filter_name);

        for case in &suite.cases {
            total_cases += 1;
            if case.passed {
                total_passed += 1;
                println!("    \u{2713} {}", case.name);
            } else {
                println!("    \u{2717} {}", case.name);
                for failure in &case.failures {
                    for line in failure.lines() {
                        println!("        {line}");
                    }
                }
            }
        }
    }

    println!();
    println!("{total_passed}/{total_cases} passed");
}

fn print_json(results: &[SuiteResult]) {
    match serde_json::to_string_pretty(results) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("[tokf] JSON serialization error: {e}"),
    }
}

// --- Entry point ---

pub fn cmd_verify(filter: Option<&str>, list: bool, json: bool) -> i32 {
    // Exit codes: 0 = all pass, 1 = assertion failure, 2 = config/IO error.
    let search_dirs = verify_search_dirs();
    let suites = discover_suites(&search_dirs, filter);

    if suites.is_empty() {
        if let Some(name) = filter {
            eprintln!("[tokf] no test suite found for filter: {name}");
            return 2;
        }
        eprintln!("[tokf] no test suites discovered");
        return 0;
    }

    if list {
        print_list(&suites);
        return 0;
    }

    let results: Vec<SuiteResult> = suites.iter().map(run_suite).collect();

    let has_io_error = results.iter().any(|s| s.error.is_some());
    let has_failure = results.iter().any(|s| s.cases.iter().any(|c| !c.passed));

    if json {
        print_json(&results);
    } else {
        print_results(&results);
    }

    if has_io_error {
        2
    } else {
        i32::from(has_failure)
    }
}
