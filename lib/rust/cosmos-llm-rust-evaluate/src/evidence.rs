//! Verifying that quoted evidence actually appears in its source document.
//!
//! When a model is asked to extract claims and quote the text supporting
//! each one, checking those quotes mechanically is what separates an
//! extraction from a plausible-sounding invention. A model that cannot find
//! support tends to write something that reads like support.
//!
//! This is deliberately not a similarity score. A quote either appears in
//! the source or it does not; a claim assembled from words scattered through
//! the document is exactly the failure this catches, so the check stays a
//! contiguous substring match.
//!
//! ## What normalization forgives, and what it does not
//!
//! Markdown conversion, PDF extraction, and HTML-to-text all rewrite
//! punctuation and line wrapping. A quote that differs from its source only
//! in curly quotes, dash width, or where a line happened to break is verbatim
//! in every way that matters, so [`normalize`] folds those.
//!
//! Case is folded for the same reason: a model copying a sentence from
//! mid-paragraph routinely lowercases the leading word. Folding case does not
//! weaken the check the way loosening it to token matching would — the quote
//! must still appear contiguously.
//!
//! # Examples
//!
//! ```
//! use cosmos_llm_evaluate::evidence;
//! use serde_json::json;
//!
//! let source = "They saw their conversion rate increase by 7-12%\nafter the migration.";
//!
//! assert!(evidence::is_present("conversion rate increase by 7–12% after", source));
//! assert!(!evidence::is_present("conversion rate after migration", source));
//!
//! let finding = json!({ "claims": [{ "evidence": "revenue tripled" }] });
//! assert_eq!(
//!     evidence::unverified(&finding, source, "evidence"),
//!     ["claims[0].evidence: quote not found in source: \"revenue tripled\""]
//! );
//! ```

use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

/// Quote characters rewritten to a plain apostrophe.
pub const SINGLE_QUOTES: &[char] = &['\u{2018}', '\u{2019}', '\u{201B}', '\u{2032}'];
/// Quote characters rewritten to a plain double quote.
pub const DOUBLE_QUOTES: &[char] = &['\u{201C}', '\u{201D}', '\u{201F}', '\u{2033}'];
/// Dash characters rewritten to a plain hyphen.
pub const DASHES: &[char] = &['\u{2013}', '\u{2014}', '\u{2212}'];

/// The longest quote excerpt, in characters, an [`unverified`] message shows.
pub const EXCERPT_CHARS: usize = 80;

/// Collapses whitespace and normalizes the punctuation that text conversion
/// tends to rewrite.
///
/// Applies Unicode NFKC, folds curly quotes and long dashes to their ASCII
/// forms, collapses every run of whitespace to one space, and trims. Case is
/// left alone; [`is_present`] folds it separately.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::evidence::normalize;
///
/// assert_eq!(normalize("a  \u{2018}b\u{2019}\n \u{201C}c\u{201D}  \u{2014} d"), "a 'b' \"c\" - d");
/// ```
pub fn normalize(text: &str) -> String {
    let folded: String = text
        .nfkc()
        .map(|c| {
            if SINGLE_QUOTES.contains(&c) {
                '\''
            } else if DOUBLE_QUOTES.contains(&c) {
                '"'
            } else if DASHES.contains(&c) {
                '-'
            } else {
                c
            }
        })
        .collect();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a quote appears in the given text.
///
/// An empty quote is never present: a model that omits its evidence has not
/// supported anything, and treating `""` as trivially found would verify
/// every claim that skipped the field.
pub fn is_present(quote: &str, text: &str) -> bool {
    let needle = normalize(quote);
    if needle.is_empty() {
        return false;
    }
    normalize(text)
        .to_lowercase()
        .contains(&needle.to_lowercase())
}

/// Every quote in a nested structure, paired with its path.
///
/// Walks objects and arrays, collecting each string value stored under
/// `key`. Paths read like `findings[1].evidence`. A value under `key` that is
/// not a string is descended into rather than collected, since it cannot be a
/// quote.
///
/// # Examples
///
/// ```
/// use cosmos_llm_evaluate::evidence::collect;
/// use serde_json::json;
///
/// let record = json!({ "findings": [{ "evidence": "one" }, { "evidence": "two" }] });
/// assert_eq!(
///     collect(&record, "evidence"),
///     [
///         ("findings[0].evidence".to_owned(), "one".to_owned()),
///         ("findings[1].evidence".to_owned(), "two".to_owned()),
///     ]
/// );
/// ```
pub fn collect(node: &Value, key: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    walk(node, String::new(), key, &mut found);
    found
}

fn walk(node: &Value, path: String, key: &str, found: &mut Vec<(String, String)>) {
    match node {
        Value::Object(object) => {
            for (name, value) in object {
                let child = if path.is_empty() {
                    name.clone()
                } else {
                    format!("{path}.{name}")
                };
                match value {
                    Value::String(quote) if name == key => found.push((child, quote.clone())),
                    other => walk(other, child, key, found),
                }
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(item, format!("{path}[{index}]"), key, found);
            }
        }
        _ => {}
    }
}

/// One message per quote in `record` that cannot be found in `text`.
///
/// Each message names the quote's path and shows up to [`EXCERPT_CHARS`]
/// characters of it. An empty result means everything verified.
pub fn unverified(record: &Value, text: &str, key: &str) -> Vec<String> {
    collect(record, key)
        .into_iter()
        .filter(|(_, quote)| !is_present(quote, text))
        .map(|(path, quote)| {
            let excerpt: String = quote.chars().take(EXCERPT_CHARS).collect();
            format!("{path}: quote not found in source: {excerpt:?}")
        })
        .collect()
}

/// Whether every quote in `record` is supported by `text`.
pub fn is_verified(record: &Value, text: &str, key: &str) -> bool {
    unverified(record, text, key).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SOURCE: &str = "They saw their conversion rate increase by 7-12% after the migration.\n\
        The client came to us with a checkout flow that took eleven seconds\n\
        to render on a cold cache.\n";

    #[test]
    fn verbatim_quotes_are_present_and_inventions_are_not() {
        assert!(is_present("conversion rate increase by 7-12%", SOURCE));
        assert!(!is_present("conversion rate fell by 40%", SOURCE));
    }

    #[test]
    fn a_lowercased_leading_word_still_verifies() {
        assert!(is_present("they saw their conversion rate", SOURCE));
    }

    #[test]
    fn a_quote_spanning_a_line_break_verifies() {
        assert!(is_present(
            "took eleven seconds to render on a cold cache",
            SOURCE
        ));
    }

    #[test]
    fn curly_quotes_and_em_dashes_are_folded() {
        let source = "The vendor said \u{201C}it doubled\u{201D} \u{2014} every quarter.";
        assert!(is_present("\"it doubled\" - every quarter", source));
    }

    #[test]
    fn scattered_words_do_not_verify() {
        assert!(!is_present("conversion rate cold cache eleven", SOURCE));
    }

    #[test]
    fn empty_quotes_are_never_present() {
        assert!(!is_present("", SOURCE));
        assert!(!is_present("   \n", SOURCE));
    }

    #[test]
    fn nfkc_folds_compatibility_forms() {
        // A fullwidth digit and a ligature, as PDF extraction produces them.
        assert!(is_present("\u{FF17}-12%", SOURCE));
        assert!(is_present("e\u{FB00}ect", "the effect was large"));
    }

    #[test]
    fn collect_ignores_non_string_values_and_honors_the_key() {
        assert!(collect(&json!({ "evidence": { "nested": 1 } }), "evidence").is_empty());
        let record = json!({ "quote": "text here" });
        assert_eq!(
            collect(&record, "quote"),
            [("quote".to_owned(), "text here".to_owned())]
        );
        assert!(collect(&record, "evidence").is_empty());
    }

    #[test]
    fn collect_paths_through_a_top_level_array() {
        let record = json!([{ "evidence": "a" }]);
        assert_eq!(collect(&record, "evidence")[0].0, "[0].evidence");
    }

    #[test]
    fn unverified_reports_only_unsupported_quotes() {
        let record = json!({ "findings": [
            { "evidence": "conversion rate increase by 7-12%" },
            { "evidence": "revenue tripled overnight" },
        ]});
        let messages = unverified(&record, SOURCE, "evidence");
        assert_eq!(messages.len(), 1);
        assert!(
            messages[0].starts_with("findings[1].evidence:"),
            "{}",
            messages[0]
        );
        assert!(is_verified(
            &json!({ "evidence": "the client came to us" }),
            SOURCE,
            "evidence"
        ));
    }

    #[test]
    fn long_quotes_are_truncated_in_messages() {
        let record = json!({ "evidence": format!("{} never appears", "x".repeat(200)) });
        let message = &unverified(&record, SOURCE, "evidence")[0];
        assert!(message.chars().count() < 140, "{message}");
    }
}
