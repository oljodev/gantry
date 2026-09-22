//! The chat book: chats, turns and the transcript, on the store (docs/plan/06 §3). The DTOs
//! are the ones M1 defined; only the backing changed.

use std::sync::Arc;

use gantry_core::{
    ChatDetail, ChatGrant, ChatId, ChatSummary, ContentPart, Feedback, GantryError, MediaSource,
    Message, MessageId, Mode, ModelRef, ProjectGrant, ProjectId, ReasoningEffort, Role, StopReason,
    SubAgentNode, Surface, ToolCallDto, TurnDto, TurnId, TurnStatus, Usage, now_ms,
};
use gantry_store::{
    BlobStore, Store,
    repos::{
        artifacts, blobs,
        chats::{self, ChatRecord},
        connectors, grants, memories,
        messages::{self, AttachmentRecord, MessageRecord},
        skills, tool_calls,
        turns::{self, TurnRecord},
    },
};

/// What the turn needs to know before it can choose its context block (12 §A4 rule 5, §B4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnContextOptions {
    /// Skills the user named with `/name` in the composer; they are injected whatever they
    /// score and whatever the last six turns carried.
    pub invoked: Vec<String>,
    /// Settings → Memory, inverted: paused means nothing is selected and nothing is injected.
    pub memory_on: bool,
    /// The turn this one replaces: **Retry**, which re-runs the last turn from the same user
    /// message. It is deleted in the same write that inserts the new one, so the message's
    /// attachments are never momentarily unreferenced — a blob sweep landing in that gap would
    /// delete files the new turn is about to claim — and so a retry that cannot start has not
    /// already eaten the turn it was going to replace.
    pub replacing: Option<TurnId>,
    /// Set when this turn is a sub agent's rather than a person's (18 A1). It travels into
    /// [`TurnInput`] and from there decides where a permission card goes and whose grants
    /// answer it.
    pub sub_agent: Option<SubAgentOrigin>,
    /// Whether skills may be injected. A type that says no gets none: a playbook about how the
    /// user works is not something a sub agent was asked to follow (12, 18 §3).
    pub skills_on: bool,
}

impl Default for TurnContextOptions {
    fn default() -> Self {
        Self {
            invoked: Vec::new(),
            memory_on: false,
            replacing: None,
            sub_agent: None,
            skills_on: true,
        }
    }
}

/// Where a sub agent's turn came from (18 A1, §6).
///
/// The parent ids are what makes a permission card appear in the conversation the user is
/// actually looking at, and what makes "allow for this chat" mean their chat rather than a
/// transcript that will be finished in a minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubAgentOrigin {
    pub parent_chat: ChatId,
    pub parent_turn: TurnId,
    /// The type it was started from, for the card's line and the tree.
    pub agent: String,
    /// A type that may not change anything is shown no tool that could (18 §3).
    pub read_only: bool,
}

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
    /// The tool namespaces this chat attached (03 §11). Runtime tools are always available;
    /// a connector is not, until the chat asks for it.
    pub connectors: Vec<String>,
    /// Which surface the session is on (16 §6): it decides which tools are offered.
    pub surface: Surface,
    /// The folders the session may reach, primary first (16 §7). Empty for most chats.
    pub roots: Vec<String>,
    /// The skills and memories this turn's context block carried, for `context.injected`.
    pub injected: gantry_core::InjectedContext,
    /// An incognito session (15 A21): no memory reaches it and the memory tools are not
    /// offered, so it can neither read what the user has been remembered to like nor add to it.
    pub incognito: bool,
    /// Set when a model, not a person, is having this conversation (18 A1).
    pub sub_agent: Option<SubAgentOrigin>,
}

/// What creating a session needs (16 C3, C5).
#[derive(Debug, Clone, PartialEq)]
pub struct NewChat {
    pub surface: Surface,
    /// The folders it starts with. Required on the code surface, empty for most chats.
    pub roots: Vec<String>,
    pub model: ModelRef,
    pub mode: Mode,
    pub guard: bool,
    pub effort: ReasoningEffort,
    pub system_snapshot: String,
    pub system_snapshot_version: u32,
    /// Namespaces to attach at once (03 §11, `chat.default_connectors`). Anything not
    /// installed or not enabled is skipped without complaint: a default is a preference, and
    /// a preference that fails to create the chat would be worse than one that does nothing.
    pub connectors: Vec<String>,
    /// A session that never joins the history (15 A21): no memory in, no memory out, absent
    /// from every list, and deleted with its window.
    pub incognito: bool,
    /// The project this chat is filed in, which is where its instructions, knowledge, defaults
    /// and the artifacts it can see come from (09 M11, 13 §9).
    pub project: Option<ProjectId>,
    /// Standing permissions the project hands to every chat it opens (04 §8), recorded with
    /// `GrantSource::ProjectDefault` so the Permissions page can say where they came from.
    pub grants: Vec<ProjectGrant>,
    /// The turn that started this as a sub agent, and the type it was started from (18 A1).
    /// `None` for every conversation a person is having.
    pub parent: Option<(TurnId, String)>,
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

    /// A new session on one surface, with the folders it starts with (16 C5: a code session
    /// needs one before its first message, and the picker is how it gets there).
    pub fn create(&self, new: NewChat) -> Result<ChatSummary, GantryError> {
        let NewChat {
            surface,
            roots,
            model,
            mode,
            guard,
            effort,
            system_snapshot,
            system_snapshot_version,
            connectors,
            incognito,
            project,
            grants,
            parent,
        } = new;
        let now = now_ms();
        let chat = ChatRecord {
            id: ChatId::new(),
            surface,
            project_id: project,
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
            incognito,
            parent_turn_id: parent.as_ref().map(|(t, _)| *t),
            agent_type: parent.as_ref().map(|(_, a)| a.clone()),
        };
        let mut summary = summary(&chat, None);
        summary.roots.clone_from(&roots);
        self.store
            .write_blocking(move |conn| {
                chats::insert(conn, &chat)?;
                for root in &roots {
                    chats::add_root(conn, chat.id, root)?;
                }
                if !connectors.is_empty() {
                    let installed = gantry_store::repos::connectors::list(conn)?;
                    for want in &connectors {
                        if let Some(i) =
                            installed.iter().find(|i| i.enabled && &i.namespace == want)
                        {
                            gantry_store::repos::connectors::attach(
                                conn, chat.id, i.id, "default",
                            )?;
                        }
                    }
                }
                // A project's standing permissions, written as this chat's own (04 §8). They are
                // grants like any other from here on: the Permissions panel lists them, says
                // they came from the project, and revokes them for this chat alone.
                for grant in &grants {
                    gantry_store::repos::grants::insert(
                        conn,
                        &gantry_core::ChatGrant {
                            id: gantry_core::GrantId::new(),
                            chat_id: chat.id,
                            instance_id: grant.instance_name.clone(),
                            instance_name: grant.instance_name.clone(),
                            tool_name: grant.tool_name.clone(),
                            tier_ceiling: grant.tier_ceiling,
                            arg_scope: None,
                            source: gantry_core::GrantSource::ProjectDefault,
                            created_at: now,
                            revoked_at: None,
                        },
                    )?;
                }
                Ok(())
            })
            .map_err(store_err)?;
        Ok(summary)
    }

    /// Adds a folder to a session (16 §7, the composer's folder chip).
    pub fn add_root(&self, chat_id: ChatId, path: String) -> Result<Vec<String>, GantryError> {
        self.store
            .write_blocking(move |conn| {
                chats::add_root(conn, chat_id, &path)?;
                chats::roots(conn, chat_id)
            })
            .map_err(store_err)
    }

    pub fn remove_root(&self, chat_id: ChatId, path: String) -> Result<Vec<String>, GantryError> {
        self.store
            .write_blocking(move |conn| {
                chats::remove_root(conn, chat_id, &path)?;
                chats::roots(conn, chat_id)
            })
            .map_err(store_err)
    }

    pub fn roots(&self, chat_id: ChatId) -> Result<Vec<String>, GantryError> {
        self.store
            .read(move |conn| chats::roots(conn, chat_id))
            .map_err(store_err)
    }

    /// One surface's sessions, most recent first, archived ones included (the sidebar groups
    /// them). The two lists never mix (16 §6).
    pub fn list(&self, surface: Surface) -> Result<Vec<ChatSummary>, GantryError> {
        self.store
            .read(move |conn| {
                let running: std::collections::HashMap<ChatId, TurnId> =
                    turns::chats_with_running_turns(conn)?.into_iter().collect();
                chats::list(conn, surface)?
                    .iter()
                    .map(|c| {
                        let mut s = summary(c, running.get(&c.id).copied());
                        s.roots = chats::roots(conn, c.id)?;
                        Ok(s)
                    })
                    .collect()
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
                let roots = chats::roots(conn, id)?;
                Ok(Some(detail(
                    &chat, roots, &turns, &messages, &calls, &notices,
                )))
            })
            .map_err(store_err)
    }

    /// One chat's sidebar row, without its transcript.
    pub fn summary(&self, id: ChatId) -> Result<Option<ChatSummary>, GantryError> {
        self.store
            .read(move |conn| {
                let Some(chat) = chats::get(conn, id)? else {
                    return Ok(None);
                };
                let running = turns::running_for_chat(conn, id)?.map(|t| t.id);
                let mut s = summary(&chat, running);
                s.roots = chats::roots(conn, id)?;
                Ok(Some(s))
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
    ///
    /// The per-turn context block (10 §5, 12) is chosen *here*, inside the same transaction
    /// that writes the message, and appended to it as a `TurnContext` part. Doing it here is
    /// what makes the six-turn rule work: the selector needs the transcript to know what the
    /// last few turns already carried, and the transcript is assembled two lines below.
    pub fn begin_turn(
        &self,
        chat_id: ChatId,
        user: Message,
        attachments: Vec<NewAttachment>,
        context: TurnContextOptions,
    ) -> Result<TurnInput, GantryError> {
        let blobs = self.blobs.clone();
        self.store
            .write_blocking(move |conn| {
                let mut chat = chats::get(conn, chat_id)?.ok_or_else(|| {
                    gantry_store::StoreError::Other(format!("chat {chat_id} not found"))
                })?;
                if let Some(replaced) = context.replacing {
                    check_retryable(conn, chat_id, replaced)?;
                    turns::delete(conn, replaced)?;
                }
                if turns::running_for_chat(conn, chat_id)?.is_some() {
                    return Err(gantry_store::StoreError::Other(
                        "this chat already has a turn running".into(),
                    ));
                }
                // 16 C5: a code session is defined by the folder it works in, and the check is
                // here rather than in the schema so the folder and the first message can land
                // in one transaction.
                let roots = chats::roots(conn, chat_id)?;
                if chat.surface.needs_folder() && roots.is_empty() {
                    return Err(gantry_store::StoreError::Other(
                        "this code session has no folder to work in".into(),
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

                // The context block is chosen against the transcript as it stands *before*
                // this message joins it, so the six-turn rule never counts the copy this turn
                // is about to send (12 §A4 rule 3). It is then appended to the message, which
                // is what puts it in the transcript rather than only in one request.
                let mut user = user;
                let pinned = skills::pinned_for_chat(conn, chat_id)?;
                let before = transcript(&messages::list_for_chat(conn, chat_id)?);
                let block = crate::memory::selector::build(
                    &user.text(),
                    &crate::memory::selector::Selection {
                        conn,
                        project: chat.project_id,
                        transcript: &before,
                        invoked: &context.invoked,
                        pinned: &pinned,
                        skills_on: context.skills_on,
                        // Incognito reads nothing from memory; skills still apply, because a
                        // playbook is how the user works, not something learned about them.
                        memory_on: context.memory_on && !chat.incognito,
                    },
                );
                let injected = block.injected.clone();
                if !block.is_empty() {
                    skills::mark_used(
                        conn,
                        &injected
                            .skills
                            .iter()
                            .map(|s| s.name.clone())
                            .collect::<Vec<_>>(),
                    )?;
                    memories::mark_used(
                        conn,
                        &injected.memories.iter().map(|m| m.id).collect::<Vec<_>>(),
                    )?;
                    user.parts.push(block.part());
                }

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
                    blobs::record(conn, &a.blob_hash, a.size, Some(&a.mime))?;
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
                    sub_agent: context.sub_agent.clone(),
                    injected,
                    incognito: chat.incognito,
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
                    connectors: connectors::attached_namespaces(conn, chat_id)?,
                    surface: chat.surface,
                    roots,
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
        let message = store_media(&self.blobs, message);
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
                    // Trimmed here rather than at the command, so that the stored text is what
                    // the prompt layer will hold whoever wrote it — and so that "did this
                    // change?" is a question about the instructions, not about trailing spaces.
                    chat.instructions = i.trim().to_owned();
                }
                chats::update(conn, &chat)?;
                let running = turns::running_for_chat(conn, chat_id)?.map(|t| t.id);
                Ok(summary(&chat, running))
            })
            .map_err(not_found_or_store)
    }

    /// The chat's standing permissions (04 §8), oldest first.
    /// Every installed connector, for the inventory the system prompt carries (04 §9).
    pub fn installed_connectors(
        &self,
    ) -> Result<Vec<gantry_core::ConnectorInstanceDto>, GantryError> {
        self.store.read(connectors::list).map_err(store_err)
    }

    /// The connector namespaces the chat may use right now (03 §11). Read again between tool
    /// rounds, because an access request or the user's own menu can change it mid-turn.
    pub fn attached_connectors(&self, chat_id: ChatId) -> Result<Vec<String>, GantryError> {
        self.store
            .read(move |conn| connectors::attached_namespaces(conn, chat_id))
            .map_err(store_err)
    }

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

    /// One tool call by id, for the parts of the app that act on a call after its turn ended:
    /// **Allow anyway** and the guard's feedback toggle (04 §6).
    pub fn tool_call(&self, id: &gantry_core::CallId) -> Result<Option<ToolCallDto>, GantryError> {
        let id = id.clone();
        self.store
            .read(move |conn| tool_calls::get(conn, &id))
            .map_err(store_err)
    }

    /// Rewrites the guard's verdict on one call: the user overrode it, or said it was wrong
    /// (04 §6). The decision itself is never rewritten — only what the user said about it.
    pub fn amend_verdict(
        &self,
        id: &gantry_core::CallId,
        f: impl FnOnce(&mut gantry_core::JudgeVerdict) + Send + 'static,
    ) -> Result<(), GantryError> {
        let id = id.clone();
        self.store
            .write_blocking(move |conn| {
                let Some(mut call) = tool_calls::get(conn, &id)? else {
                    return Err(gantry_store::StoreError::Other(format!("no call {id}")));
                };
                let Some(verdict) = call.judge.as_mut() else {
                    return Err(gantry_store::StoreError::Other(
                        "no guard decided this call".into(),
                    ));
                };
                f(verdict);
                tool_calls::update(conn, &call)
            })
            .map_err(store_err)
    }

    /// Records which memories a chat's frozen prompt was built from (12 §B4). Provenance, not
    /// behaviour: a failure here is logged and the chat still works.
    pub fn record_snapshot_memories(&self, chat_id: ChatId, ids: &[gantry_core::MemoryId]) {
        let ids = ids.to_vec();
        if let Err(err) = self
            .store
            .write_blocking(move |c| chats::set_snapshot_memories(c, chat_id, &ids))
        {
            log::warn!("could not record the chat's memory snapshot: {err}");
        }
    }

    /// Which memories a chat's frozen prompt was built from (12 §B4). Empty for a chat that
    /// predates the record or that froze none.
    pub fn snapshot_memories(&self, chat_id: ChatId) -> Vec<gantry_core::MemoryId> {
        self.store
            .read(move |c| chats::snapshot_memories(c, chat_id))
            .unwrap_or_default()
    }

    /// The skills pinned to a chat (12 §A3). They live in the frozen prompt, so the matcher
    /// skips them and the inventory lists them first.
    pub fn pinned_skills(&self, chat_id: ChatId) -> Result<Vec<String>, GantryError> {
        self.store
            .read(move |c| skills::pinned_for_chat(c, chat_id))
            .map_err(store_err)
    }

    /// Pins a skill to a chat, or unpins it. Returns the note the model is told (10 §4).
    pub fn pin_skill(
        &self,
        chat_id: ChatId,
        skill_id: &str,
        pinned: bool,
    ) -> Result<(), GantryError> {
        let skill_id = skill_id.to_owned();
        self.store
            .write_blocking(move |c| {
                if pinned {
                    skills::pin_to_chat(c, chat_id, &skill_id)
                } else {
                    skills::unpin_from_chat(c, chat_id, &skill_id)
                }
            })
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
                Ok(chats::list_all(conn)?
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

    /// The chat's last turn, for **Retry**: its user message and the attachments that came with
    /// it. Nothing is deleted here — the turn goes in the same write that starts its replacement
    /// (`TurnContextOptions::replacing`), so a retry that fails to start leaves the chat as it
    /// was.
    pub fn last_turn_to_retry(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
    ) -> Result<(Message, Vec<NewAttachment>), GantryError> {
        self.store
            .read(move |conn| {
                check_retryable(conn, chat_id, turn_id)?;
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

    /// The sub agents one turn started, for the tree (18 §7).
    ///
    /// Everything the tree draws in one query rather than one per node: a turn with six sub
    /// agents under it is refetched while it runs, and six round trips per second to say
    /// "still running" is six times the work of one.
    ///
    /// The name comes from the library as it stands today, and falls back to the id: a type
    /// the user has since deleted still started this conversation, and the tree has to be able
    /// to say so rather than draw a node with no name on it.
    pub fn sub_agents(&self, parent_turn: TurnId) -> Result<Vec<SubAgentNode>, GantryError> {
        self.store
            .read(move |conn| {
                let library = gantry_store::repos::agents::list(conn)?;
                let mut out = Vec::new();
                for chat in chats::for_parent_turn(conn, parent_turn)? {
                    let turn = turns::last_for_chat(conn, chat.id)?;
                    let agent = chat.agent_type.clone().unwrap_or_default();
                    let task = messages::list_for_chat(conn, chat.id)?
                        .into_iter()
                        .find(|m| m.message.role == Role::User)
                        .map(|m| m.message.text())
                        .unwrap_or_default();
                    out.push(SubAgentNode {
                        chat_id: chat.id,
                        turn_id: turn.as_ref().map(|t| t.id),
                        name: library
                            .iter()
                            .find(|a| a.id == agent)
                            .map_or_else(|| agent.clone(), |a| a.name.clone()),
                        agent,
                        task,
                        model: turn.as_ref().map_or(chat.model, |t| t.model.clone()),
                        status: turn.as_ref().map_or(TurnStatus::Running, |t| t.status),
                        started_at: turn.as_ref().map_or(chat.created_at, |t| t.started_at),
                        ended_at: turn.as_ref().and_then(|t| t.ended_at),
                        usage: turn.and_then(|t| t.usage),
                    });
                }
                Ok(out)
            })
            .map_err(store_err)
    }

    /// Deletes every incognito session left behind (15 A21). An incognito chat is meant to
    /// live exactly as long as its window, and a crash is the one thing that can outlive it;
    /// this runs at startup, before anything can read the table.
    ///
    /// The blobs of a deleted incognito chat are released by the ordinary sweep rather than
    /// here: nothing points at them once the rows are gone.
    pub fn sweep_incognito(&self) -> Result<usize, GantryError> {
        self.store
            .write_blocking(|conn| chats::delete_incognito(conn))
            .map_err(store_err)
    }

    /// Deletes sub-agent transcripts past the age the user set (18 §9). Zero days means
    /// forever, and then nothing is swept: a transcript belongs to the chat that started it and
    /// goes when that chat does.
    ///
    /// Beside `sweep_incognito` at startup, for the same reason: a sweep that ran while the app
    /// was in use would delete a transcript somebody had open.
    pub fn sweep_sub_agents(&self, keep_days: u32) -> Result<usize, GantryError> {
        if keep_days == 0 {
            return Ok(0);
        }
        let cutoff = now_ms() - i64::from(keep_days) * 24 * 60 * 60 * 1000;
        self.store
            .write_blocking(move |conn| chats::delete_old_sub_agents(conn, cutoff))
            .map_err(store_err)
    }

    /// Deletes the chat with everything under it, and collects the blobs that leaves behind.
    ///
    /// The collection is here as well as in the weekly sweep so that deleting a chat full of
    /// PDFs gives the disk space back now rather than at the end of the week. It asks the same
    /// question the sweep asks — does any row still reference this hash — of the hashes the
    /// chat was holding, once its own rows are gone.
    pub fn delete(&self, chat_id: ChatId) -> Result<bool, GantryError> {
        let blobs = self.blobs.clone();
        self.store
            .write_blocking(move |conn| {
                let mut hashes: Vec<String> = artifacts::hashes_for_chat(conn, chat_id)?;
                hashes.extend(tool_calls::output_hashes_for_chat(conn, chat_id)?);
                hashes.extend(
                    messages::list_for_chat(conn, chat_id)?
                        .into_iter()
                        .flat_map(|m| {
                            media_hashes(&m.message).into_iter().chain(
                                messages::list_attachments(conn, m.message.id)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .map(|a| a.blob_hash),
                            )
                        })
                        .collect::<Vec<_>>(),
                );
                let existed = chats::delete(conn, chat_id)?;
                let report = gantry_store::sweep::collect(
                    conn,
                    &blobs,
                    &hashes,
                    gantry_store::sweep::GRACE,
                )?;
                if !report.is_empty() {
                    log::info!(
                        "deleted chat {chat_id}: {} blob(s), {} bytes",
                        report.files,
                        report.bytes
                    );
                }
                Ok(existed)
            })
            .map_err(store_err)
    }
}

/// The blobs a message's media parts point at: an image the user attached, the text extracted
/// from their PDF, anything the model rendered. The `attachments` row keeps the file the user
/// chose; the part keeps what the prompt carries, and the two are not always the same blob.
fn media_hashes(message: &Message) -> Vec<String> {
    message
        .parts
        .iter()
        .filter_map(|p| match p {
            ContentPart::Image { source, .. }
            | ContentPart::Document { source, .. }
            | ContentPart::Audio { source, .. }
            | ContentPart::Video { source, .. } => match source {
                MediaSource::Blob { hash } => Some(hash.clone()),
                MediaSource::Base64 { .. } => None,
            },
            _ => None,
        })
        .collect()
}

/// Whether `turn_id` is a turn **Retry** may re-run: the chat's last, and not still going.
/// Asked once before the message is read and again inside the write that replaces it, because
/// between those two the user may have sent something else.
fn check_retryable(
    conn: &gantry_store::Connection,
    chat_id: ChatId,
    turn_id: TurnId,
) -> Result<(), gantry_store::StoreError> {
    match turns::last_for_chat(conn, chat_id)? {
        Some(t) if t.id == turn_id && t.status != TurnStatus::Running => Ok(()),
        Some(t) if t.id == turn_id => Err(gantry_store::StoreError::Other(
            "invalid: the turn is still running".into(),
        )),
        _ => Err(gantry_store::StoreError::Other(
            "invalid: only the last turn can be retried".into(),
        )),
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
        surface: chat.surface,
        roots: Vec::new(),
        title: chat.title.clone(),
        pinned: chat.pinned,
        archived: chat.archived_at.is_some(),
        project_id: chat.project_id,
        created_at: chat.created_at,
        last_message_at: chat.last_message_at,
        active_turn: active,
        incognito: chat.incognito,
    }
}

fn detail(
    chat: &ChatRecord,
    roots: Vec<String>,
    turns: &[TurnRecord],
    messages: &[MessageRecord],
    calls: &[gantry_core::ToolCallDto],
    notices: &[(TurnId, String)],
) -> ChatDetail {
    let turn_dtos = turns
        .iter()
        .map(|t| {
            // The turn's opening message, whatever its role. Almost always the user's; a turn
            // started by **Allow anyway** opens with a `System` note instead, and the view
            // renders that as a note rather than as words the user did not say (04 §6).
            let mut turn_messages = messages.iter().filter(|m| m.turn_id == Some(t.id));
            let user = turn_messages
                .next()
                .map(|m| m.message.clone())
                .unwrap_or_else(|| Message::user_text(""));
            let replies: Vec<Message> = turn_messages.map(|m| m.message.clone()).collect();
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
        surface: chat.surface,
        roots,
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
        instructions: chat.instructions.clone(),
        turns: turn_dtos,
        incognito: chat.incognito,
    }
}

/// Every message of the chat in order, minus assistant messages that kept nothing.
/// The stored rows as a turn sees them: empty assistant messages dropped, and everything a
/// compaction marker stands for left behind (02 §6). The rows themselves are untouched — the
/// chat still shows all of them; this is only what the next request carries.
fn transcript(messages: &[MessageRecord]) -> Vec<Message> {
    let all: Vec<Message> = messages
        .iter()
        .filter(|m| !(m.message.role == Role::Assistant && m.message.parts.is_empty()))
        .map(|m| m.message.clone())
        .collect();
    crate::context::live(&all)
}

/// Media a model produced is parked in the blob store instead of being kept inside the message.
/// A picture is a megabyte and a clip is tens of them, and a transcript that holds them is read
/// whole, out of SQLite, every time the chat is opened. The live event still carries the bytes,
/// so the answer appears the moment it arrives; only what is written down changes.
fn store_media(blobs: &BlobStore, mut message: Message) -> Message {
    use base64::Engine;

    message.parts = message
        .parts
        .into_iter()
        .map(|part| {
            let source = match &part {
                ContentPart::Image { source, .. }
                | ContentPart::Audio { source, .. }
                | ContentPart::Video { source, .. } => source,
                _ => return part,
            };
            let MediaSource::Base64 { data } = source else {
                return part;
            };
            let stored = base64::engine::general_purpose::STANDARD
                .decode(data.as_bytes())
                .ok()
                .and_then(|bytes| blobs.put(&bytes).ok());
            // A chat that cannot write a blob keeps the bytes in the message: worse, not broken.
            let Some(hash) = stored else { return part };
            let source = MediaSource::Blob { hash };
            match part {
                ContentPart::Image { mime, .. } => ContentPart::Image { source, mime },
                ContentPart::Audio { mime, .. } => ContentPart::Audio { source, mime },
                ContentPart::Video { mime, .. } => ContentPart::Video { source, mime },
                other => other,
            }
        })
        .collect();
    message
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
                    // No provider takes a sound file or a clip as input, and a message whose
                    // only part was one would project to nothing at all. A sentence saying what
                    // happened keeps the conversation readable — and the words a voice model
                    // spoke are already in the text part beside it.
                    ContentPart::Audio { .. } => ContentPart::Text {
                        text: "[the assistant answered with a sound file]".to_owned(),
                    },
                    ContentPart::Video { .. } => ContentPart::Text {
                        text: "[the assistant answered with a video clip]".to_owned(),
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
            .create(NewChat {
                surface: Surface::Chat,
                roots: Vec::new(),
                model: ModelRef::new(gantry_core::ProviderId::openrouter(), "test/model"),
                mode: Mode::AutoEdit,
                guard: true,
                effort: ReasoningEffort::Off,
                system_snapshot: "sys".into(),
                system_snapshot_version: 1,
                connectors: Vec::new(),
                incognito: false,
                project: None,
                grants: Vec::new(),
                parent: None,
            })
            .unwrap();
        (dir, book, c.id)
    }

    #[test]
    fn media_a_model_made_is_parked_in_the_blob_store() {
        use base64::Engine;

        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(dir.path().join("blobs")).unwrap();
        let bytes = b"ID3 pretend this is a song";
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let message = Message {
            id: MessageId::new(),
            role: Role::Assistant,
            parts: vec![
                ContentPart::Text {
                    text: "here you go".into(),
                },
                ContentPart::Audio {
                    source: MediaSource::Base64 { data },
                    mime: "audio/mpeg".into(),
                },
            ],
            origin: None,
            created_at: now_ms(),
        };

        let stored = store_media(&blobs, message);
        assert!(
            matches!(&stored.parts[0], ContentPart::Text { .. }),
            "text is left alone"
        );
        let ContentPart::Audio {
            source: MediaSource::Blob { hash },
            mime,
        } = &stored.parts[1]
        else {
            panic!("the sound should be a blob now: {:?}", stored.parts[1]);
        };
        assert_eq!(mime, "audio/mpeg");
        assert_eq!(blobs.get(hash).unwrap(), bytes, "byte for byte");
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
            .begin_turn(
                id,
                Message::user_text("Hello there friend"),
                Vec::new(),
                TurnContextOptions::default(),
            )
            .unwrap();
        assert_eq!(input.messages.len(), 1);
        assert_eq!(input.system, "sys");
        assert!(input.first_turn);
        assert!(
            book.begin_turn(
                id,
                Message::user_text("again"),
                Vec::new(),
                TurnContextOptions::default()
            )
            .is_err()
        );
        assert_eq!(book.get(id).unwrap().unwrap().title, "Hello there friend");
        assert_eq!(
            book.list(Surface::Chat).unwrap()[0].active_turn,
            Some(input.turn_id)
        );

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
            .begin_turn(
                id,
                Message::user_text("more"),
                Vec::new(),
                TurnContextOptions::default(),
            )
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
            .begin_turn(
                id,
                Message::user_text("one"),
                Vec::new(),
                TurnContextOptions::default(),
            )
            .unwrap();
        assert!(
            book.last_turn_to_retry(id, first.turn_id).is_err(),
            "running"
        );
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
        let (user, attachments) = book.last_turn_to_retry(id, first.turn_id).unwrap();
        assert_eq!(user.text(), "one");
        assert!(attachments.is_empty());
        // Reading it leaves the turn where it was; the turn that replaces it takes it away in
        // the same write, so a retry that never starts costs the chat nothing.
        assert_eq!(book.get(id).unwrap().unwrap().turns.len(), 1);
        book.begin_turn(
            id,
            user,
            attachments,
            TurnContextOptions {
                replacing: Some(first.turn_id),
                ..Default::default()
            },
        )
        .unwrap();
        let turns = book.get(id).unwrap().unwrap().turns;
        assert_eq!(turns.len(), 1, "the retry replaced it rather than adding");
        assert_ne!(turns[0].id, first.turn_id);
        assert!(book.last_turn_to_retry(id, first.turn_id).is_err(), "gone");
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
        book.begin_turn(
            id,
            Message::user_text("hello world"),
            Vec::new(),
            TurnContextOptions::default(),
        )
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
                TurnContextOptions::default(),
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
        let (_, attachments) = book.last_turn_to_retry(id, input.turn_id).unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].name, "main.rs");
    }
}
