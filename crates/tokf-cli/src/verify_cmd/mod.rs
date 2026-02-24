mod discovery;
mod runner;

use serde::Deserialize;
use serde::Serialize;

use self::discovery::DiscoveredSuite;

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum VerifyScope {
    /// Repo-local custom filters only (`.tokf/filters/`)
    Project,
    /// User-level custom filters only (`<config_dir>/tokf/filters/`)
    Global,
    /// Stdlib filters in CWD only (`filters/`, for repo development)
    Stdlib,
}

// --- Types ---

#[derive(Deserialize)]
pub struct TestCase {
    pub name: String,
    #[serde(default)]
    pub fixture: Option<String>,
    #[serde(default)]
    pub inline: Option<String>,
    #[serde(default)]
    pub exit_code: i32,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(rename = "expect", default)]
    pub expects: Vec<Expectation>,
}

#[derive(Deserialize)]
pub struct Expectation {
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub not_contains: Option<String>,
    #[serde(default)]
    pub equals: Option<String>,
    #[serde(default)]
    pub starts_with: Option<String>,
    #[serde(default)]
    pub ends_with: Option<String>,
    #[serde(default)]
    pub line_count: Option<usize>,
    #[serde(default)]
    pub matches: Option<String>,
    #[serde(default)]
    pub not_matches: Option<String>,
}

#[derive(Serialize)]
pub struct CaseResult {
    pub name: String,
    pub passed: bool,
    pub failures: Vec<String>,
}

#[derive(Serialize)]
pub struct SuiteResult {
    pub filter_name: String,
    pub cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// --- Output formatting ---

fn case_count_in_dir(dir: &std::path::Path) -> usize {
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

// --- Entry point ---

// cmd_verify orchestrates list, require-all, JSON, and run modes in a single
// entry point. Splitting the modes into separate functions would force passing
// the same 5 parameters and duplicating suite-discovery logic.
#[allow(clippy::too_many_lines)]
pub fn cmd_verify(
    filter: Option<&str>,
    list: bool,
    json: bool,
    require_all: bool,
    scope: Option<&VerifyScope>,
) -> i32 {
    // Exit codes: 0 = all pass, 1 = assertion failure, 2 = config/IO error.
    let search_dirs = discovery::verify_search_dirs(scope);

    if list && require_all {
        let all = discovery::discover_all_filters_with_coverage(&search_dirs, filter);
        for (name, covered) in &all {
            let icon = if *covered { "\u{2713}" } else { "\u{2717}" };
            println!("{icon} {name}");
        }
        let uncovered = all.iter().filter(|(_, c)| !c).count();
        if uncovered > 0 {
            println!("\n{uncovered} filter(s) have no test suite.");
            return 2;
        }
        return 0;
    }

    if require_all {
        let all = discovery::discover_all_filters_with_coverage(&search_dirs, filter);
        let uncovered: Vec<_> = all
            .iter()
            .filter(|(_, c)| !c)
            .map(|(n, _)| n.as_str())
            .collect();
        if !uncovered.is_empty() {
            eprintln!("\u{2717} uncovered filters (no test suite found):");
            for name in &uncovered {
                eprintln!("  {name}");
            }
            eprintln!("\nRun `tokf verify --list` to see discovered suites.");
            return 2;
        }
    }

    let suites = discovery::discover_suites(&search_dirs, filter);

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

    let results: Vec<SuiteResult> = suites.iter().map(runner::run_suite).collect();

    let has_io_error = results.iter().any(|s| s.error.is_some());
    let has_failure = results.iter().any(|s| s.cases.iter().any(|c| !c.passed));

    if json {
        crate::output::print_json(&results);
    } else {
        print_results(&results);
    }

    if has_io_error {
        2
    } else {
        i32::from(has_failure)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_search_dirs_none_returns_all() {
        let dirs = discovery::verify_search_dirs(None);
        // Should have at least stdlib (filters/) and project (.tokf/filters/)
        assert!(
            dirs.len() >= 2,
            "expected at least 2 dirs, got {}",
            dirs.len()
        );
        let joined: String = dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.contains("filters"),
            "expected 'filters' in paths: {joined}"
        );
    }

    #[test]
    fn verify_search_dirs_project_only_has_tokf() {
        let dirs = discovery::verify_search_dirs(Some(&VerifyScope::Project));
        assert_eq!(dirs.len(), 1, "project scope should return exactly 1 dir");
        let path = dirs[0].display().to_string();
        assert!(
            path.contains(".tokf/filters"),
            "expected .tokf/filters, got {path}"
        );
    }

    #[test]
    fn verify_search_dirs_stdlib_only_has_filters() {
        let dirs = discovery::verify_search_dirs(Some(&VerifyScope::Stdlib));
        assert_eq!(dirs.len(), 1, "stdlib scope should return exactly 1 dir");
        let path = dirs[0].display().to_string();
        assert!(
            path.ends_with("filters"),
            "expected path ending in 'filters', got {path}"
        );
        assert!(
            !path.contains(".tokf"),
            "stdlib should not contain .tokf: {path}"
        );
    }

    #[test]
    fn verify_search_dirs_global_only_has_config() {
        let dirs = discovery::verify_search_dirs(Some(&VerifyScope::Global));
        // May be 0 on systems without config_dir, but typically 1
        assert!(dirs.len() <= 1, "global scope should return at most 1 dir");
        if let Some(dir) = dirs.first() {
            let path = dir.display().to_string();
            assert!(
                path.contains("tokf/filters"),
                "expected tokf/filters in path: {path}"
            );
        }
    }
}
