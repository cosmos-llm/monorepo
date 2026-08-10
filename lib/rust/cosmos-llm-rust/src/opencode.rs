//! Optional interop with the [opencode](https://opencode.ai) CLI's credential
//! store.
//!
//! Developers who already authenticated providers through `opencode auth
//! login` have keys sitting in `~/.local/share/opencode/auth.json`. This
//! module reads that file so those keys can be reused without re-exporting
//! them into the environment.
//!
//! Loading is always explicit — nothing here runs unless the caller asks for
//! it. See [`Config::load_opencode_auth`](crate::Config::load_opencode_auth).
//!
//! # File format
//!
//! ```json
//! {
//!   "openrouter": { "type": "api", "key": "sk-or-..." },
//!   "anthropic":  { "type": "oauth", "access": "...", "refresh": "..." }
//! }
//! ```
//!
//! Only entries with `"type": "api"` carry a reusable key; OAuth entries hold
//! short-lived tokens that opencode refreshes itself, so they are skipped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::CosmosError;

/// A single credential entry in opencode's `auth.json`.
///
/// Non-`api` entries (OAuth, and any future kinds) deserialize with
/// `key: None` and are filtered out by [`load_keys`].
#[derive(Debug, Clone, Deserialize)]
pub struct OpencodeCredential {
    /// Credential kind, e.g. `"api"` or `"oauth"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// API key, present when `kind` is `"api"`.
    #[serde(default)]
    pub key: Option<String>,
}

/// Returns the path to opencode's `auth.json`, honouring `XDG_DATA_HOME`.
///
/// Falls back to `$HOME/.local/share/opencode/auth.json`. Returns `None` when
/// neither `XDG_DATA_HOME` nor `HOME` is set.
///
/// # Examples
///
/// ```
/// use cosmos_llm::opencode::auth_path;
///
/// // Present on any machine with a home directory.
/// if let Some(path) = auth_path() {
///     assert!(path.ends_with("opencode/auth.json"));
/// }
/// ```
pub fn auth_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return Some(Path::new(&dir).join("opencode").join("auth.json"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".local")
            .join("share")
            .join("opencode")
            .join("auth.json"),
    )
}

/// Maps an opencode provider name onto this crate's provider name.
///
/// opencode uses its own provider identifiers, most of which already match.
/// Returns `None` for providers this crate has no backend for, so unknown
/// entries are ignored rather than stored under a name that cannot resolve.
fn normalize_provider(name: &str) -> Option<&'static str> {
    match name.to_lowercase().as_str() {
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "openrouter" => Some("openrouter"),
        _ => None,
    }
}

/// Parses opencode credentials from a JSON string.
///
/// Returns provider name → API key for every `api`-type entry whose provider
/// this crate supports. Malformed individual entries are skipped; only a
/// document that is not valid JSON produces an error.
///
/// # Errors
///
/// Returns [`CosmosError::Json`] when `contents` is not valid JSON.
///
/// # Examples
///
/// ```
/// use cosmos_llm::opencode::parse_keys;
///
/// let json = r#"{
///   "openrouter": { "type": "api", "key": "sk-or-test" },
///   "anthropic":  { "type": "oauth", "access": "tok" },
///   "groq":       { "type": "api", "key": "gsk-test" }
/// }"#;
///
/// let keys = parse_keys(json).unwrap();
/// assert_eq!(keys.get("openrouter").map(String::as_str), Some("sk-or-test"));
/// // OAuth entries and unsupported providers are skipped.
/// assert!(!keys.contains_key("anthropic"));
/// assert!(!keys.contains_key("groq"));
/// ```
pub fn parse_keys(contents: &str) -> Result<HashMap<String, String>, CosmosError> {
    // Deserialize permissively: an entry with an unexpected shape becomes an
    // error we drop, rather than failing the whole file.
    let raw: HashMap<String, serde_json::Value> = serde_json::from_str(contents)?;

    let mut keys = HashMap::new();
    for (provider, value) in raw {
        let Some(name) = normalize_provider(&provider) else {
            continue;
        };
        let Ok(cred) = serde_json::from_value::<OpencodeCredential>(value) else {
            continue;
        };
        if cred.kind != "api" {
            continue;
        }
        // Keys pasted into `opencode auth login` can pick up surrounding
        // whitespace, and a trailing newline makes an invalid HTTP header
        // value once it reaches a provider.
        let Some(key) = cred.key else { continue };
        let key = key.trim();
        if !key.is_empty() {
            keys.insert(name.to_owned(), key.to_owned());
        }
    }
    Ok(keys)
}

/// Reads and parses opencode's `auth.json` from an explicit path.
///
/// # Errors
///
/// Returns [`CosmosError::Configuration`] when the file cannot be read, and
/// [`CosmosError::Json`] when its contents are not valid JSON.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::opencode::load_keys_from;
///
/// let keys = load_keys_from("/home/me/.local/share/opencode/auth.json").unwrap();
/// println!("{} provider(s) available", keys.len());
/// ```
pub fn load_keys_from(path: impl AsRef<Path>) -> Result<HashMap<String, String>, CosmosError> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path)
        .map_err(|e| CosmosError::config(format!("cannot read {}: {e}", path.display())))?;
    parse_keys(&contents)
}

/// Reads and parses opencode's `auth.json` from its default location.
///
/// Returns an empty map when the file does not exist — opencode simply is not
/// installed or has no credentials, which is not an error.
///
/// # Errors
///
/// Returns [`CosmosError::Configuration`] when the home directory cannot be
/// determined or the file exists but cannot be read, and [`CosmosError::Json`]
/// when its contents are not valid JSON.
///
/// # Examples
///
/// ```no_run
/// use cosmos_llm::opencode::load_keys;
///
/// let keys = load_keys().unwrap();
/// if let Some(key) = keys.get("openrouter") {
///     println!("found an OpenRouter key ({} chars)", key.len());
/// }
/// ```
pub fn load_keys() -> Result<HashMap<String, String>, CosmosError> {
    let path = auth_path().ok_or_else(|| {
        CosmosError::config(
            "cannot locate opencode auth.json: neither XDG_DATA_HOME nor HOME is set",
        )
    })?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    load_keys_from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "openrouter": { "type": "api", "key": "sk-or-test" },
        "openai":     { "type": "api", "key": "sk-test" },
        "anthropic":  { "type": "oauth", "access": "tok", "refresh": "ref" },
        "groq":       { "type": "api", "key": "gsk-test" }
    }"#;

    #[test]
    fn parse_keys_extracts_supported_api_entries() {
        let keys = parse_keys(SAMPLE).unwrap();
        assert_eq!(
            keys.get("openrouter").map(String::as_str),
            Some("sk-or-test")
        );
        assert_eq!(keys.get("openai").map(String::as_str), Some("sk-test"));
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn parse_keys_skips_oauth_entries() {
        let keys = parse_keys(SAMPLE).unwrap();
        assert!(!keys.contains_key("anthropic"));
    }

    #[test]
    fn parse_keys_skips_unsupported_providers() {
        let keys = parse_keys(SAMPLE).unwrap();
        assert!(!keys.contains_key("groq"));
    }

    #[test]
    fn parse_keys_trims_surrounding_whitespace() {
        // A trailing newline in the stored key would otherwise produce an
        // invalid HTTP header value at request time.
        let json = r#"{ "openrouter": { "type": "api", "key": "sk-or-test\n\n" } }"#;
        let keys = parse_keys(json).unwrap();
        assert_eq!(
            keys.get("openrouter").map(String::as_str),
            Some("sk-or-test")
        );
    }

    #[test]
    fn parse_keys_skips_whitespace_only_keys() {
        let json = r#"{ "openrouter": { "type": "api", "key": "   \n" } }"#;
        assert!(parse_keys(json).unwrap().is_empty());
    }

    #[test]
    fn parse_keys_skips_empty_and_malformed_entries() {
        let json = r#"{
            "openai":     { "type": "api", "key": "" },
            "anthropic":  { "type": "api" },
            "openrouter": "not-an-object"
        }"#;
        let keys = parse_keys(json).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn parse_keys_rejects_invalid_json() {
        let err = parse_keys("{not json").unwrap_err();
        assert!(matches!(err, CosmosError::Json(_)));
    }

    #[test]
    fn parse_keys_accepts_empty_document() {
        assert!(parse_keys("{}").unwrap().is_empty());
    }

    #[test]
    fn load_keys_from_missing_file_is_configuration_error() {
        let err = load_keys_from("/nonexistent/opencode/auth.json").unwrap_err();
        assert!(matches!(err, CosmosError::Configuration { .. }));
    }

    #[test]
    fn normalize_provider_is_case_insensitive() {
        assert_eq!(normalize_provider("OpenRouter"), Some("openrouter"));
        assert_eq!(normalize_provider("zai"), None);
    }
}
