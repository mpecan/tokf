use std::path::Path;

use tokf::history;

use crate::commands::HistoryAction;

use tokf::runtime::Runtime;

/// Dispatch a `tokf history <action>` subcommand.
///
/// # Errors
/// Returns an error if the underlying history command fails.
pub fn dispatch_history(rt: &Runtime, action: &HistoryAction) -> anyhow::Result<i32> {
    match action {
        HistoryAction::List { limit, all } => cmd_history_list(rt, *limit, *all),
        HistoryAction::Show { id, raw } => cmd_history_show(rt, *id, *raw),
        HistoryAction::Last { raw, all } => cmd_history_last(rt, *raw, *all),
        HistoryAction::Search { query, limit, all } => cmd_history_search(rt, query, *limit, *all),
        HistoryAction::Clear { all } => cmd_history_clear(rt, *all),
    }
}

/// Dispatch `tokf raw <target>` where target is `last` or a numeric entry ID.
///
/// # Errors
/// Returns an error if the underlying history command fails.
pub fn dispatch_raw(rt: &Runtime, target: &str) -> anyhow::Result<i32> {
    if target == "last" {
        cmd_history_last(rt, true, false)
    } else if let Ok(id) = target.parse::<i64>() {
        cmd_history_show(rt, id, true)
    } else {
        eprintln!("[tokf] expected `last` or a numeric ID, got: {target}");
        Ok(1)
    }
}

fn open_history_conn(rt: &Runtime) -> anyhow::Result<rusqlite::Connection> {
    let path = rt
        .tracking_db_path()
        .ok_or_else(|| anyhow::anyhow!("cannot determine history DB path"))?;
    history::open_db(&path)
}

pub fn cmd_history_list(rt: &Runtime, limit: usize, all: bool) -> anyhow::Result<i32> {
    let conn = open_history_conn(rt)?;
    let project = if all {
        None
    } else {
        Some(history::current_project(rt))
    };
    let project_ref = project.as_deref();

    let entries = history::list_history(&conn, limit, project_ref)?;

    if entries.is_empty() {
        eprintln!("[tokf] no history entries found");
        return Ok(0);
    }

    for entry in entries {
        print_entry_line(&entry, all);
    }
    Ok(0)
}

pub fn cmd_history_show(rt: &Runtime, id: i64, raw: bool) -> anyhow::Result<i32> {
    let conn = open_history_conn(rt)?;

    let entry = history::get_history_entry(&conn, id)?;
    let Some(entry) = entry else {
        eprintln!("[tokf] history entry {id} not found");
        return Ok(1);
    };

    if raw {
        print!("{}", entry.raw_output);
        return Ok(0);
    }

    print_entry_detail(&entry);
    Ok(0)
}

pub fn cmd_history_last(rt: &Runtime, raw: bool, all: bool) -> anyhow::Result<i32> {
    let conn = open_history_conn(rt)?;
    let project = if all {
        None
    } else {
        Some(history::current_project(rt))
    };
    let project_ref = project.as_deref();

    let entry = history::get_latest_entry(&conn, project_ref)?;
    let Some(entry) = entry else {
        eprintln!("[tokf] no history entries found");
        return Ok(0);
    };

    if raw {
        print!("{}", entry.raw_output);
        return Ok(0);
    }

    print_entry_detail(&entry);
    Ok(0)
}

fn print_entry_detail(entry: &history::HistoryEntry) {
    println!("ID: {}", entry.id);
    println!("Timestamp: {}", entry.timestamp);
    println!("Project: {}", entry.project);
    println!("Command: {}", entry.command);
    // filter_name is always Some for recorded entries; the Option is defensive for
    // manually-inserted rows or future code paths.
    println!(
        "Filter: {}",
        entry.filter_name.as_deref().unwrap_or("(unknown)")
    );
    println!("Exit Code: {}", entry.exit_code);
    println!("\n--- Raw Output ---");
    println!("{}", entry.raw_output);
    println!("\n--- Filtered Output ---");
    println!("{}", entry.filtered_output);
}

pub fn cmd_history_search(
    rt: &Runtime,
    query: &str,
    limit: usize,
    all: bool,
) -> anyhow::Result<i32> {
    let conn = open_history_conn(rt)?;
    let project = if all {
        None
    } else {
        Some(history::current_project(rt))
    };
    let project_ref = project.as_deref();

    let entries = history::search_history(&conn, query, limit, project_ref)?;

    if entries.is_empty() {
        eprintln!("[tokf] no matching history entries found");
        return Ok(0);
    }

    for entry in entries {
        print_entry_line(&entry, all);
    }
    Ok(0)
}

pub fn cmd_history_clear(rt: &Runtime, all: bool) -> anyhow::Result<i32> {
    let conn = open_history_conn(rt)?;
    let project = if all {
        None
    } else {
        Some(history::current_project(rt))
    };
    let project_ref = project.as_deref();

    history::clear_history(&conn, project_ref)?;

    if all {
        eprintln!("[tokf] history cleared (all projects)");
    } else {
        eprintln!("[tokf] history cleared for current project");
    }
    Ok(0)
}

fn print_entry_line(entry: &history::HistoryEntry, show_project: bool) {
    // filter_name is always Some for recorded entries (try_record always sets it).
    // The fallback is defensive for rows inserted outside the normal code path.
    let filter = entry.filter_name.as_deref().unwrap_or("(unknown)");
    let exit_status = if entry.exit_code == 0 {
        "\u{2713}".to_string()
    } else {
        format!("\u{2717}({})", entry.exit_code)
    };
    let project_suffix = if show_project {
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
