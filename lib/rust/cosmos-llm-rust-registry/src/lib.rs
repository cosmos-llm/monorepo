//! # cosmos-llm-registry
//!
//! Link-time plugin registries with priority-based overrides.
//!
//! Every swappable piece of the cosmos-llm Rust crates — field codecs,
//! adapters, cache stores, metrics — is a trait, and every implementation that
//! should be reachable *by name* is registered through [`inventory`]. This
//! crate is the small amount of shared logic that makes those registries
//! behave the same way everywhere: how a name is matched, which registration
//! wins when two claim it, and how to list what is installed.
//!
//! ## Swapping an implementation
//!
//! Built-in registrations use [`priority::BUILTIN`]. A registration made
//! without an explicit priority gets [`priority::DEFAULT`], which is higher,
//! so registering your own implementation under a built-in's name replaces it
//! with no further configuration. Nothing needs to be unregistered, and the
//! crate that owns the built-in does not need to know yours exists.
//!
//! ```
//! use cosmos_llm_registry::{meta, resolve, Meta, Plugin};
//!
//! /// A registration type, as a crate that owns a registry would declare it.
//! pub struct Greeter {
//!     pub meta: Meta,
//!     pub greet: fn(&str) -> String,
//! }
//!
//! impl Plugin for Greeter {
//!     fn meta(&self) -> &Meta {
//!         &self.meta
//!     }
//! }
//!
//! cosmos_llm_registry::inventory::collect!(Greeter);
//!
//! fn plain(name: &str) -> String {
//!     format!("Hello, {name}.")
//! }
//!
//! fn loud(name: &str) -> String {
//!     format!("HELLO, {}!", name.to_uppercase())
//! }
//!
//! // The owning crate ships a built-in...
//! cosmos_llm_registry::inventory::submit! {
//!     Greeter { meta: meta!("hello").builtin(), greet: plain }
//! }
//!
//! // ...and a downstream crate replaces it by registering the same name.
//! cosmos_llm_registry::inventory::submit! {
//!     Greeter { meta: meta!("hello"), greet: loud }
//! }
//!
//! let greeter = resolve::<Greeter>("hello").unwrap();
//! assert_eq!((greeter.greet)("ada"), "HELLO, ADA!");
//! ```
//!
//! ## Linking a plugin crate
//!
//! `inventory` collects registrations from every crate that is linked into
//! the final binary. A plugin crate that is listed in `Cargo.toml` but never
//! referenced from code may be dropped by the linker, so name it once:
//!
//! ```ignore
//! use my_cosmos_plugins as _;
//! ```
//!
//! ## Name matching
//!
//! Names are matched case-insensitively, and `-` and `_` are treated as the
//! same character, so `chain-of-thought`, `Chain_Of_Thought`, and
//! `chain_of_thought` all resolve to the same registration. That keeps CLI
//! flags and config files forgiving without each registry reimplementing it.

#![warn(missing_docs)]

use std::cmp::Ordering;
use std::fmt;

/// Re-exported so that plugin crates can call [`inventory::submit!`] through
/// whichever cosmos-llm crate owns the registry, without depending on
/// `inventory` themselves.
pub use inventory;

/// Priorities that order competing registrations for the same name.
///
/// Higher wins. The gap between the two named levels leaves room for a
/// plugin that wants to sit above the built-ins but below an application's
/// own overrides.
pub mod priority {
    /// The priority every built-in registration uses.
    pub const BUILTIN: i32 = -100;
    /// The priority a registration gets when it does not ask for one.
    pub const DEFAULT: i32 = 0;
}

/// Identifying information carried by every registration.
///
/// Built with `const` methods so it can appear inside
/// [`inventory::submit!`], which requires a constant expression. Prefer the
/// [`meta!`] macro, which fills in [`Meta::origin`] from the calling module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meta {
    /// The canonical name the registration is resolved by.
    pub name: &'static str,
    /// Further names that also resolve to this registration.
    pub aliases: &'static [&'static str],
    /// Ordering among registrations that claim the same name; higher wins.
    pub priority: i32,
    /// One line describing the registration, for listings.
    pub summary: &'static str,
    /// Where the registration was made, usually a module path.
    ///
    /// Breaks priority ties deterministically and tells a reader of
    /// [`listing`] output which crate a registration came from.
    pub origin: &'static str,
}

impl Meta {
    /// Starts a registration's metadata with a name and default priority.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_registry::{priority, Meta};
    ///
    /// const META: Meta = Meta::new("chat").summary("Section markers");
    /// assert_eq!(META.priority, priority::DEFAULT);
    /// ```
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            aliases: &[],
            priority: priority::DEFAULT,
            summary: "",
            origin: "",
        }
    }

    /// Sets additional names that resolve to this registration.
    pub const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    /// Sets the priority; higher wins.
    pub const fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Marks this as a built-in, so any same-named registration replaces it.
    pub const fn builtin(self) -> Self {
        self.priority(priority::BUILTIN)
    }

    /// Sets the one-line summary.
    pub const fn summary(mut self, summary: &'static str) -> Self {
        self.summary = summary;
        self
    }

    /// Sets where the registration was made.
    pub const fn origin(mut self, origin: &'static str) -> Self {
        self.origin = origin;
        self
    }

    /// Whether `name` matches this registration's name.
    pub fn is_named(&self, name: &str) -> bool {
        same_name(self.name, name)
    }

    /// Whether `name` matches this registration's name or one of its aliases.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm_registry::Meta;
    ///
    /// let meta = Meta::new("chain_of_thought").aliases(&["cot"]);
    /// assert!(meta.answers_to("Chain-Of-Thought"));
    /// assert!(meta.answers_to("COT"));
    /// assert!(!meta.answers_to("predict"));
    /// ```
    pub fn answers_to(&self, name: &str) -> bool {
        self.is_named(name) || self.aliases.iter().any(|alias| same_name(alias, name))
    }
}

/// Builds a [`Meta`] with its origin set to the calling module.
///
/// # Examples
///
/// ```
/// use cosmos_llm_registry::meta;
///
/// let m = meta!("json").summary("A single JSON object");
/// assert_eq!(m.origin, module_path!());
/// ```
#[macro_export]
macro_rules! meta {
    ($name:expr) => {
        $crate::Meta::new($name).origin(::core::module_path!())
    };
}

/// A value that can be collected into a registry.
///
/// A crate that owns a registry declares a registration struct holding a
/// [`Meta`] and whatever it needs to build the implementation — usually a
/// constructor `fn` — implements this trait for it, and calls
/// `inventory::collect!` on it.
pub trait Plugin: Sync + 'static {
    /// The registration's identifying metadata.
    fn meta(&self) -> &Meta;
}

/// Raised when a name resolves to nothing.
///
/// Carries the names that *are* available, so the message can tell the user
/// what to type instead.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} '{name}'{}", available_hint(.available))]
pub struct NotFound {
    /// What kind of thing was being looked up, e.g. `"adapter"`.
    pub kind: &'static str,
    /// The name that was asked for.
    pub name: String,
    /// The canonical names that are registered.
    pub available: Vec<&'static str>,
}

fn available_hint(available: &[&str]) -> String {
    if available.is_empty() {
        "; none are registered".to_owned()
    } else {
        format!("; available: {}", available.join(", "))
    }
}

/// Resolves a name to its winning registration, if any.
///
/// Among registrations whose name or alias matches, the winner is the one with
/// the highest [`Meta::priority`]. At equal priority, a match on the canonical
/// name beats a match on an alias, and after that the lexically smallest
/// [`Meta::origin`] wins. `inventory` makes no promise about iteration order,
/// so that last rule exists only to make a tie deterministic; [`conflicts`]
/// reports ties so they can be fixed rather than relied upon.
pub fn resolve<P>(name: &str) -> Option<&'static P>
where
    P: Plugin + inventory::Collect,
{
    inventory::iter::<P>
        .into_iter()
        .filter(|plugin| plugin.meta().answers_to(name))
        .max_by(|a, b| precedence(a.meta(), b.meta(), name))
}

/// Resolves a name, or explains what is available when it does not resolve.
///
/// # Errors
///
/// Returns [`NotFound`] naming the registered alternatives.
pub fn require<P>(kind: &'static str, name: &str) -> Result<&'static P, NotFound>
where
    P: Plugin + inventory::Collect,
{
    resolve::<P>(name).ok_or_else(|| NotFound {
        kind,
        name: name.to_owned(),
        available: names::<P>(),
    })
}

/// The winning registration for each distinct name, sorted by name.
pub fn active<P>() -> Vec<&'static P>
where
    P: Plugin + inventory::Collect,
{
    let mut names = names::<P>();
    names.dedup();
    names
        .into_iter()
        .filter_map(|name| {
            inventory::iter::<P>
                .into_iter()
                .filter(|plugin| plugin.meta().is_named(name))
                .max_by(|a, b| precedence(a.meta(), b.meta(), name))
        })
        .collect()
}

/// The distinct canonical names that are registered, sorted.
pub fn names<P>() -> Vec<&'static str>
where
    P: Plugin + inventory::Collect,
{
    let mut names: Vec<&'static str> = Vec::new();
    for plugin in inventory::iter::<P> {
        let name = plugin.meta().name;
        if !names.iter().any(|seen| same_name(seen, name)) {
            names.push(name);
        }
    }
    names.sort_by_key(|name| normalize(name));
    names
}

/// One row of a [`listing`]: a registration and whether it is the one in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Listing {
    /// The registration's metadata.
    pub meta: Meta,
    /// `true` when this registration wins its name; `false` when another
    /// registration shadows it.
    pub active: bool,
}

impl fmt::Display for Listing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let marker = if self.active { "*" } else { " " };
        write!(
            f,
            "{marker} {:<20} {:>5}  {}",
            self.meta.name, self.meta.priority, self.meta.summary
        )?;
        if !self.meta.aliases.is_empty() {
            write!(f, " (aliases: {})", self.meta.aliases.join(", "))?;
        }
        if !self.meta.origin.is_empty() {
            write!(f, " [{}]", self.meta.origin)?;
        }
        Ok(())
    }
}

/// Every registration, winners and shadowed alike, sorted by name and then by
/// precedence.
///
/// This is what a CLI's `plugins` subcommand prints: it shows not just what
/// is in use but what was overridden, which is the first thing to check when
/// a swapped-in implementation does not seem to take effect.
pub fn listing<P>() -> Vec<Listing>
where
    P: Plugin + inventory::Collect,
{
    let winners = active::<P>();
    let mut rows: Vec<Listing> = inventory::iter::<P>
        .into_iter()
        .map(|plugin| Listing {
            meta: *plugin.meta(),
            active: winners.iter().any(|winner| std::ptr::eq(*winner, plugin)),
        })
        .collect();
    rows.sort_by(|a, b| {
        normalize(a.meta.name)
            .cmp(&normalize(b.meta.name))
            .then_with(|| precedence(&b.meta, &a.meta, a.meta.name))
    });
    rows
}

/// Names claimed by more than one registration at the same top priority.
///
/// Such a tie is resolved deterministically by [`resolve`], but by origin
/// path rather than by intent. A non-empty result almost always means two
/// plugin crates are fighting over a name and one of them should set a
/// priority.
pub fn conflicts<P>() -> Vec<&'static str>
where
    P: Plugin + inventory::Collect,
{
    names::<P>()
        .into_iter()
        .filter(|name| {
            let mut priorities: Vec<i32> = inventory::iter::<P>
                .into_iter()
                .filter(|plugin| plugin.meta().is_named(name))
                .map(|plugin| plugin.meta().priority)
                .collect();
            priorities.sort_unstable_by(|a, b| b.cmp(a));
            priorities.len() > 1 && priorities[0] == priorities[1]
        })
        .collect()
}

/// Orders two candidates for `name`; `Greater` means `a` wins.
fn precedence(a: &Meta, b: &Meta, name: &str) -> Ordering {
    a.priority
        .cmp(&b.priority)
        .then_with(|| a.is_named(name).cmp(&b.is_named(name)))
        .then_with(|| b.origin.cmp(a.origin))
}

/// Normalizes a name for comparison.
///
/// # Examples
///
/// ```
/// assert_eq!(cosmos_llm_registry::normalize("Chain-Of-Thought"), "chain_of_thought");
/// ```
pub fn normalize(name: &str) -> String {
    name.trim()
        .chars()
        .map(|c| {
            if c == '-' {
                '_'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

fn same_name(a: &str, b: &str) -> bool {
    normalize(a) == normalize(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Shape {
        meta: Meta,
        sides: u32,
    }

    impl Plugin for Shape {
        fn meta(&self) -> &Meta {
            &self.meta
        }
    }

    inventory::collect!(Shape);

    inventory::submit! { Shape { meta: Meta::new("triangle").builtin().origin("builtin"), sides: 3 } }
    inventory::submit! { Shape { meta: Meta::new("square").builtin().origin("builtin").aliases(&["box"]), sides: 4 } }
    // An override of a built-in at default priority.
    inventory::submit! { Shape { meta: Meta::new("triangle").origin("override"), sides: 30 } }
    // Two registrations tied at the same priority.
    inventory::submit! { Shape { meta: Meta::new("pentagon").origin("b_crate"), sides: 5 } }
    inventory::submit! { Shape { meta: Meta::new("pentagon").origin("a_crate"), sides: 50 } }
    // A canonical name that collides with another registration's alias.
    inventory::submit! { Shape { meta: Meta::new("box").builtin().origin("builtin"), sides: 6 } }

    #[test]
    fn a_default_priority_registration_overrides_a_builtin() {
        assert_eq!(resolve::<Shape>("triangle").unwrap().sides, 30);
    }

    #[test]
    fn names_match_case_and_separator_insensitively() {
        let meta = Meta::new("chain_of_thought");
        assert!(meta.answers_to("CHAIN-of-thought"));
        assert!(meta.answers_to("  chain_of_thought "));
    }

    #[test]
    fn aliases_resolve_when_no_canonical_name_competes() {
        assert_eq!(resolve::<Shape>("square").unwrap().sides, 4);
    }

    #[test]
    fn a_canonical_name_beats_an_alias_at_equal_priority() {
        assert_eq!(resolve::<Shape>("box").unwrap().sides, 6);
    }

    #[test]
    fn ties_are_broken_by_origin_and_reported() {
        assert_eq!(resolve::<Shape>("pentagon").unwrap().sides, 50);
        assert_eq!(conflicts::<Shape>(), vec!["pentagon"]);
    }

    #[test]
    fn active_has_one_winner_per_name() {
        let winners: Vec<(&str, u32)> = active::<Shape>()
            .iter()
            .map(|shape| (shape.meta.name, shape.sides))
            .collect();
        assert_eq!(
            winners,
            vec![
                ("box", 6),
                ("pentagon", 50),
                ("square", 4),
                ("triangle", 30)
            ]
        );
    }

    #[test]
    fn listing_shows_shadowed_registrations_after_their_winner() {
        let triangles: Vec<(bool, &str)> = listing::<Shape>()
            .into_iter()
            .filter(|row| row.meta.name == "triangle")
            .map(|row| (row.active, row.meta.origin))
            .collect();
        assert_eq!(triangles, vec![(true, "override"), (false, "builtin")]);
    }

    #[test]
    fn require_lists_alternatives_when_a_name_is_unknown() {
        let err = require::<Shape>("shape", "hexagon").err().unwrap();
        assert_eq!(err.available, vec!["box", "pentagon", "square", "triangle"]);
        assert_eq!(
            err.to_string(),
            "unknown shape 'hexagon'; available: box, pentagon, square, triangle"
        );
    }
}
