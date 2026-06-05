use serde::{Deserialize, Serialize};

/// A single message in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message author: `"system"`, `"user"`, or `"assistant"`.
    pub role: String,
    /// Text content of the message.
    pub content: String,
}

impl Message {
    /// Creates a new message.
    ///
    /// # Arguments
    ///
    /// * `role` — one of `"system"`, `"user"`, or `"assistant"`.
    /// * `content` — message text.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::new("user", "Hello!");
    /// assert_eq!(msg.role, "user");
    /// ```
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Creates a `system` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::system("You are a helpful assistant.");
    /// assert_eq!(msg.role, "system");
    /// ```
    pub fn system(content: impl Into<String>) -> Self {
        Self::new("system", content)
    }

    /// Creates a `user` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::user("What is 2+2?");
    /// assert_eq!(msg.role, "user");
    /// ```
    pub fn user(content: impl Into<String>) -> Self {
        Self::new("user", content)
    }

    /// Creates an `assistant` role message.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::Message;
    /// let msg = Message::assistant("4");
    /// assert_eq!(msg.role, "assistant");
    /// ```
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new("assistant", content)
    }
}

/// Parameters for a completion or chat request.
///
/// # Examples
///
/// ```
/// use cosmos_llm::{CompletionRequest, Message};
///
/// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hi")])
///     .with_temperature(0.7)
///     .with_max_tokens(256);
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct CompletionRequest {
    /// Model identifier, e.g. `"gpt-4o"` or `"claude-3-5-sonnet-20241022"`.
    pub model: String,
    /// Conversation history to send to the provider.
    pub messages: Vec<Message>,
    /// Sampling temperature in `[0.0, 2.0]`. Higher values increase randomness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Maximum number of tokens to generate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Nucleus sampling cutoff in `[0.0, 1.0]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    /// Sequences at which the model will stop generating further tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop: Option<Vec<String>>,
}

impl CompletionRequest {
    /// Creates a new [`CompletionRequest`].
    ///
    /// # Arguments
    ///
    /// * `model` — model identifier.
    /// * `messages` — conversation history.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("Hello")]);
    /// assert_eq!(req.model, "gpt-4o");
    /// ```
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
        }
    }

    /// Sets the sampling temperature.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_temperature(0.5);
    /// assert_eq!(req.temperature, Some(0.5));
    /// ```
    pub fn with_temperature(mut self, t: f32) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Sets the maximum number of tokens to generate.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_max_tokens(100);
    /// assert_eq!(req.max_tokens, Some(100));
    /// ```
    pub fn with_max_tokens(mut self, n: u32) -> Self {
        self.max_tokens = Some(n);
        self
    }

    /// Sets the top-p nucleus sampling value.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_top_p(0.9);
    /// assert_eq!(req.top_p, Some(0.9));
    /// ```
    pub fn with_top_p(mut self, p: f32) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Adds stop sequences.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionRequest, Message};
    /// let req = CompletionRequest::new("gpt-4o", vec![Message::user("hi")])
    ///     .with_stop(vec!["END".into()]);
    /// assert!(req.stop.is_some());
    /// ```
    pub fn with_stop(mut self, stop: Vec<String>) -> Self {
        self.stop = Some(stop);
        self
    }
}

/// Token usage reported by the provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Usage {
    /// Tokens consumed by the prompt.
    pub prompt_tokens: u32,
    /// Tokens generated in the completion.
    pub completion_tokens: u32,
    /// Total tokens (prompt + completion).
    pub total_tokens: u32,
}

/// A single choice returned by the provider.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Choice {
    /// Zero-based index of this choice.
    pub index: u32,
    /// The generated message.
    pub message: Message,
    /// Reason the generation stopped (e.g. `"stop"`, `"length"`).
    pub finish_reason: Option<String>,
}

/// The response from a completion request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CompletionResponse {
    /// Provider-assigned response identifier.
    pub id: Option<String>,
    /// Model that produced the response.
    pub model: Option<String>,
    /// One or more completion choices.
    pub choices: Vec<Choice>,
    /// Token usage statistics, if provided.
    pub usage: Option<Usage>,
}

impl CompletionResponse {
    /// Returns the text content of the first choice, if present.
    ///
    /// This is a convenience method for the common case of requesting a
    /// single completion.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::{CompletionResponse, Choice, Message};
    ///
    /// let resp = CompletionResponse {
    ///     id: None,
    ///     model: None,
    ///     choices: vec![Choice {
    ///         index: 0,
    ///         message: Message::assistant("Hello!"),
    ///         finish_reason: Some("stop".into()),
    ///     }],
    ///     usage: None,
    /// };
    /// assert_eq!(resp.content(), Some("Hello!"));
    /// ```
    pub fn content(&self) -> Option<&str> {
        self.choices.first().map(|c| c.message.content.as_str())
    }
}

/// A delta chunk delivered during a streaming response.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamChunk {
    /// Incremental text fragment for this chunk.
    pub delta: String,
    /// Set when the stream is complete.
    pub finish_reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors() {
        let s = Message::system("sys");
        assert_eq!(s.role, "system");
        let u = Message::user("usr");
        assert_eq!(u.role, "user");
        let a = Message::assistant("asst");
        assert_eq!(a.role, "assistant");
    }

    #[test]
    fn completion_request_builder() {
        let req = CompletionRequest::new("m", vec![Message::user("hi")])
            .with_temperature(0.5)
            .with_max_tokens(100)
            .with_top_p(0.9)
            .with_stop(vec!["END".into()]);
        assert_eq!(req.temperature, Some(0.5));
        assert_eq!(req.max_tokens, Some(100));
        assert_eq!(req.top_p, Some(0.9));
        assert!(req.stop.is_some());
    }

    #[test]
    fn completion_response_content() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![Choice {
                index: 0,
                message: Message::assistant("hello"),
                finish_reason: None,
            }],
            usage: None,
        };
        assert_eq!(resp.content(), Some("hello"));
    }

    #[test]
    fn empty_choices_returns_none() {
        let resp = CompletionResponse {
            id: None,
            model: None,
            choices: vec![],
            usage: None,
        };
        assert_eq!(resp.content(), None);
    }
}
