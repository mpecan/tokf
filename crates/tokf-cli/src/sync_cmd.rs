use tokf::remote::{http, machine};
use tokf::tracking;

/// Handle the `tokf sync` command.
///
/// In `--status` mode, prints the last sync time and pending event count.
/// Otherwise, performs a full sync of pending events to the remote server.
///
/// # Errors
///
/// Returns an error if the DB cannot be opened, or (in sync mode) if
/// the user is not logged in or no machine is registered.
pub fn cmd_sync(status: bool) -> anyhow::Result<i32> {
    if status {
        return cmd_sync_status();
    }

    let auth = http::load_auth()
        .map_err(|_| anyhow::anyhow!("not logged in — run `tokf auth login` to sync usage data"))?;

    let machine = machine::load()
        .ok_or_else(|| anyhow::anyhow!("machine not registered — run `tokf remote setup` first"))?;

    let db_path =
        tracking::db_path().ok_or_else(|| anyhow::anyhow!("cannot determine tracking DB path"))?;
    let conn = tracking::open_db(&db_path)?;

    let pending = tracking::get_pending_count(&conn)?;
    if pending == 0 {
        eprintln!("[tokf] Nothing to sync");
        return Ok(0);
    }

    let result = tokf::sync_core::perform_sync(&auth, &machine, &conn)?;
    eprintln!(
        "[tokf] Synced {} event(s). Cursor: {}.",
        result.synced_count, result.cursor
    );

    Ok(0)
}

fn cmd_sync_status() -> anyhow::Result<i32> {
    let db_path =
        tracking::db_path().ok_or_else(|| anyhow::anyhow!("cannot determine tracking DB path"))?;
    let conn = tracking::open_db(&db_path)?;

    let last_synced_at = tracking::get_last_synced_at(&conn)?;
    let pending = tracking::get_pending_count(&conn)?;

    match last_synced_at {
        Some(ts) => println!("Last sync: {ts}"),
        None => println!("Last sync: Never"),
    }
    println!("Pending events: {pending}");

    Ok(0)
}
