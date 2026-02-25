use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredMachine {
    /// UUID v4 identifying this machine
    pub machine_id: String,
    pub hostname: String,
}

/// Returns the path to `~/.config/tokf/machine.toml`.
pub fn machine_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("tokf").join("machine.toml"))
}

/// Load the stored machine registration.
///
/// Returns `None` if the machine has not been registered yet or the file is
/// missing. Prints a warning to stderr if `machine.toml` exists but is
/// malformed (e.g., corrupted).
pub fn load() -> Option<StoredMachine> {
    let path = machine_config_path()?;
    let content = fs::read_to_string(&path).ok()?;
    match toml::from_str(&content) {
        Ok(m) => Some(m),
        Err(e) => {
            eprintln!("[tokf] warning: machine.toml is malformed and will be ignored: {e}");
            None
        }
    }
}

/// Persist the machine registration to `~/.config/tokf/machine.toml`.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined or the file
/// cannot be written.
pub fn save(machine_id: &str, hostname: &str) -> anyhow::Result<()> {
    let path = machine_config_path()
        .ok_or_else(|| anyhow::anyhow!("cannot determine config directory"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let machine = StoredMachine {
        machine_id: machine_id.to_string(),
        hostname: hostname.to_string(),
    };
    let content = toml::to_string_pretty(&machine)?;
    write_config_file(&path, &content)
}

/// Write a config file with restrictive permissions (0600 on Unix).
fn write_config_file(path: &Path, content: &str) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(content.as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(path, content)?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // Tests for private write_config_file — not reachable from the external test file.

    #[test]
    fn write_config_file_creates_file_with_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.toml");
        write_config_file(&path, "key = \"value\"").unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "key = \"value\"");
    }

    #[test]
    #[cfg(unix)]
    fn write_config_file_sets_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.toml");
        write_config_file(&path, "").unwrap();
        let perms = fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }
}
