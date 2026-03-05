use std::collections::HashMap;

use regex::Regex;

use tokf_common::config::types::Section;

/// Collected data for a single named section.
pub type SectionMap = HashMap<String, SectionData>;

/// Lines or blocks collected by a section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SectionData {
    pub lines: Vec<String>,
    pub blocks: Vec<String>,
}

impl SectionData {
    /// Block count if `split_on` was used, otherwise line count.
    pub const fn count(&self) -> usize {
        if self.blocks.is_empty() {
            self.lines.len()
        } else {
            self.blocks.len()
        }
    }

    /// Blocks if available, otherwise lines.
    pub fn items(&self) -> &[String] {
        if self.blocks.is_empty() {
            &self.lines
        } else {
            &self.blocks
        }
    }
}

/// Internal per-section tracking during the collection pass.
struct SectionRunner {
    collect_as: String,
    enter_re: Option<Regex>,
    exit_re: Option<Regex>,
    match_re: Option<Regex>,
    split_re: Option<Regex>,
    is_stateful: bool,
    active: bool,
    collected: Vec<String>,
}

/// Compile an optional regex pattern, returning `None` if absent or invalid.
fn compile_optional(pattern: Option<&String>) -> Option<Regex> {
    pattern.and_then(|p| Regex::new(p).ok())
}

impl SectionRunner {
    fn new(section: &Section) -> Option<Self> {
        let collect_as = section.collect_as.as_ref()?;

        let enter_re = compile_optional(section.enter.as_ref());
        let exit_re = compile_optional(section.exit.as_ref());
        let match_re = compile_optional(section.match_pattern.as_ref());
        let split_re = compile_optional(section.split_on.as_ref());

        // Skip section if any specified regex failed to compile
        if section.enter.is_some() && enter_re.is_none()
            || section.exit.is_some() && exit_re.is_none()
            || section.match_pattern.is_some() && match_re.is_none()
            || section.split_on.is_some() && split_re.is_none()
        {
            return None;
        }

        let is_stateful = section.enter.is_some();

        Some(Self {
            collect_as: collect_as.clone(),
            enter_re,
            exit_re,
            match_re,
            split_re,
            is_stateful,
            active: !is_stateful, // stateless sections are always active
            collected: Vec::new(),
        })
    }

    fn process_line(&mut self, line: &str) {
        if self.is_stateful {
            // Check enter/exit transitions
            if !self.active {
                if let Some(ref re) = self.enter_re
                    && re.is_match(line)
                {
                    self.active = true;
                }
                return; // enter line not collected (or not active)
            }

            // Active — check exit
            if let Some(ref re) = self.exit_re
                && re.is_match(line)
            {
                self.active = false;
                return; // exit line not collected
            }
        }

        // Collect (filtered by match if present)
        self.collect_if_matches(line);
    }

    fn collect_if_matches(&mut self, line: &str) {
        if let Some(ref re) = self.match_re {
            if re.is_match(line) {
                self.collected.push(line.to_string());
            }
        } else {
            self.collected.push(line.to_string());
        }
    }

    fn finish(self) -> (String, SectionData) {
        let mut data = SectionData {
            lines: self.collected,
            blocks: Vec::new(),
        };

        if let Some(ref re) = self.split_re {
            data.blocks = split_into_blocks(&data.lines, re);
        }

        (self.collect_as, data)
    }
}

/// Split collected lines into blocks using a separator regex.
/// Consecutive separators do not produce empty blocks.
fn split_into_blocks(lines: &[String], separator: &Regex) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for line in lines {
        if separator.is_match(line) {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }

    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }

    blocks
}

/// Run all section definitions over the input lines, collecting into a `SectionMap`.
///
/// If multiple sections share the same `collect_as` name, the last one wins (`HashMap` insert order).
pub fn collect_sections(sections: &[Section], lines: &[&str]) -> SectionMap {
    let mut runners: Vec<SectionRunner> = sections.iter().filter_map(SectionRunner::new).collect();

    for line in lines {
        for runner in &mut runners {
            runner.process_line(line);
        }
    }

    runners.into_iter().map(SectionRunner::finish).collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_arguments)]
mod tests {
    use super::*;

    fn section(
        name: &str,
        enter: Option<&str>,
        exit: Option<&str>,
        match_pat: Option<&str>,
        split_on: Option<&str>,
        collect_as: &str,
    ) -> Section {
        Section {
            name: Some(name.to_string()),
            enter: enter.map(String::from),
            exit: exit.map(String::from),
            match_pattern: match_pat.map(String::from),
            split_on: split_on.map(String::from),
            collect_as: Some(collect_as.to_string()),
        }
    }

    #[test]
    fn stateful_basic() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            None,
            "data",
        )];
        let lines: Vec<&str> = vec!["noise", "BEGIN", "line1", "line2", "END", "noise"];
        let map = collect_sections(&sections, &lines);
        let data = &map["data"];
        assert_eq!(data.lines, vec!["line1", "line2"]);
        assert!(data.blocks.is_empty());
        assert_eq!(data.count(), 2);
    }

    #[test]
    fn stateful_with_match_filter() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            Some("^keep"),
            None,
            "data",
        )];
        let lines: Vec<&str> = vec!["BEGIN", "keep1", "drop", "keep2", "END"];
        let map = collect_sections(&sections, &lines);
        assert_eq!(map["data"].lines, vec!["keep1", "keep2"]);
    }

    #[test]
    fn stateful_with_split_on() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            Some("^---$"),
            "data",
        )];
        let lines: Vec<&str> = vec!["BEGIN", "a", "b", "---", "c", "d", "END"];
        let map = collect_sections(&sections, &lines);
        let data = &map["data"];
        assert_eq!(data.blocks, vec!["a\nb", "c\nd"]);
        assert_eq!(data.count(), 2);
        assert_eq!(data.items(), &["a\nb".to_string(), "c\nd".to_string()]);
    }

    #[test]
    fn stateless_match_only() {
        let sections = vec![section(
            "s",
            None,
            None,
            Some("^test result:"),
            None,
            "summary",
        )];
        let lines: Vec<&str> = vec![
            "running 5 tests",
            "test result: ok. 5 passed",
            "running 3 tests",
            "test result: ok. 3 passed",
        ];
        let map = collect_sections(&sections, &lines);
        assert_eq!(
            map["summary"].lines,
            vec!["test result: ok. 5 passed", "test result: ok. 3 passed"]
        );
    }

    #[test]
    fn multiple_simultaneous_sections() {
        let sections = vec![
            section(
                "failures",
                Some("^failures:$"),
                Some("^test result:"),
                None,
                None,
                "blocks",
            ),
            section(
                "names",
                Some("^failures:$"),
                Some("^$"),
                Some(r"^\s+\S+"),
                None,
                "names",
            ),
        ];
        let lines: Vec<&str> = vec![
            "failures:",
            "    test_one",
            "    test_two",
            "",
            "test result: FAILED",
        ];
        let map = collect_sections(&sections, &lines);
        // "blocks" collects everything between failures: and test result:
        assert_eq!(
            map["blocks"].lines,
            vec!["    test_one", "    test_two", ""]
        );
        // "names" collects only matching lines between failures: and blank line
        assert_eq!(map["names"].lines, vec!["    test_one", "    test_two"]);
    }

    #[test]
    fn never_enters() {
        let sections = vec![section(
            "s",
            Some("^NEVER$"),
            Some("^END$"),
            None,
            None,
            "data",
        )];
        let lines: Vec<&str> = vec!["a", "b", "c"];
        let map = collect_sections(&sections, &lines);
        assert!(map["data"].lines.is_empty());
        assert_eq!(map["data"].count(), 0);
    }

    #[test]
    fn enters_but_never_exits() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            None,
            "data",
        )];
        let lines: Vec<&str> = vec!["BEGIN", "a", "b", "c"];
        let map = collect_sections(&sections, &lines);
        assert_eq!(map["data"].lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn reentry_after_exit() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            None,
            "data",
        )];
        let lines: Vec<&str> = vec!["BEGIN", "a", "END", "noise", "BEGIN", "b", "END"];
        let map = collect_sections(&sections, &lines);
        assert_eq!(map["data"].lines, vec!["a", "b"]);
    }

    #[test]
    fn invalid_regex_skipped() {
        let sections = vec![Section {
            name: Some("bad".to_string()),
            enter: Some("[invalid".to_string()),
            exit: None,
            match_pattern: None,
            split_on: None,
            collect_as: Some("data".to_string()),
        }];
        let lines: Vec<&str> = vec!["a", "b"];
        let map = collect_sections(&sections, &lines);
        // Section with invalid enter regex is skipped entirely
        assert!(!map.contains_key("data"));
    }

    #[test]
    fn no_collect_as_ignored() {
        let sections = vec![Section {
            name: Some("anon".to_string()),
            enter: Some("^BEGIN$".to_string()),
            exit: Some("^END$".to_string()),
            match_pattern: None,
            split_on: None,
            collect_as: None,
        }];
        let lines: Vec<&str> = vec!["BEGIN", "a", "END"];
        let map = collect_sections(&sections, &lines);
        assert!(map.is_empty());
    }

    #[test]
    fn empty_input() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            None,
            "data",
        )];
        let lines: Vec<&str> = vec![];
        let map = collect_sections(&sections, &lines);
        assert!(map["data"].lines.is_empty());
    }

    #[test]
    fn consecutive_split_separators_no_empty_blocks() {
        let sections = vec![section(
            "s",
            Some("^BEGIN$"),
            Some("^END$"),
            None,
            Some("^---$"),
            "data",
        )];
        let lines: Vec<&str> = vec!["BEGIN", "a", "---", "---", "b", "END"];
        let map = collect_sections(&sections, &lines);
        assert_eq!(map["data"].blocks, vec!["a", "b"]);
    }

    #[test]
    fn section_data_count_lines() {
        let data = SectionData {
            lines: vec!["a".to_string(), "b".to_string()],
            blocks: Vec::new(),
        };
        assert_eq!(data.count(), 2);
        assert_eq!(data.items(), &["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn section_data_count_blocks() {
        let data = SectionData {
            lines: vec!["a".to_string(), "b".to_string()],
            blocks: vec!["block1".to_string()],
        };
        assert_eq!(data.count(), 1);
        assert_eq!(data.items(), &["block1".to_string()]);
    }

    #[test]
    fn invalid_exit_regex_skipped() {
        let sections = vec![Section {
            name: Some("bad_exit".to_string()),
            enter: Some("^BEGIN$".to_string()),
            exit: Some("[invalid".to_string()),
            match_pattern: None,
            split_on: None,
            collect_as: Some("data".to_string()),
        }];
        let lines: Vec<&str> = vec!["BEGIN", "a"];
        let map = collect_sections(&sections, &lines);
        assert!(!map.contains_key("data"));
    }

    #[test]
    fn invalid_match_regex_skipped() {
        let sections = vec![Section {
            name: Some("bad_match".to_string()),
            enter: None,
            exit: None,
            match_pattern: Some("[invalid".to_string()),
            split_on: None,
            collect_as: Some("data".to_string()),
        }];
        let lines: Vec<&str> = vec!["a", "b"];
        let map = collect_sections(&sections, &lines);
        assert!(!map.contains_key("data"));
    }

    #[test]
    fn invalid_split_on_regex_skipped() {
        let sections = vec![Section {
            name: Some("bad_split".to_string()),
            enter: Some("^BEGIN$".to_string()),
            exit: Some("^END$".to_string()),
            match_pattern: None,
            split_on: Some("[invalid".to_string()),
            collect_as: Some("data".to_string()),
        }];
        let lines: Vec<&str> = vec!["BEGIN", "a", "END"];
        let map = collect_sections(&sections, &lines);
        assert!(!map.contains_key("data"));
    }
}
