# cosmos-llm-registry

Link-time plugin registries with priority-based overrides, built on
[`inventory`](https://docs.rs/inventory).

Every swappable piece of the cosmos-llm Rust crates is a trait: field codecs,
adapters, cache stores, backends, strategies, metrics, devset formats. Every
implementation that should be reachable *by name* — from a CLI flag or a config
file — is registered through `inventory`. This crate is the small amount of
shared logic that makes those registries behave the same way everywhere: how a
name is matched, which registration wins when two claim the same name, and how
to list what is installed.

It has no CLI. It is plumbing; each crate that owns a registry exposes it
through its own `plugins` subcommand.

## Replacing a built-in

Built-ins register at `priority::BUILTIN` (-100). A registration that does not
ask for a priority gets `priority::DEFAULT` (0). So registering your own
implementation under a built-in's name replaces it with no further
configuration: nothing is unregistered, and the crate that owns the built-in
does not need to know yours exists.

```rust
use cosmos_llm_adapter::{AdapterPlugin, registry::meta};

fn build() -> Box<dyn cosmos_llm_adapter::Adapter> {
    Box::new(MyChatAdapter::default())
}

// Every predictor that asks for "chat" now gets MyChatAdapter.
cosmos_llm_adapter::registry::inventory::submit! {
    AdapterPlugin { meta: meta!("chat").summary("Our house prompt format"), build }
}
```

Each owning crate re-exports this crate as `registry`, and this crate
re-exports `inventory`, so a plugin crate does not need either as a direct
dependency.

## Resolution rules

1. **Names are forgiving.** Matching ignores case and treats `-` and `_` as the
   same, so `chain-of-thought`, `Chain_Of_Thought`, and `chain_of_thought` are one
   name.
2. **Higher priority wins.**
3. **At equal priority, a canonical name beats an alias.**
4. **After that, the lexically smallest origin wins.** `inventory` makes no
   promise about iteration order, so this rule exists only to make a tie
   deterministic. `conflicts()` reports ties so they can be fixed rather than
   relied upon.

`listing()` returns every registration, including shadowed ones, which is the
first thing to check when a swapped-in implementation does not seem to take
effect.

## Linking a plugin crate

`inventory` collects registrations from every crate linked into the final
binary. A plugin crate that is listed in `Cargo.toml` but never referenced may
be dropped by the linker, so name it once:

```rust
use my_cosmos_plugins as _;
```

The same applies to the CLIs. A stock `cosmos-llm-predict` binary sees only the
built-ins. To get yours, write a three-line binary that links your plugin crate
and calls the library's `cli::main()`:

```rust
use my_cosmos_plugins as _;

fn main() -> std::process::ExitCode {
    cosmos_llm_predict::cli::main()
}
```

## Declaring a registry

For a crate that wants its own registry:

```rust
use cosmos_llm_registry::{Meta, Plugin};

pub struct GreeterPlugin {
    pub meta: Meta,
    pub build: fn() -> Box<dyn Greeter>,
}

impl Plugin for GreeterPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(GreeterPlugin);

pub fn greeter(name: &str) -> Result<Box<dyn Greeter>, cosmos_llm_registry::NotFound> {
    Ok((cosmos_llm_registry::require::<GreeterPlugin>("greeter", name)?.build)())
}
```

`NotFound` lists the registered names, so the error a user sees says what to
type instead.

## Requirements

Rust 1.75 or later. `inventory` supports Linux, macOS, Windows, and WebAssembly;
see its documentation for the platforms where link-time collection is not
available.

## License

MIT. Enterprise support from [Durable Programming](https://durableprogramming.com).
