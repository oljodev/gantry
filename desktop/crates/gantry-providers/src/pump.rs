//! The loop every client shares: SSE events in, [`StreamEvent`]s out, with the early-close
//! rule of docs/plan/02 §7 (flush what the parser has, drop the synthetic end, report
//! `StreamInterrupted` so the turn ends as failed with its partial text kept).

use futures_util::{Stream, StreamExt};

use crate::{error::ProviderError, provider::StreamEvent, sse::SseEvent};

/// One provider's wire-event parser.
pub trait StreamParser: Send + 'static {
    /// One SSE event → zero or more events; an error ends the stream.
    fn parse(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, ProviderError>;
    /// Closes the message when the connection ended without the provider's own end event.
    fn finish(&mut self) -> Vec<StreamEvent>;
    /// Whether the provider closed the message itself.
    fn ended(&self) -> bool;
}

/// Wraps an SSE event stream into a [`ChatStream`](crate::provider::ChatStream).
pub fn into_chat_stream<S, P>(sse: S, parser: P) -> crate::provider::ChatStream
where
    S: Stream<Item = Result<SseEvent, ProviderError>> + Send + 'static,
    P: StreamParser,
{
    let stream = futures_util::stream::unfold(
        (
            Box::pin(sse),
            parser,
            Vec::<StreamEvent>::new(),
            false,
            None::<ProviderError>,
        ),
        |(mut sse, mut parser, mut pending, mut done, mut pending_error)| async move {
            loop {
                if !pending.is_empty() {
                    let ev = pending.remove(0);
                    return Some((Ok(ev), (sse, parser, pending, done, pending_error)));
                }
                if let Some(err) = pending_error.take() {
                    return Some((Err(err), (sse, parser, pending, done, None)));
                }
                if done {
                    return None;
                }
                match sse.next().await {
                    None => {
                        done = true;
                        if !parser.ended() {
                            pending.extend(parser.finish());
                            pending.retain(|e| !matches!(e, StreamEvent::MessageEnd { .. }));
                            pending_error =
                                Some(ProviderError::interrupted("the stream ended early"));
                        }
                    }
                    Some(Err(e)) => {
                        done = true;
                        return Some((Err(e), (sse, parser, pending, done, pending_error)));
                    }
                    Some(Ok(ev)) => match parser.parse(&ev) {
                        Ok(events) => {
                            pending.extend(events);
                            if parser.ended() {
                                done = true;
                            }
                        }
                        Err(e) => {
                            done = true;
                            return Some((Err(e), (sse, parser, pending, done, pending_error)));
                        }
                    },
                }
            }
        },
    );
    Box::pin(stream)
}
