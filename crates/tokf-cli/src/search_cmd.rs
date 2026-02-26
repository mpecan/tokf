use tokf::remote::{filter_client, http};

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
    let auth = http::load_auth()?;
    let client = http::build_client(http::LIGHT_TIMEOUT_SECS)?;

    let results =
        filter_client::search_filters(&client, &auth.server_url, &auth.token, query, limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(0);
    }

    if results.is_empty() {
        eprintln!("[tokf] no filters found");
        return Ok(0);
    }

    print_table(&results);
    Ok(0)
}

fn print_table(results: &[filter_client::FilterSummary]) {
    let cmd_width = results
        .iter()
        .map(|r| r.command_pattern.len())
        .max()
        .unwrap_or(7)
        .max(7);
    let author_width = results
        .iter()
        .map(|r| r.author.len())
        .max()
        .unwrap_or(6)
        .max(6);

    println!(
        "{:<cmd_width$}  {:<author_width$}  {:>8}  {:>8}",
        "COMMAND",
        "AUTHOR",
        "SAVINGS%",
        "INSTALLS",
        cmd_width = cmd_width,
        author_width = author_width,
    );
    println!(
        "{:-<cmd_width$}  {:-<author_width$}  {:->8}  {:->8}",
        "",
        "",
        "",
        "",
        cmd_width = cmd_width,
        author_width = author_width,
    );

    for r in results {
        println!(
            "{:<cmd_width$}  {:<author_width$}  {:>7.1}%  {:>8}",
            r.command_pattern,
            r.author,
            r.savings_pct,
            format_number(r.total_commands),
            cmd_width = cmd_width,
            author_width = author_width,
        );
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
}
