//! Predictors driven by a scripted `LanguageModel`, which doubles as proof
//! that the model is swappable without the client crate.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cosmos_llm_adapter::Message;
use cosmos_llm_cache::MemoryStore;
use cosmos_llm_example::Example;
use cosmos_llm_predict::{
    strategy, BestOfN, CallOptions, Completion, LanguageModel, Module, Parts, Predict,
    PredictError, Usage,
};
use cosmos_llm_signature::Signature;
use futures_util::future::BoxFuture;
use serde_json::{json, Map, Value};

/// Replies from a script, and records every request.
#[derive(Default)]
struct Scripted {
    replies: Mutex<VecDeque<Result<String, String>>>,
    seen: Mutex<Vec<(Vec<Message>, CallOptions)>>,
}

impl Scripted {
    fn new<I: IntoIterator<Item = &'static str>>(replies: I) -> Arc<Self> {
        Arc::new(Self {
            replies: Mutex::new(replies.into_iter().map(|r| Ok(r.to_owned())).collect()),
            seen: Mutex::default(),
        })
    }

    fn then_fail(self: &Arc<Self>, message: &str) -> Arc<Self> {
        self.replies
            .lock()
            .unwrap()
            .push_back(Err(message.to_owned()));
        Arc::clone(self)
    }

    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }

    fn request(&self, index: usize) -> (Vec<Message>, CallOptions) {
        self.seen.lock().unwrap()[index].clone()
    }
}

impl LanguageModel for Scripted {
    fn complete<'a>(
        &'a self,
        messages: &'a [Message],
        options: &'a CallOptions,
    ) -> BoxFuture<'a, Result<Completion, PredictError>> {
        self.seen
            .lock()
            .unwrap()
            .push((messages.to_vec(), options.clone()));
        let reply = self.replies.lock().unwrap().pop_front();
        Box::pin(async move {
            match reply {
                Some(Ok(text)) => Ok(Completion {
                    text,
                    usage: Some(Usage {
                        prompt_tokens: 10,
                        completion_tokens: 2,
                        cost: Some(0.01),
                    }),
                }),
                Some(Err(message)) => Err(PredictError::Model(message.into())),
                None => Err(PredictError::Model("script exhausted".into())),
            }
        })
    }

    fn identity(&self) -> String {
        "scripted".to_owned()
    }
}

fn answer(text: &str) -> String {
    format!("[[ ## answer ## ]]\n{text}\n\n[[ ## done ## ]]")
}

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn qa() -> Signature {
    Signature::parse("question -> answer: int").unwrap()
}

fn inputs(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

#[tokio::test]
async fn renders_asks_and_parses() {
    let model = Scripted::new([leak(answer("4"))]);
    let prediction = Predict::new(qa(), model.clone())
        .call(&json!({ "question": "2+2?" }))
        .await
        .unwrap();

    assert_eq!(prediction.values, inputs(json!({ "answer": 4 })));
    assert_eq!((prediction.attempts, prediction.cached), (1, false));
    let (messages, _) = model.request(0);
    assert_eq!(messages[0].role, "system");
    assert!(messages.last().unwrap().content.contains("2+2?"));
}

#[tokio::test]
async fn an_unparseable_reply_gets_one_correction_turn() {
    let model = Scripted::new(["The answer is four.", leak(answer("4"))]);
    let prediction = Predict::new(qa(), model.clone())
        .call(&json!({ "question": "2+2?" }))
        .await
        .unwrap();

    assert_eq!(prediction.attempts, 2);
    let (retry, _) = model.request(1);
    let n = retry.len();
    assert_eq!(retry[n - 2], Message::assistant("The answer is four."));
    assert!(retry[n - 1]
        .content
        .starts_with("That response could not be parsed"));
    // Both attempts reached the model, so both are paid for.
    assert_eq!(prediction.usage.unwrap().prompt_tokens, 20);
}

#[tokio::test]
async fn parse_failures_surface_after_the_retry_budget() {
    let model = Scripted::new(["nope", "still nope"]);
    let err = Predict::new(qa(), model.clone())
        .call(&json!({ "question": "2+2?" }))
        .await
        .unwrap_err();
    match err {
        PredictError::Parse {
            attempts,
            completion,
            ..
        } => {
            assert_eq!((attempts, completion.as_str()), (2, "still nope"));
        }
        other => panic!("expected a parse error, got {other}"),
    }

    let model = Scripted::new(["nope"]);
    let err = Predict::new(qa(), model.clone())
        .max_parse_retries(0)
        .call(&json!({ "question": "2+2?" }))
        .await
        .unwrap_err();
    assert!(matches!(err, PredictError::Parse { attempts: 1, .. }));
    assert_eq!(model.calls(), 1);
}

#[tokio::test]
async fn inputs_are_checked_against_the_signature() {
    let model = Scripted::new([]);
    let err = Predict::new(qa(), model.clone())
        .call(&json!({ "q": "?" }))
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("missing required input field 'question'"),
        "{err}"
    );
    assert_eq!(model.calls(), 0);
}

#[tokio::test]
async fn a_cached_reply_is_served_without_calling_the_model() {
    let model = Scripted::new([leak(answer("4"))]);
    let predict = Predict::new(qa(), model.clone()).cache(Arc::new(MemoryStore::new(10)));

    let first = predict.call(&json!({ "question": "2+2?" })).await.unwrap();
    let second = predict.call(&json!({ "question": "2+2?" })).await.unwrap();
    assert_eq!(model.calls(), 1);
    assert!(!first.cached && second.cached);
    assert_eq!(first.values, second.values);
    assert!(second.usage.is_none(), "a cache hit costs nothing");
}

#[tokio::test]
async fn chain_of_thought_asks_for_reasoning_first() {
    let reply =
        "[[ ## reasoning ## ]]\n2 and 2 make 4.\n\n[[ ## answer ## ]]\n4\n\n[[ ## done ## ]]";
    let model = Scripted::new([reply]);
    let cot = Predict::chain_of_thought(qa(), model.clone()).unwrap();
    let prediction = cot.call(&json!({ "question": "2+2?" })).await.unwrap();

    assert_eq!(
        prediction.values.keys().collect::<Vec<_>>(),
        ["reasoning", "answer"]
    );
    let system = &model.request(0).0[0].content;
    assert!(system.find("reasoning").unwrap() < system.find("answer (an integer)").unwrap());

    let taken = Signature::parse("q -> reasoning, answer").unwrap();
    assert!(Predict::chain_of_thought(taken, model).is_err());
}

#[tokio::test]
async fn demos_are_rendered_as_prior_turns() {
    let model = Scripted::new([leak(answer("6"))]);
    let demo = Example::from_value(json!({ "question": "1+1?", "answer": 2 })).unwrap();
    Predict::new(qa(), model.clone())
        .demos(vec![demo])
        .call(&json!({ "question": "3+3?" }))
        .await
        .unwrap();

    let roles: Vec<String> = model.request(0).0.into_iter().map(|m| m.role).collect();
    assert_eq!(roles, ["system", "user", "assistant", "user"]);
}

#[tokio::test]
async fn adapter_params_reach_the_model_but_explicit_options_win() {
    let model = Scripted::new([r#"{"answer": 4}"#, r#"{"answer": 4}"#]);
    let adapter: Arc<dyn cosmos_llm_adapter::Adapter> =
        Arc::from(cosmos_llm_adapter::adapter("json_schema").unwrap());
    let predict = Predict::new(qa(), model.clone()).adapter(adapter);

    predict.call(&json!({ "question": "2+2?" })).await.unwrap();
    assert_eq!(
        model.request(0).1.extra["response_format"]["type"],
        "json_schema"
    );

    let mut options = CallOptions::default();
    options
        .extra
        .insert("response_format".into(), json!({ "type": "text" }));
    predict
        .forward(&inputs(json!({ "question": "2+2?" })), &options)
        .await
        .unwrap();
    assert_eq!(
        model.request(1).1.extra["response_format"],
        json!({ "type": "text" })
    );
}

#[tokio::test]
async fn per_call_options_layer_over_predictor_defaults() {
    let model = Scripted::new([leak(answer("4"))]);
    let defaults = CallOptions {
        model: Some("small".into()),
        temperature: Some(0.0),
        ..CallOptions::default()
    };
    let call = CallOptions {
        temperature: Some(0.7),
        ..CallOptions::default()
    };
    Predict::new(qa(), model.clone())
        .options(defaults)
        .forward(&inputs(json!({ "question": "2+2?" })), &call)
        .await
        .unwrap();
    let (_, sent) = model.request(0);
    assert_eq!(
        (sent.model.as_deref(), sent.temperature),
        (Some("small"), Some(0.7))
    );
}

#[tokio::test]
async fn best_of_n_keeps_the_top_draw_and_stops_at_the_threshold() {
    let model = Scripted::new([leak(answer("3")), leak(answer("9")), leak(answer("5"))]);
    let predict = Predict::new(qa(), model.clone()).cache(Arc::new(MemoryStore::new(10)));
    let best = BestOfN::new(predict, 3, |_, p| p.values["answer"].as_f64().unwrap())
        .unwrap()
        .threshold(9.0)
        .temperature(0.9);

    let prediction = best
        .forward(&inputs(json!({ "question": "?" })), &CallOptions::default())
        .await
        .unwrap();
    assert_eq!(prediction.values["answer"], 9);
    assert_eq!(prediction.meta["best_of_n_attempts"], 2);
    assert_eq!(model.calls(), 2, "the threshold ended sampling early");
    // Each draw is salted, so the cache did not replay the first one.
    let (_, first) = model.request(0);
    let (_, second) = model.request(1);
    assert_ne!(first.cache_salt, second.cache_salt);
    assert_eq!(first.temperature, Some(0.9));
    assert_eq!(
        prediction.usage.unwrap().completion_tokens,
        4,
        "usage covers every draw"
    );
}

#[tokio::test]
async fn best_of_n_skips_failed_draws_and_reports_when_all_fail() {
    let model = Scripted::new([]).then_fail("overloaded");
    let model = model.then_fail("overloaded again");
    let predict = Predict::new(qa(), model.clone()).max_parse_retries(0);
    let best = BestOfN::new(predict, 2, |_, _| 1.0).unwrap();
    let err = best
        .forward(&inputs(json!({ "question": "?" })), &CallOptions::default())
        .await
        .unwrap_err();
    assert!(
        matches!(err, PredictError::AllAttemptsFailed { attempts: 2, .. }),
        "{err}"
    );
    assert!(err.to_string().contains("overloaded again"));

    assert!(BestOfN::new(Predict::new(qa(), model), 0, |_, _| 1.0).is_err());
}

#[tokio::test]
async fn state_round_trips_instructions_and_demos() {
    let model = Scripted::new([]);
    let demo = Example::from_value(json!({ "question": "1+1?", "answer": 2 })).unwrap();
    let tuned = Predict::new(qa(), model.clone())
        .with_instructions("Add.")
        .demos(vec![demo]);
    let state = tuned.dump_state();

    let mut fresh = Predict::new(qa(), model);
    fresh.load_state(&state).unwrap();
    assert_eq!(fresh.signature().instructions(), "Add.");
    assert_eq!(fresh.demo_examples().len(), 1);
    assert!(fresh.load_state(&json!({ "demos": "nope" })).is_err());
}

#[tokio::test]
async fn strategies_resolve_by_name_and_alias() {
    let reply = "[[ ## reasoning ## ]]\nr\n\n[[ ## answer ## ]]\n1\n\n[[ ## done ## ]]";
    let model = Scripted::new([reply]);
    let module = strategy("COT", Parts::new(qa(), model)).unwrap();
    let prediction = module
        .forward(&inputs(json!({ "question": "?" })), &CallOptions::default())
        .await
        .unwrap();
    assert!(prediction.values.contains_key("reasoning"));

    let err = strategy("tree_of_thought", Parts::new(qa(), Scripted::new([])))
        .err()
        .unwrap();
    assert!(
        err.to_string()
            .starts_with("unknown strategy 'tree_of_thought'; available:"),
        "{err}"
    );
}

#[tokio::test]
async fn model_errors_are_reported_as_such() {
    let model = Scripted::new([]).then_fail("401 unauthorized");
    let err = Predict::new(qa(), model)
        .call(&json!({ "question": "?" }))
        .await
        .unwrap_err();
    assert_eq!(err.to_string(), "language model error: 401 unauthorized");
}

#[cfg(feature = "client")]
#[tokio::test]
async fn the_client_is_a_language_model() {
    use cosmos_llm::{providers::mock::MockProvider, Client};

    let client = Client::from_provider(Box::new(MockProvider::new().respond(answer("4"))))
        .with_model("mock");
    assert_eq!(client.identity(), "mock");
    let prediction = Predict::new(qa(), Arc::new(client))
        .call(&json!({ "question": "2+2?" }))
        .await
        .unwrap();
    assert_eq!(prediction.values["answer"], 4);
}
