#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use super::*;
use crate::info_cmd::{
    CacheInfo, ConfigFileEntry, FilterCounts, InfoOutput, SearchDir, TrackingDb, WriteAccess,
};

fn sample_info() -> InfoOutput {
    InfoOutput {
        version: "0.2.41".to_string(),
        home_override: None,
        search_dirs: vec![
            SearchDir {
                scope: "local",
                path: "/Users/alice/proj/.tokf/filters".to_string(),
                exists: false,
                access: None,
            },
            SearchDir {
                scope: "user",
                path: "/Users/alice/.config/tokf/filters".to_string(),
                exists: true,
                access: Some(WriteAccess::Writable),
            },
            SearchDir {
                scope: "built-in",
                path: "<embedded>".to_string(),
                exists: true,
                access: None,
            },
        ],
        tracking_db: TrackingDb {
            env_override: None,
            path: Some("/Users/alice/.local/share/tokf/tracking.db".to_string()),
            exists: true,
            access: Some(WriteAccess::Writable),
        },
        cache: CacheInfo {
            path: Some("/Users/alice/.cache/tokf/filter-cache.bin".to_string()),
            exists: true,
            access: Some(WriteAccess::Writable),
        },
        config_files: vec![ConfigFileEntry {
            scope: "global",
            path: "/Users/alice/.config/tokf/config.toml".to_string(),
            exists: false,
        }],
        filters: Some(FilterCounts {
            local: 0,
            user: 3,
            builtin: 92,
            total: 95,
        }),
    }
}

fn sample_env() -> EnvSnapshot {
    EnvSnapshot {
        os: "linux",
        arch: "x86_64",
        shell: Some("zsh".to_string()),
        has_gh: true,
        has_git: true,
    }
}

#[test]
fn redact_home_strips_user_home() {
    let home = PathBuf::from("/Users/alice");
    assert_eq!(
        redact_home("/Users/alice/.config/tokf", Some(&home)),
        "~/.config/tokf"
    );
    assert_eq!(redact_home("/etc/hosts", Some(&home)), "/etc/hosts");
}

#[test]
fn redact_home_idempotent() {
    let home = PathBuf::from("/Users/alice");
    let once = redact_home("/Users/alice/x", Some(&home));
    let twice = redact_home(&once, Some(&home));
    assert_eq!(once, twice);
}

#[test]
fn redact_home_handles_no_home() {
    assert_eq!(redact_home("/Users/alice/x", None), "/Users/alice/x");
}

fn render_with(
    info: &InfoOutput,
    env: &EnvSnapshot,
    body: &str,
    filters: Option<&FilterNames>,
    home: Option<&Path>,
) -> String {
    render_markdown(&MarkdownInputs {
        user_body: body,
        info,
        env,
        filters,
        home,
    })
}

#[test]
fn render_markdown_includes_version_and_counts() {
    let info = sample_info();
    let env = sample_env();
    let home = PathBuf::from("/Users/alice");
    let md = render_with(&info, &env, "something is broken", None, Some(&home));
    assert!(md.contains("**tokf**: 0.2.41"), "missing version:\n{md}");
    assert!(
        md.contains("92 built-in, 3 user, 0 local (95 total)"),
        "missing counts:\n{md}"
    );
    assert!(md.contains("something is broken"), "missing body:\n{md}");
    assert!(
        md.contains("Excluded for privacy"),
        "missing privacy footer:\n{md}"
    );
    assert!(
        md.contains("Filter names omitted"),
        "missing filter-names note when filters not included:\n{md}"
    );
}

#[test]
fn render_markdown_redacts_home_paths() {
    let info = sample_info();
    let env = sample_env();
    let home = PathBuf::from("/Users/alice");
    let md = render_with(&info, &env, "b", None, Some(&home));
    assert!(!md.contains("/Users/alice"), "home prefix leaked:\n{md}");
    assert!(
        md.contains("~/.config/tokf"),
        "expected redacted ~/.config:\n{md}"
    );
}

#[test]
fn render_markdown_handles_missing_optional_paths() {
    let mut info = sample_info();
    info.tracking_db.path = None;
    info.tracking_db.access = None;
    info.cache.path = None;
    info.cache.access = None;
    info.filters = None;
    let env = sample_env();
    let md = render_with(&info, &env, "b", None, None);
    assert!(md.contains("Tracking DB: (not available)"), "{md}");
    assert!(md.contains("Filter cache: (not available)"), "{md}");
    assert!(md.contains("Filters: (discovery error)"), "{md}");
}

#[test]
fn render_markdown_omits_filter_names_by_default() {
    let info = sample_info();
    let env = sample_env();
    let names = FilterNames {
        local: vec!["git/internal-secret".to_string()],
        user: vec![],
        builtin: vec!["cargo/build".to_string()],
    };
    let without = render_with(&info, &env, "b", None, None);
    assert!(!without.contains("git/internal-secret"));
    assert!(!without.contains("## Filters (opt-in)"));

    let with = render_with(&info, &env, "b", Some(&names), None);
    assert!(with.contains("## Filters (opt-in)"));
    assert!(with.contains("git/internal-secret"));
    assert!(with.contains("cargo/build"));
    assert!(
        !with.contains("Filter names omitted"),
        "footer note should disappear when names are included"
    );
}

#[test]
fn render_markdown_empty_body_shows_placeholder() {
    let info = sample_info();
    let env = sample_env();
    let md = render_with(&info, &env, "   ", None, None);
    assert!(md.contains("_(no description provided)_"), "{md}");
}

#[test]
fn custom_filter_count_sums_user_and_local() {
    let mut info = sample_info();
    info.filters = Some(FilterCounts {
        local: 2,
        user: 5,
        builtin: 99,
        total: 106,
    });
    assert_eq!(custom_filter_count(&info), 7);
}

#[test]
fn custom_filter_count_zero_when_no_counts() {
    let mut info = sample_info();
    info.filters = None;
    assert_eq!(custom_filter_count(&info), 0);
}

#[test]
fn url_encode_escapes_reserved() {
    assert_eq!(url_encode("hello world"), "hello%20world");
    assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
    assert_eq!(url_encode("abc-_.~"), "abc-_.~");
}

#[test]
fn build_issues_new_url_truncates_when_oversize() {
    let big = "x".repeat(ISSUES_NEW_URL_BUDGET);
    assert!(build_issues_new_url("o/r", "t", &big).is_none());
}

#[test]
fn build_issues_new_url_succeeds_for_small_body() {
    let url = build_issues_new_url("mpecan/tokf", "title with space", "body").unwrap();
    assert!(url.starts_with("https://github.com/mpecan/tokf/issues/new?"));
    assert!(url.contains("title=title%20with%20space"));
    assert!(url.contains("body=body"));
}
