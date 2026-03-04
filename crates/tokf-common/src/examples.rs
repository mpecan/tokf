use serde::{Deserialize, Serialize};

use crate::safety::SafetyWarning;

/// A single before/after example for a filter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

/// Collection of examples with aggregated safety results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        let examples = FilterExamples {
            examples: vec![FilterExample {
                name: "basic".to_string(),
                exit_code: 0,
                raw: "line1\nline2\nline3".to_string(),
                filtered: "line1".to_string(),
                raw_line_count: 3,
                filtered_line_count: 1,
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
