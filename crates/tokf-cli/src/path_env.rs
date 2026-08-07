//! Building `PATH`-style environment values.
//!
//! Split out of `resolve.rs`: assembling a search path is its own concern, and
//! it is the one piece of the shim injection with platform-specific rules.

/// Prepend `dir` to a `PATH`-style variable using the platform's separator.
///
/// `std::env::join_paths` picks `;` on Windows and `:` elsewhere, matching the
/// `std::env::split_paths` that reads the value back in `runner::resolve_program`.
/// Hard-coding `:` produced a malformed `PATH` on Windows, where `:` also occurs
/// inside every absolute path — `C:\...\shims:C` parsed as one entry, so the
/// shim injection silently did nothing *and* the first real entry was dropped.
pub fn prepend_to_path(dir: &std::path::Path, original_path: &str) -> String {
    let entries = std::iter::once(dir.to_path_buf())
        .chain(std::env::split_paths(original_path))
        .map(std::path::PathBuf::into_os_string);

    std::env::join_paths(entries).map_or_else(
        // join_paths only fails if an entry itself contains the separator, in
        // which case there is no correct answer — fall back to the platform
        // separator rather than dropping the injection entirely.
        |_| {
            let sep = if cfg!(windows) { ';' } else { ':' };
            format!("{}{sep}{original_path}", dir.display())
        },
        |joined| joined.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    // --- prepend_to_path (#451) ---

    /// The value must be readable by the `split_paths` that consumes it in
    /// `runner::resolve_program`. Hard-coding `:` broke this on Windows, where
    /// the separator is `;` and `:` occurs inside every absolute path.
    #[test]
    fn prepend_to_path_round_trips_through_split_paths() {
        let original: Vec<std::path::PathBuf> =
            std::env::split_paths(&std::env::join_paths(["/one/two", "/three"].iter()).unwrap())
                .collect();
        let original_str = std::env::join_paths(original.iter())
            .unwrap()
            .to_string_lossy()
            .into_owned();

        let shims = std::path::Path::new("/shims/dir");
        let joined = prepend_to_path(shims, &original_str);

        let parsed: Vec<std::path::PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(
            parsed.len(),
            original.len() + 1,
            "entry count must be preserved"
        );
        assert_eq!(parsed[0], shims, "shims dir must lead");
        assert_eq!(
            &parsed[1..],
            &original[..],
            "existing entries must survive intact"
        );
    }

    /// The separator must be the platform's own, never a hard-coded `:`.
    #[test]
    fn prepend_to_path_uses_the_platform_separator() {
        let sep = if cfg!(windows) { ';' } else { ':' };
        let joined = prepend_to_path(std::path::Path::new("/shims"), "/existing");
        assert!(
            joined.contains(sep),
            "expected {sep:?} to separate entries in {joined:?}"
        );
    }

    /// An absolute Windows path contains a colon. Splitting on one would cut
    /// `C:\\shims` into `C` and `\\shims`, which is the #451 bug.
    ///
    /// Windows-only by nature: on Unix `:` really is the separator, so the
    /// same input correctly yields two entries there.
    #[cfg(windows)]
    #[test]
    fn prepend_to_path_keeps_a_drive_letter_entry_whole() {
        let dir = std::path::Path::new("C:/tokf/shims");
        let joined = prepend_to_path(dir, "C:/existing/bin");
        let parsed: Vec<std::path::PathBuf> = std::env::split_paths(&joined).collect();
        assert_eq!(parsed[0], dir, "drive-lettered entry must survive whole");
        assert_eq!(parsed.len(), 2, "must be exactly two entries, not four");
    }
}
