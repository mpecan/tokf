//! E2E tests for machine registration and listing via CLI client functions.
//!
//! Each test is `#[ignore]` — only runs when `DATABASE_URL` is set.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod harness;

/// Register a machine via the CLI client function → verify response.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn register_machine_via_client(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let machine_id = uuid::Uuid::new_v4().to_string();

    let registered = h.blocking_register_machine(&machine_id, "e2e-laptop").await;

    assert_eq!(registered.machine_id, machine_id);
    assert_eq!(registered.hostname, "e2e-laptop");
    assert!(!registered.created_at.is_empty());
}

/// Register a machine → list machines → verify it appears.
#[crdb_test_macro::crdb_test(migrations = "../tokf-server/migrations")]
async fn list_machines_returns_registered(pool: PgPool) {
    let h = harness::TestHarness::new(pool).await;
    let machine_id = uuid::Uuid::new_v4().to_string();

    // Register
    h.blocking_register_machine(&machine_id, "e2e-desktop")
        .await;

    // List
    let machines = h.blocking_list_machines().await;

    // The harness already creates one machine, plus we registered another
    assert_eq!(
        machines.len(),
        2,
        "expected exactly 2 machines, got {}",
        machines.len()
    );

    let found = machines.iter().any(|m| m.machine_id == machine_id);
    assert!(found, "registered machine {machine_id} not found in list");

    let our_machine = machines
        .iter()
        .find(|m| m.machine_id == machine_id)
        .unwrap();
    assert_eq!(our_machine.hostname, "e2e-desktop");
    assert!(our_machine.last_sync_at.is_none());
}
