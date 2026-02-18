use std::collections::HashMap;

use regex::Regex;

use super::section::SectionMap;
use crate::config::types::AggregateRule;

/// Run an aggregation rule against collected sections.
///
/// Extracts numeric values from section items using a regex pattern,
/// producing sum and/or count results as string key-value pairs.
pub fn run_aggregate(rule: &AggregateRule, sections: &SectionMap) -> HashMap<String, String> {
    let mut result = HashMap::new();

    let Some(section_data) = sections.get(&rule.from) else {
        return result;
    };

    let Ok(re) = Regex::new(&rule.pattern) else {
        return result;
    };

    let mut sum: i64 = 0;
    let mut count: usize = 0;

    for item in section_data.items() {
        if let Some(caps) = re.captures(item) {
            count += 1;
            if let Some(m) = caps.get(1)
                && let Ok(n) = m.as_str().parse::<i64>()
            {
                sum += n;
            }
        }
    }

    if let Some(ref sum_name) = rule.sum {
        result.insert(sum_name.clone(), sum.to_string());
    }

    if let Some(ref count_name) = rule.count_as {
        result.insert(count_name.clone(), count.to_string());
    }

    result
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::filter::section::SectionData;

    fn make_sections(name: &str, items: Vec<&str>) -> SectionMap {
        let mut map = SectionMap::new();
        map.insert(
            name.to_string(),
            SectionData {
                lines: items.into_iter().map(String::from).collect(),
                blocks: Vec::new(),
            },
        );
        map
    }

    fn rule(from: &str, pattern: &str, sum: Option<&str>, count_as: Option<&str>) -> AggregateRule {
        AggregateRule {
            from: from.to_string(),
            pattern: pattern.to_string(),
            sum: sum.map(String::from),
            count_as: count_as.map(String::from),
        }
    }

    #[test]
    fn sum_and_count_cargo_test_scenario() {
        let sections = make_sections(
            "summary",
            vec![
                "test result: ok. 12 passed; 0 failed",
                "test result: ok. 8 passed; 0 failed",
            ],
        );
        let r = rule(
            "summary",
            r"ok\. (\d+) passed",
            Some("passed"),
            Some("suites"),
        );
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["passed"], "20");
        assert_eq!(result["suites"], "2");
    }

    #[test]
    fn sum_only() {
        let sections = make_sections("data", vec!["count: 5", "count: 3"]);
        let r = rule("data", r"count: (\d+)", Some("total"), None);
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["total"], "8");
        assert!(!result.contains_key("count"));
    }

    #[test]
    fn count_only() {
        let sections = make_sections("data", vec!["match", "match", "no"]);
        let r = rule("data", r"^match$", None, Some("hits"));
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["hits"], "2");
    }

    #[test]
    fn missing_section_empty() {
        let sections = SectionMap::new();
        let r = rule("nonexistent", r"(\d+)", Some("total"), None);
        let result = run_aggregate(&r, &sections);
        assert!(result.is_empty());
    }

    #[test]
    fn invalid_regex_empty() {
        let sections = make_sections("data", vec!["a"]);
        let r = rule("data", r"[invalid", Some("total"), None);
        let result = run_aggregate(&r, &sections);
        assert!(result.is_empty());
    }

    #[test]
    fn no_matches_zero() {
        let sections = make_sections("data", vec!["no numbers here"]);
        let r = rule("data", r"(\d+)", Some("total"), Some("count"));
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["total"], "0");
        assert_eq!(result["count"], "0");
    }

    #[test]
    fn non_numeric_capture_skipped_for_sum() {
        let sections = make_sections("data", vec!["val: abc", "val: 5"]);
        let r = rule("data", r"val: (\S+)", Some("total"), Some("count"));
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["total"], "5");
        assert_eq!(result["count"], "2"); // both matched, even though "abc" isn't numeric
    }

    #[test]
    fn multiple_matches_across_items() {
        let sections = make_sections(
            "data",
            vec![
                "test result: ok. 3 passed",
                "test result: ok. 7 passed",
                "test result: ok. 10 passed",
            ],
        );
        let r = rule("data", r"ok\. (\d+) passed", Some("passed"), Some("suites"));
        let result = run_aggregate(&r, &sections);
        assert_eq!(result["passed"], "20");
        assert_eq!(result["suites"], "3");
    }
}
