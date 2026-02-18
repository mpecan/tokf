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
                match serde_json::to_string_pretty(&s) {
                    Ok(out) => println!("{out}"),
                    Err(e) => {
                        eprintln!("[tokf] error: {e}");
                        return 1;
                    }
                }
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
                match serde_json::to_string_pretty(&rows) {
                    Ok(out) => println!("{out}"),
                    Err(e) => {
                        eprintln!("[tokf] error: {e}");
                        return 1;
                    }
                }
            } else {
                println!("tokf gain by filter");
                for r in &rows {
                    println!(
                        "  {:30}  runs: {:4}  saved: {} est. ({:.1}%)",
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
                match serde_json::to_string_pretty(&rows) {
                    Ok(out) => println!("{out}"),
                    Err(e) => {
                        eprintln!("[tokf] error: {e}");
                        return 1;
                    }
                }
            } else {
                println!("tokf gain daily");
                for r in &rows {
                    println!(
                        "  {}  runs: {:4}  saved: {} est. ({:.1}%)",
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
}
