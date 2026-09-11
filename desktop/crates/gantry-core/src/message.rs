//! The provider-neutral transcript model (docs/plan/02 §2).
//!
//! Every provider client maps its wire format onto these types and back, so the agent loop,
//! the store and the UI never see a vendor shape.

use serde::{Deserialize, Serialize};

use crate::ids::{CallId, MessageId};

/// Who wrote a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
    System,
}

/// Which client implementation produced or must replay a part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAiResponses,
    OpenAiChat,
    Gemini,
}

/// One message in a transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Message {
    pub id: MessageId,
    pub role: Role,
    pub parts: Vec<ContentPart>,
    /// The provider that produced an assistant message; `None` for user and system messages.
    pub origin: Option<ProviderKind>,
    /// Milliseconds since the Unix epoch.
    #[specta(type = specta_typescript::Number)]
    pub created_at: i64,
}

impl Message {
    /// A user message holding one text part.
    #[must_use]
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            id: MessageId::new(),
            role: Role::User,
            parts: vec![ContentPart::Text { text: text.into() }],
            origin: None,
            created_at: crate::time::now_ms(),
        }
    }

    /// The concatenated text parts, ignoring everything else.
    #[must_use]
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

/// Where the bytes of an image or document live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MediaSource {
    /// Inline, base64-encoded.
    Base64 { data: String },
    /// A content-addressed blob in the store.
    Blob { hash: String },
}

/// One block of a message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        source: MediaSource,
        mime: String,
    },
    Document {
        source: MediaSource,
        mime: String,
        name: String,
    },
    /// Sound the model produced: a voice reading a passage, or music. What it says, where it
    /// says anything, is the text part beside it — a model that talks writes the same words as
    /// a transcript, and one copy of them is enough.
    Audio {
        source: MediaSource,
        mime: String,
    },
    /// A clip the model rendered.
    Video {
        source: MediaSource,
        mime: String,
    },
    ToolCall {
        id: CallId,
        name: String,
        #[specta(type = specta_typescript::Unknown)]
        args: serde_json::Value,
        /// A provider token bound to the call that must be echoed on replay (Gemini thought
        /// signatures). Opaque; only the provider that produced it reads it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        signature: Option<String>,
    },
    ToolResult {
        call_id: CallId,
        content: Vec<ResultPart>,
        is_error: bool,
    },
    /// Model reasoning. Opaque: replayed only to the provider that produced it.
    Thinking {
        text: String,
        signature: Option<String>,
        provider: ProviderKind,
        /// The provider's own id for the block when replay needs it (OpenAI Responses reasoning
        /// items carry an `rs_…` id next to their encrypted content).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[specta(optional)]
        item_id: Option<String>,
    },
    /// Server-tool blocks, citations and other vendor content persisted and replayed verbatim.
    ProviderOpaque {
        provider: ProviderKind,
        block_kind: String,
        #[specta(type = specta_typescript::Unknown)]
        json: serde_json::Value,
    },
    /// An instruction change mid-chat (role `System`).
    SystemNote {
        text: String,
    },
    /// Connectors attached or detached mid-chat (role `System`).
    ToolSetChange {
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// Everything before this point, summarized (docs/plan/02 §6, role `System`).
    ///
    /// The messages it stands for are still in the database and still drawn in the chat — the
    /// transcript is append-only and nothing is ever edited out of it (02 §6). Only the request
    /// skips them, which is why this is a marker and not a deletion.
    Compacted {
        summary: String,
        /// The last message the summary covers. Projection drops everything up to and
        /// including it, wherever this marker itself happens to sit.
        up_to: MessageId,
        /// How many messages that was, for the row the reader sees.
        replaced: u32,
        /// Artifacts made in the summarized span (13 §7). They outlive the messages that made
        /// them, so the model is told their ids and can read one back when it needs it.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        artifacts: Vec<String>,
    },
}

impl ContentPart {
    /// What a part of a `System` message says to the model (10 §4). A tool-set change is the
    /// one part that is structured rather than prose, so the sentence is written here once
    /// instead of in each provider's projection.
    #[must_use]
    pub fn system_text(&self) -> Option<String> {
        match self {
            ContentPart::SystemNote { text } => Some(text.clone()),
            ContentPart::ToolSetChange { added, removed } => {
                let mut lines = Vec::new();
                if !added.is_empty() {
                    lines.push(format!(
                        "Connectors now available in this chat: {}. Their tools are in your tool \
                         list from your next call.",
                        added.join(", ")
                    ));
                }
                if !removed.is_empty() {
                    lines.push(format!(
                        "No longer available in this chat: {}.",
                        removed.join(", ")
                    ));
                }
                (!lines.is_empty()).then(|| lines.join(" "))
            }
            ContentPart::Compacted {
                summary,
                replaced,
                artifacts,
                ..
            } => {
                let mut text = format!(
                    "The earlier part of this conversation ({replaced} messages) is no longer \
                     in your context. Here is what happened in it:\n\n{summary}"
                );
                if !artifacts.is_empty() {
                    text.push_str(&format!(
                        "\n\nArtifacts made in that part, still readable with \
                         `gantry__read_artifact`: {}.",
                        artifacts.join(", ")
                    ));
                }
                text.push_str(
                    "\n\nEverything after this point is the conversation verbatim. If you need \
                     a detail from before it that the summary does not have, say so rather than \
                     inventing one.",
                );
                Some(text)
            }
            _ => None,
        }
    }
}

/// One piece of a tool result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResultPart {
    Text {
        text: String,
    },
    Json {
        #[specta(type = specta_typescript::Unknown)]
        json: serde_json::Value,
    },
    /// Base64-encoded image bytes.
    Image {
        data: String,
        mime: String,
    },
    /// A large in-memory object parked by a native connector (01 §7).
    Resource {
        handle: String,
        summary: String,
    },
}

/// Why the model stopped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal { category: Option<String> },
    ContentFilter,
    PauseTurn,
    Cancelled,
    Other { reason: String },
}

/// Token accounting for one request, in the provider's own count.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, specta::Type)]
pub struct Usage {
    #[specta(type = specta_typescript::Number)]
    pub input: u64,
    #[specta(type = specta_typescript::Number)]
    pub output: u64,
    #[specta(type = specta_typescript::Number)]
    pub cache_read: u64,
    #[specta(type = specta_typescript::Number)]
    pub cache_write: u64,
    #[specta(type = specta_typescript::Number)]
    pub reasoning: u64,
    /// What the provider says the request cost, in US dollars, when it says so (OpenRouter does).
    pub cost_usd: Option<f64>,
}

impl Usage {
    /// Field-wise sum; costs add when both sides know theirs.
    #[must_use]
    pub fn plus(self, other: Usage) -> Usage {
        Usage {
            input: self.input + other.input,
            output: self.output + other.output,
            cache_read: self.cache_read + other.cache_read,
            cache_write: self.cache_write + other.cache_write,
            reasoning: self.reasoning + other.reasoning,
            cost_usd: match (self.cost_usd, other.cost_usd) {
                (Some(a), Some(b)) => Some(a + b),
                (a, b) => a.or(b),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_serialise_with_a_kind_tag() {
        let part = ContentPart::Thinking {
            item_id: None,
            text: "hm".into(),
            signature: None,
            provider: ProviderKind::OpenAiChat,
        };
        let json = serde_json::to_value(&part).unwrap();
        assert_eq!(json["kind"], "thinking");
        assert_eq!(json["provider"], "open_ai_chat");
        let back: ContentPart = serde_json::from_value(json).unwrap();
        assert_eq!(back, part);
    }

    #[test]
    fn message_text_joins_text_parts_only() {
        let mut m = Message::user_text("a");
        m.parts.push(ContentPart::SystemNote { text: "x".into() });
        m.parts.push(ContentPart::Text { text: "b".into() });
        assert_eq!(m.text(), "ab");
    }
}
