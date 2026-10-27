//! What the client knows about a model: limits, capabilities, prices.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::Usage;

/// Prices in US dollars per million tokens.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Pricing {
    /// Uncached input tokens.
    pub input: f64,
    /// Output tokens, reasoning included.
    pub output: f64,
    /// Cached input tokens read. `None` bills them at the input price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Input tokens written to the cache. `None` bills them at the input price.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl Pricing {
    /// Prices for input and output only.
    pub fn new(input: f64, output: f64) -> Self {
        Self {
            input,
            output,
            cache_read: None,
            cache_write: None,
        }
    }
}

/// Limits and capabilities of one model.
///
/// Unknown limits are `None`: callers should fall back to a conservative
/// default (128k, say) and say so, rather than guess silently. The capability
/// flags of a model nobody has described are `false`, so check
/// `context_window.is_some()` to tell "cannot" from "not known".
///
/// Every field but `id` has a default, so an override can be as short as
/// `{:id "my-local-model" :context_window 32768 :supports_tools true}`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct ModelInfo {
    /// Model identifier as the provider spells it.
    pub id: String,
    /// Provider the model belongs to, e.g. `"anthropic"`.
    pub provider: String,
    /// Input context window, in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    /// Largest completion the model will produce, in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    /// Accepts tool definitions and calls.
    pub supports_tools: bool,
    /// Accepts image input.
    pub supports_images: bool,
    /// Accepts a reasoning request.
    pub supports_reasoning: bool,
    /// Caches prompt prefixes, automatically or by marker.
    pub supports_prompt_cache: bool,
    /// Prices, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing: Option<Pricing>,
}

static TABLE: OnceLock<Vec<ModelInfo>> = OnceLock::new();

/// The table `build.rs` produced from `models.trongo`.
fn table() -> &'static [ModelInfo] {
    TABLE.get_or_init(|| {
        serde_json::from_str(include_str!(concat!(env!("OUT_DIR"), "/models.json")))
            .expect("built-in model table is valid")
    })
}

/// Lowercases, drops an OpenRouter vendor prefix (`anthropic/`) and variant
/// suffix (`:free`), and turns dots into dashes (`claude-3.5-sonnet`).
fn normalize(id: &str) -> String {
    let id = id.rsplit('/').next().unwrap_or(id);
    let id = id.split(':').next().unwrap_or(id);
    id.to_lowercase().replace('.', "-")
}

impl ModelInfo {
    /// A model with nothing known about it.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::ModelInfo;
    /// let info = ModelInfo::new("openai", "my-local-model");
    /// assert_eq!(info.context_window, None);
    /// ```
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            id: id.into(),
            ..Self::default()
        }
    }

    /// Sets the context window, returning `self`.
    #[must_use]
    pub fn with_context_window(mut self, tokens: u32) -> Self {
        self.context_window = Some(tokens);
        self
    }

    /// Sets the output cap, returning `self`.
    #[must_use]
    pub fn with_max_output_tokens(mut self, tokens: u32) -> Self {
        self.max_output_tokens = Some(tokens);
        self
    }

    /// Sets the prices, returning `self`.
    #[must_use]
    pub fn with_pricing(mut self, pricing: Pricing) -> Self {
        self.pricing = Some(pricing);
        self
    }

    /// Looks `id` up in the built-in table, with no network.
    ///
    /// Matches by longest prefix at a `-` boundary after normalizing the id,
    /// so dated variants (`claude-sonnet-4-6-20260101`) and OpenRouter spellings
    /// (`anthropic/claude-3.5-sonnet`) find their entry. The returned `id` is
    /// the one asked for.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::ModelInfo;
    /// let info = ModelInfo::builtin("claude-3-5-sonnet-20241022").unwrap();
    /// assert_eq!(info.context_window, Some(200_000));
    /// assert!(ModelInfo::builtin("no-such-model").is_none());
    /// ```
    pub fn builtin(id: &str) -> Option<Self> {
        let wanted = normalize(id);
        table()
            .iter()
            .filter(|entry| {
                let key = normalize(&entry.id);
                wanted == key
                    || (wanted.starts_with(&key) && wanted.as_bytes().get(key.len()) == Some(&b'-'))
            })
            .max_by_key(|entry| entry.id.len())
            .map(|entry| Self {
                id: id.to_owned(),
                ..entry.clone()
            })
    }

    /// Every entry in the built-in table.
    pub fn builtin_table() -> &'static [ModelInfo] {
        table()
    }

    /// Fills the fields `self` leaves unknown from `other`.
    ///
    /// Provider data comes first and the table fills its gaps. Capability
    /// flags are or-ed, since `false` also means "not stated".
    pub(crate) fn fill_from(&mut self, other: &ModelInfo) {
        self.context_window = self.context_window.or(other.context_window);
        self.max_output_tokens = self.max_output_tokens.or(other.max_output_tokens);
        self.supports_tools |= other.supports_tools;
        self.supports_images |= other.supports_images;
        self.supports_reasoning |= other.supports_reasoning;
        self.supports_prompt_cache |= other.supports_prompt_cache;
        if self.pricing.is_none() {
            self.pricing = other.pricing.clone();
        }
    }

    /// Reads one entry of OpenRouter's `/models` listing.
    ///
    /// OpenRouter quotes prices in dollars per token, as strings; they are
    /// scaled to per-million here. Returns `None` when the entry has no id.
    pub(crate) fn from_openrouter(entry: &Value) -> Option<Self> {
        let id = entry["id"].as_str()?;
        let per_million = |key: &str| -> Option<f64> {
            let v = &entry["pricing"][key];
            let n = v
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                .or_else(|| v.as_f64())?;
            Some(n * 1_000_000.0)
        };
        let params = entry["supported_parameters"].as_array();
        let has_param =
            |name: &str| params.is_some_and(|p| p.iter().any(|v| v.as_str() == Some(name)));
        let pricing = match (per_million("prompt"), per_million("completion")) {
            (Some(input), Some(output)) => Some(Pricing {
                input,
                output,
                cache_read: per_million("input_cache_read"),
                cache_write: per_million("input_cache_write"),
            }),
            _ => None,
        };
        let supports_images = entry["architecture"]["input_modalities"]
            .as_array()
            .is_some_and(|m| m.iter().any(|v| v.as_str() == Some("image")));

        Some(Self {
            id: id.to_owned(),
            provider: "openrouter".to_owned(),
            context_window: entry["context_length"].as_u64().map(|n| n as u32),
            max_output_tokens: entry["top_provider"]["max_completion_tokens"]
                .as_u64()
                .map(|n| n as u32),
            supports_tools: has_param("tools"),
            supports_images,
            supports_reasoning: has_param("reasoning") || has_param("include_reasoning"),
            supports_prompt_cache: pricing.as_ref().is_some_and(|p| p.cache_read.is_some()),
            pricing,
        })
    }

    /// Estimates what `usage` cost at this model's list prices.
    ///
    /// For providers that report no charge ([`Usage::cost`] is `None`). This is
    /// an estimate from a table, a different thing from a charged figure; keep
    /// the two in separate fields and never sum them into one number. Cache
    /// reads and writes are billed at their own prices when the table has them,
    /// and reasoning tokens are already inside `completion_tokens`.
    ///
    /// Returns `None` when the model has no known pricing.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{ModelInfo, Pricing, Usage};
    ///
    /// let mut info = ModelInfo::new("anthropic", "m");
    /// let mut p = Pricing::new(3.0, 15.0);
    /// p.cache_read = Some(0.3);
    /// info = info.with_pricing(p);
    ///
    /// // 1M prompt tokens, 1M of them cache reads, plus 1M output.
    /// let usage = Usage::from_tokens(1_000_000, 1_000_000).with_cache(1_000_000, 0);
    /// assert!((info.estimate_cost(&usage).unwrap() - 15.3).abs() < 1e-9);
    /// ```
    pub fn estimate_cost(&self, usage: &Usage) -> Option<f64> {
        let p = self.pricing.as_ref()?;
        let read = usage.cache_read_tokens.unwrap_or(0);
        let write = usage.cache_write_tokens.unwrap_or(0);
        let plain = usage
            .prompt_tokens
            .saturating_sub(read)
            .saturating_sub(write);

        let total = f64::from(plain) * p.input
            + f64::from(read) * p.cache_read.unwrap_or(p.input)
            + f64::from(write) * p.cache_write.unwrap_or(p.input)
            + f64::from(usage.completion_tokens) * p.output;
        Some(total / 1_000_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses_and_has_unique_ids() {
        let ids: Vec<&str> = ModelInfo::builtin_table()
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert!(ids.len() > 10);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn every_model_the_provider_docs_and_tests_name_resolves() {
        for id in [
            "claude-opus-4-8",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
            "claude-3-5-sonnet-20241022",
            "claude-3-5-haiku-20241022",
            "claude-3-opus-20240229",
            "claude-3-haiku-20240307",
            "claude-opus-5-5",
            "gpt-4o",
            "gpt-4o-mini",
            "anthropic/claude-3.5-sonnet",
            "openai/gpt-4o:free",
        ] {
            assert!(ModelInfo::builtin(id).is_some(), "{id} has no entry");
        }
    }

    #[test]
    fn the_longest_prefix_wins_at_a_dash_boundary() {
        assert!(ModelInfo::builtin("gpt-4o-mini-2024-07-18")
            .unwrap()
            .pricing
            .is_some_and(|p| (p.input - 0.15).abs() < 1e-9));
        // "o3" must not capture "o3-mini", nor "gpt-4o" capture "gpt-4omni".
        assert!(!ModelInfo::builtin("o3-mini").unwrap().supports_images);
        assert!(ModelInfo::builtin("gpt-4omni").is_none());
        assert_eq!(ModelInfo::builtin("gpt-4o-x").unwrap().id, "gpt-4o-x");
    }

    #[test]
    fn capabilities_differ_by_model() {
        assert!(!ModelInfo::builtin("gpt-4o").unwrap().supports_reasoning);
        assert!(
            ModelInfo::builtin("claude-sonnet-4-6")
                .unwrap()
                .supports_reasoning
        );
        assert!(
            !ModelInfo::builtin("claude-3-5-sonnet-20241022")
                .unwrap()
                .supports_reasoning
        );
    }

    #[test]
    fn estimate_cost_prices_cache_reads_and_writes_separately() {
        let info = ModelInfo::new("anthropic", "m").with_pricing(Pricing {
            input: 3.0,
            output: 15.0,
            cache_read: Some(0.3),
            cache_write: Some(3.75),
        });
        // 1M prompt = 400k plain + 500k read + 100k write; 1M output.
        let usage = Usage::from_tokens(1_000_000, 1_000_000).with_cache(500_000, 100_000);
        let expected = 0.4 * 3.0 + 0.5 * 0.3 + 0.1 * 3.75 + 15.0;
        assert!((info.estimate_cost(&usage).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn estimate_cost_is_none_without_pricing() {
        assert!(ModelInfo::new("p", "m")
            .estimate_cost(&Usage::from_tokens(1, 1))
            .is_none());
    }

    #[test]
    fn a_partial_override_deserializes() {
        let info: ModelInfo = serde_json::from_str(
            r#"{"id": "my-local-model", "context_window": 32768, "supports_tools": true}"#,
        )
        .unwrap();
        assert_eq!(info.context_window, Some(32768));
        assert!(info.supports_tools && !info.supports_images);
    }
}
