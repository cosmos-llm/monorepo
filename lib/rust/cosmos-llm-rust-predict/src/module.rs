use futures_util::future::BoxFuture;
use serde_json::{Map, Value};

use crate::error::PredictError;
use crate::model::CallOptions;
use crate::prediction::Prediction;

/// Anything that turns inputs into a [`Prediction`].
///
/// [`Predict`](crate::Predict) calls a model once; [`BestOfN`](crate::BestOfN)
/// wraps another module; a pipeline that retrieves context and then calls a
/// predictor is a module too. What they share is the calling convention —
/// input values and per-call options in, a prediction out — which is what
/// lets them nest.
///
/// `options` are layered over whatever defaults a module holds (see
/// [`CallOptions::over`]) and passed down to nested modules, which is how a
/// sampler raises the temperature of a predictor it does not own.
pub trait Module: Send + Sync {
    /// Runs the module.
    ///
    /// # Errors
    ///
    /// Returns whatever error stopped the run.
    fn forward<'a>(
        &'a self,
        inputs: &'a Map<String, Value>,
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Prediction, PredictError>>;
}

impl<M: Module + ?Sized> Module for Box<M> {
    fn forward<'a>(
        &'a self,
        inputs: &'a Map<String, Value>,
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Prediction, PredictError>> {
        (**self).forward(inputs, options)
    }
}

impl<M: Module + ?Sized> Module for std::sync::Arc<M> {
    fn forward<'a>(
        &'a self,
        inputs: &'a Map<String, Value>,
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Prediction, PredictError>> {
        (**self).forward(inputs, options)
    }
}
