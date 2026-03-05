use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

/// Validate test case bytes: checks UTF-8, TOML parsing, non-empty name,
/// at least one `[[expect]]` block, and regex compilation for `matches`
/// and `not_matches` fields.
///
/// # Errors
///
/// Returns a human-readable error string if validation fails.
#[cfg(feature = "validation")]
pub fn validate(bytes: &[u8]) -> Result<TestCase, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "test file is not valid UTF-8")?;
    let tc: TestCase = toml::from_str(text).map_err(|e| format!("invalid test case TOML: {e}"))?;
    if tc.name.trim().is_empty() {
        return Err("test case 'name' must be non-empty".to_string());
    }
    if tc.expects.is_empty() {
        return Err("test case must have at least one [[expect]] block".to_string());
    }
    for (i, exp) in tc.expects.iter().enumerate() {
        if let Some(pat) = &exp.matches {
            regex::Regex::new(pat)
                .map_err(|e| format!("expect[{i}].matches: invalid regex: {e}"))?;
        }
        if let Some(pat) = &exp.not_matches {
            regex::Regex::new(pat)
                .map_err(|e| format!("expect[{i}].not_matches: invalid regex: {e}"))?;
        }
    }
    Ok(tc)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_test_case() {
        let toml_str = r#"
name = "basic"

[[expect]]
contains = "hello"
"#;
        let tc: TestCase = toml::from_str(toml_str).unwrap();
        assert_eq!(tc.name, "basic");
        assert_eq!(tc.expects.len(), 1);
        assert_eq!(tc.expects[0].contains.as_deref(), Some("hello"));
    }

    #[test]
    fn deserialize_full_test_case() {
        let toml_str = r#"
name = "full"
fixture = "output.txt"
exit_code = 1
args = ["--verbose"]

[[expect]]
contains = "error"
not_contains = "success"
matches = "\\d+ errors?"
"#;
        let tc: TestCase = toml::from_str(toml_str).unwrap();
        assert_eq!(tc.name, "full");
        assert_eq!(tc.fixture.as_deref(), Some("output.txt"));
        assert_eq!(tc.exit_code, 1);
        assert_eq!(tc.args, vec!["--verbose"]);
        assert_eq!(tc.expects[0].matches.as_deref(), Some("\\d+ errors?"));
    }

    #[test]
    fn serialize_round_trip() {
        let tc = TestCase {
            name: "roundtrip".to_string(),
            fixture: None,
            inline: Some("hello world".to_string()),
            exit_code: 0,
            args: vec![],
            expects: vec![Expectation {
                contains: Some("hello".to_string()),
                not_contains: None,
                equals: None,
                starts_with: None,
                ends_with: None,
                line_count: None,
                matches: None,
                not_matches: None,
            }],
        };
        let json = serde_json::to_string(&tc).unwrap();
        let parsed: TestCase = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, "roundtrip");
        assert_eq!(parsed.expects[0].contains.as_deref(), Some("hello"));
    }
}

#[cfg(all(test, feature = "validation"))]
#[allow(clippy::unwrap_used)]
mod validation_tests {
    use super::*;

    #[test]
    fn validate_accepts_valid_test_case() {
        let bytes = br#"
name = "basic"

[[expect]]
contains = "hello"
"#;
        let tc = validate(bytes).unwrap();
        assert_eq!(tc.name, "basic");
    }

    #[test]
    fn validate_rejects_invalid_utf8() {
        let bytes = &[0xFF, 0xFE, 0x00];
        let err = validate(bytes).unwrap_err();
        assert!(err.contains("UTF-8"), "expected UTF-8 error, got: {err}");
    }

    #[test]
    fn validate_rejects_invalid_toml() {
        let bytes = b"not valid toml [[[";
        let err = validate(bytes).unwrap_err();
        assert!(err.contains("TOML"), "expected TOML error, got: {err}");
    }

    #[test]
    fn validate_rejects_empty_name() {
        let bytes = br#"
name = ""

[[expect]]
contains = "x"
"#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("non-empty"),
            "expected non-empty name error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_missing_expects() {
        let bytes = br#"name = "no expects""#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("[[expect]]"),
            "expected expect error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_invalid_regex_in_matches() {
        let bytes = br#"
name = "bad regex"

[[expect]]
matches = "[invalid("
"#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("invalid regex"),
            "expected regex error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_invalid_regex_in_not_matches() {
        let bytes = br#"
name = "bad not_matches"

[[expect]]
not_matches = "(?P<>)"
"#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("invalid regex"),
            "expected regex error, got: {err}"
        );
    }

    #[test]
    fn validate_rejects_whitespace_only_name() {
        let bytes = br#"
name = "   "

[[expect]]
contains = "x"
"#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("non-empty"),
            "expected non-empty name error, got: {err}"
        );
    }

    #[test]
    fn validate_accepts_multiple_valid_expects() {
        let bytes = br#"
name = "multi-expect"

[[expect]]
contains = "hello"

[[expect]]
not_contains = "error"
starts_with = "OK"
"#;
        let tc = validate(bytes).unwrap();
        assert_eq!(tc.expects.len(), 2);
    }

    #[test]
    fn validate_rejects_second_expect_with_bad_regex() {
        let bytes = br#"
name = "mixed"

[[expect]]
contains = "valid"

[[expect]]
matches = "[bad("
"#;
        let err = validate(bytes).unwrap_err();
        assert!(
            err.contains("expect[1]"),
            "expected error on second expect block, got: {err}"
        );
    }
}
