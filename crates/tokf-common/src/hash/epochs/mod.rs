//! Frozen historical hash epochs.
//!
//! Each module here is a byte-for-byte snapshot of `FilterConfig` (and its
//! dependent types) as they existed at a specific commit, plus a `hash()`
//! function that reproduces the canonical hash a binary at that commit
//! would have produced.
//!
//! ## Adding a new epoch
//!
//! When `FilterConfig` (or any dependent type) changes in a way that
//! affects `current::canonical_hash` output, also:
//!
//! 1. Add `eN.rs` with a verbatim copy of `types.rs` at the change commit,
//!    wrapped in a private `mod schema { ... }`. Use
//!    `git show <sha>:crates/tokf-common/src/config/types.rs` for fidelity.
//! 2. Append a `HashVersion` entry to `super::KNOWN_VERSIONS`.
//! 3. Add corpus fixtures under
//!    `crates/tokf-common/tests/hash_corpus/eN/` covering the schema's
//!    distinguishing features.
//! 4. **Never modify** an existing epoch. If the snapshot was wrong,
//!    consider whether to introduce a *new* epoch with the corrected
//!    schema instead — old `eN:…` hashes in the wild already exist.

pub(super) mod e1;
