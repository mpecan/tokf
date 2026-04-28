# Hash corpus

Frozen test vectors for `tokf_common::hash::KNOWN_VERSIONS`. Each
sub-directory is named after a hash version (`e1`, `e2`, …). Inside,
every `.toml` file has a sibling `.expected` containing the SHA-256 hash
that version's hasher must produce for that input.

Loaded by `tests/hash_corpus.rs` and run on every `cargo test`.

## Rules

1. **Never modify an existing `.expected`.** It captures the hash of a
   real-world filter as published; changing it silently invalidates that
   filter's identity. If a hasher would now produce a different value,
   the *hasher* changed (a bug, fix it) — never the expected value.
2. **Add new fixtures freely.** Any `.toml`/`.expected` pair under an
   existing version's directory is exercised automatically. Capture the
   expected hash by running the fixture once through that version's
   hasher and pasting the result.
3. **A new version goes in a new directory.** Existing directories stay
   pinned to their existing schema.
