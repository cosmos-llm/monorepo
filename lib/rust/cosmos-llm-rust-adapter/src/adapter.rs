use serde_json::{Map, Value};

use cosmos_llm_registry::{Meta, Plugin};
use cosmos_llm_signature::{Field, Signature};

use crate::error::AdapterError;
use crate::message::Message;

/// Turns a signature and its inputs into chat messages, and a model's reply
/// back into the signature's typed outputs.
///
/// The two required halves of a prompting strategy are how the model is told
/// to lay out its answer ([`Adapter::format_instructions`],
/// [`Adapter::render_inputs`], [`Adapter::render_demo_outputs`]) and how that
/// layout is read back ([`Adapter::extract`]). Everything else has a default
/// built from those, and any of it can be overridden.
///
/// Implementations are shared across calls, so they hold configuration, not
/// per-call state.
pub trait Adapter: Send + Sync {
    /// Renders the full conversation: a system prompt, one user/assistant
    /// pair per demonstration, then the user turn holding `inputs`.
    ///
    /// Demonstrations are plain value maps holding both input and output
    /// fields; each half is picked out by the signature, so a demo may carry
    /// extra keys.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::Signature`] when a field cannot be described,
    /// e.g. its type is not registered in this binary.
    fn format(
        &self,
        sig: &Signature,
        inputs: &Map<String, Value>,
        demos: &[Map<String, Value>],
    ) -> Result<Vec<Message>, AdapterError> {
        let mut messages = vec![Message::system(self.system_prompt(sig)?)];
        for demo in demos {
            messages.push(Message::user(
                self.render_inputs(sig, &input_values(sig, demo))?,
            ));
            messages.push(Message::assistant(self.render_demo_outputs(sig, demo)?));
        }
        messages.push(Message::user(self.render_inputs(sig, inputs)?));
        Ok(messages)
    }

    /// Parses a model reply into the signature's coerced outputs.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::Parse`], carrying the reply, when the layout
    /// cannot be found or a value does not coerce to its declared type.
    fn parse(&self, sig: &Signature, completion: &str) -> Result<Map<String, Value>, AdapterError> {
        let raw = self.extract(sig, completion)?;
        sig.coerce_outputs(&Value::Object(raw))
            .map_err(|e| AdapterError::parse(e.to_string(), completion))
    }

    /// Extra top-level fields for the completion request body, such as a
    /// provider's structured-output `response_format`.
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::Signature`] when the parameters depend on a
    /// schema that cannot be built.
    fn request_params(&self, sig: &Signature) -> Result<Map<String, Value>, AdapterError> {
        let _ = sig;
        Ok(Map::new())
    }

    /// The system prompt: instructions, field descriptions, and format
    /// instructions, separated by blank lines, empty parts skipped.
    ///
    /// # Errors
    ///
    /// As for [`Adapter::format`].
    fn system_prompt(&self, sig: &Signature) -> Result<String, AdapterError> {
        let parts = [
            sig.instructions(),
            self.field_description(sig)?,
            self.format_instructions(sig)?,
        ];
        Ok(parts
            .iter()
            .filter(|part| !part.trim().is_empty())
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n\n"))
    }

    /// Lists the input and output fields with their type hints.
    ///
    /// # Errors
    ///
    /// As for [`Adapter::format`].
    fn field_description(&self, sig: &Signature) -> Result<String, AdapterError> {
        let mut lines = vec!["Your input fields are:".to_owned()];
        for field in sig.input_fields() {
            lines.push(describe_field(field)?);
        }
        lines.push(String::new());
        lines.push("Your output fields are:".to_owned());
        for field in sig.output_fields() {
            lines.push(describe_field(field)?);
        }
        Ok(lines.join("\n"))
    }

    /// Tells the model how to lay out its reply.
    ///
    /// # Errors
    ///
    /// As for [`Adapter::format`].
    fn format_instructions(&self, sig: &Signature) -> Result<String, AdapterError>;

    /// Renders input values as the text of a user turn.
    ///
    /// # Errors
    ///
    /// As for [`Adapter::format`].
    fn render_inputs(
        &self,
        sig: &Signature,
        inputs: &Map<String, Value>,
    ) -> Result<String, AdapterError>;

    /// Renders a demonstration's outputs as the text of an assistant turn,
    /// in exactly the layout [`Adapter::extract`] reads.
    ///
    /// # Errors
    ///
    /// As for [`Adapter::format`].
    fn render_demo_outputs(
        &self,
        sig: &Signature,
        demo: &Map<String, Value>,
    ) -> Result<String, AdapterError>;

    /// Pulls raw, uncoerced output values out of a reply.
    ///
    /// Undeclared keys should be dropped here; coercion happens afterwards in
    /// [`Adapter::parse`].
    ///
    /// # Errors
    ///
    /// Returns [`AdapterError::Parse`] when the expected layout is absent.
    fn extract(
        &self,
        sig: &Signature,
        completion: &str,
    ) -> Result<Map<String, Value>, AdapterError>;
}

/// An adapter registration.
///
/// ```
/// use cosmos_llm_adapter::{adapter, AdapterPlugin, ChatAdapter};
/// use cosmos_llm_adapter::registry::meta;
///
/// fn build() -> Box<dyn cosmos_llm_adapter::Adapter> {
///     Box::new(ChatAdapter)
/// }
///
/// cosmos_llm_adapter::registry::inventory::submit! {
///     AdapterPlugin { meta: meta!("sections").summary("Chat markers, by another name"), build }
/// }
///
/// assert!(adapter("sections").is_ok());
/// ```
pub struct AdapterPlugin {
    /// The adapter's name and aliases.
    pub meta: Meta,
    /// Builds the adapter.
    pub build: fn() -> Box<dyn Adapter>,
}

impl Plugin for AdapterPlugin {
    fn meta(&self) -> &Meta {
        &self.meta
    }
}

cosmos_llm_registry::inventory::collect!(AdapterPlugin);

/// Builds the adapter registered under `name`.
///
/// # Errors
///
/// Returns [`AdapterError::NotFound`] naming the registered adapters.
///
/// # Examples
///
/// ```
/// let chat = cosmos_llm_adapter::adapter("chat").unwrap();
/// assert!(cosmos_llm_adapter::adapter("telepathy").is_err());
/// ```
pub fn adapter(name: &str) -> Result<Box<dyn Adapter>, AdapterError> {
    let plugin = cosmos_llm_registry::require::<AdapterPlugin>("adapter", name)?;
    Ok((plugin.build)())
}

/// The values of `values` that are the signature's input fields, in field
/// order.
pub fn input_values(sig: &Signature, values: &Map<String, Value>) -> Map<String, Value> {
    pick(sig.input_fields(), values)
}

/// The values of `values` that are the signature's output fields, in field
/// order.
pub fn output_values(sig: &Signature, values: &Map<String, Value>) -> Map<String, Value> {
    pick(sig.output_fields(), values)
}

fn pick<'a>(
    fields: impl Iterator<Item = &'a Field>,
    values: &Map<String, Value>,
) -> Map<String, Value> {
    fields
        .filter_map(|field| {
            values
                .get(&field.name)
                .map(|value| (field.name.clone(), value.clone()))
        })
        .collect()
}

/// Renders a value as prompt text: strings bare, `null` empty, everything
/// else as compact JSON.
///
/// # Examples
///
/// ```
/// use cosmos_llm_adapter::render_value;
/// use serde_json::json;
///
/// assert_eq!(render_value(&json!("plain")), "plain");
/// assert_eq!(render_value(&json!(["a", 1])), "[\"a\",1]");
/// ```
pub fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Describes a field as `- name (type hint): description`.
///
/// # Errors
///
/// Returns [`AdapterError::Signature`] when the field's type is unknown.
pub fn describe_field(field: &Field) -> Result<String, AdapterError> {
    let mut line = format!("- {} ({})", field.name, field.type_hint()?);
    if !field.desc.is_empty() {
        line.push_str(": ");
        line.push_str(&field.desc);
    }
    Ok(line)
}
