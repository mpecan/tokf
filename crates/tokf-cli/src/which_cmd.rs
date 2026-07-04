use tokf::config;
use tokf::rewrite;

use crate::resolve;

/// `tokf which <command>` — report which filter (if any) matches a command,
/// including through a local environment wrapper such as `nix develop -c`.
pub fn cmd_which(command: &str, verbose: bool) -> i32 {
    let Ok(filters) = resolve::discover_filters(false) else {
        eprintln!("[tokf] error: failed to discover filters");
        return 1;
    };

    let words: Vec<&str> = command.split_whitespace().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    let wrapper_cfg = rewrite::load_local_wrapper_config();

    // Match directly, or after stripping a local environment wrapper prefix
    // (e.g. `nix develop -c cargo test` reports the `cargo test` filter).
    let Some((filter, _pattern, _consumed)) =
        config::local_wrapper::match_filters_with_wrapper(&filters, &words, &wrapper_cfg)
    else {
        eprintln!("[tokf] no filter found for \"{command}\"");
        return 1;
    };

    let display_name = filter
        .relative_path
        .with_extension("")
        .display()
        .to_string();

    let variant_info = if filter.config.variant.is_empty() {
        String::new()
    } else {
        let res = config::variant::resolve_variants(&filter.config, &filters, &cwd, verbose);
        let resolved = res.config.command.first().to_string();
        if resolved != filter.config.command.first() {
            format!(" -> variant: \"{resolved}\"")
        } else if res.output_variants.is_empty() {
            format!(
                " ({} variant(s), none matched by file)",
                filter.config.variant.len()
            )
        } else {
            let names: Vec<&str> = res
                .output_variants
                .iter()
                .map(|v| v.name.as_str())
                .collect();
            format!(
                " ({} variant(s), {} deferred to output-pattern: {})",
                filter.config.variant.len(),
                res.output_variants.len(),
                names.join(", ")
            )
        }
    };
    println!(
        "{display_name}  [{}]  command: \"{}\"{variant_info}",
        filter.priority_label(),
        filter.config.command.first()
    );
    if verbose {
        eprintln!("[tokf] source: {}", filter.source_path.display());
    }
    0
}
