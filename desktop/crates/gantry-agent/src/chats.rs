//! The in-memory chat book of M1. M2 moves it behind `gantry-store` without changing the DTOs.

use std::{
    collections::HashMap,
    sync::{Mutex, RwLock},
};

use gantry_core::{
    ChatDetail, ChatId, ChatSummary, Feedback, GantryError, Message, Mode, ModelRef,
    ReasoningEffort, StopReason, TurnDto, TurnId, TurnStatus, Usage, now_ms,
};

#[derive(Debug, Clone)]
pub struct Chat {
    pub id: ChatId,
    pub title: String,
    pub pinned: bool,
    pub archived_at: Option<i64>,
    pub created_at: i64,
    pub last_message_at: i64,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    /// The frozen system prompt (10 §2).
    pub system_snapshot: String,
    pub system_snapshot_version: u32,
    pub turns: Vec<TurnRecord>,
}

#[derive(Debug, Clone)]
pub struct TurnRecord {
    pub id: TurnId,
    pub status: TurnStatus,
    pub model: ModelRef,
    pub user: Message,
    pub assistant: Option<Message>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
    pub feedback: Option<Feedback>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

/// What a runner needs to build a request: the frozen prompt and the transcript so far,
/// including the new user message.
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub turn_id: TurnId,
    pub chat_id: ChatId,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    pub system: String,
    pub messages: Vec<Message>,
}

/// Chat settings the composer and the sidebar can change; `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct ChatPatch {
    pub model: Option<ModelRef>,
    pub mode: Option<Mode>,
    pub guard: Option<bool>,
    pub effort: Option<ReasoningEffort>,
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
}

/// How a finished turn is recorded.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub status: TurnStatus,
    pub assistant: Option<Message>,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub struct ChatBook {
    chats: RwLock<HashMap<ChatId, Chat>>,
    /// Insertion order, so equal timestamps still list deterministically.
    order: Mutex<Vec<ChatId>>,
}

/// "New chat" until the first user message names it (M2 adds the model-written title).
pub fn title_from(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().take(6).collect();
    if words.is_empty() {
        return "New chat".to_owned();
    }
    let mut title = words.join(" ");
    if text.split_whitespace().count() > 6 {
        title.push('…');
    }
    title
}

impl ChatBook {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(
        &self,
        model: ModelRef,
        mode: Mode,
        guard: bool,
        effort: ReasoningEffort,
        system_snapshot: String,
        system_snapshot_version: u32,
    ) -> ChatSummary {
        let now = now_ms();
        let chat = Chat {
            id: ChatId::new(),
            title: "New chat".to_owned(),
            pinned: false,
            archived_at: None,
            created_at: now,
            last_message_at: now,
            model,
            mode,
            guard,
            effort,
            system_snapshot,
            system_snapshot_version,
            turns: Vec::new(),
        };
        let summary = summary(&chat, None);
        self.order
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(chat.id);
        self.chats
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(chat.id, chat);
        summary
    }

    /// Most recent first.
    #[must_use]
    pub fn list(&self) -> Vec<ChatSummary> {
        let chats = self.chats.read().unwrap_or_else(|e| e.into_inner());
        let mut rows: Vec<ChatSummary> =
            chats.values().map(|c| summary(c, active_turn(c))).collect();
        rows.sort_by(|a, b| {
            b.last_message_at
                .cmp(&a.last_message_at)
                .then(b.id.cmp(&a.id))
        });
        rows
    }

    #[must_use]
    pub fn get(&self, id: ChatId) -> Option<ChatDetail> {
        let chats = self.chats.read().unwrap_or_else(|e| e.into_inner());
        chats.get(&id).map(detail)
    }

    #[must_use]
    pub fn contains(&self, id: ChatId) -> bool {
        self.chats
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&id)
    }

    /// Records the user message, opens a running turn and returns what the runner needs.
    /// Fails when the chat is unknown or already has a running turn.
    pub fn begin_turn(&self, chat_id: ChatId, user: Message) -> Result<TurnInput, GantryError> {
        let mut chats = self.chats.write().unwrap_or_else(|e| e.into_inner());
        let chat = chats
            .get_mut(&chat_id)
            .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
        if chat.turns.iter().any(|t| t.status == TurnStatus::Running) {
            return Err(GantryError::invalid("this chat already has a turn running"));
        }
        if chat.turns.is_empty() {
            chat.title = title_from(&user.text());
        }
        let now = now_ms();
        chat.last_message_at = now;
        let turn = TurnRecord {
            id: TurnId::new(),
            status: TurnStatus::Running,
            model: chat.model.clone(),
            user: user.clone(),
            assistant: None,
            usage: None,
            stop_reason: None,
            error: None,
            feedback: None,
            started_at: now,
            ended_at: None,
        };
        let turn_id = turn.id;
        chat.turns.push(turn);
        let messages = transcript(chat);
        Ok(TurnInput {
            turn_id,
            chat_id,
            model: chat.model.clone(),
            mode: chat.mode,
            guard: chat.guard,
            effort: chat.effort,
            system: chat.system_snapshot.clone(),
            messages,
        })
    }

    pub fn finish_turn(&self, chat_id: ChatId, turn_id: TurnId, outcome: TurnOutcome) {
        let mut chats = self.chats.write().unwrap_or_else(|e| e.into_inner());
        let Some(chat) = chats.get_mut(&chat_id) else {
            return;
        };
        let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) else {
            return;
        };
        let now = now_ms();
        turn.status = outcome.status;
        turn.assistant = outcome.assistant;
        turn.usage = outcome.usage;
        turn.stop_reason = outcome.stop_reason;
        turn.error = outcome.error;
        turn.ended_at = Some(now);
        chat.last_message_at = now;
    }

    /// Applies a patch between turns.
    pub fn update(&self, chat_id: ChatId, patch: ChatPatch) -> Result<ChatSummary, GantryError> {
        let mut chats = self.chats.write().unwrap_or_else(|e| e.into_inner());
        let chat = chats
            .get_mut(&chat_id)
            .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
        if let Some(m) = patch.model {
            chat.model = m;
        }
        if let Some(m) = patch.mode {
            chat.mode = m;
        }
        if let Some(g) = patch.guard {
            chat.guard = g;
        }
        if let Some(e) = patch.effort {
            chat.effort = e;
        }
        if let Some(t) = patch.title {
            chat.title = t;
        }
        if let Some(p) = patch.pinned {
            chat.pinned = p;
        }
        if let Some(a) = patch.archived {
            chat.archived_at = if a { Some(now_ms()) } else { None };
        }
        Ok(summary(chat, active_turn(chat)))
    }

    /// Records the user's verdict on a finished turn; `None` clears it.
    pub fn rate_turn(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        feedback: Option<Feedback>,
    ) -> Result<(), GantryError> {
        let mut chats = self.chats.write().unwrap_or_else(|e| e.into_inner());
        let chat = chats
            .get_mut(&chat_id)
            .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
        let turn = chat
            .turns
            .iter_mut()
            .find(|t| t.id == turn_id)
            .ok_or_else(|| GantryError::not_found(format!("turn {turn_id}")))?;
        turn.feedback = feedback;
        Ok(())
    }

    /// Removes the chat's last turn so it can be re-run, returning its user message. Only the
    /// last turn can be retried, and not while it runs.
    pub fn take_last_turn(&self, chat_id: ChatId, turn_id: TurnId) -> Result<Message, GantryError> {
        let mut chats = self.chats.write().unwrap_or_else(|e| e.into_inner());
        let chat = chats
            .get_mut(&chat_id)
            .ok_or_else(|| GantryError::not_found(format!("chat {chat_id}")))?;
        match chat.turns.last() {
            Some(t) if t.id == turn_id && t.status != TurnStatus::Running => {}
            Some(t) if t.id == turn_id => {
                return Err(GantryError::invalid("the turn is still running"));
            }
            _ => return Err(GantryError::invalid("only the last turn can be retried")),
        }
        let turn = chat.turns.pop().expect("checked above");
        Ok(turn.user)
    }

    pub fn delete(&self, chat_id: ChatId) -> bool {
        self.order
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|id| *id != chat_id);
        self.chats
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&chat_id)
            .is_some()
    }
}

fn active_turn(chat: &Chat) -> Option<TurnId> {
    chat.turns
        .iter()
        .find(|t| t.status == TurnStatus::Running)
        .map(|t| t.id)
}

fn summary(chat: &Chat, active: Option<TurnId>) -> ChatSummary {
    ChatSummary {
        id: chat.id,
        title: chat.title.clone(),
        pinned: chat.pinned,
        archived: chat.archived_at.is_some(),
        project_id: None,
        created_at: chat.created_at,
        last_message_at: chat.last_message_at,
        active_turn: active,
    }
}

fn detail(chat: &Chat) -> ChatDetail {
    ChatDetail {
        id: chat.id,
        title: chat.title.clone(),
        pinned: chat.pinned,
        archived: chat.archived_at.is_some(),
        project_id: None,
        created_at: chat.created_at,
        last_message_at: chat.last_message_at,
        model: chat.model.clone(),
        mode: chat.mode,
        guard: chat.guard,
        effort: chat.effort,
        active_turn: active_turn(chat),
        turns: chat
            .turns
            .iter()
            .map(|t| TurnDto {
                id: t.id,
                status: t.status,
                model: t.model.clone(),
                user: t.user.clone(),
                assistant: t.assistant.clone(),
                usage: t.usage,
                stop_reason: t.stop_reason.clone(),
                error: t.error.clone(),
                feedback: t.feedback,
                started_at: t.started_at,
                ended_at: t.ended_at,
            })
            .collect(),
    }
}

/// User and assistant messages of every turn, in order; failed turns contribute their user
/// message and whatever partial answer they kept.
fn transcript(chat: &Chat) -> Vec<Message> {
    let mut out = Vec::with_capacity(chat.turns.len() * 2);
    for t in &chat.turns {
        out.push(t.user.clone());
        if let Some(a) = &t.assistant
            && !a.parts.is_empty()
        {
            out.push(a.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book_with_chat() -> (ChatBook, ChatId) {
        let book = ChatBook::new();
        let c = book.create(
            ModelRef::default_model(),
            Mode::AutoEdit,
            true,
            ReasoningEffort::Off,
            "sys".into(),
            1,
        );
        (book, c.id)
    }

    #[test]
    fn titles_come_from_the_first_message() {
        assert_eq!(
            title_from("Fix the failing auth tests please now"),
            "Fix the failing auth tests please…"
        );
        assert_eq!(title_from("hi"), "hi");
        assert_eq!(title_from("   "), "New chat");
    }

    #[test]
    fn one_running_turn_at_a_time_and_the_transcript_grows() {
        let (book, id) = book_with_chat();
        let input = book
            .begin_turn(id, Message::user_text("Hello there friend"))
            .unwrap();
        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.system, "sys");
        assert!(book.begin_turn(id, Message::user_text("again")).is_err());
        assert_eq!(book.get(id).unwrap().title, "Hello there friend");
        assert_eq!(book.list()[0].active_turn, Some(input.turn_id));

        let mut assistant = Message::user_text("Hi!");
        assistant.role = gantry_core::Role::Assistant;
        book.finish_turn(
            id,
            input.turn_id,
            TurnOutcome {
                status: TurnStatus::Completed,
                assistant: Some(assistant),
                usage: None,
                stop_reason: Some(StopReason::EndTurn),
                error: None,
            },
        );
        let next = book.begin_turn(id, Message::user_text("more")).unwrap();
        assert_eq!(next.messages.len(), 3);
        assert_eq!(book.get(id).unwrap().turns[0].status, TurnStatus::Completed);
    }

    #[test]
    fn only_the_last_finished_turn_can_be_retried() {
        let (book, id) = book_with_chat();
        let first = book.begin_turn(id, Message::user_text("one")).unwrap();
        assert!(book.take_last_turn(id, first.turn_id).is_err(), "running");
        book.finish_turn(
            id,
            first.turn_id,
            TurnOutcome {
                status: TurnStatus::Failed,
                assistant: None,
                usage: None,
                stop_reason: None,
                error: Some("x".into()),
            },
        );
        book.rate_turn(id, first.turn_id, Some(Feedback::Bad))
            .unwrap();
        assert_eq!(book.get(id).unwrap().turns[0].feedback, Some(Feedback::Bad));
        let user = book.take_last_turn(id, first.turn_id).unwrap();
        assert_eq!(user.text(), "one");
        assert!(book.get(id).unwrap().turns.is_empty());
        assert!(book.take_last_turn(id, first.turn_id).is_err(), "gone");
    }
}
