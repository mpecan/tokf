use tokf::remote::gain_client;
use tokf::remote::http::Client;
use tokf::tracking;

pub fn cmd_gain(daily: bool, by_filter: bool, json: bool) -> i32 {
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
        cmd_gain_summary(&conn, json)
    }
}

fn cmd_gain_summary(conn: &rusqlite::Connection, json: bool) -> i32 {
    match tracking::query_summary(conn) {
        Ok(s) => {
            if json {
                crate::output::print_json(&s);
            } else {
                println!("tokf gain summary");
                println!("  total runs:     {}", s.total_commands);
                println!(
                    "  input tokens:   {} est.",
                    format_num(s.total_input_tokens)
                );
                println!(
                    "  output tokens:  {} est.",
                    format_num(s.total_output_tokens)
                );
                println!(
                    "  tokens saved:   {} est. ({:.1}%)",
                    format_num(s.tokens_saved),
                    s.savings_pct
                );
                if s.pipe_override_count > 0 {
                    println!(
                        "  pipe preferred: {} runs (pipe output was smaller than filter)",
                        s.pipe_override_count
                    );
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

fn cmd_gain_by_filter(conn: &rusqlite::Connection, json: bool) -> i32 {
    match tracking::query_by_filter(conn) {
        Ok(rows) => {
            if json {
                crate::output::print_json(&rows);
            } else {
                println!("tokf gain by filter");
                for r in &rows {
                    let override_note = if r.pipe_override_count > 0 {
                        format!("  pipe: {}", r.pipe_override_count)
                    } else {
                        String::new()
                    };
                    println!(
                        "  {:30}  runs: {:4}  saved: {} est. ({:.1}%){override_note}",
                        r.filter_name,
                        r.commands,
                        format_num(r.tokens_saved),
                        r.savings_pct
                    );
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

fn cmd_gain_daily(conn: &rusqlite::Connection, json: bool) -> i32 {
    match tracking::query_daily(conn) {
        Ok(rows) => {
            if json {
                crate::output::print_json(&rows);
            } else {
                println!("tokf gain daily");
                for r in &rows {
                    let override_note = if r.pipe_override_count > 0 {
                        format!("  pipe: {}", r.pipe_override_count)
                    } else {
                        String::new()
                    };
                    println!(
                        "  {}  runs: {:4}  saved: {} est. ({:.1}%){override_note}",
                        r.date,
                        r.commands,
                        format_num(r.tokens_saved),
                        r.savings_pct
                    );
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

pub fn cmd_gain_remote(daily: bool, by_filter: bool, json: bool) -> i32 {
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

    if by_filter {
        return cmd_gain_remote_by_filter(&resp);
    }

    cmd_gain_remote_summary(&resp)
}

fn cmd_gain_remote_summary(resp: &gain_client::GainResponse) -> i32 {
    let tokens_saved = resp.total_input_tokens - resp.total_output_tokens;
    let savings_pct = if resp.total_input_tokens == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let pct = tokens_saved as f64 / resp.total_input_tokens as f64 * 100.0;
        pct
    };

    println!("tokf gain summary (remote)");
    println!("  total runs:     {}", resp.total_commands);
    println!(
        "  input tokens:   {} est.",
        format_num(resp.total_input_tokens)
    );
    println!(
        "  output tokens:  {} est.",
        format_num(resp.total_output_tokens)
    );
    println!(
        "  tokens saved:   {} est. ({:.1}%)",
        format_num(tokens_saved),
        savings_pct
    );
    0
}

fn cmd_gain_remote_by_filter(resp: &gain_client::GainResponse) -> i32 {
    println!("tokf gain by filter (remote)");
    for entry in &resp.by_filter {
        let name = entry.filter_name.as_deref().unwrap_or("passthrough");
        let saved = entry.total_input_tokens - entry.total_output_tokens;
        let pct = if entry.total_input_tokens == 0 {
            0.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let p = saved as f64 / entry.total_input_tokens as f64 * 100.0;
            p
        };
        println!(
            "  {:30}  runs: {:4}  saved: {} est. ({:.1}%)",
            name,
            entry.total_commands,
            format_num(saved),
            pct
        );
    }
    0
}

fn format_num(n: i64) -> String {
    // Simple thousands-separator formatting without extra deps.
    let s = n.abs().to_string();
    let chunks: Vec<&str> = s
        .as_bytes()
        .rchunks(3)
        .rev()
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect();
    let formatted = chunks.join(",");
    if n < 0 {
        format!("-{formatted}")
    } else {
        formatted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_num_basic() {
        assert_eq!(format_num(0), "0");
        assert_eq!(format_num(999), "999");
        assert_eq!(format_num(1000), "1,000");
        assert_eq!(format_num(84320), "84,320");
        assert_eq!(format_num(-73080), "-73,080");
    }

    #[test]
    fn cmd_gain_remote_daily_returns_error() {
        // --daily is not supported for remote stats; should return 1 without network.
        let code = cmd_gain_remote(true, false, false);
        assert_eq!(code, 1);
    }

    #[test]
    fn cmd_gain_remote_summary_returns_zero() {
        let resp = gain_client::GainResponse {
            total_input_tokens: 10_000,
            total_output_tokens: 2_000,
            total_commands: 5,
            by_machine: vec![],
            by_filter: vec![],
        };
        let code = cmd_gain_remote_summary(&resp);
        assert_eq!(code, 0);
    }

    #[test]
    fn cmd_gain_remote_by_filter_returns_zero() {
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
        let code = cmd_gain_remote_by_filter(&resp);
        assert_eq!(code, 0);
    }

    #[test]
    fn cmd_gain_remote_summary_zero_tokens() {
        let resp = gain_client::GainResponse {
            total_input_tokens: 0,
            total_output_tokens: 0,
            total_commands: 0,
            by_machine: vec![],
            by_filter: vec![],
        };
        // Should not panic on zero division.
        let code = cmd_gain_remote_summary(&resp);
        assert_eq!(code, 0);
    }
}
