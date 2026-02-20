use tokf::history;
use tokf::tracking;

pub fn cmd_history_list(limit: usize) -> i32 {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine history DB path");
        return 1;
    };
    let conn = match history::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            return 1;
        }
    };

    let entries = match history::list_history(&conn, limit) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[tokf] error listing history: {e:#}");
            return 1;
        }
    };

    if entries.is_empty() {
        eprintln!("[tokf] no history entries found");
        return 0;
    }

    for entry in entries {
        let filter = entry.filter_name.as_deref().unwrap_or("passthrough");
        let exit_status = if entry.exit_code == 0 {
            "✓".to_string()
        } else {
            format!("✗({})", entry.exit_code)
        };
        println!(
            "{} {} {} [{}] {}",
            entry.id, entry.timestamp, exit_status, filter, entry.command
        );
    }
    0
}

pub fn cmd_history_show(id: i64) -> i32 {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine history DB path");
        return 1;
    };
    let conn = match history::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            return 1;
        }
    };

    let entry = match history::get_history_entry(&conn, id) {
        Ok(Some(e)) => e,
        Ok(None) => {
            eprintln!("[tokf] history entry {id} not found");
            return 1;
        }
        Err(e) => {
            eprintln!("[tokf] error getting history entry: {e:#}");
            return 1;
        }
    };

    println!("ID: {}", entry.id);
    println!("Timestamp: {}", entry.timestamp);
    println!("Command: {}", entry.command);
    println!(
        "Filter: {}",
        entry.filter_name.as_deref().unwrap_or("passthrough")
    );
    println!("Exit Code: {}", entry.exit_code);
    println!("\n--- Raw Output ---");
    println!("{}", entry.raw_output);
    println!("\n--- Filtered Output ---");
    println!("{}", entry.filtered_output);
    0
}

pub fn cmd_history_search(query: &str, limit: usize) -> i32 {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine history DB path");
        return 1;
    };
    let conn = match history::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            return 1;
        }
    };

    let entries = match history::search_history(&conn, query, limit) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("[tokf] error searching history: {e:#}");
            return 1;
        }
    };

    if entries.is_empty() {
        eprintln!("[tokf] no matching history entries found");
        return 0;
    }

    for entry in entries {
        let filter = entry.filter_name.as_deref().unwrap_or("passthrough");
        let exit_status = if entry.exit_code == 0 {
            "✓".to_string()
        } else {
            format!("✗({})", entry.exit_code)
        };
        println!(
            "{} {} {} [{}] {}",
            entry.id, entry.timestamp, exit_status, filter, entry.command
        );
    }
    0
}

pub fn cmd_history_clear() -> i32 {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine history DB path");
        return 1;
    };
    let conn = match history::open_db(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            return 1;
        }
    };

    if let Err(e) = history::clear_history(&conn) {
        eprintln!("[tokf] error clearing history: {e:#}");
        return 1;
    }

    eprintln!("[tokf] history cleared");
    0
}
