//! The chat book: chats, turns and the transcript, on the store (docs/plan/06 §3). The DTOs
//! are the ones M1 defined; only the backing changed.

use std::sync::Arc;

use gantry_core::{
    ChatDetail, ChatGrant, ChatId, ChatSummary, ContentPart, Feedback, GantryError, MediaSource,
    Message, MessageId, Mode, ModelRef, ReasoningEffort, Role, StopReason, TurnDto, TurnId,
    TurnStatus, Usage, now_ms,
};
use gantry_store::{
    BlobStore, Store,
    repos::{
        artifacts, blobs,
        chats::{self, ChatRecord},
        grants,
        messages::{self, AttachmentRecord, MessageRecord},
        tool_calls,
        turns::{self, TurnRecord},
    },
};

/// What a runner needs to build a request: the frozen prompt and the transcript so far,
/// including the new user message, with media inlined for the provider.
#[derive(Debug, Clone)]
pub struct TurnInput {
    pub turn_id: TurnId,
    pub chat_id: ChatId,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    pub web_search: bool,
    pub system: String,
    pub messages: Vec<Message>,
    /// Whether this is the chat's first turn (the title generator runs after it).
    pub first_turn: bool,
    /// The model of the chat's previous turn, so the runner can say when thinking from another
    /// model is left behind (02 §5).
    pub previous_model: Option<ModelRef>,
}

/// Chat settings the composer and the sidebar can change; `None` leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct ChatPatch {
    pub model: Option<ModelRef>,
    pub mode: Option<Mode>,
    pub guard: Option<bool>,
    pub effort: Option<ReasoningEffort>,
    pub web_search: Option<bool>,
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub archived: Option<bool>,
    pub instructions: Option<String>,
}

/// How a finished turn is recorded; its messages were appended as they completed.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    pub status: TurnStatus,
    pub usage: Option<Usage>,
    pub stop_reason: Option<StopReason>,
    pub error: Option<String>,
    pub tool_call_count: u32,
}

/// An attachment already written to the blob store, to be recorded with a user message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAttachment {
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub blob_hash: String,
    pub extracted_text: Option<String>,
}

pub struct ChatBook {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
}

impl std::fmt::Debug for ChatBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatBook").finish()
    }
}

/// "New chat" until the first user message names it; the title generator replaces that.
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

fn store_err(e: gantry_store::StoreError) -> GantryError {
    GantryError::Store(e.to_string())
}

impl ChatBook {
    #[must_use]
    pub fn new(store: Arc<Store>, blobs: Arc<BlobStore>) -> Self {
        Self { store, blobs }
    }

    #[must_use]
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    #[must_use]
    pub fn blobs(&self) -> &Arc<BlobStore> {
        &self.blobs
    }

    pub fn create(
        &self,
        model: ModelRef,
        mode: Mode,
        guard: bool,
        effort: ReasoningEffort,
        system_snapshot: String,
        system_snapshot_version: u32,
    ) -> Result<ChatSummary, GantryError> {
        let now = now_ms();
        let chat = ChatRecord {
            id: ChatId::new(),
            project_id: None,
            title: "New chat".to_owned(),
            title_source: "auto".into(),
            pinned: false,
            mode,
            guard,
            model,
            effort,
            web_search: false,
            instructions: String::new(),
            system_snapshot,
            system_snapshot_version,
            created_at: now,
            updated_at: now,
            last_message_at: now,
            archived_at: None,
        };
        let summary = summary(&chat, None);
        self.store
            .write_blocking(move |conn| chats::insert(conn, &chat))
            .map_err(store_err)?;
        Ok(summary)
    }

    /// Most recent first, archived chats included (the sidebar groups them).
    pub fn list(&self) -> Result<Vec<ChatSummary>, GantryError> {
        self.store
            .read(|conn| {
                let running: std::collections::HashMap<ChatId, TurnId> =
                    turns::chats_with_running_turns(conn)?.into_iter().collect();
                Ok(chats::list(conn)?
                    .iter()
                    .map(|c| summary(c, running.get(&c.id).copied()))
                    .collect())
            })
            .map_err(store_err)
    }

    pub fn get(&self, id: ChatId) -> Result<Option<ChatDetail>, GantryError> {
        self.store
            .read(|conn| {
                let Some(chat) = chats::get(conn, id)? else {
                    return Ok(None);
                };
                let turns = turns::list_for_chat(conn, id)?;
                let messages = messages::list_for_chat(conn, id)?;
                let calls = tool_calls::list_for_chat(conn, id)?;
                let notices = gantry_store::repos::events::list_notices_for_chat(conn, id)?;
                Ok(Some(detail(&chat, &turns, &messages, &calls, &notices)))
            })
            .map_err(store_err)
    }

    pub fn contains(&self, id: ChatId) -> Result<bool, GantryError> {
        self.store
            .read(|conn| Ok(chats::get(conn, id)?.is_some()))
            .map_err(store_err)
    }

    /// Records the user message and its attachments, opens a running turn and returns what the
    /// runner needs. Fails when the chat is unknown or already has a running turn.
    pub fn begin_turn(
        &self,
        chat_id: ChatId,
        user: Message,
        attachments: Vec<NewAttachment>,
    ) -> Result<TurnInput, GantryError> {
        let blobs = self.blobs.clone();
        self.store
            .write_blocking(move |conn| {
                let mut chat = chats::get(conn, chat_id)?.ok_or_else(|| {
                    gantry_store::StoreError::Other(format!("chat {chat_id} not found"))
                })?;
                if turns::running_for_chat(conn, chat_id)?.is_some() {
                    return Err(gantry_store::StoreError::Other(
                        "this chat already has a turn running".into(),
                    ));
                }
                let seq = turns::next_seq(conn, chat_id)?;
                let first_turn = seq == 1;
                let previous_model = turns::last_for_chat(conn, chat_id)?.map(|t| t.model);
                if first_turn && chat.title_source == "auto" {
                    chat.title = title_from(&user.text());
                }
                let now = now_ms();
                chat.last_message_at = now;
                chats::update(conn, &chat)?;
                let turn = TurnRecord {
                    id: TurnId::new(),
                    chat_id,
                    seq,
                    status: TurnStatus::Running,
                    model: chat.model.clone(),
                    started_at: now,
                    ended_at: None,
                    usage: None,
                    stop_reason: None,
                    error: None,
                    feedback: None,
                    tool_call_count: 0,
                };
                turns::insert(conn, &turn)?;
                messages::insert(
                    conn,
                    &MessageRecord {
                        message: user.clone(),
                        chat_id,
                        turn_id: Some(turn.id),
                        seq: messages::next_seq(conn, chat_id)?,
                        stop_reason: None,
                        usage: None,
                    },
                )?;
                for a in &attachments {
                    blobs::add_ref(conn, &a.blob_hash, a.size, Some(&a.mime))?;
                    messages::insert_attachment(
                        conn,
                        &AttachmentRecord {
                            id: gantry_core::EventId::new().to_string(),
                            message_id: user.id,
                            chat_id,
                            name: a.name.clone(),
                            mime: a.mime.clone(),
                            size: a.size,
                            blob_hash: a.blob_hash.clone(),
                            extracted_text: a.extracted_text.clone(),
                            created_at: now,
                        },
                    )?;
                }
                let transcript = transcript(&messages::list_for_chat(conn, chat_id)?);
                Ok(TurnInput {
                    turn_id: turn.id,
                    chat_id,
                    model: chat.model.clone(),
                    mode: chat.mode,
                    guard: chat.guard,
                    effort: chat.effort,
                    web_search: chat.web_search,
                    system: chat.system_snapshot.clone(),
                    messages: inline_media(&blobs, transcript),
                    first_turn,
                    previous_model,
                })
            })
            .map_err(|e| match e {
                gantry_store::StoreError::Other(m) if m.contains("not found") => {
                    GantryError::not_found(m)
                }
                gantry_store::StoreError::Other(m) => GantryError::invalid(m),
                other => store_err(other),
            })
    }

    /// Appends an assistant or tool message of a running turn (one per model round and one
    /// per batch of results), in order, so a crash loses at most the round in flight.
    pub fn append_turn_message(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        message: Message,
        stop_reason: Option<StopReason>,
        usage: Option<Usage>,
    ) {
        let result = self.store.write_blocking(move |conn| {
            messages::insert(
                conn,
                &MessageRecord {
                    message,
                    chat_id,
                    turn_id: Some(turn_id),
                    seq: messages::next_seq(conn, chat_id)?,
                    stop_reason,
                    usage,
                },
            )
        });
        if let Err(err) = result {
            log::error!("could not record a message of turn {turn_id}: {err}");
        }
    }

    pub fn finish_turn(&self, chat_id: ChatId, turn_id: TurnId, outcome: TurnOutcome) {
        let result = self.store.write_blocking(move |conn| {
            let Some(mut turn) = turns::get(conn, turn_id)? else {
                return Ok(());
            };
            let now = now_ms();
            turn.status = outcome.status;
            turn.usage = outcome.usage;
            turn.stop_reason = outcome.stop_reason.clone();
            turn.error = outcome.error;
            turn.ended_at = Some(now);
            turn.tool_call_count = outcome.tool_call_count;
            turns::update(conn, &turn)?;
            chats::set_last_message_at(conn, chat_id, now)?;
            Ok(())
        });
        if let Err(err) = result {
            log::error!("could not record turn {turn_id}: {err}");
        }
    }

    /// Applies a patch between turns. Returns the summary and whether the mode changed on a chat
    /// that already has turns (the caller appends the mode note, 04 §3).
    pub fn update(&self, chat_id: ChatId, patch: ChatPatch) -> Result<ChatSummary, GantryError> {
        self.store
            .write_blocking(move |conn| {
                let mut chat = chats::get(conn, chat_id)?.ok_or_else(|| {
                    gantry_store::StoreError::Other(format!("chat {chat_id} not found"))
                })?;
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
                if let Some(w) = patch.web_search {
                    chat.web_search = w;
                }
                if let Some(t) = patch.title {
                    chat.title = t;
                    chat.title_source = "user".into();
                }
                if let Some(p) = patch.pinned {
                    chat.pinned = p;
                }
                if let Some(a) = patch.archived {
                    chat.archived_at = if a { Some(now_ms()) } else { None };
                }
                if let Some(i) = patch.instructions {
                    chat.instructions = i;
                }
                chats::update(conn, &chat)?;
                let running = turns::running_for_chat(conn, chat_id)?.map(|t| t.id);
                Ok(summary(&chat, running))
            })
            .map_err(not_found_or_store)
    }

    /// The chat's standing permissions (04 §8), oldest first.
    pub fn grants(&self, chat_id: ChatId) -> Result<Vec<ChatGrant>, GantryError> {
        self.store
            .read(move |conn| grants::active(conn, chat_id))
            .map_err(store_err)
    }

    /// Remembers the scope the user picked on a permission card.
    pub fn add_grant(&self, grant: ChatGrant) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |conn| grants::insert(conn, &grant))
            .map_err(store_err)
    }

    /// Revokes one grant and answers with the chat it belonged to.
    pub fn revoke_grant(&self, id: gantry_core::GrantId) -> Result<Option<ChatId>, GantryError> {
        self.store
            .write_blocking(move |conn| grants::revoke(conn, id))
            .map_err(store_err)
    }

    /// Revokes every standing grant of one chat.
    pub fn revoke_all_grants(&self, chat_id: ChatId) -> Result<usize, GantryError> {
        self.store
            .write_blocking(move |conn| grants::revoke_all(conn, chat_id))
            .map_err(store_err)
    }

    /// Whether the chat has any turn; new chats get a fresh snapshot instead of a note.
    pub fn has_turns(&self, chat_id: ChatId) -> Result<bool, GantryError> {
        self.store
            .read(|conn| Ok(turns::last_for_chat(conn, chat_id)?.is_some()))
            .map_err(store_err)
    }

    /// Appends a `SystemNote` between turns (10 §4).
    pub fn append_system_note(&self, chat_id: ChatId, text: String) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |conn| {
                let seq = messages::next_seq(conn, chat_id)?;
                messages::insert(
                    conn,
                    &MessageRecord {
                        message: Message {
                            id: MessageId::new(),
                            role: Role::System,
                            parts: vec![ContentPart::SystemNote { text }],
                            origin: None,
                            created_at: now_ms(),
                        },
                        chat_id,
                        turn_id: None,
                        seq,
                        stop_reason: None,
                        usage: None,
                    },
                )
            })
            .map_err(store_err)
    }

    /// Replaces the frozen prompt of a chat that has no turns yet.
    pub fn replace_snapshot(
        &self,
        chat_id: ChatId,
        snapshot: String,
        version: u32,
    ) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |conn| {
                if let Some(mut chat) = chats::get(conn, chat_id)? {
                    chat.system_snapshot = snapshot;
                    chat.system_snapshot_version = version;
                    chats::update(conn, &chat)?;
                }
                Ok(())
            })
            .map_err(store_err)
    }

    /// Ids of every chat that is not archived.
    pub fn open_chat_ids(&self) -> Result<Vec<ChatId>, GantryError> {
        self.store
            .read(|conn| {
                Ok(chats::list(conn)?
                    .into_iter()
                    .filter(|c| c.archived_at.is_none())
                    .map(|c| c.id)
                    .collect())
            })
            .map_err(store_err)
    }

    /// The frozen snapshot and the system notes appended since, for developer mode.
    pub fn system_prompt(
        &self,
        chat_id: ChatId,
    ) -> Result<Option<(String, Vec<String>)>, GantryError> {
        self.store
            .read(|conn| {
                let Some(chat) = chats::get(conn, chat_id)? else {
                    return Ok(None);
                };
                let notes = messages::list_for_chat(conn, chat_id)?
                    .into_iter()
                    .filter(|m| m.message.role == Role::System)
                    .flat_map(|m| {
                        m.message.parts.into_iter().filter_map(|p| match p {
                            ContentPart::SystemNote { text } => Some(text),
                            _ => None,
                        })
                    })
                    .collect();
                Ok(Some((chat.system_snapshot, notes)))
            })
            .map_err(store_err)
    }

    /// Sets the generated title unless the user renamed the chat; returns whether it changed.
    pub fn set_auto_title(&self, chat_id: ChatId, title: String) -> Result<bool, GantryError> {
        self.store
            .write_blocking(move |conn| chats::set_auto_title(conn, chat_id, &title))
            .map_err(store_err)
    }

    /// Records the user's verdict on a finished turn; `None` clears it.
    pub fn rate_turn(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        feedback: Option<Feedback>,
    ) -> Result<(), GantryError> {
        self.store
            .write_blocking(move |conn| {
                let mut turn = turns::get(conn, turn_id)?
                    .filter(|t| t.chat_id == chat_id)
                    .ok_or_else(|| {
                        gantry_store::StoreError::Other(format!("turn {turn_id} not found"))
                    })?;
                turn.feedback = feedback;
                turns::update(conn, &turn)
            })
            .map_err(not_found_or_store)
    }

    /// Removes the chat's last turn so it can be re-run, returning its user message and
    /// attachments. Only the last turn can be retried, and not while it runs.
    pub fn take_last_turn(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
    ) -> Result<(Message, Vec<NewAttachment>), GantryError> {
        self.store
            .write_blocking(move |conn| {
                let last = turns::last_for_chat(conn, chat_id)?;
                match &last {
                    Some(t) if t.id == turn_id && t.status != TurnStatus::Running => {}
                    Some(t) if t.id == turn_id => {
                        return Err(gantry_store::StoreError::Other(
                            "invalid: the turn is still running".into(),
                        ));
                    }
                    _ => {
                        return Err(gantry_store::StoreError::Other(
                            "invalid: only the last turn can be retried".into(),
                        ));
                    }
                }
                let user = messages::list_for_chat(conn, chat_id)?
                    .into_iter()
                    .find(|m| m.turn_id == Some(turn_id) && m.message.role == Role::User)
                    .map(|m| m.message)
                    .ok_or_else(|| {
                        gantry_store::StoreError::Other("the turn has no user message".into())
                    })?;
                let attachments: Vec<NewAttachment> = messages::list_attachments(conn, user.id)?
                    .into_iter()
                    .map(|a| NewAttachment {
                        name: a.name,
                        mime: a.mime,
                        size: a.size,
                        blob_hash: a.blob_hash,
                        extracted_text: a.extracted_text,
                    })
                    .collect();
                for a in &attachments {
                    blobs::release(conn, &a.blob_hash)?;
                }
                turns::delete(conn, turn_id)?;
                Ok((user, attachments))
            })
            .map_err(|e| match e {
                gantry_store::StoreError::Other(m) => match m.strip_prefix("invalid: ") {
                    Some(rest) => GantryError::invalid(rest.to_owned()),
                    None => GantryError::invalid(m),
                },
                other => store_err(other),
            })
    }

    /// Deletes the chat with everything under it and sweeps blobs nobody references any more.
    pub fn delete(&self, chat_id: ChatId) -> Result<bool, GantryError> {
        let blobs = self.blobs.clone();
        self.store
            .write_blocking(move |conn| {
                let mut hashes: Vec<String> = artifacts::hashes_for_chat(conn, chat_id)?;
                hashes.extend(
                    messages::list_for_chat(conn, chat_id)?
                        .into_iter()
                        .filter(|m| m.message.role == Role::User)
                        .flat_map(|m| {
                            messages::list_attachments(conn, m.message.id).unwrap_or_default()
                        })
                        .map(|a| a.blob_hash),
                );
                let existed = chats::delete(conn, chat_id)?;
                for h in hashes {
                    if blobs::release(conn, &h)? {
                        blobs::delete(conn, &h)?;
                        if let Err(err) = blobs.remove(&h) {
                            log::warn!("could not remove blob {h}: {err}");
                        }
                    }
                }
                Ok(existed)
            })
            .map_err(store_err)
    }
}

fn not_found_or_store(e: gantry_store::StoreError) -> GantryError {
    match e {
        gantry_store::StoreError::Other(m) if m.contains("not found") => GantryError::not_found(m),
        other => store_err(other),
    }
}

fn summary(chat: &ChatRecord, active: Option<TurnId>) -> ChatSummary {
    ChatSummary {
        id: chat.id,
        title: chat.title.clone(),
        pinned: chat.pinned,
        archived: chat.archived_at.is_some(),
        project_id: chat.project_id,
        created_at: chat.created_at,
        last_message_at: chat.last_message_at,
        active_turn: active,
    }
}

fn detail(
    chat: &ChatRecord,
    turns: &[TurnRecord],
    messages: &[MessageRecord],
    calls: &[gantry_core::ToolCallDto],
    notices: &[(TurnId, String)],
) -> ChatDetail {
    let turn_dtos = turns
        .iter()
        .map(|t| {
            let user = messages
                .iter()
                .find(|m| m.turn_id == Some(t.id) && m.message.role == Role::User)
                .map(|m| m.message.clone())
                .unwrap_or_else(|| Message::user_text(""));
            let replies: Vec<Message> = messages
                .iter()
                .filter(|m| m.turn_id == Some(t.id) && m.message.role != Role::User)
                .map(|m| m.message.clone())
                .collect();
            // Results live in the transcript's tool messages; the row keeps only a preview.
            let results: std::collections::HashMap<
                &gantry_core::CallId,
                (&Vec<gantry_core::ResultPart>, bool),
            > = replies
                .iter()
                .filter(|m| m.role == Role::Tool)
                .flat_map(|m| m.parts.iter())
                .filter_map(|p| match p {
                    ContentPart::ToolResult {
                        call_id,
                        content,
                        is_error,
                    } => Some((call_id, (content, *is_error))),
                    _ => None,
                })
                .collect();
            let tool_calls = calls
                .iter()
                .filter(|c| c.turn_id == t.id)
                .map(|c| {
                    let mut c = c.clone();
                    if let Some((content, _)) = results.get(&c.id) {
                        c.result = Some((*content).clone());
                    }
                    c
                })
                .collect();
            TurnDto {
                id: t.id,
                status: t.status,
                model: t.model.clone(),
                user,
                messages: replies,
                tool_calls,
                notices: notices
                    .iter()
                    .filter(|(turn, _)| *turn == t.id)
                    .map(|(_, n)| n.clone())
                    .collect(),
                usage: t.usage,
                stop_reason: t.stop_reason.clone(),
                error: t.error.clone(),
                feedback: t.feedback,
                started_at: t.started_at,
                ended_at: t.ended_at,
            }
        })
        .collect();
    ChatDetail {
        id: chat.id,
        title: chat.title.clone(),
        pinned: chat.pinned,
        archived: chat.archived_at.is_some(),
        project_id: chat.project_id,
        created_at: chat.created_at,
        last_message_at: chat.last_message_at,
        model: chat.model.clone(),
        mode: chat.mode,
        guard: chat.guard,
        effort: chat.effort,
        web_search: chat.web_search,
        active_turn: turns
            .iter()
            .find(|t| t.status == TurnStatus::Running)
            .map(|t| t.id),
        turns: turn_dtos,
    }
}

/// Every message of the chat in order, minus assistant messages that kept nothing.
fn transcript(messages: &[MessageRecord]) -> Vec<Message> {
    messages
        .iter()
        .filter(|m| !(m.message.role == Role::Assistant && m.message.parts.is_empty()))
        .map(|m| m.message.clone())
        .collect()
}

/// Blob-backed parts become what a provider can consume: images inline as base64, text
/// documents as tagged text (02 §3).
fn inline_media(blobs: &BlobStore, messages: Vec<Message>) -> Vec<Message> {
    messages
        .into_iter()
        .map(|mut m| {
            m.parts = m
                .parts
                .into_iter()
                .map(|p| match p {
                    ContentPart::Image {
                        source: MediaSource::Blob { hash },
                        mime,
                    } => match blobs.get(&hash) {
                        Ok(bytes) => ContentPart::Image {
                            source: MediaSource::Base64 {
                                data: base64_encode(&bytes),
                            },
                            mime,
                        },
                        Err(err) => ContentPart::Text {
                            text: format!("[image unavailable: {err}]"),
                        },
                    },
                    ContentPart::Document {
                        source: MediaSource::Blob { hash },
                        name,
                        ..
                    } => {
                        let body = blobs
                            .get(&hash)
                            .map(|b| String::from_utf8_lossy(&b).into_owned())
                            .unwrap_or_else(|err| format!("[file unavailable: {err}]"));
                        ContentPart::Text {
                            text: format!("<attachment name=\"{name}\">\n{body}\n</attachment>"),
                        }
                    }
                    other => other,
                })
                .collect();
            m
        })
        .collect()
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn temp_book() -> (tempfile::TempDir, ChatBook) {
        let dir = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(dir.path().join("t.db")).unwrap());
        let blobs = Arc::new(BlobStore::open(dir.path().join("blobs")).unwrap());
        (dir, ChatBook::new(store, blobs))
    }

    fn book_with_chat() -> (tempfile::TempDir, ChatBook, ChatId) {
        let (dir, book) = temp_book();
        let c = book
            .create(
                ModelRef::default_model(),
                Mode::AutoEdit,
                true,
                ReasoningEffort::Off,
                "sys".into(),
                1,
            )
            .unwrap();
        (dir, book, c.id)
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
        let (_d, book, id) = book_with_chat();
        let input = book
            .begin_turn(id, Message::user_text("Hello there friend"), Vec::new())
            .unwrap();
        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.system, "sys");
        assert!(input.first_turn);
        assert!(
            book.begin_turn(id, Message::user_text("again"), Vec::new())
                .is_err()
        );
        assert_eq!(book.get(id).unwrap().unwrap().title, "Hello there friend");
        assert_eq!(book.list().unwrap()[0].active_turn, Some(input.turn_id));

        let mut assistant = Message::user_text("Hi!");
        assistant.role = Role::Assistant;
        book.append_turn_message(
            id,
            input.turn_id,
            assistant,
            Some(StopReason::EndTurn),
            None,
        );
        book.finish_turn(
            id,
            input.turn_id,
            TurnOutcome {
                status: TurnStatus::Completed,
                usage: None,
                stop_reason: Some(StopReason::EndTurn),
                error: None,
                tool_call_count: 0,
            },
        );
        book.append_system_note(id, "Permission mode is now Plan.".into())
            .unwrap();
        let next = book
            .begin_turn(id, Message::user_text("more"), Vec::new())
            .unwrap();
        assert_eq!(next.messages.len(), 4, "user, assistant, note, user");
        assert_eq!(next.messages[2].role, Role::System);
        assert!(!next.first_turn);
        let detail = book.get(id).unwrap().unwrap();
        assert_eq!(detail.turns.len(), 2);
        assert_eq!(detail.turns[0].status, TurnStatus::Completed);
        assert_eq!(detail.turns[0].assistant_text(), "Hi!");
        let (snapshot, notes) = book.system_prompt(id).unwrap().unwrap();
        assert_eq!(snapshot, "sys");
        assert_eq!(notes, vec!["Permission mode is now Plan.".to_owned()]);
    }

    #[test]
    fn only_the_last_finished_turn_can_be_retried() {
        let (_d, book, id) = book_with_chat();
        let first = book
            .begin_turn(id, Message::user_text("one"), Vec::new())
            .unwrap();
        assert!(book.take_last_turn(id, first.turn_id).is_err(), "running");
        book.finish_turn(
            id,
            first.turn_id,
            TurnOutcome {
                status: TurnStatus::Failed,
                usage: None,
                stop_reason: None,
                error: Some("x".into()),
                tool_call_count: 0,
            },
        );
        book.rate_turn(id, first.turn_id, Some(Feedback::Bad))
            .unwrap();
        assert_eq!(
            book.get(id).unwrap().unwrap().turns[0].feedback,
            Some(Feedback::Bad)
        );
        let (user, attachments) = book.take_last_turn(id, first.turn_id).unwrap();
        assert_eq!(user.text(), "one");
        assert!(attachments.is_empty());
        assert!(book.get(id).unwrap().unwrap().turns.is_empty());
        assert!(book.take_last_turn(id, first.turn_id).is_err(), "gone");
    }

    #[test]
    fn a_user_rename_survives_the_first_message_and_delete_cascades() {
        let (_d, book, id) = book_with_chat();
        book.update(
            id,
            ChatPatch {
                title: Some("Mine".into()),
                ..Default::default()
            },
        )
        .unwrap();
        book.begin_turn(id, Message::user_text("hello world"), Vec::new())
            .unwrap();
        assert_eq!(book.get(id).unwrap().unwrap().title, "Mine");
        assert!(!book.set_auto_title(id, "Generated".into()).unwrap());
        assert!(book.delete(id).unwrap());
        assert!(!book.delete(id).unwrap());
        assert!(book.get(id).unwrap().is_none());
    }

    #[test]
    fn attachments_are_inlined_for_the_provider_and_kept_as_parts() {
        let (_d, book, id) = book_with_chat();
        let hash = book.blobs().put(b"fn main() {}").unwrap();
        let mut user = Message::user_text("Review this");
        user.parts.push(ContentPart::Document {
            source: MediaSource::Blob { hash: hash.clone() },
            mime: "text/x-rust".into(),
            name: "main.rs".into(),
        });
        let input = book
            .begin_turn(
                id,
                user,
                vec![NewAttachment {
                    name: "main.rs".into(),
                    mime: "text/x-rust".into(),
                    size: 12,
                    blob_hash: hash,
                    extracted_text: Some("fn main() {}".into()),
                }],
            )
            .unwrap();
        let sent = &input.messages[0];
        assert!(
            matches!(&sent.parts[1], ContentPart::Text { text } if text.contains("<attachment name=\"main.rs\">\nfn main() {}"))
        );
        let stored = &book.get(id).unwrap().unwrap().turns[0].user;
        assert!(
            matches!(&stored.parts[1], ContentPart::Document { name, .. } if name == "main.rs")
        );
        book.finish_turn(
            id,
            input.turn_id,
            TurnOutcome {
                status: TurnStatus::Completed,
                usage: None,
                stop_reason: Some(StopReason::EndTurn),
                error: None,
                tool_call_count: 0,
            },
        );
        let (_, attachments) = book.take_last_turn(id, input.turn_id).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "main.rs");
    }
}
