//! Frozen-corpus test for every registered `HashVersion`.
//!
//! Walks `tests/hash_corpus/<id>/` for each entry of
//! [`tokf_common::hash::KNOWN_VERSIONS`] and asserts every `<n>.toml`
//! produces the hash recorded in its `<n>.expected` sibling. A change
//! that breaks any expected value is either a bug in the hasher (fix
//! it) or an unintended schema drift (revert it / introduce a new
//! version). Modifying `.expected` files in place is the wrong response.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use tokf_common::hash;

/// Print every corpus hash to stdout; used during authoring to capture
/// `.expected` values for new fixtures. Run with:
///
/// ```sh
/// cargo test -p tokf-common --test hash_corpus -- print_all_hashes --ignored --nocapture
/// ```
#[test]
#[ignore = "authoring helper; run explicitly to capture expected values"]
fn print_all_hashes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hash_corpus");
    for version in hash::KNOWN_VERSIONS {
        let dir = root.join(version.id);
        if !dir.is_dir() {
            continue;
        }
        let mut entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| {
                let p = e.ok()?.path();
                (p.extension().is_some_and(|x| x == "toml")).then_some(p)
            })
            .collect();
        entries.sort();
        for path in entries {
            let toml = std::fs::read_to_string(&path).unwrap();
            let hash = version.hash(&toml).unwrap();
            println!("{}: {}", path.display(), hash);
        }
    }
}

#[test]
fn corpus_round_trip() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hash_corpus");
    let mut total = 0usize;

    for version in hash::KNOWN_VERSIONS {
        let dir = root.join(version.id);
        assert!(
            dir.is_dir(),
            "missing corpus directory for version {}: {}",
            version.id,
            dir.display()
        );
        let mut for_version = 0usize;
        let mut tomls = Vec::new();
        let mut expecteds = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            match path.extension().and_then(|e| e.to_str()) {
                Some("toml") => tomls.push(path),
                Some("expected") => expecteds.push(path),
                _ => {}
            }
        }

        // Detect orphan `.expected` files (a `.toml` was deleted but the
        // expected hash was left behind — the silent-clutter case).
        for ex in &expecteds {
            let toml_sibling = ex.with_extension("toml");
            assert!(
                toml_sibling.exists(),
                "orphan expected file (no matching .toml): {}",
                ex.display()
            );
        }

        for path in tomls {
            let toml = std::fs::read_to_string(&path).unwrap();
            let expected_path = path.with_extension("expected");
            let expected = std::fs::read_to_string(&expected_path)
                .unwrap_or_else(|_| panic!("missing expected file: {}", expected_path.display()));
            let expected = expected.trim();

            let actual = version.hash(&toml).unwrap();
            assert_eq!(
                actual,
                expected,
                "{} hash drift in {}",
                version.id,
                path.display()
            );
            for_version += 1;
            total += 1;
        }
        assert!(for_version > 0, "no corpus entries under {}", dir.display());
    }

    assert!(total > 0, "corpus is empty");
}
