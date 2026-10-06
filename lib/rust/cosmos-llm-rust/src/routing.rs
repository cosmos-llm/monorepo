//! OpenRouter provider routing.
//!
//! OpenRouter serves one model identifier from any of several upstream
//! providers, and they are not interchangeable. They differ in price, in
//! throughput, in context length, in quantization, in whether they retain
//! prompts for training, and in whether they honour a parameter the request
//! depends on. Left alone, OpenRouter load-balances across the ones it thinks
//! are healthy.
//!
//! [`OpenRouterRouting`] is how a caller overrides that: pin to one provider,
//! rank a preference order, exclude a provider outright, or sort the whole
//! candidate set by price, throughput, or latency.
//!
//! Attach it to a request with
//! [`CompletionRequest::with_openrouter_routing`](crate::CompletionRequest::with_openrouter_routing),
//! or set a default for every request on a provider with
//! [`OpenRouterProvider::with_routing`](crate::providers::openrouter::OpenRouterProvider::with_routing).
//!
//! # Examples
//!
//! Pin a model to a single upstream and fail rather than silently fall back:
//!
//! ```
//! use cosmos_llm::OpenRouterRouting;
//!
//! let routing = OpenRouterRouting::pinned_to("together");
//! assert_eq!(routing.only, vec!["together"]);
//! assert_eq!(routing.allow_fallbacks, Some(false));
//! ```
//!
//! Rank a preference order, keeping the rest of the field as backup:
//!
//! ```
//! use cosmos_llm::OpenRouterRouting;
//!
//! let routing = OpenRouterRouting::new()
//!     .order(["azure", "openai"])
//!     .ignore(["deepinfra"]);
//! ```
//!
//! Take the cheapest provider that can serve the request:
//!
//! ```
//! use cosmos_llm::{OpenRouterRouting, ProviderSort};
//!
//! let routing = OpenRouterRouting::new().sort(ProviderSort::Price);
//! ```

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The attribute OpenRouter ranks candidate providers by.
///
/// Setting a sort replaces OpenRouter's default load balancing, which weighs
/// price against recent reliability, with a single-attribute ordering.
///
/// # Examples
///
/// ```
/// use cosmos_llm::ProviderSort;
///
/// assert_eq!(ProviderSort::Throughput.as_str(), "throughput");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ProviderSort {
    /// Cheapest first, by the model's per-token price at each provider.
    Price,
    /// Highest measured tokens per second first.
    Throughput,
    /// Lowest measured time to first token first.
    Latency,
    /// OpenRouter's accuracy-oriented ranking.
    ///
    /// Present in the API schema but not on the prose documentation page, so
    /// treat its exact behaviour as less settled than the other three.
    Exacto,
}

impl ProviderSort {
    /// Returns the wire representation OpenRouter expects.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::ProviderSort;
    ///
    /// assert_eq!(ProviderSort::Price.as_str(), "price");
    /// assert_eq!(ProviderSort::Latency.as_str(), "latency");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Price => "price",
            Self::Throughput => "throughput",
            Self::Latency => "latency",
            Self::Exacto => "exacto",
        }
    }
}

impl std::fmt::Display for ProviderSort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a provider may retain and train on the request.
///
/// # Examples
///
/// ```
/// use cosmos_llm::DataCollection;
///
/// assert_eq!(DataCollection::Deny.as_str(), "deny");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DataCollection {
    /// Providers that store data are eligible. OpenRouter's default.
    Allow,
    /// Restrict routing to providers with a no-retention policy.
    Deny,
}

impl DataCollection {
    /// Returns the wire representation OpenRouter expects.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::DataCollection;
    ///
    /// assert_eq!(DataCollection::Allow.as_str(), "allow");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

impl std::fmt::Display for DataCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a [`ProviderSort`] is applied across a fallback model list.
///
/// Only meaningful alongside a `models` fallback list; with a single model
/// the two behave the same.
///
/// # Examples
///
/// ```
/// use cosmos_llm::SortPartition;
///
/// assert_eq!(SortPartition::Model.as_str(), "model");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SortPartition {
    /// Exhaust the first model's providers before trying the next model.
    /// OpenRouter's default.
    Model,
    /// Rank every provider of every listed model together, so a cheaper
    /// provider of a fallback model can beat a pricier one of the first.
    None,
}

impl SortPartition {
    /// Returns the wire representation OpenRouter expects.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::SortPartition;
    ///
    /// assert_eq!(SortPartition::None.as_str(), "none");
    /// ```
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::None => "none",
        }
    }
}

impl std::fmt::Display for SortPartition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A per-token price ceiling, in US dollars per million tokens.
///
/// A provider whose price for the model exceeds any set ceiling is dropped
/// from the candidate set. Unset fields are unconstrained.
///
/// # Examples
///
/// ```
/// use cosmos_llm::MaxPrice;
///
/// // At most $3/M in, $15/M out.
/// let cap = MaxPrice::new().prompt(3.0).completion(15.0);
/// assert_eq!(cap.prompt, Some(3.0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct MaxPrice {
    /// Ceiling on input tokens, in dollars per million.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<f64>,
    /// Ceiling on output tokens, in dollars per million.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion: Option<f64>,
    /// Ceiling on image input, in dollars per image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<f64>,
    /// Ceiling on the flat per-request charge, in dollars per request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<f64>,
    /// Ceiling on audio input, in dollars per audio unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<f64>,
}

impl MaxPrice {
    /// Creates an unconstrained price ceiling.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new(), MaxPrice::default());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Caps the input-token price, in dollars per million tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new().prompt(1.5).prompt, Some(1.5));
    /// ```
    pub fn prompt(mut self, dollars_per_million: f64) -> Self {
        self.prompt = Some(dollars_per_million);
        self
    }

    /// Caps the output-token price, in dollars per million tokens.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new().completion(9.0).completion, Some(9.0));
    /// ```
    pub fn completion(mut self, dollars_per_million: f64) -> Self {
        self.completion = Some(dollars_per_million);
        self
    }

    /// Caps the image-input price, in dollars per image.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new().image(2.0).image, Some(2.0));
    /// ```
    pub fn image(mut self, dollars_per_image: f64) -> Self {
        self.image = Some(dollars_per_image);
        self
    }

    /// Caps the flat per-request charge, in dollars per request.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new().request(0.5).request, Some(0.5));
    /// ```
    pub fn request(mut self, dollars_per_request: f64) -> Self {
        self.request = Some(dollars_per_request);
        self
    }

    /// Caps the audio-input price, in dollars per audio unit.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert_eq!(MaxPrice::new().audio(4.0).audio, Some(4.0));
    /// ```
    pub fn audio(mut self, dollars_per_unit: f64) -> Self {
        self.audio = Some(dollars_per_unit);
        self
    }

    /// Returns `true` when no ceiling is set on any field.
    ///
    /// A fully empty [`MaxPrice`] is omitted from the request body rather than
    /// sent as `{}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::MaxPrice;
    ///
    /// assert!(MaxPrice::new().is_empty());
    /// assert!(!MaxPrice::new().prompt(1.0).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.prompt.is_none()
            && self.completion.is_none()
            && self.image.is_none()
            && self.request.is_none()
            && self.audio.is_none()
    }
}

/// An OpenRouter provider-routing preference.
///
/// Serialized into the `provider` field of the request body. Every field is
/// optional, and an untouched [`OpenRouterRouting`] serializes to an empty
/// object, which leaves OpenRouter's default behaviour alone.
///
/// Provider names are OpenRouter's slugs — `"anthropic"`, `"together"`,
/// `"deepinfra"`, `"azure"` — not the vendor prefix of a model id. The two
/// look alike and are not the same namespace: `"anthropic/claude-3.5-sonnet"`
/// is a model, `"anthropic"` in [`OpenRouterRouting::only`] is the upstream
/// serving it.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{OpenRouterRouting, ProviderSort};
///
/// let routing = OpenRouterRouting::new()
///     .order(["azure", "openai"])
///     .sort(ProviderSort::Throughput)
///     .require_parameters(true);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenRouterRouting {
    /// Providers to try first, most preferred first.
    ///
    /// Providers not named here are still eligible as fallbacks unless
    /// [`OpenRouterRouting::allow_fallbacks`] is `Some(false)`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub order: Vec<String>,
    /// The only providers allowed to serve the request.
    ///
    /// Anything not named is excluded outright, which is the difference
    /// between this and [`OpenRouterRouting::order`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only: Vec<String>,
    /// Providers excluded from serving the request.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore: Vec<String>,
    /// Whether OpenRouter may fall back to a provider outside
    /// [`OpenRouterRouting::order`] / [`OpenRouterRouting::only`].
    ///
    /// `Some(false)` makes the request fail rather than route somewhere
    /// unlisted. OpenRouter defaults to allowing fallbacks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_fallbacks: Option<bool>,
    /// Restricts routing to providers that support every parameter the request
    /// sets.
    ///
    /// Without this a provider that ignores, say, `stop` can still serve the
    /// call, and the parameter is silently dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_parameters: Option<bool>,
    /// Whether providers that retain and train on prompts are eligible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_collection: Option<DataCollection>,
    /// Ranks candidate providers by a single attribute.
    ///
    /// Setting this replaces OpenRouter's default load balancing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<ProviderSort>,
    /// Restricts routing to providers serving the model at one of these
    /// quantization levels, e.g. `"fp8"`, `"fp16"`, `"bf16"`, `"int4"`.
    ///
    /// A model served at a lower precision is cheaper and measurably worse;
    /// this is how a caller who cares refuses the cheap one.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quantizations: Vec<String>,
    /// How [`OpenRouterRouting::sort`] is applied across a fallback model
    /// list. Serialized by promoting `sort` to its object form.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_partition: Option<SortPartition>,
    /// Per-token price ceilings. Providers above any ceiling are dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_price: Option<MaxPrice>,
    /// Restricts routing to providers under a zero-data-retention policy.
    ///
    /// Stricter than [`DataCollection::Deny`], which only rules out training
    /// on the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zdr: Option<bool>,
    /// Restricts routing to providers whose terms permit using the output to
    /// train another model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enforce_distillable_text: Option<bool>,
    /// Deprioritizes providers below this throughput, in tokens per second.
    ///
    /// A preference, not a filter: unlike [`OpenRouterRouting::max_price`],
    /// a provider under the bar is ranked down rather than excluded, so the
    /// request will not fail for want of a fast enough provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_min_throughput: Option<f64>,
    /// Deprioritizes providers slower than this time to first token, in
    /// seconds. A preference, not a filter — see
    /// [`OpenRouterRouting::preferred_min_throughput`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_max_latency: Option<f64>,
}

impl OpenRouterRouting {
    /// Creates an empty routing preference, which changes nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// assert!(OpenRouterRouting::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Pins the request to a single provider, with fallbacks disabled.
    ///
    /// The strict reading of "pin": the named provider serves the request or
    /// it fails. Use [`OpenRouterRouting::only`] alone to allow the rest of
    /// the field as backup.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::pinned_to("deepinfra");
    /// assert_eq!(routing.only, vec!["deepinfra"]);
    /// assert_eq!(routing.allow_fallbacks, Some(false));
    /// ```
    pub fn pinned_to(provider: impl Into<String>) -> Self {
        Self::new().only([provider.into()]).allow_fallbacks(false)
    }

    /// Sets the preference order, most preferred first.
    ///
    /// Replaces any order already set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().order(["azure", "openai"]);
    /// assert_eq!(routing.order, vec!["azure", "openai"]);
    /// ```
    pub fn order<I, S>(mut self, providers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.order = providers.into_iter().map(Into::into).collect();
        self
    }

    /// Restricts routing to the named providers.
    ///
    /// Replaces any allow-list already set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().only(["anthropic"]);
    /// assert_eq!(routing.only, vec!["anthropic"]);
    /// ```
    pub fn only<I, S>(mut self, providers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.only = providers.into_iter().map(Into::into).collect();
        self
    }

    /// Excludes the named providers.
    ///
    /// Replaces any deny-list already set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().ignore(["deepinfra"]);
    /// assert_eq!(routing.ignore, vec!["deepinfra"]);
    /// ```
    pub fn ignore<I, S>(mut self, providers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.ignore = providers.into_iter().map(Into::into).collect();
        self
    }

    /// Sets whether unlisted providers may serve the request as a fallback.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().allow_fallbacks(false);
    /// assert_eq!(routing.allow_fallbacks, Some(false));
    /// ```
    pub fn allow_fallbacks(mut self, allow: bool) -> Self {
        self.allow_fallbacks = Some(allow);
        self
    }

    /// Requires that the serving provider support every parameter set on the
    /// request.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().require_parameters(true);
    /// assert_eq!(routing.require_parameters, Some(true));
    /// ```
    pub fn require_parameters(mut self, require: bool) -> Self {
        self.require_parameters = Some(require);
        self
    }

    /// Sets whether prompt-retaining providers are eligible.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{DataCollection, OpenRouterRouting};
    ///
    /// let routing = OpenRouterRouting::new().data_collection(DataCollection::Deny);
    /// assert_eq!(routing.data_collection, Some(DataCollection::Deny));
    /// ```
    pub fn data_collection(mut self, policy: DataCollection) -> Self {
        self.data_collection = Some(policy);
        self
    }

    /// Ranks candidate providers by a single attribute.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{OpenRouterRouting, ProviderSort};
    ///
    /// let routing = OpenRouterRouting::new().sort(ProviderSort::Price);
    /// assert_eq!(routing.sort, Some(ProviderSort::Price));
    /// ```
    pub fn sort(mut self, sort: ProviderSort) -> Self {
        self.sort = Some(sort);
        self
    }

    /// Sets how the sort is applied across a fallback model list.
    ///
    /// Has no effect unless [`OpenRouterRouting::sort`] is also set.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{OpenRouterRouting, ProviderSort, SortPartition};
    ///
    /// let routing = OpenRouterRouting::new()
    ///     .sort(ProviderSort::Price)
    ///     .sort_partition(SortPartition::None);
    /// assert_eq!(routing.to_value()["sort"]["partition"], "none");
    /// ```
    pub fn sort_partition(mut self, partition: SortPartition) -> Self {
        self.sort_partition = Some(partition);
        self
    }

    /// Restricts routing to zero-data-retention providers.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().zdr(true);
    /// assert_eq!(routing.zdr, Some(true));
    /// ```
    pub fn zdr(mut self, require: bool) -> Self {
        self.zdr = Some(require);
        self
    }

    /// Restricts routing to providers whose terms permit distilling the
    /// output into another model.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().enforce_distillable_text(true);
    /// assert_eq!(routing.enforce_distillable_text, Some(true));
    /// ```
    pub fn enforce_distillable_text(mut self, require: bool) -> Self {
        self.enforce_distillable_text = Some(require);
        self
    }

    /// Deprioritizes providers below this throughput, in tokens per second.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().preferred_min_throughput(40.0);
    /// assert_eq!(routing.preferred_min_throughput, Some(40.0));
    /// ```
    pub fn preferred_min_throughput(mut self, tokens_per_second: f64) -> Self {
        self.preferred_min_throughput = Some(tokens_per_second);
        self
    }

    /// Deprioritizes providers slower than this time to first token, in
    /// seconds.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().preferred_max_latency(2.5);
    /// assert_eq!(routing.preferred_max_latency, Some(2.5));
    /// ```
    pub fn preferred_max_latency(mut self, seconds: f64) -> Self {
        self.preferred_max_latency = Some(seconds);
        self
    }

    /// Restricts routing to the named quantization levels.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// let routing = OpenRouterRouting::new().quantizations(["fp16", "bf16"]);
    /// assert_eq!(routing.quantizations, vec!["fp16", "bf16"]);
    /// ```
    pub fn quantizations<I, S>(mut self, levels: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.quantizations = levels.into_iter().map(Into::into).collect();
        self
    }

    /// Sets per-token price ceilings.
    ///
    /// An empty [`MaxPrice`] clears the ceiling rather than sending `{}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{MaxPrice, OpenRouterRouting};
    ///
    /// let routing = OpenRouterRouting::new().max_price(MaxPrice::new().prompt(3.0));
    /// assert_eq!(routing.max_price.unwrap().prompt, Some(3.0));
    /// ```
    pub fn max_price(mut self, max: MaxPrice) -> Self {
        self.max_price = if max.is_empty() { None } else { Some(max) };
        self
    }

    /// Returns `true` when no preference of any kind is set.
    ///
    /// An empty routing block is omitted from the request body rather than
    /// sent as `{}`.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::OpenRouterRouting;
    ///
    /// assert!(OpenRouterRouting::new().is_empty());
    /// assert!(!OpenRouterRouting::new().sort(cosmos_llm::ProviderSort::Price).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
            && self.only.is_empty()
            && self.ignore.is_empty()
            && self.allow_fallbacks.is_none()
            && self.require_parameters.is_none()
            && self.data_collection.is_none()
            && self.sort.is_none()
            && self.sort_partition.is_none()
            && self.quantizations.is_empty()
            && self.max_price.is_none()
            && self.zdr.is_none()
            && self.enforce_distillable_text.is_none()
            && self.preferred_min_throughput.is_none()
            && self.preferred_max_latency.is_none()
    }

    /// Serializes to the JSON object OpenRouter expects in the `provider`
    /// field.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{OpenRouterRouting, ProviderSort};
    ///
    /// let value = OpenRouterRouting::new()
    ///     .sort(ProviderSort::Price)
    ///     .to_value();
    /// assert_eq!(value["sort"], "price");
    /// ```
    pub fn to_value(&self) -> Value {
        // Every field is skipped when unset, so this cannot fail and cannot
        // produce anything but an object.
        let mut value =
            serde_json::to_value(self).unwrap_or_else(|_| Value::Object(Default::default()));

        // `sort_partition` is this crate's field, not OpenRouter's. On the
        // wire the partition lives inside `sort`, which becomes an object
        // instead of a string. OpenRouter rejects unknown keys in this block
        // with a 400, so the local field has to be folded in and removed
        // rather than left to ride along.
        if let Some(map) = value.as_object_mut() {
            if let Some(partition) = map.remove("sort_partition") {
                if let Some(by) = map.remove("sort") {
                    map.insert(
                        "sort".to_owned(),
                        json!({ "by": by, "partition": partition }),
                    );
                }
            }
        }

        value
    }
}

impl OpenRouterRouting {
    /// Parses a routing block back from the JSON shape
    /// [`OpenRouterRouting::to_value`] produces.
    ///
    /// Accepts both spellings of `sort`: the plain string, and the
    /// `{"by", "partition"}` object that a partition promotes it to.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{OpenRouterRouting, ProviderSort, SortPartition};
    ///
    /// let routing = OpenRouterRouting::new()
    ///     .sort(ProviderSort::Price)
    ///     .sort_partition(SortPartition::None);
    /// let back = OpenRouterRouting::from_value(&routing.to_value()).unwrap();
    /// assert_eq!(back, routing);
    /// ```
    pub fn from_value(value: &Value) -> Option<Self> {
        let mut value = value.clone();

        // Undo the `sort` promotion done by `to_value`, so a round trip lands
        // back on the same struct rather than failing to type-check `sort`.
        if let Some(map) = value.as_object_mut() {
            if let Some(sort) = map.get("sort").cloned() {
                if let Some(obj) = sort.as_object() {
                    match obj.get("by") {
                        Some(by) => map.insert("sort".to_owned(), by.clone()),
                        None => map.remove("sort"),
                    };
                    if let Some(partition) = obj.get("partition") {
                        map.insert("sort_partition".to_owned(), partition.clone());
                    }
                }
            }
        }

        serde_json::from_value(value).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_routing_serializes_to_an_empty_object() {
        // An untouched preference must not change how the request routes.
        assert_eq!(OpenRouterRouting::new().to_value(), json!({}));
        assert!(OpenRouterRouting::new().is_empty());
    }

    #[test]
    fn order_only_and_ignore_serialize_as_arrays() {
        let value = OpenRouterRouting::new()
            .order(["azure", "openai"])
            .only(["azure", "openai", "together"])
            .ignore(["deepinfra"])
            .to_value();

        assert_eq!(value["order"], json!(["azure", "openai"]));
        assert_eq!(value["only"], json!(["azure", "openai", "together"]));
        assert_eq!(value["ignore"], json!(["deepinfra"]));
    }

    #[test]
    fn order_preserves_the_caller_sequence() {
        // The whole point of `order` is the sequence; a set would lose it.
        let value = OpenRouterRouting::new().order(["c", "a", "b"]).to_value();
        assert_eq!(value["order"], json!(["c", "a", "b"]));
    }

    #[test]
    fn sort_serializes_to_openrouter_spelling() {
        for (sort, expected) in [
            (ProviderSort::Price, "price"),
            (ProviderSort::Throughput, "throughput"),
            (ProviderSort::Latency, "latency"),
        ] {
            let value = OpenRouterRouting::new().sort(sort).to_value();
            assert_eq!(value["sort"], json!(expected));
        }
    }

    #[test]
    fn data_collection_serializes_to_allow_or_deny() {
        let deny = OpenRouterRouting::new()
            .data_collection(DataCollection::Deny)
            .to_value();
        assert_eq!(deny["data_collection"], json!("deny"));

        let allow = OpenRouterRouting::new()
            .data_collection(DataCollection::Allow)
            .to_value();
        assert_eq!(allow["data_collection"], json!("allow"));
    }

    #[test]
    fn booleans_serialize_when_set_and_vanish_when_not() {
        let value = OpenRouterRouting::new()
            .allow_fallbacks(false)
            .require_parameters(true)
            .to_value();
        assert_eq!(value["allow_fallbacks"], json!(false));
        assert_eq!(value["require_parameters"], json!(true));

        // `false` and "unset" are different instructions to OpenRouter, so an
        // unset flag must be absent rather than defaulted.
        let bare = OpenRouterRouting::new().to_value();
        assert!(bare.get("allow_fallbacks").is_none());
        assert!(bare.get("require_parameters").is_none());
    }

    #[test]
    fn pinned_to_sets_only_and_forbids_fallbacks() {
        let value = OpenRouterRouting::pinned_to("together").to_value();
        assert_eq!(value["only"], json!(["together"]));
        assert_eq!(value["allow_fallbacks"], json!(false));
    }

    #[test]
    fn max_price_serializes_only_the_ceilings_that_are_set() {
        let value = OpenRouterRouting::new()
            .max_price(MaxPrice::new().prompt(3.0).completion(15.0))
            .to_value();

        assert_eq!(value["max_price"]["prompt"], json!(3.0));
        assert_eq!(value["max_price"]["completion"], json!(15.0));
        assert!(value["max_price"].get("image").is_none());
        assert!(value["max_price"].get("request").is_none());
    }

    #[test]
    fn an_empty_max_price_is_dropped_rather_than_sent_as_an_empty_object() {
        let routing = OpenRouterRouting::new().max_price(MaxPrice::new());
        assert_eq!(routing.max_price, None);
        assert!(routing.to_value().get("max_price").is_none());
    }

    #[test]
    fn quantizations_serialize_as_an_array() {
        let value = OpenRouterRouting::new()
            .quantizations(["fp16", "bf16"])
            .to_value();
        assert_eq!(value["quantizations"], json!(["fp16", "bf16"]));
    }

    #[test]
    fn routing_round_trips_through_json() {
        let routing = OpenRouterRouting::new()
            .order(["azure"])
            .ignore(["deepinfra"])
            .sort(ProviderSort::Latency)
            .data_collection(DataCollection::Deny)
            .require_parameters(true)
            .allow_fallbacks(false)
            .quantizations(["fp8"])
            .max_price(MaxPrice::new().prompt(1.25));

        let back: OpenRouterRouting = serde_json::from_value(routing.to_value()).unwrap();
        assert_eq!(back, routing);
    }

    #[test]
    fn builders_replace_rather_than_append() {
        let routing = OpenRouterRouting::new().only(["a"]).only(["b"]);
        assert_eq!(routing.only, vec!["b"]);
    }

    #[test]
    fn sort_and_data_collection_render_as_strings() {
        assert_eq!(ProviderSort::Price.to_string(), "price");
        assert_eq!(DataCollection::Deny.to_string(), "deny");
    }
}
