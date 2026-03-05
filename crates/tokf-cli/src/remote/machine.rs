use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::fs::write_config_file;

#[derive(Debug, Serialize, Deserialize)]
pub struct StoredMachine {
    /// UUID v4 identifying this machine
    pub machine_id: String,
    pub hostname: String,
}

/// Returns the path to the tokf machine config file.
/// Uses `TOKF_HOME` if set, else the platform config directory.
pub fn machine_config_path() -> Option<PathBuf> {
    crate::paths::user_dir().map(|d| d.join("machine.toml"))
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
