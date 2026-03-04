use std::fmt;
use std::io::IsTerminal as _;

use tokf::remote::filter_client::{self, FilterSummary};
use tokf::remote::http::Client;

/// Entry point for the `tokf search` subcommand.
pub fn cmd_search(query: &str, limit: usize, json: bool) -> i32 {
    match search(query, limit, json) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn search(query: &str, limit: usize, json: bool) -> anyhow::Result<i32> {
    let client = Client::authed()?;

    let results = filter_client::search_filters(&client, query, limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(0);
    }

    if results.is_empty() {
        eprintln!("[tokf] no filters found");
        return Ok(0);
    }

    if std::io::stderr().is_terminal() {
        interactive_select(&results)
    } else {
        print_table(&results);
        Ok(0)
    }
}

fn interactive_select(results: &[FilterSummary]) -> anyhow::Result<i32> {
    let items: Vec<SelectableFilter<'_>> = results.iter().map(SelectableFilter).collect();

    eprintln!();
    let selection = dialoguer::Select::new()
        .with_prompt("Select a filter to install (Esc to cancel)")
        .items(&items)
        .default(0)
        .interact_on_opt(&dialoguer::console::Term::stderr())?;

    selection.map_or_else(
        || {
            eprintln!("[tokf] cancelled");
            Ok(0)
        },
        |idx| {
            let selected = &results[idx];
            eprintln!();
            Ok(crate::install_cmd::cmd_install(
                &selected.content_hash,
                false, // local
                false, // force
                false, // dry_run
                true,  // yes — interactive selection is itself confirmation
            ))
        },
    )
}

struct SelectableFilter<'a>(&'a FilterSummary);

impl fmt::Display for SelectableFilter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let r = self.0;
        write!(f, "{}", r.command_pattern)?;
        if r.is_stdlib {
            write!(f, " [stdlib]")?;
        }
        write!(f, "  @{}", r.author)?;
        write!(f, "  savings:{:.0}%", r.savings_pct)?;
        write!(f, "  tests:{}", r.test_count)?;
        write!(f, "  runs:{}", format_number(r.total_commands))?;
        Ok(())
    }
}

fn print_table(results: &[FilterSummary]) {
    let cmd_width = results
        .iter()
        .map(|r| display_command(r).len())
        .max()
        .unwrap_or(7)
        .max(7);
    let author_width = results
        .iter()
        .map(|r| r.author.len())
        .max()
        .unwrap_or(6)
        .max(6);

    eprintln!(
        "{:<cmd_width$}  {:<author_width$}  {:>8}  {:>5}  {:>8}",
        "COMMAND",
        "AUTHOR",
        "SAVINGS%",
        "TESTS",
        "RUNS",
        cmd_width = cmd_width,
        author_width = author_width,
    );
    eprintln!(
        "{:-<cmd_width$}  {:-<author_width$}  {:->8}  {:->5}  {:->8}",
        "",
        "",
        "",
        "",
        "",
        cmd_width = cmd_width,
        author_width = author_width,
    );

    for r in results {
        eprintln!(
            "{:<cmd_width$}  {:<author_width$}  {:>7.1}%  {:>5}  {:>8}",
            display_command(r),
            r.author,
            r.savings_pct,
            r.test_count,
            format_number(r.total_commands),
            cmd_width = cmd_width,
            author_width = author_width,
        );
    }
}

fn display_command(r: &FilterSummary) -> String {
    if r.is_stdlib {
        format!("{} [stdlib]", r.command_pattern)
    } else {
        r.command_pattern.clone()
    }
}

fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn format_number_small() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(999), "999");
    }

    #[test]
    fn format_number_thousands() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1_000_000), "1,000,000");
    }

    fn make_summary(command: &str, is_stdlib: bool) -> FilterSummary {
        FilterSummary {
            content_hash: String::new(),
            command_pattern: command.to_string(),
            author: String::new(),
            savings_pct: 0.0,
            total_commands: 0,
            created_at: String::new(),
            test_count: 0,
            is_stdlib,
        }
    }

    #[test]
    fn display_command_appends_stdlib_badge() {
        let r = make_summary("git push", true);
        assert_eq!(display_command(&r), "git push [stdlib]");
    }

    #[test]
    fn display_command_no_badge_for_community() {
        let r = make_summary("git push", false);
        assert_eq!(display_command(&r), "git push");
    }

    #[test]
    fn selectable_filter_display_stdlib() {
        let summary = FilterSummary {
            content_hash: "abc123".to_string(),
            command_pattern: "git push".to_string(),
            author: "mpecan".to_string(),
            savings_pct: 45.0,
            total_commands: 12234,
            created_at: String::new(),
            test_count: 3,
            is_stdlib: true,
        };
        let display = format!("{}", SelectableFilter(&summary));
        assert_eq!(
            display,
            "git push [stdlib]  @mpecan  savings:45%  tests:3  runs:12,234"
        );
    }

    #[test]
    fn selectable_filter_display_community() {
        let summary = FilterSummary {
            content_hash: "def456".to_string(),
            command_pattern: "cargo build".to_string(),
            author: "alice".to_string(),
            savings_pct: 72.8,
            total_commands: 500,
            created_at: String::new(),
            test_count: 0,
            is_stdlib: false,
        };
        let display = format!("{}", SelectableFilter(&summary));
        assert_eq!(
            display,
            "cargo build  @alice  savings:73%  tests:0  runs:500"
        );
    }

    #[test]
    fn selectable_filter_display_zero_savings() {
        let summary = FilterSummary {
            content_hash: String::new(),
            command_pattern: "npm test".to_string(),
            author: "bob".to_string(),
            savings_pct: 0.0,
            total_commands: 0,
            created_at: String::new(),
            test_count: 1,
            is_stdlib: false,
        };
        let display = format!("{}", SelectableFilter(&summary));
        assert_eq!(display, "npm test  @bob  savings:0%  tests:1  runs:0");
    }

    #[test]
    fn selectable_filter_display_large_runs() {
        let summary = FilterSummary {
            content_hash: String::new(),
            command_pattern: "git status".to_string(),
            author: "dev".to_string(),
            savings_pct: 90.0,
            total_commands: 1_234_567_890,
            created_at: String::new(),
            test_count: 10,
            is_stdlib: true,
        };
        let display = format!("{}", SelectableFilter(&summary));
        assert_eq!(
            display,
            "git status [stdlib]  @dev  savings:90%  tests:10  runs:1,234,567,890"
        );
    }

    #[test]
    fn print_table_column_alignment() {
        // Capture output by calling format functions directly — print_table writes
        // to stderr which we can't easily capture, so we verify the building blocks.
        let results = [
            FilterSummary {
                content_hash: String::new(),
                command_pattern: "git push".to_string(),
                author: "alice".to_string(),
                savings_pct: 42.3,
                total_commands: 1234,
                created_at: String::new(),
                test_count: 2,
                is_stdlib: true,
            },
            FilterSummary {
                content_hash: String::new(),
                command_pattern: "cargo build".to_string(),
                author: "bob".to_string(),
                savings_pct: 80.0,
                total_commands: 500,
                created_at: String::new(),
                test_count: 0,
                is_stdlib: false,
            },
        ];

        // Verify cmd_width calculation (stdlib badge adds 9 chars)
        let cmd_width = results
            .iter()
            .map(|r| display_command(r).len())
            .max()
            .unwrap()
            .max(7);
        // "git push [stdlib]" = 17 chars, "cargo build" = 11 chars → max is 17
        assert_eq!(cmd_width, 17);

        // Verify author_width
        let author_width = results.iter().map(|r| r.author.len()).max().unwrap().max(6);
        // "alice" = 5, "bob" = 3, min 6 → 6
        assert_eq!(author_width, 6);

        // Verify row formatting produces consistent-width output
        let row1 = format!(
            "{:<cmd_width$}  {:<author_width$}  {:>7.1}%  {:>5}  {:>8}",
            display_command(&results[0]),
            results[0].author,
            results[0].savings_pct,
            results[0].test_count,
            format_number(results[0].total_commands),
        );
        let row2 = format!(
            "{:<cmd_width$}  {:<author_width$}  {:>7.1}%  {:>5}  {:>8}",
            display_command(&results[1]),
            results[1].author,
            results[1].savings_pct,
            results[1].test_count,
            format_number(results[1].total_commands),
        );
        assert_eq!(row1.len(), row2.len(), "rows should have equal width");
    }
}
