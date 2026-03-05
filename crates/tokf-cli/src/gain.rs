use std::io::{BufRead, IsTerminal, Write};

use tokf::auth::credentials;
use tokf::remote::gain_client;
use tokf::remote::http::Client;
use tokf::tracking;

use crate::gain_render;

#[allow(clippy::fn_params_excessive_bools)]
pub fn cmd_gain(daily: bool, by_filter: bool, json: bool, top: usize, no_color: bool) -> i32 {
    prompt_upload_stats_if_needed();

    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine DB path");
        return 1;
    };
    let conn = match tracking::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            return 1;
        }
    };

    if daily {
        cmd_gain_daily(&conn, json)
    } else if by_filter {
        cmd_gain_by_filter(&conn, json)
    } else {
        cmd_gain_summary(&conn, json, top, no_color)
    }
}

fn cmd_gain_summary(conn: &rusqlite::Connection, json: bool, top: usize, no_color: bool) -> i32 {
    let summary = match tracking::query_summary(conn) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            return 1;
        }
    };

    if json {
        crate::output::print_json(&summary);
        return 0;
    }

    let filters = match tracking::query_by_filter(conn) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            return 1;
        }
    };

    if std::io::stdout().is_terminal() {
        let colors = if gain_render::should_disable_color(no_color) {
            gain_render::ColorMode::new(false)
        } else {
            gain_render::ColorMode::new(true)
        };
        print!(
            "{}",
            gain_render::render_summary_tty(&summary, &filters, top, &colors)
        );
    } else {
        print!(
            "{}",
            gain_render::render_summary_plain(&summary, &filters, top)
        );
    }
    0
}

fn query_and_print<T, Q, F>(
    conn: &rusqlite::Connection,
    json: bool,
    header: &str,
    query: Q,
    fmt_row: F,
) -> i32
where
    T: serde::Serialize,
    Q: FnOnce(&rusqlite::Connection) -> anyhow::Result<Vec<T>>,
    F: Fn(&T) -> String,
{
    match query(conn) {
        Ok(rows) => {
            if json {
                crate::output::print_json(&rows);
            } else {
                println!("{header}");
                for r in &rows {
                    println!("{}", fmt_row(r));
                }
            }
            0
        }
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            1
        }
    }
}

fn fmt_gain_row(
    label: &str,
    commands: i64,
    tokens_saved: i64,
    savings_pct: f64,
    pipe_override_count: i64,
) -> String {
    let override_note = if pipe_override_count > 0 {
        format!("  pipe: {pipe_override_count}")
    } else {
        String::new()
    };
    format!(
        "  {label}  runs: {commands:4}  saved: {} est. ({savings_pct:.1}%){override_note}",
        gain_render::format_num(tokens_saved),
    )
}

fn cmd_gain_by_filter(conn: &rusqlite::Connection, json: bool) -> i32 {
    query_and_print(
        conn,
        json,
        "tokf gain by filter",
        tracking::query_by_filter,
        |r| {
            fmt_gain_row(
                &format!("{:30}", r.filter_name),
                r.commands,
                r.tokens_saved,
                r.savings_pct,
                r.pipe_override_count,
            )
        },
    )
}

fn cmd_gain_daily(conn: &rusqlite::Connection, json: bool) -> i32 {
    query_and_print(conn, json, "tokf gain daily", tracking::query_daily, |r| {
        fmt_gain_row(
            &r.date,
            r.commands,
            r.tokens_saved,
            r.savings_pct,
            r.pipe_override_count,
        )
    })
}

#[allow(clippy::fn_params_excessive_bools)]
pub fn cmd_gain_remote(
    daily: bool,
    by_filter: bool,
    json: bool,
    top: usize,
    no_color: bool,
) -> i32 {
    if daily {
        eprintln!("[tokf] --daily is not available for remote stats");
        return 1;
    }

    let client = match Client::authed() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] {e:#}");
            return 1;
        }
    };

    let resp = match gain_client::get_gain(&client) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tokf] error: {e:#}");
            return 1;
        }
    };

    if json {
        crate::output::print_json(&resp);
        return 0;
    }

    let (summary, filters) = gain_render::from_remote(&resp);

    if by_filter {
        println!("tokf gain by filter (remote)");
        for f in &filters {
            let override_note = if f.pipe_override_count > 0 {
                format!("  pipe: {}", f.pipe_override_count)
            } else {
                String::new()
            };
            println!(
                "  {:30}  runs: {:4}  saved: {} est. ({:.1}%){override_note}",
                f.filter_name,
                f.commands,
                gain_render::format_num(f.tokens_saved),
                f.savings_pct
            );
        }
        return 0;
    }

    if std::io::stdout().is_terminal() {
        let colors = if gain_render::should_disable_color(no_color) {
            gain_render::ColorMode::new(false)
        } else {
            gain_render::ColorMode::new(true)
        };
        print!(
            "{}",
            gain_render::render_summary_tty(&summary, &filters, top, &colors)
        );
    } else {
        print!(
            "{}",
            gain_render::render_summary_plain(&summary, &filters, top)
        );
    }
    0
}

/// One-time prompt for existing users who are logged in but haven't set their
/// usage statistics preference yet. Only shows when stdin is a TTY so it won't
/// pollute LLM/piped contexts.
fn prompt_upload_stats_if_needed() {
    if !std::io::stdin().is_terminal() {
        return;
    }

    if credentials::load().is_none() {
        return;
    }

    let config = tokf::history::SyncConfig::load(None);
    if config.upload_usage_stats.is_some() {
        return; // already set
    }

    eprintln!("[tokf] You're logged in but haven't set your usage statistics preference.");
    eprintln!("[tokf] tokf can periodically sync aggregate token counts in the background.");
    eprintln!("[tokf] No command content or output is ever sent.");
    eprint!("[tokf] Upload usage statistics? [y/N]: ");
    let _ = std::io::stderr().flush();

    let mut input = String::new();
    if std::io::stdin().lock().read_line(&mut input).is_err() {
        return;
    }
    let enabled =
        input.trim().eq_ignore_ascii_case("y") || input.trim().eq_ignore_ascii_case("yes");

    if let Err(e) = tokf::history::save_upload_stats(enabled) {
        eprintln!("[tokf] Failed to save preference: {e:#}");
        return;
    }

    if enabled {
        eprintln!("[tokf] Usage statistics upload enabled.");
    } else {
        eprintln!("[tokf] Usage statistics upload disabled.");
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_num_basic() {
        assert_eq!(gain_render::format_num(0), "0");
        assert_eq!(gain_render::format_num(999), "999");
        assert_eq!(gain_render::format_num(1000), "1,000");
        assert_eq!(gain_render::format_num(84320), "84,320");
        assert_eq!(gain_render::format_num(-73080), "-73,080");
    }

    #[test]
    fn cmd_gain_remote_daily_returns_error() {
        // --daily is not supported for remote stats; should return 1 without network.
        let code = cmd_gain_remote(true, false, false, 10, false);
        assert_eq!(code, 1);
    }

    #[test]
    fn from_remote_converts_correctly() {
        let resp = gain_client::GainResponse {
            total_input_tokens: 10_000,
            total_output_tokens: 2_000,
            total_commands: 5,
            by_machine: vec![],
            by_filter: vec![gain_client::FilterGainEntry {
                filter_name: Some("git/status".to_string()),
                filter_hash: Some("abc".to_string()),
                total_input_tokens: 5_000,
                total_output_tokens: 1_000,
                total_commands: 3,
            }],
        };
        let (summary, filters) = gain_render::from_remote(&resp);
        assert_eq!(summary.total_commands, 5);
        assert_eq!(summary.tokens_saved, 8_000);
        assert_eq!(summary.total_filter_time_ms, 0);
        assert_eq!(filters.len(), 1);
        assert_eq!(filters[0].filter_name, "git/status");
        assert_eq!(filters[0].tokens_saved, 4_000);
    }

    #[test]
    fn from_remote_zero_tokens_no_panic() {
        let resp = gain_client::GainResponse {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_commands: 0,
            by_machine: vec![],
            by_filter: vec![],
        };
        let (summary, filters) = gain_render::from_remote(&resp);
        assert!(summary.savings_pct.abs() < f64::EPSILON);
        assert!(filters.is_empty());
    }

    #[test]
    fn from_remote_null_filter_name_becomes_passthrough() {
        let resp = gain_client::GainResponse {
            total_input_tokens: 100,
            total_output_tokens: 50,
            total_commands: 1,
            by_machine: vec![],
            by_filter: vec![gain_client::FilterGainEntry {
                filter_name: None,
                filter_hash: None,
                total_input_tokens: 100,
                total_output_tokens: 50,
                total_commands: 1,
            }],
        };
        let (_, filters) = gain_render::from_remote(&resp);
        assert_eq!(filters[0].filter_name, "passthrough");
    }
}
