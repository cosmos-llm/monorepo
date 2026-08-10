use serde_json::{Map, Value};

use crate::definition::{Handler, ToolDefinition};
use crate::error::ToolError;

/// Executes a [`ToolDefinition`] against a set of parameters.
///
/// Validates all parameters, applies defaults, then invokes the handler.
///
/// Two entry points: [`Executor::execute`] for synchronous tools, and
/// [`Executor::execute_async`] for either kind. The async path also enforces
/// [`ToolDefinition::timeout`] and is what [`Executor::execute_all`] builds on.
///
/// # Examples
///
/// ```rust
/// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
/// use serde_json::json;
///
/// let tool = ToolDefinition::new("greet")
///     .param("name", ParameterType::String, true, "Name to greet")
///     .handler(|p| {
///         Ok(json!(format!("Hello, {}!", p["name"].as_str().unwrap())))
///     });
///
/// let result = Executor::execute(&tool, &json!({ "name": "Alice" })).unwrap();
/// assert_eq!(result, json!("Hello, Alice!"));
/// ```
pub struct Executor;

impl Executor {
    /// Validates parameters and applies defaults, returning the resolved object.
    ///
    /// Shared by both execution paths so a tool sees the same parameters however
    /// it is run — an async tool validated differently from a sync one would be
    /// a subtle and miserable bug.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::Validation`] when `params` is not an object, or when
    /// any parameter fails its own validation.
    fn resolve_params(tool: &ToolDefinition, params: &Value) -> Result<Value, ToolError> {
        let obj = match params {
            Value::Object(m) => m.clone(),
            Value::Null => Map::new(),
            _ => {
                return Err(ToolError::Validation {
                    param: "(root)".into(),
                    message: "params must be a JSON object".into(),
                })
            }
        };

        let mut resolved = Map::new();
        for param in &tool.parameters {
            let value = obj.get(&param.name).cloned().unwrap_or(Value::Null);

            // Apply default before validation
            let effective = if value.is_null() {
                param.default.clone().unwrap_or(Value::Null)
            } else {
                value
            };

            param.validate(&effective)?;

            if !effective.is_null() {
                resolved.insert(param.name.clone(), effective);
            }
        }

        Ok(Value::Object(resolved))
    }

    /// Validates parameters, applies defaults, and calls the tool handler.
    ///
    /// `params` must be a JSON object; other shapes return a validation error.
    ///
    /// Synchronous handlers only. An async tool returns
    /// [`ToolError::AsyncHandler`] rather than being driven to completion here,
    /// because blocking on a future inside a library deadlocks on a
    /// current-thread runtime.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NoHandler`] if the tool has no handler.
    /// Returns [`ToolError::AsyncHandler`] if the handler is asynchronous.
    /// Returns [`ToolError::Validation`] or [`ToolError::MissingParam`] on bad input.
    /// Returns [`ToolError::Execution`] if the handler returns an error string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{ToolDefinition, ParameterType, Executor};
    /// use serde_json::json;
    ///
    /// let tool = ToolDefinition::new("noop").handler(|_| Ok(json!(null)));
    /// assert!(Executor::execute(&tool, &json!({})).is_ok());
    /// ```
    pub fn execute(tool: &ToolDefinition, params: &Value) -> Result<Value, ToolError> {
        let handler = tool
            .handler_ref()
            .ok_or_else(|| ToolError::NoHandler(tool.name.clone()))?;

        let sync = match handler {
            Handler::Sync(f) => f,
            #[cfg(feature = "async")]
            Handler::Async(_) => return Err(ToolError::AsyncHandler(tool.name.clone())),
        };

        let resolved = Self::resolve_params(tool, params)?;

        sync(&resolved).map_err(|msg| ToolError::Execution {
            tool: tool.name.clone(),
            message: msg,
        })
    }

    /// Runs a tool, awaiting an async handler and enforcing its timeout.
    ///
    /// Handles both handler kinds: an async handler is awaited, a sync one is
    /// called inline. A sync handler is *not* moved to a blocking thread pool —
    /// this crate does not know whether the caller has one, and a sync handler
    /// is supposed to be quick. A slow sync handler should be an async one.
    ///
    /// When [`ToolDefinition::timeout`] is set, exceeding it drops the future.
    /// What that guarantees depends on the handler: `reqwest`-based work is
    /// cancelled at the next await point, while a handler that blocks between
    /// await points runs to completion in the background even though its result
    /// is discarded. Handlers that need to be interruptible must await
    /// something.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::NoHandler`] if the tool has no handler,
    /// [`ToolError::Validation`] or [`ToolError::MissingParam`] on bad input,
    /// [`ToolError::Timeout`] if the tool exceeds its timeout, and
    /// [`ToolError::Execution`] if the handler returns an error string.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Executor, ToolDefinition};
    /// use serde_json::json;
    ///
    /// # tokio_test::block_on(async {
    /// // Sync and async tools go through the same call.
    /// let sync = ToolDefinition::new("s").handler(|_| Ok(json!("sync")));
    /// let async_tool = ToolDefinition::new("a")
    ///     .async_handler(|_| Box::pin(async { Ok(json!("async")) }));
    ///
    /// assert_eq!(Executor::execute_async(&sync, json!({})).await.unwrap(), json!("sync"));
    /// assert_eq!(Executor::execute_async(&async_tool, json!({})).await.unwrap(), json!("async"));
    /// # })
    /// ```
    #[cfg(feature = "async")]
    pub async fn execute_async(tool: &ToolDefinition, params: Value) -> Result<Value, ToolError> {
        let handler = tool
            .handler_ref()
            .ok_or_else(|| ToolError::NoHandler(tool.name.clone()))?
            .clone();

        let resolved = Self::resolve_params(tool, &params)?;

        let run = async move {
            match handler {
                Handler::Sync(f) => f(&resolved),
                Handler::Async(f) => f(resolved).await,
            }
        };

        let outcome = match tool.timeout {
            Some(limit) => {
                tokio::time::timeout(limit, run)
                    .await
                    .map_err(|_| ToolError::Timeout {
                        tool: tool.name.clone(),
                        elapsed: limit,
                    })?
            }
            None => run.await,
        };

        outcome.map_err(|msg| ToolError::Execution {
            tool: tool.name.clone(),
            message: msg,
        })
    }

    /// Runs several tools concurrently, bounded by `max_concurrent`.
    ///
    /// Results come back in the order the calls were given, regardless of
    /// completion order, so a caller can pair each result with the tool call it
    /// answers. Each entry is that call's own `Result`: one tool failing does
    /// not cancel the others, because a model that requested three tools wants
    /// the two that worked.
    ///
    /// The bound matters. A model can request a dozen calls at once, and firing
    /// a dozen simultaneous HTTP requests at one host is how a research tool
    /// gets itself rate-limited. `max_concurrent` of 0 is treated as 1.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use cosmos_llm_tool::{Executor, ParameterType, ToolDefinition};
    /// use serde_json::json;
    ///
    /// # tokio_test::block_on(async {
    /// let double = ToolDefinition::new("double")
    ///     .param("n", ParameterType::Number, true, "")
    ///     .async_handler(|p| Box::pin(async move {
    ///         Ok(json!(p["n"].as_f64().unwrap_or_default() * 2.0))
    ///     }));
    ///
    /// let calls = vec![
    ///     (&double, json!({"n": 1.0})),
    ///     (&double, json!({"n": 2.0})),
    ///     (&double, json!({"n": 3.0})),
    /// ];
    ///
    /// let results = Executor::execute_all(calls, 2).await;
    /// assert_eq!(results[0].as_ref().unwrap(), &json!(2.0));
    /// assert_eq!(results[2].as_ref().unwrap(), &json!(6.0));
    /// # })
    /// ```
    #[cfg(feature = "async")]
    pub async fn execute_all(
        calls: Vec<(&ToolDefinition, Value)>,
        max_concurrent: usize,
    ) -> Vec<Result<Value, ToolError>> {
        use futures_util::stream::{self, StreamExt};

        // A bound of zero would run nothing at all.
        let limit = max_concurrent.max(1);

        stream::iter(calls)
            .map(|(tool, params)| Self::execute_async(tool, params))
            // `buffered`, not `buffer_unordered`: results must line up with the
            // calls that produced them.
            .buffered(limit)
            .collect()
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameter::{ParameterDef, ParameterType};
    use serde_json::json;

    fn make_tool() -> ToolDefinition {
        ToolDefinition::new("calc")
            .param("a", ParameterType::Number, true, "")
            .param("b", ParameterType::Number, true, "")
            .handler(|p| {
                let a = p["a"].as_f64().unwrap();
                let b = p["b"].as_f64().unwrap();
                Ok(json!(a + b))
            })
    }

    #[test]
    fn happy_path() {
        let t = make_tool();
        let r = Executor::execute(&t, &json!({ "a": 1.0, "b": 2.0 })).unwrap();
        assert_eq!(r, json!(3.0));
    }

    #[test]
    fn missing_required() {
        let t = make_tool();
        assert!(Executor::execute(&t, &json!({ "a": 1.0 })).is_err());
    }

    #[test]
    fn default_applied() {
        let tool = ToolDefinition::new("t")
            .push_param(
                ParameterDef::new("limit", ParameterType::Integer, false, "")
                    .with_default(json!(10)),
            )
            .handler(|p| Ok(p["limit"].clone()));

        let r = Executor::execute(&tool, &json!({})).unwrap();
        assert_eq!(r, json!(10));
    }

    #[test]
    fn no_handler_error() {
        let t = ToolDefinition::new("t");
        assert!(matches!(
            Executor::execute(&t, &json!({})),
            Err(ToolError::NoHandler(_))
        ));
    }

    #[test]
    fn non_object_params_rejected() {
        let t = make_tool();
        assert!(matches!(
            Executor::execute(&t, &json!("not an object")),
            Err(ToolError::Validation { .. })
        ));
    }

    #[cfg(feature = "async")]
    mod async_tests {
        use super::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        fn async_double() -> ToolDefinition {
            ToolDefinition::new("double")
                .param("n", ParameterType::Number, true, "")
                .async_handler(|p| {
                    Box::pin(async move {
                        let n = p["n"].as_f64().ok_or("n must be a number")?;
                        Ok(json!(n * 2.0))
                    })
                })
        }

        #[tokio::test]
        async fn async_handler_runs() {
            let t = async_double();
            let r = Executor::execute_async(&t, json!({ "n": 21.0 }))
                .await
                .unwrap();
            assert_eq!(r, json!(42.0));
        }

        #[tokio::test]
        async fn sync_handler_runs_through_the_async_path() {
            // Both kinds must work in one call, or an agent loop needs to know
            // which sort of tool it is dispatching.
            let t = make_tool();
            let r = Executor::execute_async(&t, json!({ "a": 2.0, "b": 3.0 }))
                .await
                .unwrap();
            assert_eq!(r, json!(5.0));
        }

        #[test]
        fn async_tool_through_the_sync_path_is_an_error_not_a_deadlock() {
            let t = async_double();
            let err = Executor::execute(&t, &json!({ "n": 1.0 })).unwrap_err();
            assert!(matches!(err, ToolError::AsyncHandler(ref n) if n == "double"));
            // The message has to point at the fix.
            assert!(err.to_string().contains("execute_async"), "{err}");
        }

        #[tokio::test]
        async fn async_validation_matches_sync_validation() {
            let t = async_double();
            assert!(matches!(
                Executor::execute_async(&t, json!({})).await,
                Err(ToolError::Validation { .. }) | Err(ToolError::MissingParam(_))
            ));
        }

        #[tokio::test]
        async fn async_handler_errors_become_execution_errors() {
            let t = ToolDefinition::new("boom")
                .async_handler(|_| Box::pin(async { Err("it broke".to_owned()) }));
            let err = Executor::execute_async(&t, json!({})).await.unwrap_err();
            assert!(
                matches!(err, ToolError::Execution { ref message, .. } if message == "it broke")
            );
        }

        #[tokio::test(start_paused = true)]
        async fn timeout_fires_on_a_hanging_tool() {
            let t = ToolDefinition::new("hang")
                .timeout(Duration::from_millis(50))
                .async_handler(|_| {
                    Box::pin(async {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        Ok(json!("never"))
                    })
                });

            let err = Executor::execute_async(&t, json!({})).await.unwrap_err();
            assert!(matches!(err, ToolError::Timeout { ref tool, .. } if tool == "hang"));
        }

        #[tokio::test(start_paused = true)]
        async fn a_tool_finishing_inside_its_timeout_succeeds() {
            let t = ToolDefinition::new("quick")
                .timeout(Duration::from_secs(5))
                .async_handler(|_| {
                    Box::pin(async {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        Ok(json!("done"))
                    })
                });
            assert_eq!(
                Executor::execute_async(&t, json!({})).await.unwrap(),
                json!("done")
            );
        }

        #[tokio::test(start_paused = true)]
        async fn one_tool_timing_out_does_not_block_the_others() {
            let hang = ToolDefinition::new("hang")
                .timeout(Duration::from_millis(50))
                .async_handler(|_| {
                    Box::pin(async {
                        tokio::time::sleep(Duration::from_secs(60)).await;
                        Ok(json!("never"))
                    })
                });
            let quick = ToolDefinition::new("quick")
                .async_handler(|_| Box::pin(async { Ok(json!("fine")) }));

            let results =
                Executor::execute_all(vec![(&hang, json!({})), (&quick, json!({}))], 4).await;

            assert!(matches!(results[0], Err(ToolError::Timeout { .. })));
            assert_eq!(results[1].as_ref().unwrap(), &json!("fine"));
        }

        #[tokio::test]
        async fn execute_all_preserves_call_order() {
            // Deliberately inverted durations: the last call finishes first, so
            // an unordered implementation would fail this.
            let slow = ToolDefinition::new("slow").async_handler(|p| {
                Box::pin(async move {
                    let n = p["n"].as_u64().unwrap_or(0);
                    tokio::time::sleep(Duration::from_millis(30 - n * 10)).await;
                    Ok(json!(n))
                })
            });
            let slow = slow.push_param(ParameterDef::new("n", ParameterType::Integer, true, ""));

            let calls = vec![
                (&slow, json!({"n": 0})),
                (&slow, json!({"n": 1})),
                (&slow, json!({"n": 2})),
            ];
            let results = Executor::execute_all(calls, 4).await;

            assert_eq!(results[0].as_ref().unwrap(), &json!(0));
            assert_eq!(results[1].as_ref().unwrap(), &json!(1));
            assert_eq!(results[2].as_ref().unwrap(), &json!(2));
        }

        #[tokio::test]
        async fn execute_all_respects_the_concurrency_bound() {
            let peak = Arc::new(AtomicUsize::new(0));
            let live = Arc::new(AtomicUsize::new(0));

            let tool = {
                let peak = peak.clone();
                let live = live.clone();
                ToolDefinition::new("counted").async_handler(move |_| {
                    let peak = peak.clone();
                    let live = live.clone();
                    Box::pin(async move {
                        let now = live.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        // Real await point, so the concurrent tasks actually
                        // overlap rather than each finishing before the next
                        // is polled.
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        live.fetch_sub(1, Ordering::SeqCst);
                        Ok(json!(null))
                    })
                })
            };

            let calls: Vec<_> = (0..8).map(|_| (&tool, json!({}))).collect();
            let results = Executor::execute_all(calls, 3).await;

            assert_eq!(results.len(), 8);
            assert!(results.iter().all(Result::is_ok));
            assert!(peak.load(Ordering::SeqCst) <= 3, "peak {:?}", peak);
        }

        #[tokio::test]
        async fn execute_all_treats_zero_as_one() {
            let tool = ToolDefinition::new("t").async_handler(|_| Box::pin(async { Ok(json!(1)) }));
            let results = Executor::execute_all(vec![(&tool, json!({}))], 0).await;
            assert_eq!(results.len(), 1);
            assert!(results[0].is_ok());
        }

        #[tokio::test]
        async fn execute_all_mixes_sync_and_async_tools() {
            let sync = make_tool();
            let async_tool = async_double();

            let results = Executor::execute_all(
                vec![
                    (&sync, json!({"a": 1.0, "b": 1.0})),
                    (&async_tool, json!({"n": 5.0})),
                ],
                2,
            )
            .await;

            assert_eq!(results[0].as_ref().unwrap(), &json!(2.0));
            assert_eq!(results[1].as_ref().unwrap(), &json!(10.0));
        }

        #[tokio::test]
        async fn execute_all_reports_failures_per_call() {
            let ok = ToolDefinition::new("ok").async_handler(|_| Box::pin(async { Ok(json!(1)) }));
            let bad = ToolDefinition::new("bad");

            let results = Executor::execute_all(vec![(&bad, json!({})), (&ok, json!({}))], 2).await;

            assert!(matches!(results[0], Err(ToolError::NoHandler(_))));
            assert!(results[1].is_ok());
        }

        #[tokio::test]
        async fn empty_batch_returns_no_results() {
            assert!(Executor::execute_all(vec![], 4).await.is_empty());
        }
    }
}
