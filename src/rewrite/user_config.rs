use std::path::PathBuf;

use super::types::RewriteConfig;

/// Search config dirs for `rewrites.toml` (first found wins).
///
/// Search order:
/// 1. `.tokf/rewrites.toml` (project-local)
/// 2. `~/.config/tokf/rewrites.toml` (user-level)
pub fn load_user_config() -> Option<RewriteConfig> {
    load_user_config_from(&config_search_paths())
}

fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".tokf/rewrites.toml"));
    }

    if let Some(config) = dirs::config_dir() {
        paths.push(config.join("tokf/rewrites.toml"));
    }

    paths
}

/// Testable version that accepts explicit paths.
pub fn load_user_config_from(paths: &[PathBuf]) -> Option<RewriteConfig> {
    for path in paths {
        if let Ok(content) = std::fs::read_to_string(path) {
            match toml::from_str(&content) {
                Ok(config) => return Some(config),
                Err(e) => {
                    eprintln!("[tokf] warning: failed to parse {}: {e}", path.display());
                    return None;
                }
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn load_config_first_found_wins() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        let path1 = dir1.path().join("rewrites.toml");
        let path2 = dir2.path().join("rewrites.toml");

        fs::write(
            &path1,
            r#"
[[rewrite]]
match = "^first"
replace = "first"
"#,
        )
        .unwrap();
        fs::write(
            &path2,
            r#"
[[rewrite]]
match = "^second"
replace = "second"
"#,
        )
        .unwrap();

        let config = load_user_config_from(&[path1, path2]).unwrap();
        assert_eq!(config.rewrite[0].match_pattern, "^first");
    }

    #[test]
    fn load_config_nonexistent_returns_none() {
        let result = load_user_config_from(&[PathBuf::from("/no/such/file.toml")]);
        assert!(result.is_none());
    }

    #[test]
    fn load_config_invalid_toml_returns_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("rewrites.toml");
        fs::write(&path, "not valid [[[").unwrap();

        let result = load_user_config_from(&[path]);
        assert!(result.is_none());
    }
}
