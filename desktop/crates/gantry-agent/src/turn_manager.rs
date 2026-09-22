//! Active turns: start, cancel, subscribe, list, resolve decisions (docs/plan/01 §3, 04 §10,
//! 05 §3). A turn is a detached task; the UI is just a subscriber. Chats live in the store
//! through the [`ChatBook`].

use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, RwLock},
};

use gantry_connectors::ConnectorRegistry;
use gantry_core::{
    AgentEventKind, AttachmentInput, ChatId, ChatSummary, ContentPart, GantryError, Interaction,
    InteractionId, InteractionResolution, Message, MessageId, ModelRef, ProjectId, ProviderId,
    Role, Settings, ToolCallDto, TurnId, TurnSnapshot, TurnStatus, Usage, now_ms,
};
use gantry_providers::Provider;
use gantry_store::repos;
use tokio_util::sync::CancellationToken;

use crate::{
    attachments,
    chats::{ChatBook, ChatPatch, NewAttachment, NewChat, TurnContextOptions},
    events::{Batcher, EventSink, FanoutSink},
    interactions::Interactions,
    persist::PersistSink,
    runner::{self, RunContext},
    skills::Skills,
    subagents::SubAgentStart,
    system_prompt::{
        CORE_VERSION, PromptContext, SystemPromptBuilder, chat_instructions_note,
        connector_inventory, mode_note, now_block, with_roots, with_turn_blocks,
    },
    title,
    tools::ToolSet,
};

/// Where providers come from; the registry in the app, a mock in tests.
pub trait ProviderSource: Send + Sync {
    fn provider(&self, id: &ProviderId) -> Option<Arc<dyn Provider>>;
}

impl ProviderSource for gantry_providers::ProviderRegistry {
    fn provider(&self, id: &ProviderId) -> Option<Arc<dyn Provider>> {
        self.get(id)
    }
}

/// Told when chats changed outside a command (a turn ended, a title arrived) or when a chat's
/// pending decisions changed, so the app can emit the global events.
pub trait ChatNotifier: Send + Sync {
    fn chats_changed(&self, chat_ids: Vec<ChatId>);
    fn interactions_changed(&self, chat_id: ChatId, pending: u32) {
        let _ = (chat_id, pending);
    }
}

/// One message of a running turn, parts by block index.
#[derive(Debug, Clone)]
pub struct LiveMessage {
    pub id: MessageId,
    pub role: Role,
    pub parts: BTreeMap<u32, ContentPart>,
}

impl LiveMessage {
    #[must_use]
    pub fn finished(m: &Message) -> Self {
        Self {
            id: m.id,
            role: m.role,
            parts: m
                .parts
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, p)| (u32::try_from(i).unwrap_or(u32::MAX), p))
                .collect(),
        }
    }

    #[must_use]
    pub fn to_message(&self) -> Message {
        Message {
            id: self.id,
            role: self.role,
            parts: self.parts.values().cloned().collect(),
            origin: None,
            created_at: 0,
        }
    }
}

/// The live state of a running turn, kept for snapshots.
#[derive(Debug)]
pub struct TurnState {
    pub chat_id: ChatId,
    pub status: TurnStatus,
    pub messages: Vec<LiveMessage>,
    pub tool_calls: Vec<ToolCallDto>,
    pub pending: Vec<Interaction>,
    /// The tail of each running call's output, for the snapshot a reattaching view is sent
    /// (05 §3). Dropped when the call ends, because from then on its result carries the output.
    pub output: HashMap<gantry_core::CallId, Vec<String>>,
    pub usage: Option<Usage>,
    pub started_at: i64,
}

impl TurnState {
    /// Appends a chunk to a call's tail, keeping the last [`gantry_core::LIVE_OUTPUT_LINES`]
    /// lines. Both streams go in one list in the order they arrived, which is what a terminal
    /// shows; separating them here would reorder a command's own interleaving.
    pub fn push_output(&mut self, call_id: &gantry_core::CallId, chunk: &str) {
        let lines = self.output.entry(call_id.clone()).or_default();
        let joined = format!("{}{chunk}", lines.join("\n"));
        let mut next: Vec<String> = joined.split('\n').map(str::to_owned).collect();
        if next.len() > gantry_core::LIVE_OUTPUT_LINES {
            next.drain(..next.len() - gantry_core::LIVE_OUTPUT_LINES);
        }
        *lines = next;
    }
}

pub struct ActiveTurn {
    pub id: TurnId,
    pub chat_id: ChatId,
    pub state: Mutex<TurnState>,
    pub cancel: CancellationToken,
    pub fanout: Arc<FanoutSink>,
    pub batcher: Arc<Batcher>,
}

/// The ways one turn differs from another at the moment it starts: the skills the composer's
/// `/name` forced, the turn a **Retry** replaces, and whether a model rather than a person is
/// having this conversation (18 A1). Together rather than as three more parameters, because
/// nearly every caller wants none of them.
#[derive(Default)]
pub(crate) struct StartOptions {
    pub invoked: Vec<String>,
    pub replacing: Option<TurnId>,
    pub sub: Option<SubAgentStart>,
}

impl StartOptions {
    fn from_user(invoked: Vec<String>) -> Self {
        Self {
            invoked,
            ..Self::default()
        }
    }
}

pub struct TurnManager {
    chats: Arc<ChatBook>,
    providers: Arc<dyn ProviderSource>,
    connectors: Arc<ConnectorRegistry>,
    interactions: Arc<Interactions>,
    settings: Arc<RwLock<Settings>>,
    context: PromptContext,
    /// Turns run here whatever thread starts them; commands arrive on the UI thread.
    runtime: tokio::runtime::Handle,
    active: Mutex<HashMap<TurnId, Arc<ActiveTurn>>>,
    notifier: RwLock<Option<Arc<dyn ChatNotifier>>>,
    /// **Allow anyway**, from the last turn to the next one (04 §6).
    overrides: Arc<crate::judge::Overrides>,
    /// The skill library, when the app has one. `None` in tests that have no use for it: a
    /// turn with no skills service simply carries no skills.
    skills: RwLock<Option<Arc<Skills>>>,
}

impl TurnManager {
    #[must_use]
    pub fn new(
        chats: Arc<ChatBook>,
        providers: Arc<dyn ProviderSource>,
        connectors: Arc<ConnectorRegistry>,
        settings: Arc<RwLock<Settings>>,
        context: PromptContext,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            chats,
            providers,
            connectors,
            interactions: Interactions::new(),
            settings,
            context,
            runtime,
            active: Mutex::new(HashMap::new()),
            notifier: RwLock::new(None),
            overrides: Arc::default(),
            skills: RwLock::new(None),
        })
    }

    /// Hands the manager the skill library (12 §A): it rescans before each turn and names the
    /// installed skills in the prompt.
    pub fn set_skills(&self, skills: Arc<Skills>) {
        *self.skills.write().unwrap_or_else(|e| e.into_inner()) = Some(skills);
    }

    fn skills(&self) -> Option<Arc<Skills>> {
        self.skills
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn set_notifier(&self, notifier: Arc<dyn ChatNotifier>) {
        *self.notifier.write().unwrap_or_else(|e| e.into_inner()) = Some(notifier);
    }

    fn notifier(&self) -> Option<Arc<dyn ChatNotifier>> {
        self.notifier
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn notify(&self, chat_id: ChatId) {
        if let Some(n) = self.notifier() {
            n.chats_changed(vec![chat_id]);
        }
    }

    #[must_use]
    pub fn chats(&self) -> &Arc<ChatBook> {
        &self.chats
    }

    #[must_use]
    pub fn connectors(&self) -> &Arc<ConnectorRegistry> {
        &self.connectors
    }

    #[must_use]
    pub fn interactions(&self) -> &Arc<Interactions> {
        &self.interactions
    }

    pub(crate) fn settings(&self) -> Settings {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Re-freezes an untouched chat's prompt after something it was built from changed (10 §4):
    /// the mode, or the global instructions. Only for a chat with no turns — one that has
    /// spoken gets a `SystemNote` instead, because rewriting the prompt under a conversation
    /// would rewrite what the model was answering.
    ///
    /// It re-picks the memory core set, so the ids recorded against the chat have to move with
    /// it: they are what the Memory page reads to say which open chat still carries an entry
    /// (12 §B4). And it takes `incognito` rather than assuming, because the composer in an
    /// incognito chat can change the mode, and a rebuild that forgot would quietly hand that
    /// chat the memory its whole promise is to do without (15 A21).
    fn refreeze(
        &self,
        chat_id: ChatId,
        mode: gantry_core::Mode,
        incognito: bool,
        project: Option<ProjectId>,
        settings: &Settings,
    ) -> Result<(), GantryError> {
        let (prompt, memory_ids) =
            self.build_prompt_with_memory(settings, mode, !incognito, project, Some(chat_id));
        self.chats.replace_snapshot(chat_id, prompt, CORE_VERSION)?;
        self.chats.record_snapshot_memories(chat_id, &memory_ids);
        Ok(())
    }

    /// The frozen prompt, and the memories it froze into it (10 §2, 12 §B4).
    ///
    /// The core set is chosen **once**, here, and recorded on the chat. That is the whole point
    /// of the two tiers: a chat's standing behaviour does not change under it mid-conversation,
    /// and an edit reaches an open chat as a `SystemNote` instead (12 §B6).
    ///
    /// `project` brings four of the layers with it — the project's name in the context block, its
    /// memories, its knowledge files and its instructions — and `chat`, when the chat already
    /// exists, brings its own instructions (layer 6) and its own pinned skills, to stand beside
    /// the project's in layer 7. A chat being created has neither yet, which is the only reason
    /// that is an `Option`.
    fn build_prompt_with_memory(
        &self,
        settings: &Settings,
        mode: gantry_core::Mode,
        memory_on: bool,
        project: Option<ProjectId>,
        chat: Option<ChatId>,
    ) -> (String, Vec<gantry_core::MemoryId>) {
        let paused = settings.memory.paused || !memory_on;
        // One read for everything outside the chat that the prompt is built from, so a project
        // edited in another window cannot land half of itself in a snapshot.
        let (entries, project_row, knowledge, pinned, chat_instructions) = self
            .chats
            .store()
            .read(move |c| {
                let entries = if paused {
                    Vec::new()
                } else {
                    repos::memories::core_set(c, project)?
                };
                let row = match project {
                    Some(id) => repos::projects::get(c, id)?,
                    None => None,
                };
                let knowledge = match project {
                    Some(id) => repos::projects::knowledge(c, id)?,
                    None => Vec::new(),
                };
                let mut pins = match project {
                    Some(id) => repos::skills::pinned_for_project(c, id)?,
                    None => Vec::new(),
                };
                if let Some(chat) = chat {
                    for id in repos::skills::pinned_for_chat(c, chat)? {
                        if !pins.contains(&id) {
                            pins.push(id);
                        }
                    }
                }
                let pinned = crate::memory::selector::bodies(c, &pins);
                let instructions = match chat {
                    Some(chat) => repos::chats::get(c, chat)?.map(|r| r.instructions),
                    None => None,
                };
                Ok((
                    entries,
                    row,
                    knowledge,
                    pinned,
                    instructions.unwrap_or_default(),
                ))
            })
            .unwrap_or_else(|err| {
                log::warn!("could not read what the prompt is frozen from: {err}");
                (Vec::new(), None, Vec::new(), Vec::new(), String::new())
            });
        let (memory_block, ids) = crate::memory::selector::core_block(&entries);
        let mut context = self.context.clone();
        context.project_name = project_row.as_ref().map(|p| p.name.clone());
        let instructions = project_row.map_or_else(String::new, |p| p.instructions);
        let prompt = SystemPromptBuilder::new(mode, context)
            .memory(&memory_block)
            .knowledge(&crate::system_prompt::knowledge_block(&knowledge))
            .global_instructions(&settings.chat.custom_instructions)
            .project_instructions(&instructions)
            .chat_instructions(&chat_instructions)
            .pinned_skills(&crate::memory::selector::pinned_block(&pinned))
            .build();
        (prompt, ids)
    }

    /// A new chat with the settings' defaults and a freshly assembled system prompt.
    pub fn create_chat(&self, model: Option<ModelRef>) -> Result<ChatSummary, GantryError> {
        self.create_session(gantry_core::Surface::Chat, Vec::new(), model, false, None)
    }

    /// A new session on either surface (16 C3, C5), in a project or loose. A code session is
    /// created with the folder it will work in; a chat is created with none.
    ///
    /// A project contributes its defaults here, and every one of them is an `Option` that means
    /// "ask the settings" when unset (09 M11): a project which does not care about the mode must
    /// not freeze this week's setting into every chat it ever opens.
    pub fn create_session(
        &self,
        surface: gantry_core::Surface,
        roots: Vec<String>,
        model: Option<ModelRef>,
        incognito: bool,
        project: Option<ProjectId>,
    ) -> Result<ChatSummary, GantryError> {
        let settings = self.settings();
        let row = match project {
            Some(id) => Some(
                self.chats
                    .store()
                    .read(move |c| repos::projects::get(c, id))?
                    .ok_or_else(|| GantryError::not_found(format!("project {id}")))?,
            ),
            None => None,
        };
        let defaults = row.as_ref().map(|p| p.defaults.clone()).unwrap_or_default();
        // The project's folder is where its chats work, unless this session was started with one
        // of its own — a code session opened from the folder picker means that folder.
        let roots = if roots.is_empty() {
            row.as_ref()
                .and_then(|p| p.workspace_path.clone())
                .into_iter()
                .collect()
        } else {
            roots
        };
        if surface.needs_folder() && roots.is_empty() {
            return Err(GantryError::invalid(if row.is_some() {
                "a code session needs a folder to work in, and this project has none: pick one"
            } else {
                "a code session needs a folder to work in"
            }));
        }
        let (default_mode, default_guard) = settings.defaults_for(surface);
        let mode = defaults.mode.unwrap_or(default_mode);
        let guard = defaults.guard.unwrap_or(default_guard);
        // No model is substituted for one nobody chose. Until the user picks, there is no
        // default to fall back to, and a chat created against a guessed model would spend on a
        // provider they never asked for.
        let model = model.or_else(|| settings.default_model()).ok_or_else(|| {
            GantryError::invalid("no model is selected: pick one before starting a chat")
        })?;
        // Incognito takes no memory in and leaves none behind (15 A21). Custom instructions, and
        // a project's instructions and knowledge, stay: they are how the user has configured the
        // app and what they are working on, not something it learned about them, and a private
        // chat that forgets how to write is not what anyone asked for.
        let (prompt, memory_ids) =
            self.build_prompt_with_memory(&settings, mode, !incognito, project, None);
        let chat = self.chats.create(NewChat {
            surface,
            roots,
            model,
            mode,
            guard,
            effort: settings.chat.default_effort,
            system_snapshot: prompt,
            system_snapshot_version: CORE_VERSION,
            connectors: match surface {
                // 16 C6: choosing a folder is the explicit action, and a code session that
                // cannot read, edit or build in it is not a code session.
                gantry_core::Surface::Code => vec![
                    "filesystem".to_owned(),
                    "code-editor".to_owned(),
                    "shell".to_owned(),
                    // On by default here and a checkbox in a chat (18 A14): a long piece of
                    // work in a repository is where handing a piece of it to somebody else
                    // pays for itself.
                    crate::subagents::ID.to_owned(),
                ],
                gantry_core::Surface::Chat => defaults
                    .connectors
                    .clone()
                    .unwrap_or_else(|| settings.chat.default_connectors.clone()),
            },
            incognito,
            project,
            // An incognito chat in a project is still in that project — its instructions and its
            // knowledge apply — but it is not a place to hand out standing permissions, which
            // would outlive a session nobody can look at afterwards.
            grants: if incognito {
                Vec::new()
            } else {
                defaults.grants.unwrap_or_default()
            },
            parent: None,
        })?;
        self.chats.record_snapshot_memories(chat.id, &memory_ids);
        Ok(chat)
    }

    /// Applies a patch. Two of its fields are in the system prompt and so follow the rule of
    /// 10 §4: the mode (04 §3) and the chat's own instructions (layer 6). A chat that has not
    /// spoken is rebuilt around the new value; one that has is told, because rewriting the
    /// prompt under a conversation rewrites what the model was answering.
    ///
    /// Both can change in one patch, and then a chat that has spoken hears about both while a
    /// chat that has not is rebuilt once — the rebuild carries every layer, so doing it twice
    /// would only cost a second pick of the memory core set.
    pub fn update_chat(
        &self,
        chat_id: ChatId,
        patch: ChatPatch,
    ) -> Result<ChatSummary, GantryError> {
        let mode = patch.mode;
        let instructions = patch.instructions.clone();
        let before = self.chats.get(chat_id)?;
        let summary = self.chats.update(chat_id, patch)?;
        let Some(before) = before else {
            return Ok(summary);
        };
        let mode = mode.filter(|m| *m != before.mode);
        let instructions = instructions.filter(|i| i.trim() != before.instructions.trim());
        if mode.is_none() && instructions.is_none() {
            return Ok(summary);
        }
        if self.chats.has_turns(chat_id)? {
            if let Some(mode) = mode {
                self.chats.append_system_note(chat_id, mode_note(mode))?;
            }
            if let Some(text) = instructions {
                self.chats
                    .append_system_note(chat_id, chat_instructions_note(text.trim()))?;
            }
        } else {
            let settings = self.settings();
            self.refreeze(
                chat_id,
                mode.unwrap_or(before.mode),
                before.incognito,
                before.project_id,
                &settings,
            )?;
        }
        Ok(summary)
    }

    /// Settings → Custom instructions changed (10 §4): chats without turns get a new snapshot,
    /// the others a `SystemNote` carrying the whole new layer.
    pub fn global_instructions_changed(&self) -> Result<(), GantryError> {
        let settings = self.settings();
        let text = settings.chat.custom_instructions.trim().to_owned();
        let note = if text.is_empty() {
            "The user removed their global instructions; earlier <instructions scope=\"global\"> no longer apply.".to_owned()
        } else {
            format!(
                "Updated global instructions (replacing any earlier ones):\n<instructions scope=\"global\">\n{text}\n</instructions>"
            )
        };
        for id in self.chats.open_chat_ids()? {
            if self.chats.has_turns(id)? {
                self.chats.append_system_note(id, note.clone())?;
            } else if let Some(chat) = self.chats.get(id)? {
                self.refreeze(id, chat.mode, chat.incognito, chat.project_id, &settings)?;
            }
        }
        Ok(())
    }

    /// Something a project contributes to its chats' prompts changed (10 §4): its instructions,
    /// its knowledge, a pinned skill. The rule is the one global instructions already follow —
    /// a chat that has not spoken is rebuilt, a chat that has is *told*, because rewriting a
    /// prompt under a conversation rewrites what the model was answering.
    ///
    /// `note` is what the second kind is told. It is the caller's because only the caller knows
    /// what changed: "these are the new instructions" and "this file was added" are different
    /// sentences, and a generic "something changed" is no use to a model that cannot look.
    pub fn project_changed(&self, project: ProjectId, note: String) -> Result<(), GantryError> {
        let settings = self.settings();
        for id in self.project_chats(project)? {
            let Some(chat) = self.chats.get(id)? else {
                continue;
            };
            if self.chats.has_turns(id)? {
                self.chats.append_system_note(id, note.clone())?;
            } else {
                self.refreeze(id, chat.mode, chat.incognito, chat.project_id, &settings)?;
            }
            self.notify(id);
        }
        Ok(())
    }

    /// A memory changed, and the chats it reaches are brought up to date (12 §B6, 10 §4).
    ///
    /// The rule is every other layer's, with one addition that comes from memory being chosen
    /// in two tiers rather than one (§B4). A chat that has not spoken is rebuilt around the new
    /// core set. A chat that has spoken is *told* — but only if this entry could actually have
    /// reached it: the ones frozen into its prompt, which is what `snapshot_memory_ids_json`
    /// records, plus anything that would now be in a core set it has not got. A long-tail
    /// `fact` is re-queried every message, so an edit to one needs no announcement at all, and
    /// making one would put a system note into every open conversation twice a turn under
    /// auto-save.
    ///
    /// Two chats are never told. An incognito one reads no memory and writes none (15 A21), so
    /// a note about memory would be the one thing its whole promise is to do without. And
    /// `except` is the chat that caused the change: the model that just called
    /// `propose_memory` has the card and the tool result, and telling it again is the app
    /// talking to itself.
    pub fn memory_changed(
        &self,
        entry: &gantry_core::MemoryDto,
        edit: crate::memory::MemoryEdit,
        except: Option<ChatId>,
    ) -> Result<(), GantryError> {
        // Nothing to re-freeze and nobody to tell: a new long-tail entry is in no frozen
        // prompt and will be found by the next message that is about it.
        if matches!(edit, crate::memory::MemoryEdit::Added) && !in_core_set(entry) {
            return Ok(());
        }
        let settings = self.settings();
        for id in self.chats.open_chat_ids()? {
            if Some(id) == except {
                continue;
            }
            let Some(chat) = self.chats.get(id)? else {
                continue;
            };
            if chat.incognito || !reaches(entry, chat.project_id) {
                continue;
            }
            if !self.chats.has_turns(id)? {
                self.refreeze(id, chat.mode, chat.incognito, chat.project_id, &settings)?;
                self.notify(id);
                continue;
            }
            let mut held = self.chats.snapshot_memories(id);
            let carries = held.contains(&entry.id);
            if !carries && !in_core_set(entry) {
                continue;
            }
            if let Some(note) = crate::system_prompt::memory_note(&entry.text, &edit, carries) {
                self.chats.append_system_note(id, note)?;
                // A chat that has been *told* an entry holds it as surely as one that was
                // frozen around it, so the record of what it holds has to say so — otherwise
                // it would be told to remember something and never told to forget it. The
                // Memory page reads the same list to say which chats still carry an entry.
                match edit {
                    crate::memory::MemoryEdit::Forgotten => held.retain(|m| *m != entry.id),
                    _ if !carries => held.push(entry.id),
                    _ => {}
                }
                self.chats.record_snapshot_memories(id, &held);
                self.notify(id);
            }
        }
        Ok(())
    }

    /// The same rule for one chat: a pin was added or removed, or the chat moved projects.
    pub fn chat_context_changed(&self, chat_id: ChatId, note: String) -> Result<(), GantryError> {
        let settings = self.settings();
        let Some(chat) = self.chats.get(chat_id)? else {
            return Ok(());
        };
        if self.chats.has_turns(chat_id)? {
            self.chats.append_system_note(chat_id, note)?;
        } else {
            self.refreeze(
                chat_id,
                chat.mode,
                chat.incognito,
                chat.project_id,
                &settings,
            )?;
        }
        self.notify(chat_id);
        Ok(())
    }

    /// Moves a chat into a project, or out of every project (09 M11, 13 §9).
    ///
    /// What moves with it is what the project decides from here on: its instructions, its
    /// knowledge, its memories, its pinned skills, and which artifacts the chat can read. What
    /// does not move is anything already decided — the mode it is in, the connectors attached to
    /// it, the permissions granted to it. A project's defaults are what a chat is *opened* with,
    /// and quietly granting standing permissions to a conversation because it was filed
    /// somewhere is not a thing a user would expect.
    pub fn set_chat_project(
        &self,
        chat_id: ChatId,
        project: Option<ProjectId>,
    ) -> Result<(), GantryError> {
        let name = match project {
            Some(id) => Some(
                self.chats
                    .store()
                    .read(move |c| repos::projects::get(c, id))?
                    .ok_or_else(|| GantryError::not_found(format!("project {id}")))?
                    .name,
            ),
            None => None,
        };
        self.chats
            .store()
            .write_blocking(move |c| repos::projects::set_chat_project(c, chat_id, project))?;
        let note = match name {
            Some(name) => format!(
                "This chat is now in the project \"{name}\". Its instructions, its knowledge \
                 files and its artifacts apply from here on; anything already decided about this \
                 chat — its mode, its connectors, its permissions — is unchanged."
            ),
            None => "This chat is no longer in a project; the project's instructions and \
                     knowledge no longer apply."
                .to_owned(),
        };
        self.chat_context_changed(chat_id, note)
    }

    fn project_chats(&self, project: ProjectId) -> Result<Vec<ChatId>, GantryError> {
        Ok(self
            .chats
            .store()
            .read(move |c| repos::projects::chat_ids(c, project))?)
    }

    /// Starts a turn for `text` with `attachments` and returns at once; `sink` receives the
    /// batches.
    /// `invoked` are the skills the composer's `/name` forced for this message (12 §A4 rule 5).
    pub fn start(
        self: &Arc<Self>,
        chat_id: ChatId,
        text: String,
        attachments: Vec<AttachmentInput>,
        invoked: Vec<String>,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let text = text.trim().to_owned();
        if text.is_empty() && attachments.is_empty() {
            return Err(GantryError::invalid("the message is empty"));
        }
        let ingested = attachments::ingest(self.chats.blobs(), attachments)?;
        let mut user = Message::user_text(text);
        if user.text().is_empty() {
            user.parts.clear();
        }
        let mut records = Vec::with_capacity(ingested.len());
        for i in ingested {
            user.parts.push(i.part);
            records.push(i.record);
        }
        self.start_message(
            chat_id,
            user,
            records,
            sink,
            StartOptions::from_user(invoked),
        )
    }

    /// Re-runs the chat's last turn: the old turn is dropped and its user message sent again.
    pub fn retry(
        self: &Arc<Self>,
        chat_id: ChatId,
        turn_id: TurnId,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let (user, attachments) = self.chats.last_turn_to_retry(chat_id, turn_id)?;
        // A retry re-sends the same message; the skills it named are named again by
        // matching it, and a `/name` the user typed is still in its text. The turn being
        // retried goes in the same write that starts its replacement.
        self.start_message(
            chat_id,
            user,
            attachments,
            sink,
            StartOptions {
                replacing: Some(turn_id),
                ..StartOptions::default()
            },
        )
    }

    /// **Allow anyway** (04 §6): the user overrules a block the guard made.
    ///
    /// The blocked call cannot simply be run — its turn is over, and its result already went
    /// back to the model as `blocked_by_guard`. So the override is remembered, the chat is told
    /// in a system note what the user decided, and a new turn starts from that note. The model
    /// makes the call again, the override answers it, and the work continues from where the
    /// block stopped it. Nothing is rewritten: the block stays in the transcript, marked as
    /// overridden, because it did happen.
    pub fn allow_blocked(
        self: &Arc<Self>,
        chat_id: ChatId,
        call_id: gantry_core::CallId,
        sink: Arc<dyn EventSink>,
    ) -> Result<TurnId, GantryError> {
        let call = self
            .chats
            .tool_call(&call_id)?
            .ok_or_else(|| GantryError::invalid("that call is not in this chat's history"))?;
        if call.chat_id != chat_id {
            return Err(GantryError::invalid("that call belongs to another chat"));
        }
        let blocked = call.status == gantry_core::ToolCallStatus::Denied
            && call.judge.as_ref().is_some_and(|j| !j.allows());
        if !blocked {
            return Err(GantryError::invalid("the guard did not block that call"));
        }
        self.overrides
            .add(chat_id, &call.model_tool_name, &call.args);
        self.chats
            .amend_verdict(&call_id, |v| v.overridden = true)?;
        let reason = call
            .judge
            .as_ref()
            .map_or(String::new(), |j| j.reason.clone());
        let note = Message {
            id: MessageId::new(),
            role: Role::System,
            parts: vec![ContentPart::SystemNote {
                text: format!(
                    "The guard blocked your call to `{}` ({reason}). The user has looked at \
                     it and allowed it. Make exactly that call again and carry on; do not ask \
                     about it and do not change it.",
                    call.model_tool_name
                ),
            }],
            origin: None,
            created_at: now_ms(),
        };
        self.start_message(chat_id, note, Vec::new(), sink, StartOptions::default())
    }

    /// The Guard page's "this block was wrong" toggle (04 §6). It is stored with the decision
    /// and read by nothing yet: it is there for the prompt tuning the plan says it is for, and
    /// saying so out loud is better than a toggle that pretends to do something now.
    pub fn mark_verdict(
        &self,
        call_id: &gantry_core::CallId,
        wrong: Option<bool>,
    ) -> Result<(), GantryError> {
        self.chats.amend_verdict(call_id, move |v| v.wrong = wrong)
    }

    pub(crate) fn start_message(
        self: &Arc<Self>,
        chat_id: ChatId,
        user: Message,
        attachments: Vec<NewAttachment>,
        sink: Arc<dyn EventSink>,
        how: StartOptions,
    ) -> Result<TurnId, GantryError> {
        let StartOptions {
            invoked,
            replacing,
            sub,
        } = how;
        let sub_agent = sub.as_ref().map(|s| s.origin.clone());
        let skills_allowed = sub.as_ref().is_none_or(|s| s.skills_on);
        let read_only = sub.as_ref().is_some_and(|s| s.origin.read_only);
        let parent_turn = sub.as_ref().map(|s| s.parent.clone());
        let mut done = sub.map(|s| s.done);
        let settings = self.settings();
        // Skills are rescanned before the turn rather than on a timer: the folder is the user's
        // and they may have just edited it (12 §A3). A `stat` per folder is cheap enough to pay
        // for being right.
        let skills = self.skills();
        if let Some(skills) = &skills
            && let Err(err) = skills.rescan()
        {
            log::warn!("could not rescan the skills folder: {err}");
        }
        let mut input = self.chats.begin_turn(
            chat_id,
            user,
            attachments,
            TurnContextOptions {
                invoked,
                memory_on: !settings.memory.paused,
                replacing,
                sub_agent: sub_agent.clone(),
                skills_on: skills_allowed,
            },
        )?;
        let turn_id = input.turn_id;
        let provider = self.providers.provider(&input.model.provider);
        // The inventory rides with the turn, not with the frozen snapshot: what is installed
        // and attached changes outside the chat (03 §9, 04 §9, 10 §2).
        let installed = self.chats.installed_connectors().unwrap_or_else(|err| {
            log::warn!("could not read the installed connectors: {err}");
            Vec::new()
        });
        // Chats created before M10 froze the line "connectors: none attached" into their
        // snapshot; leaving it in would contradict the block that follows. The folders are
        // rewritten for the same reason: both change outside the chat.
        let base =
            with_roots(&input.system, &input.roots).replace("\nconnectors: none attached", "");
        let mut per_turn = vec![connector_inventory(
            &installed,
            &input.connectors,
            settings.chat.suggest_connectors,
        )];
        // The skill list rides with the turn for the connector inventory's reason: a skill
        // written after this chat started is still a skill this chat can load, and a list
        // frozen at creation would go on denying it exists (10 §2).
        if let Some(skills) = &skills {
            let pinned = self.chats.pinned_skills(chat_id).unwrap_or_default();
            match skills.inventory(&pinned) {
                Ok(block) if !block.is_empty() => per_turn.push(block),
                Ok(_) => {}
                Err(err) => log::warn!("could not list the skills for the prompt: {err}"),
            }
        }
        // The date last of the three: it is the only one that changes daily, so everything
        // above it stays in the cached prefix.
        per_turn.push(now_block());
        // Ahead of the user's own instructions, never after them — see `with_turn_blocks`.
        input.system = with_turn_blocks(&base, &per_turn.join("\n\n"));

        let fanout = Arc::new(FanoutSink::new());
        fanout.add(Arc::new(PersistSink::new(
            self.chats.store().clone(),
            chat_id,
        )));
        fanout.add(sink);
        let batcher = Batcher::start(turn_id, fanout.clone(), &self.runtime);
        let active = Arc::new(ActiveTurn {
            id: turn_id,
            chat_id,
            state: Mutex::new(TurnState {
                chat_id,
                status: TurnStatus::Running,
                messages: Vec::new(),
                tool_calls: Vec::new(),
                pending: Vec::new(),
                output: HashMap::new(),
                usage: None,
                started_at: now_ms(),
            }),
            cancel: CancellationToken::new(),
            fanout,
            batcher,
        });
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(turn_id, active.clone());

        let manager = Arc::clone(self);
        let chats = self.chats.clone();
        let connectors = self.connectors.clone();
        let interactions = self.interactions.clone();
        let notifier = self.notifier();
        let first_turn = input.first_turn;
        let user_text = input.messages.last().map(Message::text).unwrap_or_default();
        let model = input.model.clone();
        // The list prices, read once from the cached catalog: what a round costs where the
        // provider does not say. Never a network call — a turn does not wait on a price.
        let pricing = provider.as_ref().and_then(|p| {
            gantry_providers::catalog::cached(
                self.chats.store(),
                &model.provider.to_string(),
                p.kind(),
            )
            .ok()?
            .into_iter()
            .find(|m| m.id == model.model)?
            .pricing
        });
        // Per model, not per chat (11 §1): a voice belongs to the voice model you picked.
        let media = settings
            .chat
            .model_options
            .get(&format!("{}/{}", model.provider, model.model))
            .cloned()
            .unwrap_or_default();
        // Compiled once for the turn and then read by every call in it (04 §5).
        let guardrails = Arc::new(gantry_core::Guardrails::compile(&settings.guardrails));
        for problem in guardrails.problems() {
            log::warn!("guardrail rule ignored — {problem}");
        }
        let mode = input.mode;
        let incognito = input.incognito;
        let attached = input.connectors.clone();
        let overrides = self.overrides.clone();
        // One utility model for the whole turn (04 §6): the guard's decisions, the compaction
        // the turn may need and the title that follows it all ask the same one. Unset, it is
        // the cheapest fast model of the chat's own provider, so no second key is needed; set,
        // it is the user's, provider and all, because a model id means nothing without the
        // provider that serves it. Resolved once, before the turn starts, so a decision mid-turn
        // costs nothing but the request.
        let providers = self.providers.clone();
        let judge = title::resolve_judge(
            settings.guard.judge_model.as_ref(),
            &|id| providers.provider(id),
            provider.as_ref(),
            &model,
        );
        self.runtime.spawn(async move {
            let tools =
                ToolSet::assemble(&connectors, mode, &attached, !incognito, read_only).await;
            runner::run_turn(RunContext {
                input,
                provider: provider.clone(),
                max_output_tokens: settings.advanced.max_output_tokens,
                max_tool_rounds: settings.advanced.max_tool_rounds,
                max_calls_per_reply: settings.advanced.max_calls_per_reply.max(1),
                pricing,
                max_result_bytes: (settings.advanced.max_result_kb.max(1) as usize) * 1024,
                media,
                guardrails,
                judge: judge.clone(),
                overrides,
                active: active.clone(),
                chats: chats.clone(),
                tools: std::sync::RwLock::new(tools),
                connectors: connectors.clone(),
                interactions,
                notifier,
                parent: parent_turn,
            })
            .await;
            manager
                .active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&active.id);
            manager.notify(chat_id);
            // The sub agent is released the moment its turn is over, before the title
            // generator would have run: nobody reads the title of a transcript that is opened
            // from a tree, and the parent is waiting on this.
            if let Some(done) = done.take() {
                let _ = done.send(());
                return;
            }
            if first_turn && let Some((judge_provider, judge_model)) = judge {
                manager
                    .name_chat(chat_id, turn_id, judge_provider, judge_model, &user_text)
                    .await;
            }
        });
        Ok(turn_id)
    }

    /// Names the chat from its first exchange (01 §3 step 7); failures only log.
    async fn name_chat(
        &self,
        chat_id: ChatId,
        turn_id: TurnId,
        provider: Arc<dyn Provider>,
        model: String,
        user_text: &str,
    ) {
        let assistant_text = match self.chats.get(chat_id) {
            Ok(Some(detail)) => detail
                .turns
                .iter()
                .find(|t| t.id == turn_id)
                .filter(|t| t.status == TurnStatus::Completed)
                .map(|t| t.assistant_text()),
            _ => None,
        };
        let Some(assistant_text) = assistant_text.filter(|t| !t.trim().is_empty()) else {
            return;
        };
        match title::generate_title(provider, model, user_text, &assistant_text).await {
            Ok(t) if !t.is_empty() => match self.chats.set_auto_title(chat_id, t) {
                Ok(true) => self.notify(chat_id),
                Ok(false) => {}
                Err(err) => log::warn!("could not store the title of {chat_id}: {err}"),
            },
            Ok(_) => log::warn!("the title generator returned nothing for {chat_id}"),
            Err(err) => log::warn!("title generation failed for {chat_id}: {err}"),
        }
    }

    /// Trips the turn's cancellation token; pending prompts resolve as cancelled through it.
    /// Returns whether the turn was running.
    pub fn cancel(&self, turn_id: TurnId) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .cloned();
        match active {
            Some(t) => {
                t.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Answers a pending decision; the waiting turn continues (04 §10).
    pub fn resolve_interaction(
        &self,
        id: InteractionId,
        resolution: InteractionResolution,
    ) -> Result<Interaction, GantryError> {
        self.interactions.resolve(id, resolution)
    }

    /// Sends one snapshot to `sink`, then every later batch. Fails when the turn is not
    /// running (a finished turn is read from the chat). The snapshot carries every message,
    /// tool call and pending decision, so `since_seq` only marks where live events resume.
    pub fn subscribe(
        &self,
        turn_id: TurnId,
        since_seq: u32,
        sink: Arc<dyn EventSink>,
    ) -> Result<(), GantryError> {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .cloned()
            .ok_or_else(|| GantryError::not_found(format!("turn {turn_id} is not running")))?;
        // Hold the state lock across snapshot and subscription so no event slips between them.
        let state = active.state.lock().unwrap_or_else(|e| e.into_inner());
        let seq = active.batcher.last_seq();
        let snapshot = TurnSnapshot {
            chat_id: state.chat_id,
            status: state.status,
            messages: state.messages.iter().map(LiveMessage::to_message).collect(),
            tool_calls: state.tool_calls.clone(),
            pending: state.pending.clone(),
            output: state.output.clone(),
            usage: state.usage,
            started_at: state.started_at,
            seq,
        };
        let _ = since_seq;
        sink.emit(gantry_core::AgentEventBatch {
            turn_id,
            events: vec![gantry_core::AgentEvent {
                seq,
                ts: now_ms(),
                turn_id,
                event: AgentEventKind::TurnSnapshot { snapshot },
            }],
        });
        active.fanout.add(sink);
        drop(state);
        Ok(())
    }

    /// `(chat, turn)` of every running turn.
    #[must_use]
    pub fn list_active(&self) -> Vec<(ChatId, TurnId)> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .map(|t| (t.chat_id, t.id))
            .collect()
    }

    #[must_use]
    /// The running turn itself, by id: what a sub agent needs to announce a permission card on
    /// the stream the user is watching (18 §6).
    pub(crate) fn running(&self, turn_id: TurnId) -> Option<Arc<ActiveTurn>> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&turn_id)
            .cloned()
    }

    /// This machine, for a prompt assembled outside `create_session`.
    pub(crate) fn prompt_context(&self) -> PromptContext {
        self.context.clone()
    }

    pub fn active_turn_for(&self, chat_id: ChatId) -> Option<TurnId> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .find(|t| t.chat_id == chat_id)
            .map(|t| t.id)
    }
}

/// Whether an entry is chosen once into a chat's frozen prompt rather than looked up per
/// message (12 §B4): every enabled `instruction` and `preference`, and anything the user has
/// marked **Always**.
fn in_core_set(entry: &gantry_core::MemoryDto) -> bool {
    entry.enabled
        && (entry.always_include
            || matches!(
                entry.kind,
                gantry_core::MemoryKind::Instruction | gantry_core::MemoryKind::Preference
            ))
}

/// Whether an entry is in scope for a chat: a global one is in every chat's, a project one
/// only in that project's (12 §B4).
fn reaches(entry: &gantry_core::MemoryDto, project: Option<ProjectId>) -> bool {
    match entry.scope_kind {
        gantry_core::MemoryScopeKind::Global => true,
        gantry_core::MemoryScopeKind::Project => {
            entry.scope_id.is_some() && entry.scope_id == project
        }
    }
}
