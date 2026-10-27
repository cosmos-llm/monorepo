use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde_json::{json, Map, Value};

use crate::error::PredictError;
use crate::model::{CallOptions, Usage};
use crate::module::Module;
use crate::prediction::Prediction;

/// Scores one attempt: the inputs and the prediction in, a number out.
///
/// What it measures is the caller's business — a validator, a checksum,
/// another model call made synchronously elsewhere.
pub type Reward = dyn Fn(&Map<String, Value>, &Prediction) -> f64 + Send + Sync;

/// The sampling temperature used unless told otherwise.
pub const DEFAULT_TEMPERATURE: f32 = 1.0;

/// Samples a module several times and keeps the best-scoring prediction.
///
/// Draws run one after another at a raised temperature so they actually
/// differ, and each gets its own cache salt so a cache does not hand back the
/// first draw N times. The first draw scoring at or above the threshold ends
/// sampling early: an easy input costs one call, and only a hard one pays for
/// all N.
///
/// A draw that fails is skipped rather than fatal; with N samples in hand
/// there is no reason one bad draw should lose the others. A reward that
/// returns NaN ranks below every real score.
///
/// The winning prediction's [`Prediction::meta`] gains `best_of_n_score` and
/// `best_of_n_attempts`, and its usage is the total across every draw, since
/// that is what the answer cost.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "client")] {
/// use std::sync::Arc;
/// use cosmos_llm::{providers::mock::MockProvider, Client};
/// use cosmos_llm_predict::{BestOfN, CallOptions, Module, Predict};
/// use cosmos_llm_signature::Signature;
///
/// # tokio_test::block_on(async {
/// let reply = |n: u32| format!("[[ ## answer ## ]]\n{n}\n\n[[ ## done ## ]]");
/// let model = Client::from_provider(Box::new(
///     MockProvider::new().respond(reply(3)).respond(reply(8)).respond(reply(5)),
/// ))
/// .with_model("mock");
///
/// let predict = Predict::new(Signature::parse("q -> answer: int").unwrap(), Arc::new(model));
/// let best = BestOfN::new(predict, 3, |_inputs, p| p.values["answer"].as_f64().unwrap_or(0.0)).unwrap();
///
/// let inputs = serde_json::json!({ "q": "pick a number" });
/// let prediction = best.forward(inputs.as_object().unwrap(), &CallOptions::default()).await.unwrap();
/// assert_eq!(prediction.values["answer"], 8);
/// assert_eq!(prediction.meta["best_of_n_attempts"], 3);
/// # })
/// # }
/// ```
pub struct BestOfN<M> {
    module: M,
    n: usize,
    threshold: Option<f64>,
    temperature: f32,
    reward: Arc<Reward>,
}

impl<M: Module> BestOfN<M> {
    /// Samples `module` up to `n` times, keeping the draw `reward` scores
    /// highest.
    ///
    /// # Errors
    ///
    /// Returns [`PredictError::Config`] when `n` is zero.
    pub fn new<F>(module: M, n: usize, reward: F) -> Result<Self, PredictError>
    where
        F: Fn(&Map<String, Value>, &Prediction) -> f64 + Send + Sync + 'static,
    {
        if n == 0 {
            return Err(PredictError::Config(
                "BestOfN needs at least one draw".to_owned(),
            ));
        }
        Ok(Self {
            module,
            n,
            threshold: None,
            temperature: DEFAULT_TEMPERATURE,
            reward: Arc::new(reward),
        })
    }

    /// Stops sampling at the first draw scoring at least `threshold`.
    #[must_use]
    pub fn threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Sets the sampling temperature.
    #[must_use]
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// The wrapped module.
    pub fn inner(&self) -> &M {
        &self.module
    }

    async fn run(
        &self,
        inputs: &Map<String, Value>,
        options: &CallOptions,
    ) -> Result<Prediction, PredictError> {
        let mut best: Option<(f64, Prediction)> = None;
        let mut last_error = None;
        let mut total = Usage::default();
        let mut spent = false;
        let mut attempts = 0;

        for draw in 0..self.n {
            attempts += 1;
            let salt = match &options.cache_salt {
                Some(outer) => format!("{outer}/best_of_n/{draw}"),
                None => format!("best_of_n/{draw}"),
            };
            let draw_options = CallOptions {
                temperature: Some(self.temperature),
                cache_salt: Some(salt),
                ..CallOptions::default()
            }
            .over(options);

            let prediction = match self.module.forward(inputs, &draw_options).await {
                Ok(prediction) => prediction,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };
            if let Some(usage) = &prediction.usage {
                total = if spent {
                    total.plus(usage)
                } else {
                    usage.clone()
                };
                spent = true;
            }
            let raw = (self.reward)(inputs, &prediction);
            let score = if raw.is_nan() { f64::NEG_INFINITY } else { raw };
            let done = self.threshold.is_some_and(|t| score >= t);
            if best.as_ref().map_or(true, |(top, _)| score > *top) {
                best = Some((score, prediction));
            }
            if done {
                break;
            }
        }

        let Some((score, mut prediction)) = best else {
            let last =
                last_error.unwrap_or_else(|| PredictError::Config("no draws were made".to_owned()));
            return Err(PredictError::AllAttemptsFailed {
                attempts,
                last: Box::new(last),
            });
        };
        prediction.usage = spent.then_some(total);
        // A NaN-only run reports null rather than -inf, which JSON cannot hold.
        let score = if score.is_finite() {
            json!(score)
        } else {
            Value::Null
        };
        prediction.meta.insert("best_of_n_score".to_owned(), score);
        prediction
            .meta
            .insert("best_of_n_attempts".to_owned(), json!(attempts));
        Ok(prediction)
    }
}

impl<M: Module> Module for BestOfN<M> {
    fn forward<'a>(
        &'a self,
        inputs: &'a Map<String, Value>,
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Prediction, PredictError>> {
        Box::pin(self.run(inputs, options))
    }
}
