use std::future::Future;

use futures_util::future::BoxFuture;
use futures_util::stream::{self, StreamExt};

use cosmos_llm_example::Example;

use crate::error::EvaluateError;
use crate::metric::{Metric, Values};
use crate::report::{Report, Row};

/// The error a [`Program`] may fail with.
pub type ProgramError = Box<dyn std::error::Error + Send + Sync>;

/// Anything that turns an example's inputs into output values.
///
/// This is the whole contract between an evaluator and what it evaluates:
/// a predictor, a pipeline of them, a hand-written baseline, or a function
/// that looks answers up in a file. Keeping it this small is what lets this
/// crate score programs without depending on the crates that build them.
///
/// Any `Fn(Values) -> impl Future<Output = Result<Values, ProgramError>>`
/// whose future is `'static` is a program. A closure whose future borrows
/// something — a predictor held on the stack, say — can be passed to
/// [`Evaluator::run_with`] instead, which takes it by value.
pub trait Program: Send + Sync {
    /// Runs the program on one example's inputs.
    fn run<'a>(&'a self, inputs: Values) -> BoxFuture<'a, Result<Values, ProgramError>>;
}

impl<F, Fut> Program for F
where
    F: Fn(Values) -> Fut + Send + Sync,
    Fut: Future<Output = Result<Values, ProgramError>> + Send + 'static,
{
    fn run<'a>(&'a self, inputs: Values) -> BoxFuture<'a, Result<Values, ProgramError>> {
        Box::pin(self(inputs))
    }
}

/// The number of examples an evaluator runs at once unless told otherwise.
pub const DEFAULT_CONCURRENCY: usize = 4;

type Progress = Box<dyn Fn(usize, usize) + Send + Sync>;

/// Runs a program over a devset and scores each result.
///
/// Examples are independent, so they run concurrently, up to
/// [`Evaluator::concurrency`] at a time. The work is almost entirely waiting
/// on a model, which is exactly where concurrency pays.
///
/// A program or metric that fails on one example does not abort the run: the
/// failure is recorded as a zero-scoring row and evaluation continues.
/// Losing a whole devset sweep to one bad row is worse than scoring it zero.
/// When [`Evaluator::max_errors`] is set and exceeded, the run stops early
/// with [`EvaluateError::TooManyErrors`] — early, unlike the Ruby gem, which
/// finishes the sweep first, because every further call is money spent on a
/// result that will be thrown away.
///
/// The evaluator does not pick an async runtime. It polls its examples
/// concurrently on whatever executor awaits it.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::{metrics, Evaluator, ProgramError, Values};
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// # tokio_test::block_on(async {
/// let devset: Vec<Example> = [("2+2", "4"), ("3+3", "6")]
///     .iter()
///     .map(|(q, a)| Example::from_value(json!({ "q": q, "answer": a })).unwrap().with_inputs(["q"]))
///     .collect();
///
/// // A stand-in program: always answers "4".
/// let program = |_inputs: Values| async {
///     Ok::<Values, ProgramError>(json!({ "answer": "4" }).as_object().unwrap().clone())
/// };
///
/// let report = Evaluator::new(metrics::exact_match("answer", true))
///     .concurrency(8)
///     .run(&program, &devset)
///     .await
///     .unwrap();
/// assert_eq!(report.summary(), "50.0% (1/2 passed)");
/// # })
/// ```
pub struct Evaluator {
    metric: Box<dyn Metric>,
    concurrency: usize,
    max_errors: Option<usize>,
    progress: Option<Progress>,
}

impl Evaluator {
    /// An evaluator scoring with `metric`, running [`DEFAULT_CONCURRENCY`]
    /// examples at once and tolerating any number of errors.
    pub fn new(metric: impl Metric + 'static) -> Self {
        Self {
            metric: Box::new(metric),
            concurrency: DEFAULT_CONCURRENCY,
            max_errors: None,
            progress: None,
        }
    }

    /// Sets how many examples run at once. Must be positive.
    #[must_use]
    pub fn concurrency(mut self, concurrency: usize) -> Self {
        self.concurrency = concurrency;
        self
    }

    /// Stops the run once more than `max` examples have failed. `None`, the
    /// default, tolerates any number.
    #[must_use]
    pub fn max_errors(mut self, max: Option<usize>) -> Self {
        self.max_errors = max;
        self
    }

    /// Calls `progress(done, total)` after each example finishes.
    #[must_use]
    pub fn progress(mut self, progress: impl Fn(usize, usize) + Send + Sync + 'static) -> Self {
        self.progress = Some(Box::new(progress));
        self
    }

    /// Evaluates `program` over `devset`.
    ///
    /// Each example's [`inputs`](Example::inputs) go to the program; the
    /// whole example, labels included, goes to the metric.
    ///
    /// # Errors
    ///
    /// Returns [`EvaluateError::Config`] for zero concurrency, and
    /// [`EvaluateError::TooManyErrors`] when [`Evaluator::max_errors`] is
    /// exceeded. Failures on individual examples are recorded on their rows.
    pub async fn run(
        &self,
        program: &dyn Program,
        devset: &[Example],
    ) -> Result<Report, EvaluateError> {
        self.run_with(|inputs| program.run(inputs), devset).await
    }

    /// Evaluates a program given as a function, which may borrow.
    ///
    /// # Errors
    ///
    /// As for [`Evaluator::run`].
    pub async fn run_with<F, Fut>(
        &self,
        program: F,
        devset: &[Example],
    ) -> Result<Report, EvaluateError>
    where
        F: Fn(Values) -> Fut,
        Fut: Future<Output = Result<Values, ProgramError>>,
    {
        if self.concurrency == 0 {
            return Err(EvaluateError::Config(
                "concurrency must be at least 1".to_owned(),
            ));
        }

        let program = &program;
        let mut pending = stream::iter(devset.iter().enumerate())
            .map(|(index, example)| async move {
                let outcome = program(example.inputs()).await;
                self.score(index, example, outcome)
            })
            .buffer_unordered(self.concurrency);

        let mut rows: Vec<Option<Row>> = vec![None; devset.len()];
        let mut done = 0;
        let mut failed = 0;
        let mut first_failure: Option<(usize, String)> = None;
        while let Some(row) = pending.next().await {
            done += 1;
            if let Some(error) = &row.error {
                failed += 1;
                // Completion order is arbitrary; report the earliest row's
                // failure so the message is stable from run to run.
                if first_failure
                    .as_ref()
                    .map_or(true, |(index, _)| row.index < *index)
                {
                    first_failure = Some((row.index, error.clone()));
                }
            }
            if let Some(progress) = &self.progress {
                progress(done, devset.len());
            }
            let index = row.index;
            rows[index] = Some(row);

            if let Some(max) = self.max_errors.filter(|max| failed > *max) {
                return Err(EvaluateError::TooManyErrors {
                    count: failed,
                    max,
                    first: first_failure
                        .map(|(_, message)| message)
                        .unwrap_or_default(),
                });
            }
        }
        Ok(Report::new(rows.into_iter().flatten().collect()))
    }

    fn score(&self, index: usize, example: &Example, outcome: Result<Values, ProgramError>) -> Row {
        match outcome {
            Ok(prediction) => score_row(self.metric.as_ref(), index, example, prediction),
            Err(error) => Row {
                index,
                example: example.clone(),
                prediction: None,
                score: 0.0,
                error: Some(error.to_string()),
            },
        }
    }
}

/// Scores one prediction, turning a metric failure or a non-finite score
/// into an error row.
fn score_row(metric: &dyn Metric, index: usize, example: &Example, prediction: Values) -> Row {
    let (score, error) = match metric.score(example, &prediction) {
        Ok(score) if score.is_finite() => (score, None),
        Ok(score) => (
            0.0,
            Some(format!(
                "metric returned {score}, which is not a finite score"
            )),
        ),
        Err(error) => (0.0, Some(error.to_string())),
    };
    Row {
        index,
        example: example.clone(),
        prediction: Some(prediction),
        score,
        error,
    }
}

/// Scores predictions that were produced elsewhere.
///
/// `predictions[i]` is scored against `devset[i]`. This is the offline half
/// of an evaluation: run the program however you like — another process, a
/// batch API, last week — and score its output here.
///
/// # Errors
///
/// Returns [`EvaluateError::Config`] when the two slices differ in length,
/// since a misaligned file would score every row against the wrong label.
pub fn score_predictions(
    metric: &dyn Metric,
    devset: &[Example],
    predictions: &[Values],
) -> Result<Report, EvaluateError> {
    if devset.len() != predictions.len() {
        return Err(EvaluateError::Config(format!(
            "devset has {} examples but there are {} predictions",
            devset.len(),
            predictions.len()
        )));
    }
    Ok(Report::new(
        devset
            .iter()
            .zip(predictions)
            .enumerate()
            .map(|(index, (example, prediction))| {
                score_row(metric, index, example, prediction.clone())
            })
            .collect(),
    ))
}
