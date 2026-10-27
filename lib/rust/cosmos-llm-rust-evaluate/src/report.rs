use std::fmt;

use serde::{Serialize, Serializer};

use cosmos_llm_example::Example;

use crate::metric::Values;

/// The score a row must reach to pass, unless a caller says otherwise.
pub const DEFAULT_THRESHOLD: f64 = 1.0;

/// One evaluated example: what went in, what came out, and what it scored.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    /// The example's position in the devset.
    pub index: usize,
    /// The example, serialized as its plain value object.
    pub example: Example,
    /// What the program produced, or `None` when it failed.
    pub prediction: Option<Values>,
    /// The score; zero for a row that errored.
    pub score: f64,
    /// Why the program or metric failed on this example, if it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Row {
    /// Whether the program or metric failed on this example.
    pub fn is_error(&self) -> bool {
        self.error.is_some()
    }
}

/// The outcome of evaluating a program over a devset.
///
/// Holds the aggregate score and every row behind it, because the aggregate
/// on its own rarely tells you what to fix. The rows are what you sort to
/// find the worst cases, and they are kept in devset order no matter what
/// order the program finished them in.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::{metrics, score_predictions};
/// use cosmos_llm_example::Example;
/// use serde_json::json;
///
/// let devset: Vec<Example> = ["4", "6"]
///     .iter()
///     .map(|a| Example::from_value(json!({ "answer": a })).unwrap())
///     .collect();
/// let predictions = vec![
///     json!({ "answer": "4" }).as_object().unwrap().clone(),
///     json!({ "answer": "7" }).as_object().unwrap().clone(),
/// ];
///
/// let report = score_predictions(&metrics::exact_match("answer", true), &devset, &predictions).unwrap();
/// assert_eq!(report.score(), 0.5);
/// assert_eq!(report.summary(), "50.0% (1/2 passed)");
/// assert_eq!(report.failures(1.0)[0].index, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    rows: Vec<Row>,
}

impl Report {
    /// Wraps rows, which should already be in devset order.
    pub fn new(rows: Vec<Row>) -> Self {
        Self { rows }
    }

    /// Every row, in devset order.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The mean score across all rows; `0.0` for an empty report.
    ///
    /// Rows that errored count as zero rather than being dropped. A program
    /// that crashes on a third of the devset has a real problem, and
    /// averaging only over the survivors would hide it.
    pub fn score(&self) -> f64 {
        if self.rows.is_empty() {
            return 0.0;
        }
        self.rows.iter().map(|row| row.score).sum::<f64>() / self.rows.len() as f64
    }

    /// [`Report::score`] as a percentage.
    pub fn percent(&self) -> f64 {
        self.score() * 100.0
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the report has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Rows scoring at least `threshold`, in devset order.
    pub fn passes(&self, threshold: f64) -> Vec<&Row> {
        self.rows
            .iter()
            .filter(|row| row.score >= threshold)
            .collect()
    }

    /// Rows scoring below `threshold`, worst first; ties keep devset order.
    pub fn failures(&self, threshold: f64) -> Vec<&Row> {
        let mut failed: Vec<&Row> = self
            .rows
            .iter()
            .filter(|row| row.score < threshold)
            .collect();
        failed.sort_by(|a, b| a.score.total_cmp(&b.score));
        failed
    }

    /// Rows where the program or metric failed, in devset order.
    pub fn errors(&self) -> Vec<&Row> {
        self.rows.iter().filter(|row| row.is_error()).collect()
    }

    /// A short summary at the default pass threshold, e.g.
    /// `"72.0% (18/25 passed, 1 errored)"`.
    pub fn summary(&self) -> String {
        self.summary_at(DEFAULT_THRESHOLD)
    }

    /// A short summary counting passes at `threshold`.
    pub fn summary_at(&self, threshold: f64) -> String {
        let mut detail = format!("{}/{} passed", self.passes(threshold).len(), self.len());
        let errored = self.errors().len();
        if errored > 0 {
            detail.push_str(&format!(", {errored} errored"));
        }
        format!("{:.1}% ({detail})", self.percent())
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.summary())
    }
}

impl Serialize for Report {
    /// Serializes the aggregate counts at the default threshold, then every
    /// row.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shape<'a> {
            score: f64,
            size: usize,
            passed: usize,
            failed: usize,
            errored: usize,
            rows: &'a [Row],
        }
        Shape {
            score: self.score(),
            size: self.len(),
            passed: self.passes(DEFAULT_THRESHOLD).len(),
            failed: self.failures(DEFAULT_THRESHOLD).len(),
            errored: self.errors().len(),
            rows: &self.rows,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(index: usize, score: f64, error: Option<&str>) -> Row {
        Row {
            index,
            example: Example::default(),
            prediction: None,
            score,
            error: error.map(str::to_owned),
        }
    }

    #[test]
    fn score_is_the_mean_including_errors() {
        let report = Report::new(vec![
            row(0, 1.0, None),
            row(1, 0.5, None),
            row(2, 0.0, Some("boom")),
        ]);
        assert_eq!(report.score(), 0.5);
        assert_eq!(report.summary(), "50.0% (1/3 passed, 1 errored)");
    }

    #[test]
    fn failures_are_worst_first_and_threshold_controls_passing() {
        let report = Report::new(vec![
            row(0, 0.6, None),
            row(1, 0.2, None),
            row(2, 0.9, None),
        ]);
        let worst: Vec<usize> = report.failures(1.0).iter().map(|r| r.index).collect();
        assert_eq!(worst, [1, 0, 2]);
        assert_eq!(report.passes(0.5).len(), 2);
        assert_eq!(report.summary_at(0.5), "56.7% (2/3 passed)");
    }

    #[test]
    fn an_empty_report_scores_zero() {
        let report = Report::default();
        assert_eq!(report.score(), 0.0);
        assert_eq!(report.summary(), "0.0% (0/0 passed)");
    }

    #[test]
    fn serializes_counts_and_rows() {
        let report = Report::new(vec![row(0, 1.0, None), row(1, 0.0, Some("boom"))]);
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["passed"], 1);
        assert_eq!(value["errored"], 1);
        assert_eq!(value["rows"][1]["error"], "boom");
        assert!(value["rows"][0].get("error").is_none());
    }
}
