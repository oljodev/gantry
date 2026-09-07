//! ULID-backed id newtypes. One type per table so ids cannot be mixed up at compile time.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use ulid::Ulid;

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord,
            Serialize, Deserialize, specta::Type,
        )]
        #[serde(transparent)]
        #[specta(transparent)]
        pub struct $name(pub Ulid);

        impl $name {
            /// A fresh, time-ordered id.
            #[must_use]
            pub fn new() -> Self {
                Self(Ulid::new())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = ulid::DecodeError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ulid::from_str(s).map(Self)
            }
        }
    };
}

id_type!(/// A conversation. Owns its mode, connectors, grants, roots, instructions and artifacts.
    ChatId);
id_type!(/// One unit of agent work: a user message and everything until the assistant stops.
    TurnId);
id_type!(/// One message in a transcript.
    MessageId);
id_type!(/// One tool call within a turn.
    CallId);
id_type!(/// An installed, configured connector.
    InstanceId);
id_type!(/// One timestamped record of something that happened during a turn.
    EventId);
id_type!(/// A named group of chats with shared instructions, knowledge and defaults.
    ProjectId);
id_type!(/// A versioned piece of content shown beside the chat.
    ArtifactId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_text_and_json() {
        let id = ChatId::new();
        let text = id.to_string();
        assert_eq!(text.len(), 26);
        assert_eq!(ChatId::from_str(&text).unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<ChatId>(&json).unwrap(), id);
    }

    #[test]
    fn ids_carry_a_creation_timestamp() {
        let a = TurnId::new();
        let b = TurnId::new();
        assert!(a.0.timestamp_ms() <= b.0.timestamp_ms());
    }
}
