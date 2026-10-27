use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde_json::{json, Map, Value};

use cosmos_llm_adapter::{Adapter, AdapterError, ChatAdapter, Message};
use cosmos_llm_cache::CacheStore;
use cosmos_llm_example::Example;
use cosmos_llm_signature::{Field, Signature};

use crate::error::PredictError;
use crate::model::{CallOptions, Completion, LanguageModel, Usage};
use crate::module::Module;
use crate::prediction::Prediction;

/// The adapter a predictor uses unless told otherwise, resolved through the
/// adapter registry so that an application's own `chat` adapter takes effect.
pub const DEFAULT_ADAPTER: &str = "chat";

/// The output field [`Predict::chain_of_thought`] adds.
pub const REASONING_FIELD: &str = "reasoning";

/// The description given to the reasoning field.
pub const REASONING_DESC: &str =
    "Think step by step. Work through the problem before giving the other fields.";

/// How many times a reply that fails to parse is retried by default.
pub const DEFAULT_MAX_PARSE_RETRIES: usize = 1;

/// One LLM call, described by a signature.
///
/// This is where the pieces meet: the [`Adapter`] renders the signature and
/// inputs into messages, the [`LanguageModel`] answers them, and the adapter
/// parses the reply back into typed values. Each collaborator is a trait
/// object, so any of them can be replaced without touching the others.
///
/// When a reply cannot be parsed, the predictor shows the model its own
/// reply and the parse error and asks again. Models usually fix a format
/// mistake once shown it, and one extra call is cheaper than a failed run.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "client")] {
/// use std::sync::Arc;
/// use cosmos_llm::{providers::mock::MockProvider, Client};
/// use cosmos_llm_predict::Predict;
/// use cosmos_llm_signature::Signature;
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// let model = Client::from_provider(Box::new(
///     MockProvider::new().respond("[[ ## answer ## ]]\n4\n\n[[ ## done ## ]]"),
/// ))
/// .with_model("mock");
///
/// let predict = Predict::new(Signature::parse("question -> answer: int").unwrap(), Arc::new(model));
/// let prediction = predict.call(&json!({ "question": "What is 2+2?" })).await.unwrap();
/// assert_eq!(prediction.values["answer"], 4);
/// # })
/// # }
/// ```
#[derive(Clone)]
pub struct Predict {
    signature: Signature,
    model: Arc<dyn LanguageModel>,
    adapter: Arc<dyn Adapter>,
    cache: Option<Arc<dyn CacheStore>>,
    demos: Vec<Example>,
    options: CallOptions,
    max_parse_retries: usize,
}

impl Predict {
    /// A predictor for `signature`, answered by `model`, using the default
    /// adapter, no cache, and no demonstrations.
    pub fn new(signature: Signature, model: Arc<dyn LanguageModel>) -> Self {
        // The built-in is always registered, so the fallback only matters if a
        // plugin's constructor is broken; a predictor is still better than none.
        let adapter: Arc<dyn Adapter> = match cosmos_llm_adapter::adapter(DEFAULT_ADAPTER) {
            Ok(adapter) => Arc::from(adapter),
            Err(_) => Arc::new(ChatAdapter),
        };
        Self {
            signature,
            model,
            adapter,
            cache: None,
            demos: Vec::new(),
            options: CallOptions::default(),
            max_parse_retries: DEFAULT_MAX_PARSE_RETRIES,
        }
    }

    /// A predictor that asks the model to reason before answering.
    ///
    /// Adds a [`REASONING_FIELD`] output ahead of the declared outputs.
    /// Adapters render outputs in order, so the model writes its reasoning
    /// first and its answer after — that ordering is the whole mechanism. The
    /// reasoning stays on the prediction for logging or for a metric that
    /// cares how an answer was reached.
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Signature`] when the signature already has a
    /// field named `reasoning`.
    pub fn chain_of_thought(
        signature: Signature,
        model: Arc<dyn LanguageModel>,
    ) -> Result<Self, PredictError> {
        let reasoning = Field::output(REASONING_FIELD).desc(REASONING_DESC);
        Ok(Self::new(signature.with_leading_output(reasoning)?, model))
    }

    /// Replaces the adapter.
    #[must_use]
    pub fn adapter(mut self, adapter: Arc<dyn Adapter>) -> Self {
        self.adapter = adapter;
        self
    }

    /// Caches replies in `cache`, keyed by everything that shapes the request.
    #[must_use]
    pub fn cache(mut self, cache: Arc<dyn CacheStore>) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Sets the demonstrations rendered ahead of each call.
    #[must_use]
    pub fn demos(mut self, demos: Vec<Example>) -> Self {
        self.demos = demos;
        self
    }

    /// Sets the default call options; per-call options are layered over them.
    #[must_use]
    pub fn options(mut self, options: CallOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets how many times an unparseable reply is retried with a correction
    /// turn. Zero disables retries.
    #[must_use]
    pub fn max_parse_retries(mut self, retries: usize) -> Self {
        self.max_parse_retries = retries;
        self
    }

    /// A copy with different instructions, leaving this predictor untouched.
    #[must_use]
    pub fn with_instructions(&self, text: impl Into<String>) -> Self {
        let mut copy = self.clone();
        copy.signature = self.signature.with_instructions(text);
        copy
    }

    /// The signature, including any field this predictor added.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }

    /// The demonstrations.
    pub fn demo_examples(&self) -> &[Example] {
        &self.demos
    }

    /// Runs the predictor on a JSON object of inputs with default options.
    ///
    /// # Errors
    ///
    /// As for [`Module::forward`].
    pub async fn call(&self, inputs: &Value) -> Result<Prediction, PredictError> {
        let Value::Object(inputs) = inputs else {
            return Err(PredictError::Config(format!(
                "inputs must be a JSON object, got {inputs}"
            )));
        };
        self.forward(inputs, &CallOptions::default()).await
    }

    /// The tunable state — instructions and demonstrations — as JSON, for
    /// saving an optimized predictor.
    pub fn dump_state(&self) -> Value {
        let instructions = self
            .signature
            .has_instructions()
            .then(|| self.signature.instructions());
        json!({ "instructions": instructions, "demos": self.demos })
    }

    /// Restores state produced by [`Predict::dump_state`].
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Config`] when the state is not shaped as
    /// `dump_state` writes it.
    pub fn load_state(&mut self, state: &Value) -> Result<(), PredictError> {
        if let Some(text) = state.get("instructions").and_then(Value::as_str) {
            self.signature = self.signature.with_instructions(text);
        }
        if let Some(demos) = state.get("demos") {
            self.demos = serde_json::from_value(demos.clone()).map_err(|e| {
                PredictError::Config(format!("invalid demos in predictor state: {e}"))
            })?;
        }
        Ok(())
    }

    async fn run(
        &self,
        inputs: &Map<String, Value>,
        options: &CallOptions,
    ) -> Result<Prediction, PredictError> {
        let sig = &self.signature;
        let inputs = sig.coerce_inputs(&Value::Object(inputs.clone()))?;
        let demos: Vec<Map<String, Value>> = self
            .demos
            .iter()
            .map(|demo| demo.values().clone())
            .collect();
        let mut messages = self.adapter.format(sig, &inputs, &demos)?;

        let mut options = options.over(&self.options);
        // Adapter parameters fill gaps; an explicit option for the same key
        // wins, since the caller asked for it by name.
        for (key, value) in self.adapter.request_params(sig)? {
            options.extra.entry(key).or_insert(value);
        }

        let allowed = self.max_parse_retries + 1;
        let mut usage: Option<Usage> = None;
        for attempt in 1..=allowed {
            let (completion, cached) = self.complete(&messages, &options).await?;
            if let Some(spent) = completion.usage.as_ref().filter(|_| !cached) {
                usage = Some(usage.map_or_else(|| spent.clone(), |total| total.plus(spent)));
            }
            match self.adapter.parse(sig, &completion.text) {
                Ok(values) => {
                    return Ok(Prediction {
                        values,
                        completion: completion.text,
                        usage,
                        cached,
                        attempts: attempt,
                        meta: Map::new(),
                    })
                }
                Err(AdapterError::Parse { message, .. }) => {
                    if attempt == allowed {
                        return Err(PredictError::Parse {
                            message,
                            completion: completion.text,
                            attempts: attempt,
                        });
                    }
                    messages.push(Message::assistant(completion.text));
                    messages.push(Message::user(format!(
                        "That response could not be parsed: {message}\n\n\
                         Reply again with the same content in the required format. \
                         Output only the formatted fields."
                    )));
                }
                Err(other) => return Err(other.into()),
            }
        }
        unreachable!("the loop runs at least once and returns on its last attempt")
    }

    /// Sends the messages, through the cache when one is set.
    ///
    /// Returns the completion and whether it was a cache hit. An entry that
    /// does not decode is treated as a miss and overwritten, and a failed
    /// write is ignored: losing a cache entry costs a future call, but losing
    /// the completion in hand costs this one.
    async fn complete(
        &self,
        messages: &[Message],
        options: &CallOptions,
    ) -> Result<(Completion, bool), PredictError> {
        let Some(cache) = &self.cache else {
            return Ok((self.model.complete(messages, options).await?, false));
        };
        let key = cosmos_llm_cache::key_for(&json!({
            "signature": self.signature.cache_key(),
            "model": self.model.identity(),
            "messages": messages,
            "options": options,
        }));
        if let Some(hit) = cache.get(&key)? {
            if let Ok(completion) = serde_json::from_value::<Completion>(hit) {
                return Ok((completion, true));
            }
        }
        let completion = self.model.complete(messages, options).await?;
        if let Ok(value) = serde_json::to_value(&completion) {
            let _ = cache.put(&key, &value);
        }
        Ok((completion, false))
    }
}

impl Module for Predict {
    fn forward<'a>(
        &'a self,
        inputs: &'a Map<String, Value>,
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Prediction, PredictError>> {
        Box::pin(self.run(inputs, options))
    }
}

impl std::fmt::Debug for Predict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Predict")
            .field("signature", &self.signature.to_string())
            .field("model", &self.model.identity())
            .field("demos", &self.demos.len())
            .field("cached", &self.cache.is_some())
            .finish_non_exhaustive()
    }
}
