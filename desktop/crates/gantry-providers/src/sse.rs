//! A Server-Sent Events decoder for streaming responses. Push bytes in as they arrive, get
//! complete events out. Handles comment lines (OpenRouter sends `: OPENROUTER PROCESSING`
//! keep-alives), multi-line `data:`, CRLF, and events split across reads.

use std::time::Duration;

use futures_util::{Stream, StreamExt};

use crate::error::ProviderError;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseDecoder {
    buf: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes and returns every event completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\n', '\r']);
            if let Some(ev) = self.line(line) {
                out.push(ev);
            }
        }
        out
    }

    /// Flushes a trailing event that had no terminating blank line.
    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest).to_string();
            if let Some(ev) = self.line(line.trim_end_matches(['\n', '\r'])) {
                return Some(ev);
            }
        }
        self.dispatch()
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_owned()),
            "event" => self.event = Some(value.to_owned()),
            _ => {}
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        if self.data.is_empty() && self.event.is_none() {
            return None;
        }
        let data = std::mem::take(&mut self.data).join("\n");
        Some(SseEvent {
            event: self.event.take(),
            data,
        })
    }
}

/// Turns a byte stream into SSE events, failing when nothing arrives for `first` (before the
/// first chunk) or `idle` (between later chunks).
pub fn sse_stream<S, B, E>(
    bytes: S,
    first: Duration,
    idle: Duration,
) -> impl Stream<Item = Result<SseEvent, ProviderError>> + Send
where
    S: Stream<Item = Result<B, E>> + Send + Unpin + 'static,
    B: AsRef<[u8]> + Send,
    E: Into<ProviderError> + Send,
{
    futures_util::stream::unfold(
        (
            bytes,
            SseDecoder::new(),
            Vec::<SseEvent>::new(),
            true,
            false,
        ),
        move |(mut bytes, mut decoder, mut pending, is_first, done)| async move {
            loop {
                if !pending.is_empty() {
                    let ev = pending.remove(0);
                    return Some((Ok(ev), (bytes, decoder, pending, false, done)));
                }
                if done {
                    return None;
                }
                let wait = if is_first { first } else { idle };
                match tokio::time::timeout(wait, bytes.next()).await {
                    Err(_) => {
                        let what = if is_first {
                            "the first token"
                        } else {
                            "the next event"
                        };
                        let err = ProviderError::interrupted(format!(
                            "timed out waiting for {what} ({} s)",
                            wait.as_secs()
                        ));
                        return Some((Err(err), (bytes, decoder, pending, false, true)));
                    }
                    Ok(None) => {
                        if let Some(ev) = decoder.finish() {
                            pending.push(ev);
                        }
                        if pending.is_empty() {
                            return None;
                        }
                        // Drain what finish() produced, then end.
                        let ev = pending.remove(0);
                        return Some((Ok(ev), (bytes, decoder, pending, false, true)));
                    }
                    Ok(Some(Err(e))) => {
                        return Some((Err(e.into()), (bytes, decoder, pending, false, true)));
                    }
                    Ok(Some(Ok(chunk))) => {
                        pending.extend(decoder.push(chunk.as_ref()));
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_events_split_across_pushes_and_ignores_comments() {
        let text = ": OPENROUTER PROCESSING\n\ndata: {\"a\":1}\n\ndata: line1\ndata: line2\n\r\ndata: [DONE]\n\n";
        let mut d = SseDecoder::new();
        let mut events = Vec::new();
        for chunk in text.as_bytes().chunks(5) {
            events.extend(d.push(chunk));
        }
        assert_eq!(
            events.iter().map(|e| e.data.as_str()).collect::<Vec<_>>(),
            vec!["{\"a\":1}", "line1\nline2", "[DONE]"]
        );
    }

    #[test]
    fn finish_flushes_a_trailing_event_without_blank_line() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"data: tail").is_empty());
        assert_eq!(d.finish().unwrap().data, "tail");
    }

    #[test]
    fn a_field_without_a_space_after_the_colon_still_parses() {
        let mut d = SseDecoder::new();
        let ev = d.push(b"event:ping\ndata:x\n\n");
        assert_eq!(ev[0].event.as_deref(), Some("ping"));
        assert_eq!(ev[0].data, "x");
    }
}
