#![allow(clippy::unwrap_used)]

use super::types::MatchOutputRule;

#[test]
fn match_output_validate_accepts_contains() {
    let rule = MatchOutputRule {
        contains: Some("error".to_string()),
        pattern: None,
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_accepts_pattern() {
    let rule = MatchOutputRule {
        contains: None,
        pattern: Some("error".to_string()),
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_accepts_both() {
    let rule = MatchOutputRule {
        contains: Some("error".to_string()),
        pattern: Some("err.*".to_string()),
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_ok());
}

#[test]
fn match_output_validate_rejects_neither() {
    let rule = MatchOutputRule {
        contains: None,
        pattern: None,
        output: "bad".to_string(),
        unless: None,
    };
    assert!(rule.validate().is_err());
}
