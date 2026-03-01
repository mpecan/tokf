use serde::Deserialize;

use super::http::Client;

#[derive(Debug, Deserialize)]
pub struct RegisteredMachine {
    pub machine_id: String,
    pub hostname: String,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct MachineInfo {
    pub machine_id: String,
    pub hostname: String,
    pub created_at: String,
    pub last_sync_at: Option<String>,
}

/// Register this machine with the tokf server via `POST /api/machines`.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn register_machine(
    client: &Client,
    machine_id: &str,
    hostname: &str,
) -> anyhow::Result<RegisteredMachine> {
    client.post(
        "/api/machines",
        &serde_json::json!({ "machine_id": machine_id, "hostname": hostname }),
    )
}

/// List machines registered for the authenticated user via `GET /api/machines`.
///
/// # Errors
///
/// Returns an error if the server is unreachable, returns a non-success
/// status, or the response body cannot be deserialized.
pub fn list_machines(client: &Client) -> anyhow::Result<Vec<MachineInfo>> {
    client.get("/api/machines")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_registered_machine() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z"
        }"#;
        let machine: RegisteredMachine = serde_json::from_str(json).unwrap();
        assert_eq!(machine.machine_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(machine.hostname, "my-laptop");
        assert_eq!(machine.created_at, "2025-01-01T00:00:00Z");
    }

    #[test]
    fn deserialize_machine_info_with_null_last_sync() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z",
            "last_sync_at": null
        }"#;
        let info: MachineInfo = serde_json::from_str(json).unwrap();
        assert!(info.last_sync_at.is_none());
    }

    #[test]
    fn deserialize_machine_info_with_last_sync() {
        let json = r#"{
            "machine_id": "550e8400-e29b-41d4-a716-446655440000",
            "hostname": "my-laptop",
            "created_at": "2025-01-01T00:00:00Z",
            "last_sync_at": "2025-02-01T12:00:00Z"
        }"#;
        let info: MachineInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.last_sync_at.as_deref(), Some("2025-02-01T12:00:00Z"));
    }
}
