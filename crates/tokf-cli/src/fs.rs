use std::fs;
use std::path::Path;

/// Write a config file with restrictive permissions (0600 on Unix).
///
/// On non-Unix platforms, writes the file without special permission settings.
///
/// # Errors
///
/// Returns an error if the file cannot be created or written.
pub fn write_config_file(path: &Path, content: &str) -> anyhow::Result<()> {
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
