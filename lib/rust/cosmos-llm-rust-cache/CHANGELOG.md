# Changelog

## Unreleased

### Added

- `CacheStore` trait: `get`, `put`, `remove`, `clear`, and an optional `len`.
- `key_for`: SHA-256 of a request's canonical JSON, independent of key order
  and of `serde_json`'s `preserve_order` feature.
- `validate_key`: keys are limited to 1–200 characters of `[A-Za-z0-9_-]` in
  every built-in store, so no key can escape a cache directory.
- `MemoryStore`: in-process LRU, 5,000 entries by default.
- `DiskStore`: one JSON file per entry, sharded by the key's first two
  characters, written atomically through temp-file rename. Damaged or
  unreadable entries are misses. `clear` removes only files the store wrote.
  `stats` reports entry count and bytes.
- `TieredStore`: any store in front of any other; back-store hits are promoted.
- `fetch`: get-or-compute helper that keeps the computed value even when the
  cache write fails.
- `CachePlugin` registry with built-in `memory` (alias `lru`), `disk`, and
  `tiered` stores, and `store(name, &CacheOptions)` to build one by name.
- `cosmos-llm-cache` CLI (feature `cli`): `key`, `get`, `put`, `remove`,
  `clear --yes`, `stats`, `plugins`.
