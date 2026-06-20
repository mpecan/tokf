//! Regression tests for multibyte-UTF-8 compound splitting (issue #383).
//!
//! rable's AST spans are character indices (its lexer scans a `Vec<char>`),
//! so slicing the byte-indexed source by a raw span field lands early once a
//! multibyte char precedes it — splicing stray `|`/`;` bytes into the rebuilt
//! command. These tests lock in that `compound_segments` slices separators by
//! byte offset (via `char_to_byte`) so the reassembled command is valid shell.
//!
//! Kept in a sibling file rather than `bash_ast_tests.rs` to stay under the
//! 700-line hard file-size limit.

use super::bash_ast::*;

#[test]
fn round_trip_byte_for_byte_multibyte() {
    // The reassembled `seg + sep` chain must reproduce the original source
    // byte-for-byte. Each case mixes a multibyte char in one segment with an
    // operator boundary in another — the shape that previously corrupted.
    for input in [
        "echo \"✓ valid\" || echo \"✗ bad\"; ls | grep tokf",
        "echo \"✓ done\"; cargo build | tail",
        "git status | grep modified; echo \"✓\"",
        "echo café && git log",
        "echo 中文 || echo x; ls",
        "echo 🎉\nls",
        "echo ✓ &&\necho ✗",
    ] {
        let Some(p) = ParsedCommand::parse(input) else {
            continue;
        };
        let mut rebuilt = String::new();
        for (seg, sep) in p.compound_segments() {
            rebuilt.push_str(&seg);
            rebuilt.push_str(&sep);
        }
        assert_eq!(rebuilt, input, "round-trip failed for {input:?}");
    }
}

#[test]
fn split_compound_multibyte_separators() {
    // The canonical repro. Lock in exact segment text and separator content
    // so a regression can't slip through by merely round-tripping. `✓`/`✗`
    // are 3 bytes each; before the char→byte conversion the `||` and `;`
    // separators were sliced early — injecting a stray `|` after segment 0
    // and collapsing the `; ` after segment 1.
    let p = ParsedCommand::parse("echo \"✓ valid\" || echo \"✗ bad\"; ls | grep tokf").unwrap();
    let segs = p.compound_segments();
    assert_eq!(segs.len(), 3, "expected 3 segments, got {segs:?}");

    assert_eq!(segs[0].0, "echo \"✓ valid\"");
    assert_eq!(segs[0].1, " || ", "sep 0 must be the bare `||` gap");

    assert_eq!(segs[1].0, "echo \"✗ bad\"");
    assert_eq!(
        segs[1].1, "; ",
        "sep 1 must be exactly `; ` (one semicolon)"
    );

    assert_eq!(segs[2].0, "ls | grep tokf");
    assert!(segs[2].1.is_empty());
}
