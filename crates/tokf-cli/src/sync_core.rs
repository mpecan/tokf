use std::fs;
use std::io::Write;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::auth::credentials::LoadedAuth;
use crate::paths;
use crate::remote::machine::StoredMachine;
use crate::remote::sync_client::{SyncEvent, SyncRequest};
use crate::tracking;

/// Maximum age of a lock file before it's considered stale (5 minutes).
const LOCK_STALE_SECS: u64 = 300;

/// Return the path to the sync lock file (`{user_data_dir}/sync.lock`).
fn lock_path() -> Option<PathBuf> {
    paths::user_data_dir().map(|d| d.join("sync.lock"))
}

/// RAII guard that removes the lock file on drop.
pub(crate) struct SyncLock {
    path: PathBuf,
}

impl SyncLock {
    /// Try to acquire the sync lock. Returns `None` if another sync is already running.
    pub(crate) fn acquire() -> Option<Self> {
        let path = lock_path()?;
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        // Fast path: atomically create the lock file.
        if let Ok(mut f) = fs::File::create_new(&path) {
            let _ = write!(f, "{}", std::process::id());
            return Some(Self { path });
        }

        // File exists — check whether it's stale (older than LOCK_STALE_SECS).
        let stale = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .is_some_and(|age| age.as_secs() > LOCK_STALE_SECS);

        if !stale {
            return None; // another sync is still running
        }

        // Stale lock — remove and retry.
        let _ = fs::remove_file(&path);
        let mut f = fs::File::create_new(&path).ok()?;
        let _ = write!(f, "{}", std::process::id());
        Some(Self { path })
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Generate a UTC ISO 8601 timestamp string without external dependencies.
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn utc_now_iso8601() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Convert Unix timestamp to date/time components.
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Civil date from days since epoch (algorithm from Howard Hinnant).
    let z = days as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

/// Result of a sync operation.
pub struct SyncResult {
    pub synced_count: usize,
    pub cursor: i64,
}

/// Convert a `SyncableEvent` to a `SyncEvent` for the remote API.
fn to_sync_event(e: &tracking::SyncableEvent) -> SyncEvent {
    SyncEvent {
        id: e.id,
        filter_name: e.filter_name.clone(),
        filter_hash: e.filter_hash.clone(),
        input_tokens: e.input_tokens_est,
        output_tokens: e.output_tokens_est,
        command_count: 1,
        recorded_at: e.timestamp.clone(),
    }
}

/// Perform a full sync of all pending events to the remote server.
///
/// Acquires a file lock to prevent concurrent syncs. Batches events in chunks
/// of 500 (the DB query limit), sending each batch to the server and advancing
/// the cursor. Continues until no more events remain.
///
/// # Errors
///
/// Returns an error if the lock cannot be acquired, the DB query fails, the
/// HTTP request fails, or the server returns a non-success status.
pub fn perform_sync(
    auth: &LoadedAuth,
    machine: &StoredMachine,
    conn: &Connection,
) -> anyhow::Result<SyncResult> {
    let _lock =
        SyncLock::acquire().ok_or_else(|| anyhow::anyhow!("another sync is already running"))?;

    let http_client = crate::remote::http::build_client(crate::remote::http::HEAVY_TIMEOUT_SECS)?;

    let mut total_synced = 0usize;
    let mut cursor = tracking::get_last_synced_id(conn)?;

    loop {
        let events = tracking::get_events_since(conn, cursor)?;
        if events.is_empty() {
            break;
        }

        let sync_events: Vec<SyncEvent> = events.iter().map(to_sync_event).collect();

        let req = SyncRequest {
            machine_id: machine.machine_id.clone(),
            last_event_id: cursor,
            events: sync_events,
        };

        let response = crate::remote::retry::with_retry("sync", || {
            crate::remote::sync_client::sync_events(
                &http_client,
                &auth.server_url,
                &auth.token,
                &req,
            )
        })?;

        total_synced += response.accepted;
        let new_cursor = response.cursor;

        // Guard: if the server returned a cursor that didn't advance, bail out
        // to prevent an infinite loop (e.g. server bug or desync).
        if new_cursor <= cursor {
            anyhow::bail!(
                "sync stalled: server returned cursor {new_cursor} (was {cursor}). \
                 This may indicate a server issue — try again later."
            );
        }
        cursor = new_cursor;

        let tx = conn.unchecked_transaction()?;
        tracking::set_last_synced_id(&tx, cursor)?;
        tracking::set_last_synced_at(&tx, &utc_now_iso8601())?;
        tx.commit()?;

        // If we got fewer than 500, we've reached the end
        if events.len() < 500 {
            break;
        }
    }

    Ok(SyncResult {
        synced_count: total_synced,
        cursor,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serial_test::serial;

    use super::*;
    use crate::tracking::SyncableEvent;

    #[test]
    #[serial]
    fn sync_lock_acquire_and_release() {
        let dir = tempfile::TempDir::new().unwrap();
        unsafe { std::env::set_var("TOKF_HOME", dir.path()) };

        let lock = SyncLock::acquire();
        assert!(lock.is_some(), "should acquire lock on fresh dir");

        let lock_file = dir.path().join("sync.lock");
        assert!(lock_file.exists(), "lock file should exist while held");

        drop(lock);
        assert!(!lock_file.exists(), "lock file should be removed on drop");

        unsafe { std::env::remove_var("TOKF_HOME") };
    }

    #[test]
    #[serial]
    fn sync_lock_prevents_double_acquire() {
        let dir = tempfile::TempDir::new().unwrap();
        unsafe { std::env::set_var("TOKF_HOME", dir.path()) };

        let lock1 = SyncLock::acquire();
        assert!(lock1.is_some());

        let lock2 = SyncLock::acquire();
        assert!(
            lock2.is_none(),
            "second acquire should fail while first is held"
        );

        drop(lock1);

        let lock3 = SyncLock::acquire();
        assert!(
            lock3.is_some(),
            "should succeed after first lock is released"
        );

        unsafe { std::env::remove_var("TOKF_HOME") };
    }

    #[test]
    #[serial]
    fn sync_lock_reclaims_stale_lock() {
        use std::time::{Duration, SystemTime};

        let dir = tempfile::TempDir::new().unwrap();
        unsafe { std::env::set_var("TOKF_HOME", dir.path()) };

        let lock_file = dir.path().join("sync.lock");
        fs::write(&lock_file, "99999999").unwrap(); // fake PID

        // Backdate the file to make it stale
        let old_time = SystemTime::now() - Duration::from_secs(LOCK_STALE_SECS + 60);
        filetime::set_file_mtime(&lock_file, filetime::FileTime::from_system_time(old_time))
            .unwrap();

        let lock = SyncLock::acquire();
        assert!(lock.is_some(), "should reclaim stale lock");

        unsafe { std::env::remove_var("TOKF_HOME") };
    }

    #[test]
    fn to_sync_event_maps_fields() {
        let se = SyncableEvent {
            id: 42,
            filter_name: Some("git/push".to_string()),
            filter_hash: Some("abc".to_string()),
            input_tokens_est: 1000,
            output_tokens_est: 200,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
        };
        let result = to_sync_event(&se);
        assert_eq!(result.id, 42);
        assert_eq!(result.filter_name.as_deref(), Some("git/push"));
        assert_eq!(result.filter_hash.as_deref(), Some("abc"));
        assert_eq!(result.input_tokens, 1000);
        assert_eq!(result.output_tokens, 200);
        assert_eq!(result.command_count, 1);
        assert_eq!(result.recorded_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn to_sync_event_handles_none_fields() {
        let se = SyncableEvent {
            id: 1,
            filter_name: None,
            filter_hash: None,
            input_tokens_est: 500,
            output_tokens_est: 500,
            timestamp: "2026-02-01T12:00:00Z".to_string(),
        };
        let result = to_sync_event(&se);
        assert!(result.filter_name.is_none());
        assert!(result.filter_hash.is_none());
    }

    #[test]
    fn utc_now_iso8601_format() {
        let ts = utc_now_iso8601();
        // Should match YYYY-MM-DDTHH:MM:SSZ
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
                .unwrap()
                .is_match(&ts),
            "timestamp should be ISO 8601 UTC format, got: {ts}"
        );
        // Year should be reasonable (2020+)
        let year: u32 = ts[..4].parse().unwrap();
        assert!(year >= 2020, "year should be >= 2020, got {year}");
    }

    #[test]
    fn utc_now_iso8601_month_and_day_in_range() {
        let ts = utc_now_iso8601();
        let month: u32 = ts[5..7].parse().unwrap();
        let day: u32 = ts[8..10].parse().unwrap();
        assert!((1..=12).contains(&month), "month out of range: {month}");
        assert!((1..=31).contains(&day), "day out of range: {day}");
    }
}
