# cosmos-llm-cache

Swappable caches for JSON values, keyed by a stable digest of the request that
produced them: an in-process LRU, a sharded directory of JSON files, and the
two stacked together.

Evaluation loops and optimizers replay identical LLM calls constantly. A cache
is the difference between a devset sweep costing money every run and costing
money once. This crate knows nothing about LLMs, though: a request is any JSON
value and so is the cached response, and the predictor using it decides what
goes in each.

```rust
use cosmos_llm_cache::{fetch, store, CacheOptions};
use serde_json::json;

let cache = store("tiered", &CacheOptions::in_directory("/home/me/.cache/cosmos-llm"))?;

let request = json!({ "model": "claude-opus-5-5", "messages": messages });
let (reply, hit) = fetch(cache.as_ref(), &request, || call_the_model(&request))?;
```

The first run calls the model and writes the reply to memory and disk. Later
calls in the same process are answered from memory; calls in a later process
are answered from disk and promoted back into memory.

## Keys

`key_for(&request)` is the SHA-256 hex digest of the request serialized
canonically: object keys sorted at every depth, no whitespace. Two requests
that differ only in key order share a key, in any process, whether or not
`serde_json`'s `preserve_order` feature is on somewhere in your build.
Numbers are compared as written, so `1` and `1.0` are different requests, as
they are to a provider.

Keys become file names, so every built-in store accepts only 1–200 characters
of `[A-Za-z0-9_-]`. That rules out `/` and `..`, so no key can address a file
outside the cache directory. A digest from `key_for` always qualifies.

## What happens when things go wrong

**A damaged entry is a miss.** A truncated file, invalid JSON, a file whose
stored key does not match its name, or a file that cannot be read at all: `get`
returns `None`, the caller recomputes, and the next write replaces the entry. A
damaged cache entry is a reason to recompute. It should never fail the call it
was meant to speed up. The Ruby gem behaves the same way.

**Writes are atomic.** Each value goes to a temp file beside its destination
and is renamed into place. A reader sees the old entry or the new one, never
half of either. A crash mid-write leaves at most a stray `.tmp` file, which
`clear` removes.

**`fetch` never throws away a computed value.** If the write after a miss
fails, a full disk say, `fetch` still returns the value. For a completion
you've already paid for, losing it to a cache error is the worst outcome on
offer. Call `get` and `put` yourself if you need to see write failures.

**`clear` only deletes what the cache wrote.** It removes entry and temp files
inside the store's own shard directories and leaves everything else alone, so
pointing `--dir` at the wrong place by accident costs you a cache, not your
files.

## Stores

| Name     | Type          | Keeps entries                         |
|----------|---------------|---------------------------------------|
| `memory` | `MemoryStore` | until the process exits (LRU, 5,000 by default) |
| `disk`   | `DiskStore`   | at `<dir>/<first 2 key chars>/<key>.json` |
| `tiered` | `TieredStore` | memory in front of disk               |

`TieredStore::new(front, back)` takes any two stores, so you can put memory in
front of something networked just as easily.

## Swapping in your own store

`CacheStore` is the trait: `get`, `put`, `remove`, `clear`, and an optional
`len`. Code that takes a `&dyn CacheStore` works with any implementation and
never touches the registry.

To make a store reachable *by name*, from config files or the CLI's `--store`
flag, register it:

```rust
use cosmos_llm_cache::{CacheError, CacheOptions, CachePlugin, CacheStore};
use cosmos_llm_cache::registry::meta;

fn redis(options: &CacheOptions) -> Result<Box<dyn CacheStore>, CacheError> {
    Ok(Box::new(RedisStore::connect(std::env::var("REDIS_URL").map_err(|_| {
        CacheError::Config("set REDIS_URL".into())
    })?)?))
}

cosmos_llm_cache::registry::inventory::submit! {
    CachePlugin { meta: meta!("redis").summary("Shared Redis cache"), build: redis }
}
```

Register under `memory`, `disk`, or `tiered` instead and yours replaces the
built-in everywhere it's looked up by name. Built-ins sit at a lower priority
than an unadorned registration, so nothing needs unregistering.

`inventory` collects registrations from every crate linked into the final
binary, but the linker may drop a plugin crate that nothing references. Name
it once:

```rust
use my_cache_plugins as _;
```

## Command line

```sh
cargo install --path . --features cli
```

```sh
# The key for a request
echo '{"model": "m", "prompt": "hi"}' | cosmos-llm-cache key

# Store and read a value
cosmos-llm-cache put --dir ~/.cache/cosmos-llm "$KEY" '{"completion": "hello"}'
cosmos-llm-cache get --dir ~/.cache/cosmos-llm "$KEY" | jq .completion

# How big is it?
cosmos-llm-cache stats --dir ~/.cache/cosmos-llm

# Start over (deletes files, so it asks for --yes)
cosmos-llm-cache clear --dir ~/.cache/cosmos-llm --yes

# What stores are installed, and which one wins each name
cosmos-llm-cache plugins
```

`get` and `remove` exit 1 when the key has no entry. Values are JSON on stdin
and stdout, so everything composes with `jq`.

To get the same CLI with your own stores available to `--store`, build a
binary that links them:

```rust
use my_cache_plugins as _;

fn main() -> std::process::ExitCode {
    cosmos_llm_cache::cli::main()
}
```

## Requirements

Rust 1.75 or later. Runtime dependencies are `serde_json`, `sha2`,
`thiserror`, and `cosmos-llm-registry`; `clap` only with the `cli` feature.
No async runtime.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
