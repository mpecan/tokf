//! Token-estimator calibration harness.
//!
//! Compares the shipping arithmetic estimator (`bytes / DIVISOR`) against a
//! real cl100k tokenizer across the whole tokf corpus — every filter `_test/`
//! case (raw *and* filtered) and every file under `tests/fixtures/` — and
//! reports the implied divisor so the constant can be recalibrated and
//! re-checked later.
//!
//! This is a diagnostic, not a user-facing feature: it deliberately adds no
//! CLI surface. It only compiles under the optional, off-by-default
//! `tokenizer` feature. Run it with:
//!
//! ```sh
//! cargo test -p tokf --features tokenizer --test calibration -- --ignored --nocapture
//! ```
#![cfg(feature = "tokenizer")]
#![allow(clippy::cast_precision_loss, clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use tokf::runner::CommandResult;
use tokf::suite_discovery::discover_suites;
use tokf_common::test_case::TestCase;
use tokf_common::tokens::{
    ArithmeticTokenCounter, Cl100kTokenCounter, DIVISOR, TokenCounter, estimate_tokens,
    estimate_tokens_from_bytes,
};

/// One measured string: its bytes, its real cl100k token count, and a label.
struct Sample {
    label: String,
    bytes: usize,
    real_tokens: usize,
}

impl Sample {
    /// Returns `None` for empty input, which would make the implied divisor a
    /// division by zero.
    fn new(label: String, text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        let real_tokens = Cl100kTokenCounter.count(text);
        if real_tokens == 0 {
            return None;
        }
        Some(Self {
            label,
            bytes: text.len(),
            real_tokens,
        })
    }

    /// Bytes per real token for this sample.
    fn implied_divisor(&self) -> f64 {
        self.bytes as f64 / self.real_tokens as f64
    }

    /// Signed percentage error of the arithmetic estimate against the truth.
    fn error_pct(&self) -> f64 {
        let est = estimate_tokens_from_bytes(self.bytes) as f64;
        (est - self.real_tokens as f64) / self.real_tokens as f64 * 100.0
    }
}

/// A filter test case measured before and after filtering.
struct Pair {
    filter: String,
    case: String,
    raw: Sample,
    filtered: Option<Sample>,
}

fn repo_filters_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("filters")
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Mirrors `verify_cmd::runner::load_fixture` minus the assertion machinery.
/// A missing or unreadable fixture warns and is skipped — one bad case must
/// not hide the whole report.
fn load_case_text(case: &TestCase, case_path: &Path) -> Option<String> {
    if let Some(inline) = &case.inline {
        return Some(inline.trim_end().to_string());
    }
    let fixture = case.fixture.as_ref()?;
    let case_dir = case_path.parent().unwrap_or_else(|| Path::new("."));
    for candidate in [case_dir.join(fixture), PathBuf::from(fixture)] {
        if candidate.exists() {
            match std::fs::read_to_string(&candidate) {
                Ok(s) => return Some(s.trim_end().to_string()),
                Err(e) => {
                    eprintln!("  warning: cannot read {}: {e}", candidate.display());
                    return None;
                }
            }
        }
    }
    eprintln!("  warning: fixture not found: {fixture}");
    None
}

/// Corpus A — every filter `_test/` case, measured raw and filtered.
fn collect_pairs() -> Vec<Pair> {
    let suites = discover_suites(&[repo_filters_dir()], None);
    let mut pairs = Vec::new();

    for suite in &suites {
        let cfg = match tokf::config::try_load_filter(&suite.filter_path) {
            Ok(Some(c)) => c,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("  warning: {} failed to load: {e:#}", suite.filter_name);
                continue;
            }
        };

        let Ok(entries) = std::fs::read_dir(&suite.suite_dir) else {
            continue;
        };
        let mut case_files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .collect();
        case_files.sort();

        for case_path in &case_files {
            let Ok(bytes) = std::fs::read_to_string(case_path) else {
                continue;
            };
            let Ok(case) = toml::from_str::<TestCase>(&bytes) else {
                eprintln!("  warning: cannot parse case {}", case_path.display());
                continue;
            };
            let Some(raw_text) = load_case_text(&case, case_path) else {
                continue;
            };
            let label = format!("{}::{}", suite.filter_name, case.name);
            let Some(raw) = Sample::new(label.clone(), &raw_text) else {
                continue;
            };

            let cmd_result = CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: case.exit_code,
                combined: raw_text.clone(),
            };
            let filtered = tokf::filter::apply(
                &cfg,
                &cmd_result,
                &case.args,
                &tokf::filter::FilterOptions::default(),
            );

            pairs.push(Pair {
                filter: suite.filter_name.clone(),
                case: case.name.clone(),
                raw,
                filtered: Sample::new(format!("{label} (filtered)"), &filtered.output),
            });
        }
    }

    pairs
}

/// Corpus B — every `.txt` under `tests/fixtures/`, raw only. Fixtures that a
/// suite case references explicitly are already covered by corpus A; no
/// pairing heuristic is invented here.
fn collect_fixture_samples() -> Vec<Sample> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "txt") {
                out.push(path);
            }
        }
    }

    let root = fixtures_dir();
    let mut paths = Vec::new();
    walk(&root, &mut paths);

    paths
        .iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            let label = p
                .strip_prefix(&root)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned();
            Sample::new(label, text.trim_end())
        })
        .collect()
}

/// Byte-weighted implied divisor: total bytes / total real tokens.
fn aggregate_divisor(samples: &[&Sample]) -> f64 {
    let bytes: usize = samples.iter().map(|s| s.bytes).sum();
    let tokens: usize = samples.iter().map(|s| s.real_tokens).sum();
    if tokens == 0 {
        return f64::NAN;
    }
    bytes as f64 / tokens as f64
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

/// (p10, median, p90) of the per-item implied divisor, so the spread that one
/// constant cannot capture is visible.
fn spread(samples: &[&Sample]) -> (f64, f64, f64) {
    let mut d: Vec<f64> = samples.iter().map(|s| s.implied_divisor()).collect();
    d.sort_by(f64::total_cmp);
    (
        percentile(&d, 0.10),
        percentile(&d, 0.50),
        percentile(&d, 0.90),
    )
}

fn reduction_pct(before: usize, after: usize) -> f64 {
    if before == 0 {
        return 0.0;
    }
    (1.0 - after as f64 / before as f64) * 100.0
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_owned();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

fn print_sample_table(title: &str, samples: &[&Sample]) {
    println!("\n### {title} ({} items)", samples.len());
    println!(
        "{:<58} {:>9} {:>8} {:>8} {:>9} {:>8}",
        "item", "bytes", "real", "impl.div", "est.", "err%"
    );
    for s in samples {
        println!(
            "{:<58} {:>9} {:>8} {:>8.2} {:>9} {:>7.1}%",
            truncate(&s.label, 58),
            s.bytes,
            s.real_tokens,
            s.implied_divisor(),
            estimate_tokens_from_bytes(s.bytes),
            s.error_pct(),
        );
    }
    let (p10, p50, p90) = spread(samples);
    println!(
        "-> aggregate implied divisor {:.3} | p10 {p10:.2} median {p50:.2} p90 {p90:.2}",
        aggregate_divisor(samples)
    );
}

/// Per-case estimated reduction% vs real reduction%, so the
/// deleting-vs-rewriting asymmetry shows up in our own data.
fn print_reduction_table(pairs: &[Pair]) {
    println!("\n### Reduction accuracy per case (est. vs real)");
    println!(
        "{:<58} {:>9} {:>9} {:>9}",
        "case", "est.red%", "real.red%", "err(pt)"
    );
    let mut worst: Vec<(f64, String)> = Vec::new();
    for p in pairs {
        let Some(f) = &p.filtered else { continue };
        let est = reduction_pct(
            estimate_tokens_from_bytes(p.raw.bytes),
            estimate_tokens_from_bytes(f.bytes),
        );
        let real = reduction_pct(p.raw.real_tokens, f.real_tokens);
        let err = est - real;
        println!(
            "{:<58} {est:>8.1}% {real:>8.1}% {err:>+8.1}",
            truncate(&format!("{}::{}", p.filter, p.case), 58)
        );
        worst.push((
            err.abs(),
            format!("{}::{} ({err:+.1} pt)", p.filter, p.case),
        ));
    }
    worst.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("\nLargest reduction-% errors (rewriting filters flatter themselves):");
    for (_, label) in worst.iter().take(10) {
        println!("  {label}");
    }
}

#[test]
#[ignore = "calibration harness; run with --features tokenizer -- --ignored --nocapture"]
fn calibrate_token_divisor() {
    println!("Shipping DIVISOR = {DIVISOR}");

    let pairs = collect_pairs();
    let fixtures = collect_fixture_samples();

    // A broken walker must fail loudly rather than pass with zero items.
    assert!(
        pairs.len() >= 40,
        "corpus looks broken: only {} filter test cases discovered",
        pairs.len()
    );
    assert!(
        fixtures.len() >= 20,
        "corpus looks broken: only {} fixture files discovered",
        fixtures.len()
    );

    let raw: Vec<&Sample> = pairs
        .iter()
        .map(|p| &p.raw)
        .chain(fixtures.iter())
        .collect();
    let filtered: Vec<&Sample> = pairs.iter().filter_map(|p| p.filtered.as_ref()).collect();
    let combined: Vec<&Sample> = raw
        .iter()
        .copied()
        .chain(filtered.iter().copied())
        .collect();

    print_sample_table("RAW inputs (filter cases + fixtures)", &raw);
    print_sample_table("FILTERED outputs", &filtered);
    print_reduction_table(&pairs);

    let raw_div = aggregate_divisor(&raw);
    let filtered_div = aggregate_divisor(&filtered);
    let combined_div = aggregate_divisor(&combined);

    println!("\n=== AGGREGATES ===");
    println!("raw implied divisor       : {raw_div:.3}");
    println!("filtered implied divisor  : {filtered_div:.3}");
    println!("combined implied divisor  : {combined_div:.3}");
    let (p10, p50, p90) = spread(&combined);
    println!("combined spread           : p10 {p10:.2} median {p50:.2} p90 {p90:.2}");
    println!("shipping DIVISOR          : {DIVISOR:.3}");
    println!(
        "shipping error vs combined: {:+.1}%",
        (combined_div - DIVISOR) / combined_div * 100.0
    );

    // Regression tripwire: the shipped constant must stay within a generous
    // band of what the corpus actually implies. Wide enough not to be flaky
    // when fixtures change; tight enough to catch real drift. If it proves
    // noisy, widen it rather than deleting it.
    let ratio = combined_div / DIVISOR;
    assert!(
        (0.75..=1.25).contains(&ratio),
        "shipped DIVISOR {DIVISOR} is more than 25% away from the corpus-implied \
         divisor {combined_div:.3} — recalibrate it"
    );
}

/// The point of the abstraction: both counters are usable through the same
/// `&dyn TokenCounter` at a call site.
#[test]
fn counters_are_swappable_at_the_call_site() {
    let text = "error[E0308]: mismatched types\n  --> src/main.rs:4:5\n";
    let counters: [&dyn TokenCounter; 2] = [&ArithmeticTokenCounter, &Cl100kTokenCounter];
    for c in counters {
        assert!(c.count(text) > 0);
    }
    assert_eq!(ArithmeticTokenCounter.count(text), estimate_tokens(text));
}
