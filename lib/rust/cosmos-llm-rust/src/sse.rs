//! Server-sent events decoding for streaming responses.
//!
//! Providers deliver streaming completions as `text/event-stream` bodies, but
//! the HTTP layer hands us arbitrary byte chunks: a single event may be split
//! across two chunks, and one chunk may carry several events. [`SseDecoder`]
//! buffers across chunk boundaries and yields only complete events.

/// One decoded server-sent event.
///
/// Only the fields this crate needs are retained. `id` and `retry` are parsed
/// and discarded, per the SSE specification's rule that unknown or unused
/// fields are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    /// The `event:` field, when the producer set one.
    ///
    /// OpenAI omits this; Anthropic sets it to the message type (e.g.
    /// `"content_block_delta"`). The `type` field inside `data` duplicates it,
    /// so parsers may rely on either.
    pub event: Option<String>,
    /// Concatenated `data:` field values, joined with newlines.
    pub data: String,
}

impl SseEvent {
    /// Returns `true` if this event is the OpenAI end-of-stream sentinel.
    ///
    /// OpenAI and OpenAI-compatible APIs close a stream with a literal
    /// `data: [DONE]` line, which is not valid JSON and must not be parsed.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::sse::SseEvent;
    ///
    /// let done = SseEvent { event: None, data: "[DONE]".into() };
    /// assert!(done.is_done());
    /// ```
    pub fn is_done(&self) -> bool {
        self.data.trim() == "[DONE]"
    }
}

/// Incremental decoder for a `text/event-stream` body.
///
/// Feed it byte chunks as they arrive with [`SseDecoder::push`]; each call
/// returns the events that became complete. Bytes belonging to a partially
/// received event are retained until the rest arrives.
///
/// # Examples
///
/// An event split across two chunks is emitted once, after the second chunk:
///
/// ```
/// use cosmos_llm::sse::SseDecoder;
///
/// let mut decoder = SseDecoder::new();
/// assert!(decoder.push(b"data: {\"a\":").is_empty());
///
/// let events = decoder.push(b"1}\n\n");
/// assert_eq!(events.len(), 1);
/// assert_eq!(events[0].data, "{\"a\":1}");
/// ```
#[derive(Debug, Default)]
pub struct SseDecoder {
    buffer: Vec<u8>,
}

impl SseDecoder {
    /// Creates an empty decoder.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::sse::SseDecoder;
    /// let decoder = SseDecoder::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds a chunk of body bytes and returns every event completed by it.
    ///
    /// Returns an empty vector when the chunk does not finish an event.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::sse::SseDecoder;
    ///
    /// let mut decoder = SseDecoder::new();
    /// let events = decoder.push(b"data: one\n\ndata: two\n\n");
    /// assert_eq!(events.len(), 2);
    /// assert_eq!(events[1].data, "two");
    /// ```
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);

        let mut events = Vec::new();
        while let Some((block, rest)) = split_event(&self.buffer) {
            if let Some(event) = parse_block(&block) {
                events.push(event);
            }
            self.buffer = rest;
        }
        events
    }

    /// Consumes any trailing bytes as a final event.
    ///
    /// Well-behaved producers terminate the last event with a blank line, so
    /// this usually returns `None`. It exists so a stream truncated without
    /// that terminator still surfaces whatever arrived.
    ///
    /// # Examples
    ///
    /// ```
    /// use cosmos_llm::sse::SseDecoder;
    ///
    /// let mut decoder = SseDecoder::new();
    /// assert!(decoder.push(b"data: tail").is_empty());
    /// assert_eq!(decoder.finish().unwrap().data, "tail");
    /// ```
    pub fn finish(&mut self) -> Option<SseEvent> {
        let block = std::mem::take(&mut self.buffer);
        parse_block(&block)
    }
}

/// Splits the buffer at the first event terminator, returning the event block
/// and the bytes after it.
///
/// Accepts `\n\n`, `\r\n\r\n`, and `\r\r` as terminators, matching the SSE
/// specification's treatment of all three line endings.
fn split_event(buffer: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    for i in 0..buffer.len() {
        let (len, matched) = match buffer[i] {
            b'\n' if buffer.get(i + 1) == Some(&b'\n') => (2, true),
            b'\r' if buffer.get(i + 1) == Some(&b'\r') => (2, true),
            b'\r' if buffer[i + 1..].starts_with(b"\n\r\n") => (4, true),
            _ => (0, false),
        };
        if matched {
            return Some((buffer[..i].to_vec(), buffer[i + len..].to_vec()));
        }
    }
    None
}

/// Parses one event block into an [`SseEvent`], or `None` when the block
/// carries no `data` field.
///
/// Invalid UTF-8 is dropped rather than raised: a malformed frame should not
/// abort an otherwise healthy stream. Comment lines (those beginning with
/// `:`, used by some providers as keep-alives) are skipped.
fn parse_block(block: &[u8]) -> Option<SseEvent> {
    let text = std::str::from_utf8(block).ok()?;

    let mut event = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            // A single leading space after the colon is part of the
            // delimiter, not the value.
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        match field {
            "event" => event = Some(value.to_owned()),
            "data" => data_lines.push(value),
            _ => {}
        }
    }

    if data_lines.is_empty() {
        return None;
    }

    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_single_event() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: hello\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "hello");
        assert_eq!(events[0].event, None);
    }

    #[test]
    fn decodes_multiple_events_in_one_chunk() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: one\n\ndata: two\n\ndata: three\n\n");
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].data, "three");
    }

    #[test]
    fn buffers_event_split_across_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: {\"text\":").is_empty());
        assert!(d.push(b" \"par").is_empty());
        let events = d.push(b"tial\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, r#"{"text": "partial"}"#);
    }

    #[test]
    fn splits_when_terminator_itself_is_split() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: hi\n").is_empty());
        let events = d.push(b"\ndata: there\n\n");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "hi");
        assert_eq!(events[1].data, "there");
    }

    #[test]
    fn parses_event_field() {
        let mut d = SseDecoder::new();
        let events = d.push(b"event: content_block_delta\ndata: {}\n\n");
        assert_eq!(events[0].event.as_deref(), Some("content_block_delta"));
        assert_eq!(events[0].data, "{}");
    }

    #[test]
    fn handles_crlf_line_endings() {
        let mut d = SseDecoder::new();
        let events = d.push(b"event: ping\r\ndata: {}\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event.as_deref(), Some("ping"));
        assert_eq!(events[0].data, "{}");
    }

    #[test]
    fn joins_multiple_data_lines() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: line one\ndata: line two\n\n");
        assert_eq!(events[0].data, "line one\nline two");
    }

    #[test]
    fn skips_comment_keepalives() {
        let mut d = SseDecoder::new();
        let events = d.push(b": keep-alive\n\ndata: real\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "real");
    }

    #[test]
    fn ignores_unknown_fields() {
        let mut d = SseDecoder::new();
        let events = d.push(b"id: 42\nretry: 100\ndata: payload\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "payload");
    }

    #[test]
    fn preserves_data_containing_colons() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: {\"url\": \"https://example.com\"}\n\n");
        assert_eq!(events[0].data, r#"{"url": "https://example.com"}"#);
    }

    #[test]
    fn detects_done_sentinel() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: [DONE]\n\n");
        assert!(events[0].is_done());
    }

    #[test]
    fn finish_emits_unterminated_tail() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: tail").is_empty());
        assert_eq!(d.finish().unwrap().data, "tail");
    }

    #[test]
    fn finish_on_empty_buffer_is_none() {
        let mut d = SseDecoder::new();
        d.push(b"data: x\n\n");
        assert!(d.finish().is_none());
    }

    #[test]
    fn byte_at_a_time_matches_whole_chunk() {
        let input = b"event: a\ndata: {\"n\":1}\n\ndata: [DONE]\n\n";
        let mut d = SseDecoder::new();
        let mut collected = Vec::new();
        for byte in input {
            collected.extend(d.push(&[*byte]));
        }
        assert_eq!(collected.len(), 2);
        assert_eq!(collected[0].data, r#"{"n":1}"#);
        assert!(collected[1].is_done());
    }
}
