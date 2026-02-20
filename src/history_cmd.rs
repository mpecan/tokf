use std::path::Path;

use tokf::history;
use tokf::tracking;

fn open_history_conn() -> Option<rusqlite::Connection> {
    let Some(path) = tracking::db_path() else {
        eprintln!("[tokf] error: cannot determine history DB path");
        return None;
    };
    match history::open_db(&path) {
        Ok(c) => Some(c),
        Err(e) => {
            eprintln!("[tokf] error opening DB: {e:#}");
            None
        }
    }
}

pub fn cmd_history_list(limit: usize, all: bool) -> i32 {
    let Some(conn) = open_history_conn() else {
        return 1;
    };
    let project = if all {
        None
    } else {
        Some(history::current_project())
    };
    let project_ref = project.as_deref();

    let entries = match history::list_history(&conn, limit, project_ref) {
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
            "\u{2713}".to_string()
        } else {
            format!("\u{2717}({})", entry.exit_code)
        };
        let project_suffix = if all {
            let basename = Path::new(&entry.project)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.project);
            format!(" ({basename})")
        } else {
            String::new()
        };
        println!(
            "{} {} {} [{}] {}{}",
            entry.id, entry.timestamp, exit_status, filter, entry.command, project_suffix
        );
    }
    0
}

pub fn cmd_history_show(id: i64) -> i32 {
    let Some(conn) = open_history_conn() else {
        return 1;
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
    println!("Project: {}", entry.project);
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

pub fn cmd_history_search(query: &str, limit: usize, all: bool) -> i32 {
    let Some(conn) = open_history_conn() else {
        return 1;
    };
    let project = if all {
        None
    } else {
        Some(history::current_project())
    };
    let project_ref = project.as_deref();

    let entries = match history::search_history(&conn, query, limit, project_ref) {
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
            "\u{2713}".to_string()
        } else {
            format!("\u{2717}({})", entry.exit_code)
        };
        let project_suffix = if all {
            let basename = Path::new(&entry.project)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&entry.project);
            format!(" ({basename})")
        } else {
            String::new()
        };
        println!(
            "{} {} {} [{}] {}{}",
            entry.id, entry.timestamp, exit_status, filter, entry.command, project_suffix
        );
    }
    0
}

pub fn cmd_history_clear(all: bool) -> i32 {
    let Some(conn) = open_history_conn() else {
        return 1;
    };
    let project = if all {
        None
    } else {
        Some(history::current_project())
    };
    let project_ref = project.as_deref();

    if let Err(e) = history::clear_history(&conn, project_ref) {
        eprintln!("[tokf] error clearing history: {e:#}");
        return 1;
    }

    if all {
        eprintln!("[tokf] history cleared (all projects)");
    } else {
        eprintln!("[tokf] history cleared for current project");
    }
    0
}
