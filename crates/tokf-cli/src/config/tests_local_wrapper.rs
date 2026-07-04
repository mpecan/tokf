#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tokf_hook_types::{LocalWrapperConfig, LocalWrapperRule};

use super::ResolvedFilter;
use super::local_wrapper::{
    match_filters_with_wrapper, patterns_match_with_wrapper, strip_local_wrapper,
};
use super::types::FilterConfig;

fn words(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

/// Default config: built-ins on, no disables, no user rules.
fn default_cfg() -> LocalWrapperConfig {
    LocalWrapperConfig::default()
}

fn distrobox_rule() -> LocalWrapperRule {
    LocalWrapperRule {
        command: "distrobox".to_string(),
        subcommands: vec!["enter".to_string()],
        markers: vec!["--".to_string()],
    }
}

// --- strip_local_wrapper: the three issue examples ---

#[test]
fn strips_nix_develop_c_cargo_test() {
    // nix develop -c cargo test  → consume "nix develop -c" (3 words)
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c cargo test"), &default_cfg()),
        Some(3)
    );
}

#[test]
fn strips_nix_develop_attr_c_pytest() {
    // nix develop .#agent -c pytest → consume "nix develop .#agent -c" (4 words)
    assert_eq!(
        strip_local_wrapper(&words("nix develop .#agent -c pytest"), &default_cfg()),
        Some(4)
    );
}

#[test]
fn strips_nix_develop_impure_c_npm_test() {
    assert_eq!(
        strip_local_wrapper(&words("nix develop --impure -c npm test"), &default_cfg()),
        Some(4)
    );
}

#[test]
fn strips_long_form_command_marker() {
    assert_eq!(
        strip_local_wrapper(&words("nix develop --command cargo test"), &default_cfg()),
        Some(3)
    );
}

#[test]
fn strips_path_prefixed_nix() {
    assert_eq!(
        strip_local_wrapper(
            &words("/nix/store/abc/bin/nix develop -c cargo test"),
            &default_cfg()
        ),
        Some(3)
    );
}

// --- strip_local_wrapper: no-match cases ---

#[test]
fn no_match_nix_build() {
    // Not a develop subcommand.
    assert_eq!(
        strip_local_wrapper(&words("nix build .#foo"), &default_cfg()),
        None
    );
}

#[test]
fn no_match_nix_develop_without_marker() {
    // Enters an interactive shell, no inner command.
    assert_eq!(
        strip_local_wrapper(&words("nix develop"), &default_cfg()),
        None
    );
    assert_eq!(
        strip_local_wrapper(&words("nix develop .#agent"), &default_cfg()),
        None
    );
}

#[test]
fn no_match_bare_trailing_marker() {
    // Marker present but nothing after it.
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c"), &default_cfg()),
        None
    );
}

#[test]
fn no_match_unrelated_command() {
    assert_eq!(
        strip_local_wrapper(&words("cargo test"), &default_cfg()),
        None
    );
    assert_eq!(strip_local_wrapper(&words(""), &default_cfg()), None);
}

// --- disable behaviour ---

#[test]
fn disabled_list_turns_off_nix_builtin() {
    let cfg = LocalWrapperConfig {
        builtins: true,
        disabled: vec!["nix".to_string()],
        rules: vec![],
    };
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c cargo test"), &cfg),
        None
    );
}

#[test]
fn builtins_false_turns_off_all_builtins() {
    let cfg = LocalWrapperConfig {
        builtins: false,
        disabled: vec![],
        rules: vec![],
    };
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c cargo test"), &cfg),
        None
    );
}

#[test]
fn user_rule_still_matches_when_nix_disabled() {
    let cfg = LocalWrapperConfig {
        builtins: true,
        disabled: vec!["nix".to_string()],
        rules: vec![distrobox_rule()],
    };
    // nix is disabled…
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c cargo test"), &cfg),
        None
    );
    // …but the user rule works: distrobox enter my-box -- cargo test
    assert_eq!(
        strip_local_wrapper(&words("distrobox enter my-box -- cargo test"), &cfg),
        Some(4)
    );
}

#[test]
fn user_rule_works_with_builtins_off() {
    let cfg = LocalWrapperConfig {
        builtins: false,
        disabled: vec![],
        rules: vec![distrobox_rule()],
    };
    assert_eq!(
        strip_local_wrapper(&words("distrobox enter box -- pytest"), &cfg),
        Some(4)
    );
    assert_eq!(
        strip_local_wrapper(&words("nix develop -c cargo test"), &cfg),
        None
    );
}

#[test]
fn user_rule_without_subcommands() {
    let cfg = LocalWrapperConfig {
        builtins: true,
        disabled: vec![],
        rules: vec![LocalWrapperRule {
            command: "toolbox".to_string(),
            subcommands: vec![],
            markers: vec!["run".to_string()],
        }],
    };
    // toolbox run cargo test → consume "toolbox run" (2 words)
    assert_eq!(
        strip_local_wrapper(&words("toolbox run cargo test"), &cfg),
        Some(2)
    );
}

// --- patterns_match_with_wrapper ---

#[test]
fn patterns_match_direct() {
    let patterns = vec!["cargo test".to_string()];
    assert!(patterns_match_with_wrapper(
        &patterns,
        &words("cargo test"),
        &default_cfg()
    ));
}

#[test]
fn patterns_match_through_wrapper() {
    let patterns = vec!["cargo test".to_string()];
    assert!(patterns_match_with_wrapper(
        &patterns,
        &words("nix develop -c cargo test"),
        &default_cfg()
    ));
}

#[test]
fn patterns_no_match_through_wrapper_when_inner_unknown() {
    let patterns = vec!["cargo test".to_string()];
    assert!(!patterns_match_with_wrapper(
        &patterns,
        &words("nix develop -c echo hi"),
        &default_cfg()
    ));
}

#[test]
fn patterns_no_match_when_disabled() {
    let patterns = vec!["cargo test".to_string()];
    let cfg = LocalWrapperConfig {
        builtins: false,
        disabled: vec![],
        rules: vec![],
    };
    assert!(!patterns_match_with_wrapper(
        &patterns,
        &words("nix develop -c cargo test"),
        &cfg
    ));
}

#[test]
fn patterns_degenerate_nesting_terminates() {
    // Double-wrapped: nix develop -c nix develop -c cargo test.
    let patterns = vec!["cargo test".to_string()];
    assert!(patterns_match_with_wrapper(
        &patterns,
        &words("nix develop -c nix develop -c cargo test"),
        &default_cfg()
    ));
}

// --- match_filters_with_wrapper ---

fn resolved(command: &str) -> ResolvedFilter {
    let cfg: FilterConfig = toml::from_str(&format!("command = \"{command}\"")).unwrap();
    ResolvedFilter {
        config: cfg,
        hash: "hash".to_string(),
        source_path: PathBuf::from("<built-in>/test.toml"),
        relative_path: PathBuf::from("test.toml"),
        priority: 0,
    }
}

#[test]
fn match_filters_direct() {
    let filters = vec![resolved("cargo test")];
    let (_f, pattern, consumed) =
        match_filters_with_wrapper(&filters, &words("cargo test"), &default_cfg()).unwrap();
    assert_eq!(pattern, "cargo test");
    assert_eq!(consumed, 2);
}

#[test]
fn match_filters_through_wrapper_extends_consumed() {
    let filters = vec![resolved("cargo test")];
    let (_f, pattern, consumed) = match_filters_with_wrapper(
        &filters,
        &words("nix develop -c cargo test"),
        &default_cfg(),
    )
    .unwrap();
    // matched_command is the inner pattern only…
    assert_eq!(pattern, "cargo test");
    // …but consumed spans the full "nix develop -c cargo test" prefix.
    assert_eq!(consumed, 5);
}

#[test]
fn match_filters_no_match_returns_none() {
    let filters = vec![resolved("cargo test")];
    assert!(
        match_filters_with_wrapper(&filters, &words("nix develop -c echo hi"), &default_cfg())
            .is_none()
    );
}

#[test]
fn match_filters_nested_wrapper_terminates() {
    let filters = vec![resolved("cargo test")];
    let (_f, pattern, consumed) = match_filters_with_wrapper(
        &filters,
        &words("nix develop -c nix develop -c cargo test"),
        &default_cfg(),
    )
    .unwrap();
    assert_eq!(pattern, "cargo test");
    // 3 (outer wrapper) + 3 (inner wrapper) + 2 (cargo test) = 8
    assert_eq!(consumed, 8);
}
