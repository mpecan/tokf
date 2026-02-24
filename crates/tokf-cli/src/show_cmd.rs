use crate::resolve;
use tokf::config;

pub fn cmd_show(filter: &str, hash: bool) -> i32 {
    // Normalize: strip ".toml" suffix if present
    let filter_name = filter.strip_suffix(".toml").unwrap_or(filter);

    let Ok(filters) = resolve::discover_filters(false) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    let found = filters
        .iter()
        .find(|f| f.relative_path.with_extension("").to_string_lossy() == filter_name);

    let Some(resolved) = found else {
        eprintln!("[tokf] filter not found: {filter}");
        return 1;
    };

    if hash {
        match tokf_common::hash::canonical_hash(&resolved.config) {
            Ok(h) => println!("{h}"),
            Err(e) => {
                eprintln!("[tokf] error computing hash: {e}");
                return 1;
            }
        }
        return 0;
    }

    let content = if resolved.priority == u8::MAX {
        if let Some(c) = config::get_embedded_filter(&resolved.relative_path) {
            c.to_string()
        } else {
            eprintln!("[tokf] error: embedded filter not readable");
            return 1;
        }
    } else {
        match std::fs::read_to_string(&resolved.source_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[tokf] error reading filter: {e}");
                return 1;
            }
        }
    };

    print!("{content}");
    0
}
