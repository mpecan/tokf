use std::path::PathBuf;

use clap::Subcommand;

use tokf::config;
use tokf::config::cache;

#[derive(Subcommand)]
pub enum CacheAction {
    /// Delete the cache file and force a rebuild on next run
    Clear,
    /// Show cache location, size, and validity status
    Info,
}

pub fn run_cache_action(action: &CacheAction) -> i32 {
    let search_dirs = config::default_search_dirs();
    match action {
        CacheAction::Clear => cmd_cache_clear(&search_dirs),
        CacheAction::Info => cmd_cache_info(&search_dirs),
    }
}

fn cmd_cache_clear(search_dirs: &[PathBuf]) -> i32 {
    let mut rc = 0;

    if let Some(path) = cache::cache_path(search_dirs) {
        match std::fs::remove_file(&path) {
            Ok(()) => {
                eprintln!("[tokf] cache cleared: {}", path.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("[tokf] cache: nothing to clear ({})", path.display());
            }
            Err(e) => {
                eprintln!("[tokf] cache clear error: {e}");
                rc = 1;
            }
        }
    } else {
        eprintln!("[tokf] cache: no cache location determined");
    }

    // Always attempt shim cleanup, even if cache removal failed
    if let Some(shims) = tokf::paths::shims_dir() {
        match std::fs::remove_dir_all(&shims) {
            Ok(()) => {
                eprintln!("[tokf] shims cleared: {}", shims.display());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("[tokf] shims clear error: {e}");
                rc = 1;
            }
        }
    }

    rc
}

fn cmd_cache_info(search_dirs: &[PathBuf]) -> i32 {
    let Some(path) = cache::cache_path(search_dirs) else {
        eprintln!("[tokf] cache: no cache location");
        return 0;
    };
    println!("cache path: {}", path.display());

    match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("status: not present");
            return 0;
        }
        Err(e) => {
            eprintln!("[tokf] cache: error reading metadata: {e}");
            return 1;
        }
        Ok(meta) => {
            println!("size: {} bytes", meta.len());
        }
    }

    match cache::load_manifest(&path) {
        Err(e) => {
            println!("status: unreadable ({e})");
        }
        Ok(manifest) => {
            println!("version: {}", manifest.version);
            println!("filters: {}", manifest.filters.len());
            let valid = cache::is_cache_valid(&manifest, search_dirs);
            println!("valid: {valid}");
        }
    }

    0
}
