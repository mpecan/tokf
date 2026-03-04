use serde::{Deserialize, Serialize};

use crate::safety::SafetyWarning;

/// Estimate token count from a string using the bytes/4 heuristic.
///
/// This matches the estimation used by the tracking module.
pub const fn estimate_tokens(s: &str) -> usize {
    s.len() / 4
}

/// Compute the reduction percentage between raw and filtered token estimates.
///
/// Returns 0.0 when `raw_tokens` is zero.
#[allow(clippy::cast_precision_loss)]
pub fn reduction_pct(raw_tokens: usize, filtered_tokens: usize) -> f64 {
    if raw_tokens == 0 {
        0.0
    } else {
        (1.0 - filtered_tokens as f64 / raw_tokens as f64) * 100.0
    }
}

/// A single before/after example for a filter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(export, export_to = "../../tokf-server/generated/")
)]
pub struct FilterExample {
    /// Test case name.
    pub name: String,
    /// Exit code used for this example.
    #[cfg_attr(test, ts(type = "number"))]
    pub exit_code: i32,
    /// Raw (unfiltered) input.
    pub raw: String,
    /// Filtered output.
    pub filtered: String,
    /// Number of lines in raw input.
    #[cfg_attr(test, ts(type = "number"))]
    pub raw_line_count: usize,
    /// Number of lines in filtered output.
    #[cfg_attr(test, ts(type = "number"))]
    pub filtered_line_count: usize,
    /// Estimated tokens in raw input (bytes / 4).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub raw_tokens_est: usize,
    /// Estimated tokens in filtered output (bytes / 4).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "number"))]
    pub filtered_tokens_est: usize,
    /// Percentage reduction in estimated tokens.
    #[serde(default)]
    pub reduction_pct: f64,
}

/// Collection of examples with aggregated safety results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(export, export_to = "../../tokf-server/generated/")
)]
pub struct FilterExamples {
    pub examples: Vec<FilterExample>,
    pub safety: ExamplesSafety,
}

/// Serializable safety summary for the examples payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(export, export_to = "../../tokf-server/generated/")
)]
pub struct ExamplesSafety {
    pub passed: bool,
    pub warnings: Vec<SafetyWarningDto>,
}

/// A flattened, transport-friendly representation of a safety warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(export, export_to = "../../tokf-server/generated/")
)]
pub struct SafetyWarningDto {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl From<&SafetyWarning> for SafetyWarningDto {
    fn from(w: &SafetyWarning) -> Self {
        Self {
            kind: w.kind.as_str().to_string(),
            message: w.message.clone(),
            detail: w.detail.clone(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::safety::WarningKind;

    #[test]
    fn serialize_round_trip() {
        let raw = "line1\nline2\nline3";
        let filtered = "line1";
        let examples = FilterExamples {
            examples: vec![FilterExample {
                name: "basic".to_string(),
                exit_code: 0,
                raw: raw.to_string(),
                filtered: filtered.to_string(),
                raw_line_count: 3,
                filtered_line_count: 1,
                raw_tokens_est: estimate_tokens(raw),
                filtered_tokens_est: estimate_tokens(filtered),
                reduction_pct: reduction_pct(estimate_tokens(raw), estimate_tokens(filtered)),
            }],
            safety: ExamplesSafety {
                passed: true,
                warnings: vec![],
            },
        };

        let json = serde_json::to_string(&examples).unwrap();
        let parsed: FilterExamples = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.examples.len(), 1);
        assert_eq!(parsed.examples[0].name, "basic");
        assert!(parsed.safety.passed);
    }

    #[test]
    fn deserialize_without_token_fields_defaults_to_zero() {
        let json = r#"{"examples":[{"name":"old","exit_code":0,"raw":"abc","filtered":"a","raw_line_count":1,"filtered_line_count":1}],"safety":{"passed":true,"warnings":[]}}"#;
        let parsed: FilterExamples = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.examples[0].raw_tokens_est, 0);
        assert_eq!(parsed.examples[0].filtered_tokens_est, 0);
        assert!((parsed.examples[0].reduction_pct).abs() < f64::EPSILON);
    }

    #[test]
    fn estimate_tokens_basic() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        // 3 chars rounds down to 0
        assert_eq!(estimate_tokens("abc"), 0);
    }

    #[test]
    fn reduction_pct_basic() {
        assert!((reduction_pct(100, 25) - 75.0).abs() < 0.01);
        assert!((reduction_pct(100, 0) - 100.0).abs() < 0.01);
        assert!((reduction_pct(100, 100)).abs() < 0.01);
        assert!((reduction_pct(0, 0)).abs() < 0.01);
    }

    #[test]
    fn warning_dto_from_safety_warning() {
        let warning = SafetyWarning {
            kind: WarningKind::TemplateInjection,
            message: "bad template".to_string(),
            detail: Some("ignore previous instructions".to_string()),
        };
        let dto = SafetyWarningDto::from(&warning);
        assert_eq!(dto.kind, "template_injection");
        assert_eq!(dto.message, "bad template");
    }
}
