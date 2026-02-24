use std::path::PathBuf;

use serde::Serialize;

use tokf::config;
use tokf::tracking;

#[derive(Serialize)]
struct SearchDir {
    scope: &'static str,
    path: String,
    exists: bool,
}

#[derive(Serialize)]
struct TrackingDb {
    env_override: Option<String>,
    path: Option<String>,
    exists: bool,
}

#[derive(Serialize)]
struct CacheInfo {
    path: Option<String>,
    exists: bool,
}

#[derive(Serialize)]
struct FilterCounts {
    local: usize,
    user: usize,
    builtin: usize,
    total: usize,
}

#[derive(Serialize)]
struct InfoOutput {
    version: String,
    search_dirs: Vec<SearchDir>,
    tracking_db: TrackingDb,
    cache: CacheInfo,
    filters: Option<FilterCounts>,
}

pub fn cmd_info(json: bool) -> i32 {
    let search_dirs = config::default_search_dirs();
    let info = collect_info(&search_dirs);

    if json {
        print_json(&info);
    } else {
        print_human(&info);
    }
    0
}

fn collect_info(search_dirs: &[PathBuf]) -> InfoOutput {
    let mut dirs: Vec<SearchDir> = search_dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| SearchDir {
            scope: if i == 0 { "local" } else { "user" },
            path: dir.display().to_string(),
            exists: dir.exists(),
        })
        .collect();
    dirs.push(SearchDir {
        scope: "built-in",
        path: "<embedded>".to_string(),
        exists: true,
    });

    let env_override = std::env::var("TOKF_DB_PATH").ok();
    let db_path = tracking::db_path();
    let db_exists = db_path.as_ref().is_some_and(|p| p.exists());
    let tracking_db = TrackingDb {
        env_override,
        path: db_path.map(|p| p.display().to_string()),
        exists: db_exists,
    };

    let cache_path = config::cache::cache_path(search_dirs);
    let cache_exists = cache_path.as_ref().is_some_and(|p| p.exists());
    let cache = CacheInfo {
        path: cache_path.map(|p| p.display().to_string()),
        exists: cache_exists,
    };

    let filters = match config::discover_all_filters(search_dirs) {
        Ok(f) => {
            let local = f.iter().filter(|fi| fi.priority == 0).count();
            let user = f
                .iter()
                .filter(|fi| fi.priority > 0 && fi.priority < u8::MAX)
                .count();
            let builtin = f.iter().filter(|fi| fi.priority == u8::MAX).count();
            Some(FilterCounts {
                local,
                user,
                builtin,
                total: f.len(),
            })
        }
        Err(e) => {
            eprintln!("[tokf] error discovering filters: {e:#}");
            None
        }
    };

    InfoOutput {
        version: env!("CARGO_PKG_VERSION").to_string(),
        search_dirs: dirs,
        tracking_db,
        cache,
        filters,
    }
}

fn print_json(info: &InfoOutput) {
    match serde_json::to_string_pretty(info) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("[tokf] JSON serialization error: {e}"),
    }
}

fn print_human(info: &InfoOutput) {
    println!("tokf {}", info.version);

    println!("\nfilter search directories:");
    for dir in &info.search_dirs {
        let status = if dir.exists { "exists" } else { "not found" };
        if dir.scope == "built-in" {
            println!("  [{}] {} (always available)", dir.scope, dir.path);
        } else {
            println!("  [{}] {} ({status})", dir.scope, dir.path);
        }
    }

    println!("\ntracking database:");
    match &info.tracking_db.env_override {
        Some(p) => println!("  TOKF_DB_PATH: {p}"),
        None => println!("  TOKF_DB_PATH: (not set)"),
    }
    match &info.tracking_db.path {
        Some(p) => {
            let status = if info.tracking_db.exists {
                "exists"
            } else {
                "not found"
            };
            println!("  path: {p} ({status})");
        }
        None => println!("  path: (could not determine)"),
    }

    println!("\nfilter cache:");
    match &info.cache.path {
        Some(p) => {
            let status = if info.cache.exists {
                "exists"
            } else {
                "not found"
            };
            println!("  path: {p} ({status})");
        }
        None => println!("  path: (could not determine)"),
    }

    if let Some(f) = &info.filters {
        println!("\nfilters:");
        println!("  local:    {}", f.local);
        println!("  user:     {}", f.user);
        println!("  built-in: {}", f.builtin);
        println!("  total:    {}", f.total);
    }
}
