# ADR-0002: Canonical v1 Filter Hash

## Status

Accepted (2026-04-28).

## Context

`tokf_common::hash::canonical_hash(&FilterConfig)` produces a SHA-256 over `serde_json::to_vec(filter_config)`. The output is sensitive to every detail of the in-memory `FilterConfig` shape: adding a default-valued field with `#[serde(default)]` silently changes the hash for every filter that doesn't reference the new field, because that field is now serialised into the JSON.

This caused issue #350: filters published before recent schema additions cannot be re-verified by current clients, even though their content is unchanged. The current `filters` table also contains accidental duplicates — multiple rows for the same TOML, each with its own `content_hash` because they were published under different schema generations. Statistics, leaderboards, and search results are split across these duplicates.

This ADR specifies **canonical-v1**: a frozen, byte-stable, schema-decoupled hash function that the project commits to going forward. v1 hashes a normalised TOML byte stream derived from the input filter, not the parsed Rust structure. Adding fields to `FilterConfig` does not affect v1 hashes for filters that do not use those fields.

## Hash output format

```
v1:<64-character lowercase hexadecimal SHA-256 digest>
```

Total length: 67 characters. Consumers identify the hash version by the `v1:` prefix.

## Algorithm

```
canonical_v1(toml_str: bytes) -> "v1:<hex>"

1. Parse toml_str as TOML 1.0 to a `toml::Value` tree. On parse error, return Err.
2. Walk the tree, applying these transformations in order:
   a. For each path in the policy table marked unordered, sort that array.
   b. For each `command` field: if its value is a single-entry array, replace
      with the scalar form.
   c. Recursively prune entries equal to `false`, `[]`, or `{}` (TOML-level
      defaults).
3. Emit via `toml::to_string(&value)`.
4. Compute SHA-256 over the emitted UTF-8 bytes.
5. Format as "v1:" + 64 lowercase hex digits.
```

This is parse → normalise → emit → hash. Conceptually a TOML round-trip with three normalisation passes. The implementation is ~50 lines.

## Input contract

- TOML 1.0, UTF-8 encoded, no leading byte-order mark.
- The root must be a table (TOML's normal case).
- Floats must be finite. `inf`, `-inf`, and `nan` cause an error rather than a hash.

The canonicaliser never inspects `FilterConfig`. Any TOML the parser accepts is hashable; the output is independent of whether the document deserialises into the current Rust structure.

## Walk: it's a `toml::Value` tree

"Walk the tree" means recursing through `toml::Value::Table` and `toml::Value::Array` nodes — a generic TOML AST, no Rust-type knowledge. The policy table matches by **structural path in the TOML**, not by `FilterConfig` field type. The path `skip` matches the top-level `skip` key in the document; `[on_success].skip` matches the `skip` key inside the `on_success` table. New fields added to `FilterConfig` later have no effect on the canonicaliser unless the user writes them in their TOML — and even then, default policy (preserve order) handles them safely.

## Policy table

For each known field path, the policy is **ordered** (preserve source order, the safe default for new fields too) or **unordered** (sort by the values' canonical representation).

| Path | Policy | Reason |
|---|---|---|
| `skip` | unordered | match-any: line is skipped if any pattern matches; order is irrelevant |
| `keep` | unordered | match-any |
| `[on_success].skip` | unordered | same |
| `[on_failure].skip` | unordered | same |
| `[[step]]` | ordered | pipeline; runs top-to-bottom |
| `[[match_output]]` | ordered | first-match-wins |
| `[[section]]` | ordered | state-machine transitions, evaluated in order |
| `[[replace]]` | ordered | applied sequentially; later rules see earlier rules' output |
| `[[variant]]` | ordered | priority; first match delegates |
| `[[chunk]]` | ordered | sub-block parsing depends on document position |
| `[[chunk]].[[body_extract]]` | ordered | same |
| `[[chunk]].[[aggregate]]` | ordered | same |
| `passthrough_args` | ordered | shell argument order matters |
| `command` (when `Multiple`) | ordered | patterns matched left-to-right by specificity |

Default policy for any path not in the table: **ordered**. New fields the canonicaliser doesn't recognise are emitted in source order, which is always safe.

Sort key for unordered arrays: each array element's canonical TOML byte representation, compared byte-wise. For arrays of strings (the common case — `skip = ["a", "b"]`), this is a lexicographic byte sort of the strings. For arrays of objects, it would be a byte sort of each object's emitted form (in practice, no unordered array-of-tables fields exist, so this case is reserved for future use).

`parse.group.labels` is already a `BTreeMap` and emits in key order naturally; it does not need to appear in the policy table.

## Special form: `command`

The TOML schema admits two equivalent forms:

```toml
command = "git push"          # Single
command = ["git push"]        # Multiple with one entry
```

These produce identical filtering behaviour. Canonical form: emit `command = "x"` whenever there is exactly one pattern; emit `command = ["x", "y"]` only when there are two or more. A filter author who switches between the two forms does not unintentionally change the hash.

## Default omission

Recursively, in the AST, drop:

- Boolean values equal to `false`.
- Arrays with zero elements.
- Tables with zero key/value pairs.

This makes `dedup = false` indistinguishable from omitting `dedup`, and `skip = []` indistinguishable from omitting `skip`. Exact consequences:

- `0`-valued integers are **not** omitted. `dedup_window = 0` is meaningfully different from `dedup_window` being absent.
- Empty strings `""` are **not** omitted. They are a value in their own right.
- Boolean `true` is never omitted.

The omission is recursive: a table whose only contents become empty after pruning (e.g., `[on_success]` with `output = ""` and `skip = []` only) becomes an empty table itself and is then pruned. This collapses chains of trivial wrappers cleanly.

## Dependency on the `toml` crate

v1's emission is delegated to `toml::to_string(&value)`. This is a deliberate choice that trades implementation simplicity for a dependency on the crate's emission stability. To make the dependency explicit and controlled:

- Pin `toml` in `Cargo.toml` to an **exact** version: `toml = "=1.0.X"` rather than `toml = "1.0"`.
- Treat every `toml` version bump as a CI gate. The frozen test corpus (see below) is the contract: if any corpus entry's hash changes after the bump, either stay pinned or ship v2.
- In practice the toml crate's emission is stable for the inputs we care about — sorted-key tables (`toml::Value`'s `Table` is a `BTreeMap`), straightforward scalar values, no exotic Unicode patterns. Risk surface is low.

If a future toml release does break the corpus and we cannot pin (because of, say, a security fix we need to take), v2 ships at that moment. v1 hashes already in the wild remain forever computable by checking out the older crate version.

## Test corpus

The implementation is paired with a frozen corpus at `crates/tokf-common/tests/hash_corpus/v1/`. Each fixture is a `<name>.toml` with a `<name>.expected` containing the recorded `v1:<hex>` output. CI asserts every fixture still produces its expected hash on every build. **Modifying an existing `.expected` value in place is forbidden** — that silently invalidates a real-world filter's identity.

The corpus must include, at minimum, fixtures exercising:

- A minimal filter (`command = "x"` only).
- Each unordered field with values written in non-canonical order, plus a fixture in canonical order — both must hash identically.
- Each ordered field with multiple entries; reordering them must produce a *different* hash.
- The `command` collapse: `command = "x"`, `command = ["x"]`, and `command = ["x", "y"]` — first two equal, third different.
- Default-omission cases: `dedup = false`, `skip = []`, empty `[fallback]` table — all must hash identically to the same TOML with those keys absent.
- Strings exercising every escape: backslash, quote, newline, tab, control characters.
- Integers including negative, large, and the `i64` boundary; equivalent inputs from hex/octal/binary forms (which the parser normalises) hash identically to their decimal form.
- Floats including subnormal, very-small, very-large, integer-valued.
- Unicode strings written equivalently in NFC and NFD — the parser normalises so they should hash identically.

A fixture-listing test asserts that every directory under `tests/hash_corpus/<id>/` corresponds to a registered hash version, that every `.toml` has a sibling `.expected`, and that no orphan `.expected` files exist.

## When to ship v2

A new version (v2, v3, …) is shipped only when one of the following is true:

1. **A bug is discovered in v1's canonicaliser** that produces non-deterministic output for some valid input. v2 is the corrected version; v1 remains as-is for filters already published.
2. **A `toml` crate upgrade** changes our corpus output and the upgrade cannot be deferred (security fix, etc.). v2 lives with the new emission; v1 is computed by checking out the old crate.
3. **The Layer 2 policy table needs an existing entry's policy *changed*** (e.g., reclassifying `[[step]]` as unordered). This would alter v1 output for existing filters, so it requires a new version.

Adding new entries to the policy table for fields that did not exist when v1 shipped is **not** a v2 trigger, provided the addition uses the conservative default (ordered) or the obviously-correct semantic (unordered for set-like fields). Such additions are recorded as v1 spec amendments in this ADR's revision history below.

When v2 ships, both v1 and v2 hashers exist in `tokf_common::hash::KNOWN_VERSIONS`. The server emits both during the migration window. Clients verify against whichever they recognise. v1's implementation stays in the codebase — there is no value in losing the ability to verify v1 hashes already in the wild.

## Worked equivalence examples

All inputs in each row hash to the same v1 output:

| Input A | Input B | Reason |
|---|---|---|
| `command = "git push"` | `command   =   "git push"\n\n` | toml-crate emission collapses whitespace |
| `command = "x"\n# old filter\nskip = []` | `command = "x"` | comments stripped at parse, default omitted |
| `skip = ["a", "b"]` | `skip = ["b", "a"]` | policy table: `skip` unordered |
| `command = "x"` | `command = ["x"]` | special-form collapse |
| `[parse.group.labels]\nA = "added"\nM = "modified"` | `[parse.group.labels]\nM = "modified"\nA = "added"` | `Table` is a `BTreeMap`, sorted on emit |
| `dedup = false\ncommand = "x"` | `command = "x"` | default omission |

These intentionally hash differently:

- `command = "x"` vs `command = "y"` — different content.
- `skip = ["a"]` vs `strip_lines_matching = ["a"]` — alias renames change the canonical key bytes; same logical effect, different canonical form.
- `[[step]]\nrun = "a"\n[[step]]\nrun = "b"` vs `[[step]]\nrun = "b"\n[[step]]\nrun = "a"` — `[[step]]` is ordered.
- `command = ["a", "b"]` vs `command = ["b", "a"]` — `Multiple` patterns are ordered (specificity).
- `dedup = true` vs (no `dedup` key) — explicit `true` is preserved; only `false` is omitted.

## Revision history

- **2026-04-28** — Initial draft.
