//! The provider-neutral transcript model (docs/plan/02 §2).
//!
//! Every provider client maps its wire format onto these types and back, so the agent loop,
//! the store and the UI never see a vendor shape. M1 produces `Text`, `Thinking` and
//! `SystemNote`; the other parts exist so later milestones add no breaking change.

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
    ToolCall {
        id: CallId,
        name: String,
        #[specta(type = specta_typescript::Unknown)]
        args: serde_json::Value,
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
