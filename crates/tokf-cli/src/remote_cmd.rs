use std::time::Duration;

use tokf::auth::client::is_secure_url;
use tokf::auth::credentials;
use tokf::remote::{client, machine};
use uuid::Uuid;

/// Register this machine with the tokf server.
///
/// Generates a UUID v4 machine identifier, registers it with the server, and
/// stores it in `~/.config/tokf/machine.toml`. If already registered locally,
/// re-syncs with the server to repair any stale state (idempotent).
///
/// # Errors
///
/// Returns an error if the user is not logged in, the token is expired, or
/// the server is unreachable.
pub fn cmd_remote_setup() -> anyhow::Result<i32> {
    let auth = credentials::load()
        .ok_or_else(|| anyhow::anyhow!("not logged in. Run `tokf auth login` first"))?;

    if auth.is_expired() {
        anyhow::bail!("token has expired. Run `tokf auth login` to re-authenticate");
    }

    if !is_secure_url(&auth.server_url) {
        eprintln!(
            "[tokf] warning: server URL uses plain HTTP — your token may be exposed in transit"
        );
    }

    let http_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(format!("tokf-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    if let Some(m) = machine::load() {
        // Already registered locally — re-sync with server to fix any stale state.
        client::register_machine(
            &http_client,
            &auth.server_url,
            &auth.token,
            &m.machine_id,
            &m.hostname,
        )?;
        eprintln!(
            "[tokf] Already registered: {} ({})",
            m.machine_id, m.hostname
        );
    } else {
        let machine_id = Uuid::new_v4().to_string();
        let hostname = gethostname::gethostname().to_string_lossy().into_owned();
        client::register_machine(
            &http_client,
            &auth.server_url,
            &auth.token,
            &machine_id,
            &hostname,
        )?;
        machine::save(&machine_id, &hostname)?;
        eprintln!("[tokf] Machine registered: {machine_id} ({hostname})");
    }

    Ok(0)
}

/// Show remote sync registration state.
#[allow(clippy::unnecessary_wraps)] // Returns Result for or_exit() consistency
pub fn cmd_remote_status() -> anyhow::Result<i32> {
    match machine::load() {
        Some(m) => {
            println!("Machine ID: {}", m.machine_id);
            println!("Hostname:   {}", m.hostname);
        }
        None => {
            println!("Not registered. Run `tokf remote setup` to register this machine.");
        }
    }
    Ok(0)
}

/// Sync local usage events to the remote server.
///
/// # Errors
///
/// Returns an error if the user is not logged in, no machine is registered,
/// or the server is unreachable.
pub fn cmd_remote_sync() -> anyhow::Result<i32> {
    use tokf::remote::sync_client::{SyncEvent, SyncRequest};
    use tokf::tracking;

    let auth = credentials::load()
        .ok_or_else(|| anyhow::anyhow!("not logged in. Run `tokf auth login` first"))?;

    if auth.is_expired() {
        anyhow::bail!("token has expired. Run `tokf auth login` to re-authenticate");
    }

    let machine = machine::load()
        .ok_or_else(|| anyhow::anyhow!("machine not registered. Run `tokf remote setup` first"))?;

    let db_path =
        tracking::db_path().ok_or_else(|| anyhow::anyhow!("cannot determine tracking DB path"))?;
    let conn = tracking::open_db(&db_path)?;

    let last_id = tracking::get_last_synced_id(&conn)?;
    let events = tracking::get_events_since(&conn, last_id)?;

    if events.is_empty() {
        eprintln!("[tokf] Nothing to sync");
        return Ok(0);
    }

    let sync_events: Vec<SyncEvent> = events
        .iter()
        .map(|e| SyncEvent {
            id: e.id,
            filter_name: e.filter_name.clone(),
            filter_hash: e.filter_hash.clone(),
            input_tokens: e.input_tokens_est,
            output_tokens: e.output_tokens_est,
            command_count: 1,
            recorded_at: e.timestamp.clone(),
        })
        .collect();

    let http_client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(format!("tokf-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()?;

    let req = SyncRequest {
        machine_id: machine.machine_id,
        last_event_id: last_id,
        events: sync_events,
    };

    let response =
        tokf::remote::sync_client::sync_events(&http_client, &auth.server_url, &auth.token, &req)?;

    tracking::set_last_synced_id(&conn, response.cursor)?;
    eprintln!(
        "[tokf] Synced {} event(s). Cursor: {}.",
        response.accepted, response.cursor
    );

    Ok(0)
}

/// Backfill `filter_hash` for past events that have a filter name but no hash.
///
/// Discovers all currently-installed filters, then updates every event row in the
/// local DB where `filter_hash IS NULL` but `filter_name` is known. Events for
/// filters that have been removed or renamed are reported but left unchanged.
///
/// # Errors
/// Returns an error if filter discovery or DB access fails.
pub fn cmd_remote_backfill(no_cache: bool) -> anyhow::Result<i32> {
    use tokf::tracking;

    let filters = crate::resolve::discover_filters(no_cache)?;

    let db_path =
        tracking::db_path().ok_or_else(|| anyhow::anyhow!("cannot determine tracking DB path"))?;
    let conn = tracking::open_db(&db_path)?;

    let (updated, not_found) = tracking::backfill_filter_hashes(&conn, &filters)?;

    if updated == 0 && not_found.is_empty() {
        eprintln!("[tokf] Nothing to backfill — all events already have hashes.");
        return Ok(0);
    }

    if updated > 0 {
        eprintln!("[tokf] Backfilled hash for {updated} event(s).");
    }
    if !not_found.is_empty() {
        eprintln!(
            "[tokf] {} filter(s) not found (removed or renamed): {}",
            not_found.len(),
            not_found.join(", ")
        );
    }

    Ok(0)
}
