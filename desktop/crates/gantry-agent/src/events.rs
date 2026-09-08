//! The event pipeline (docs/plan/05 §3): sinks receive batches; the batcher merges and flushes
//! every 16 ms, 64 events or 64 KB, whichever comes first.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use gantry_core::{AgentEvent, AgentEventBatch, AgentEventKind, TurnId, now_ms};

pub trait EventSink: Send + Sync {
    fn emit(&self, batch: AgentEventBatch);
}

/// Delivers to every subscribed sink; sinks can be added while a turn runs.
#[derive(Default)]
pub struct FanoutSink {
    sinks: Mutex<Vec<Arc<dyn EventSink>>>,
}

impl FanoutSink {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&self, sink: Arc<dyn EventSink>) {
        self.sinks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(sink);
    }
}

impl EventSink for FanoutSink {
    fn emit(&self, batch: AgentEventBatch) {
        let sinks = self.sinks.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for s in sinks {
            s.emit(batch.clone());
        }
    }
}

pub const FLUSH_INTERVAL: Duration = Duration::from_millis(16);
pub const FLUSH_EVENTS: usize = 64;
pub const FLUSH_BYTES: usize = 64 * 1024;

pub struct Batcher {
    turn_id: TurnId,
    sink: Arc<dyn EventSink>,
    seq: AtomicU32,
    queue: Mutex<(Vec<AgentEvent>, usize)>,
    closed: AtomicBool,
}

impl Batcher {
    /// Starts the flush timer on `runtime`. The caller may be any thread: commands run on the
    /// UI thread, outside every runtime.
    #[must_use]
    pub fn start(
        turn_id: TurnId,
        sink: Arc<dyn EventSink>,
        runtime: &tokio::runtime::Handle,
    ) -> Arc<Batcher> {
        let batcher = Arc::new(Batcher {
            turn_id,
            sink,
            seq: AtomicU32::new(0),
            queue: Mutex::new((Vec::new(), 0)),
            closed: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&batcher);
        runtime.spawn(async move {
            loop {
                tokio::time::sleep(FLUSH_INTERVAL).await;
                match weak.upgrade() {
                    Some(b) if !b.closed.load(Ordering::SeqCst) => b.flush(),
                    _ => break,
                }
            }
        });
        batcher
    }

    /// The last sequence number handed out.
    #[must_use]
    pub fn last_seq(&self) -> u32 {
        self.seq.load(Ordering::SeqCst)
    }

    /// Queues an event with the next sequence number and returns that number.
    pub fn push(&self, event: AgentEventKind) -> u32 {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let size = approx_size(&event);
        let flush_now = {
            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            let merged = merge_text_delta(&mut q.0, &event);
            if !merged {
                q.0.push(AgentEvent {
                    seq,
                    ts: now_ms(),
                    turn_id: self.turn_id,
                    event,
                });
            } else if let Some(last) = q.0.last_mut() {
                last.seq = seq;
            }
            q.1 += size;
            q.0.len() >= FLUSH_EVENTS || q.1 >= FLUSH_BYTES
        };
        if flush_now {
            self.flush();
        }
        seq
    }

    pub fn flush(&self) {
        let events = {
            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            q.1 = 0;
            std::mem::take(&mut q.0)
        };
        if !events.is_empty() {
            self.sink.emit(AgentEventBatch {
                turn_id: self.turn_id,
                events,
            });
        }
    }

    /// Flushes what is queued and stops the timer.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.flush();
    }
}

/// Appends a text or thinking delta to the previous queued delta of the same block.
fn merge_text_delta(queue: &mut [AgentEvent], event: &AgentEventKind) -> bool {
    let Some(last) = queue.last_mut() else {
        return false;
    };
    match (&mut last.event, event) {
        (
            AgentEventKind::TextDelta {
                message_id: a,
                block: ab,
                text,
            },
            AgentEventKind::TextDelta {
                message_id: b,
                block: bb,
                text: more,
            },
        )
        | (
            AgentEventKind::ThinkingDelta {
                message_id: a,
                block: ab,
                text,
            },
            AgentEventKind::ThinkingDelta {
                message_id: b,
                block: bb,
                text: more,
            },
        ) if a == b && ab == bb => {
            text.push_str(more);
            true
        }
        // A command spewing output produces one event per read; the interface wants the text,
        // not the reads (05 §8).
        (
            AgentEventKind::ToolCallOutput {
                call_id: a,
                stream: sa,
                chunk,
            },
            AgentEventKind::ToolCallOutput {
                call_id: b,
                stream: sb,
                chunk: more,
            },
        ) if a == b && sa == sb => {
            chunk.push_str(more);
            true
        }
        _ => false,
    }
}

fn approx_size(event: &AgentEventKind) -> usize {
    match event {
        AgentEventKind::TextDelta { text, .. } | AgentEventKind::ThinkingDelta { text, .. } => {
            text.len() + 64
        }
        AgentEventKind::BlockDone { .. } | AgentEventKind::TurnSnapshot { .. } => 1024,
        _ => 128,
    }
}

#[cfg(test)]
mod tests {
    use gantry_core::MessageId;

    use super::*;

    #[derive(Default)]
    pub struct Collect(pub Mutex<Vec<AgentEventBatch>>);

    impl EventSink for Collect {
        fn emit(&self, batch: AgentEventBatch) {
            self.0.lock().unwrap().push(batch);
        }
    }

    #[tokio::test]
    async fn merges_adjacent_deltas_and_flushes_on_close() {
        let sink = Arc::new(Collect::default());
        let b = Batcher::start(
            TurnId::new(),
            sink.clone(),
            &tokio::runtime::Handle::current(),
        );
        let m = MessageId::new();
        for word in ["a", "b", "c"] {
            b.push(AgentEventKind::TextDelta {
                message_id: m,
                block: 0,
                text: word.into(),
            });
        }
        b.push(AgentEventKind::ThinkingDelta {
            message_id: m,
            block: 1,
            text: "t".into(),
        });
        b.close();
        let batches = sink.0.lock().unwrap();
        assert_eq!(batches.len(), 1);
        let events = &batches[0].events;
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0].event, AgentEventKind::TextDelta { text, .. } if text == "abc")
        );
        assert_eq!(events[0].seq, 3, "a merged delta carries the latest seq");
        assert_eq!(events[1].seq, 4);
    }

    #[tokio::test]
    async fn merges_a_command_spewing_output_into_one_event_per_stream() {
        let sink = Arc::new(Collect::default());
        let b = Batcher::start(
            TurnId::new(),
            sink.clone(),
            &tokio::runtime::Handle::current(),
        );
        let call = gantry_core::CallId::new();
        for line in ["one\n", "two\n", "three\n"] {
            b.push(AgentEventKind::ToolCallOutput {
                call_id: call.clone(),
                stream: gantry_core::ToolStream::Stdout,
                chunk: line.into(),
            });
        }
        // A different stream is a different event: stdout and stderr must stay distinguishable.
        b.push(AgentEventKind::ToolCallOutput {
            call_id: call.clone(),
            stream: gantry_core::ToolStream::Stderr,
            chunk: "warning\n".into(),
        });
        b.close();
        let batches = sink.0.lock().unwrap();
        let events = &batches[0].events;
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0].event, AgentEventKind::ToolCallOutput { chunk, .. }
                if chunk == "one\ntwo\nthree\n")
        );
        assert!(
            matches!(&events[1].event, AgentEventKind::ToolCallOutput { stream, .. }
                if *stream == gantry_core::ToolStream::Stderr)
        );
    }

    #[tokio::test]
    async fn flushes_at_the_event_cap() {
        let sink = Arc::new(Collect::default());
        let b = Batcher::start(
            TurnId::new(),
            sink.clone(),
            &tokio::runtime::Handle::current(),
        );
        for i in 0..FLUSH_EVENTS {
            b.push(AgentEventKind::ProviderNotice {
                kind: "n".into(),
                detail: i.to_string(),
            });
        }
        assert_eq!(sink.0.lock().unwrap().len(), 1);
        b.close();
    }
}
