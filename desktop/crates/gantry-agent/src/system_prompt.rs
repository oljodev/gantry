//! Assembling a chat's system prompt from the fixed core and the additive layers, in the order
//! of docs/plan/10 §2. Built once per chat and frozen as its `system_snapshot`.

use gantry_core::{
    AuthState, CHAT_INSTRUCTIONS_MAX_CHARS, ConnectorInstanceDto, Mode,
    PROJECT_INSTRUCTIONS_MAX_CHARS, PROJECT_KNOWLEDGE_MAX_CHARS,
};

use crate::memory::MemoryEdit;

/// Bumped whenever `assets/prompts/core.md` or a mode fragment changes meaning.
pub const CORE_VERSION: u32 = 8;

const CORE: &str = include_str!("../../../assets/prompts/core.md");
const MODE_MANUAL: &str = include_str!("../../../assets/prompts/modes/manual.md");
const MODE_AUTO_EDIT: &str = include_str!("../../../assets/prompts/modes/auto_edit.md");
const MODE_PLAN: &str = include_str!("../../../assets/prompts/modes/plan.md");
const MODE_AUTO: &str = include_str!("../../../assets/prompts/modes/auto.md");

/// Global custom instructions are capped here (10 §2).
pub const GLOBAL_INSTRUCTIONS_MAX_CHARS: usize = 4000;

/// The per-chat facts frozen at creation (layer 2).
#[derive(Debug, Clone, Default)]
pub struct PromptContext {
    /// `macos`, `windows` or `linux`.
    pub platform: String,
    pub app_version: String,
    pub workspace_roots: Vec<String>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SystemPromptBuilder {
    mode: Mode,
    context: PromptContext,
    memory: String,
    knowledge: String,
    global_instructions: String,
    project_instructions: String,
    chat_instructions: String,
    skills: String,
}

impl SystemPromptBuilder {
    #[must_use]
    pub fn new(mode: Mode, context: PromptContext) -> Self {
        Self {
            mode,
            context,
            memory: String::new(),
            knowledge: String::new(),
            global_instructions: String::new(),
            project_instructions: String::new(),
            chat_instructions: String::new(),
            skills: String::new(),
        }
    }

    /// The core memory set (layer 3), already rendered and already inside its budget by
    /// `memory::selector::core_block`. Empty means the chat carries no memory at all, and then
    /// there is no block: an empty `<memory>` would teach the model that memory exists and is
    /// empty, which is a different and less useful thing to say than nothing.
    #[must_use]
    pub fn memory(mut self, block: &str) -> Self {
        self.memory = block.trim().to_owned();
        self
    }

    /// Settings → Custom instructions (layer 4). Truncated to the cap.
    #[must_use]
    pub fn global_instructions(mut self, text: &str) -> Self {
        self.global_instructions = text
            .trim()
            .chars()
            .take(GLOBAL_INSTRUCTIONS_MAX_CHARS)
            .collect();
        self
    }

    /// The project's knowledge files (layer 3b), already rendered and capped by
    /// [`knowledge_block`].
    #[must_use]
    pub fn knowledge(mut self, block: &str) -> Self {
        self.knowledge = block.trim().to_owned();
        self
    }

    /// `projects.instructions` (layer 5). Truncated to the cap.
    #[must_use]
    pub fn project_instructions(mut self, text: &str) -> Self {
        self.project_instructions = text
            .trim()
            .chars()
            .take(PROJECT_INSTRUCTIONS_MAX_CHARS)
            .collect();
        self
    }

    /// `chats.instructions` (layer 6), the last and most specific of the three. Truncated to
    /// the cap.
    #[must_use]
    pub fn chat_instructions(mut self, text: &str) -> Self {
        self.chat_instructions = text
            .trim()
            .chars()
            .take(CHAT_INSTRUCTIONS_MAX_CHARS)
            .collect();
        self
    }

    /// Skills pinned to the project or the chat (layer 7), already rendered by
    /// `memory::selector::pinned_block`. A pinned skill is in the frozen prompt and is never
    /// matched per message (12 §A4 rule 4), which only holds if it is really in here.
    #[must_use]
    pub fn pinned_skills(mut self, block: &str) -> Self {
        self.skills = block.trim().to_owned();
        self
    }

    #[must_use]
    pub fn build(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();
        blocks.push(
            CORE.trim_end()
                .replace("{{mode}}", self.mode.as_str())
                .replace("{{mode_text}}", mode_text(self.mode).trim()),
        );
        blocks.push(self.context_block());
        if !self.memory.is_empty() {
            blocks.push(self.memory.clone());
        }
        // Knowledge sits with the facts rather than with the rules: it is reference material the
        // user put in the project, not an instruction about how to behave, and the instruction
        // layers below read as instructions precisely because nothing else is mixed into them.
        if !self.knowledge.is_empty() {
            blocks.push(self.knowledge.clone());
        }
        if !self.global_instructions.is_empty() {
            blocks.push(format!(
                "<instructions scope=\"global\">\n{}\n</instructions>",
                self.global_instructions
            ));
        }
        if !self.project_instructions.is_empty() {
            blocks.push(format!(
                "<instructions scope=\"project\">\n{}\n</instructions>",
                self.project_instructions
            ));
        }
        // Last of the three, because the precedence paragraph in the core says the more specific
        // layer wins and the order on the page is the order it describes.
        if !self.chat_instructions.is_empty() {
            blocks.push(format!(
                "<instructions scope=\"chat\">\n{}\n</instructions>",
                self.chat_instructions
            ));
        }
        if !self.skills.is_empty() {
            blocks.push(self.skills.clone());
        }
        let mut out = blocks.join("\n\n");
        out.push('\n');
        out
    }

    fn context_block(&self) -> String {
        let c = &self.context;
        let mut lines = vec![
            "<gantry_context>".to_owned(),
            format!(
                "platform: {}",
                if c.platform.is_empty() {
                    "unknown"
                } else {
                    &c.platform
                }
            ),
            format!("app: Gantry {}", c.app_version),
        ];
        if let Some(p) = &c.project_name {
            lines.push(format!("project: {p}"));
        }
        if c.workspace_roots.is_empty() {
            lines.push("workspace: none attached".to_owned());
        } else {
            lines.push(format!("workspace: {}", c.workspace_roots.join(", ")));
        }
        lines.push("</gantry_context>".to_owned());
        lines.join("\n")
    }
}

/// The project's knowledge files as one block (layer 3b), inside
/// [`PROJECT_KNOWLEDGE_MAX_CHARS`].
///
/// The budget is shared by water-filling rather than first-come: each file gets an equal share of
/// what is left, and a file smaller than its share hands the surplus back to the others. Filling
/// in order would let one long file take everything and leave the four files after it out of the
/// prompt entirely, which is the failure the user cannot see.
///
/// A file that is cut says so, in its own tag, with both numbers. A prompt that quietly holds the
/// first third of a specification is worse than one that says it holds a third: the model can ask
/// for the rest of a file it knows is cut.
#[must_use]
pub fn knowledge_block(files: &[(String, String)]) -> String {
    let files: Vec<&(String, String)> =
        files.iter().filter(|(_, t)| !t.trim().is_empty()).collect();
    if files.is_empty() {
        return String::new();
    }
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&i| files[i].1.chars().count());
    let mut shares = vec![0usize; files.len()];
    let mut left = PROJECT_KNOWLEDGE_MAX_CHARS;
    for (taken, &i) in order.iter().enumerate() {
        let share = left / (files.len() - taken);
        let want = files[i].1.chars().count();
        shares[i] = want.min(share);
        left -= shares[i];
    }
    let mut out = String::from("<project_knowledge>\n");
    for (i, (name, text)) in files.iter().enumerate() {
        let total = text.chars().count();
        let kept = shares[i];
        if kept == 0 {
            out.push_str(&format!(
                "<file name=\"{name}\" included=\"nothing: the knowledge budget was spent on the \
                 other files\"/>\n"
            ));
            continue;
        }
        if kept < total {
            out.push_str(&format!(
                "<file name=\"{name}\" included=\"the first {kept} characters of {total}\">\n"
            ));
        } else {
            out.push_str(&format!("<file name=\"{name}\">\n"));
        }
        out.push_str(text.chars().take(kept).collect::<String>().trim_end());
        out.push_str("\n</file>\n");
    }
    out.push_str("</project_knowledge>");
    out
}

/// The chat's folders, as of this turn (16 §7).
///
/// Like the connector inventory below, and for the same reason: the snapshot is frozen at
/// creation, but a folder is attached from the composer at any moment, and a prompt that still
/// says `workspace: none attached` is a prompt that makes the model ask the user for a path it
/// already has. The line is rewritten in place rather than appended, so the context block stays
/// one block and the prefix keeps caching between turns.
#[must_use]
pub fn with_roots(system: &str, roots: &[String]) -> String {
    let Some(start) = system.find("\nworkspace: ") else {
        return system.to_owned();
    };
    let from = start + 1;
    let to = system[from..]
        .find('\n')
        .map_or(system.len(), |at| from + at);
    let line = if roots.is_empty() {
        "workspace: none attached".to_owned()
    } else {
        format!("workspace: {}", roots.join(", "))
    };
    format!("{}{line}{}", &system[..from], &system[to..])
}

/// Puts the per-turn blocks into a frozen snapshot **before the user's own instructions**
/// (10 §2).
///
/// The inventories and the date have to be assembled per turn, and the obvious place to put
/// them is the end. That is the one place they must not go: the layering in 10 §2 runs from
/// general to specific, the user's standing instructions are the most specific thing in the
/// system prompt, and appending three blocks of machinery after them leaves the last thing the
/// model reads before the conversation as a list of connector ids and today's date. Whatever a
/// model's recency bias is worth, a prompt that ends on housekeeping is not the one to bet it
/// on.
///
/// The date goes last among the per-turn blocks on purpose: it is the only one that changes
/// daily, so everything above it stays in the provider's cached prefix.
///
/// A snapshot with no instructions — most chats — simply gets them appended, which is what
/// happened before and is still right when there is nothing they would come between.
#[must_use]
pub fn with_turn_blocks(snapshot: &str, blocks: &str) -> String {
    let snapshot = snapshot.trim_end();
    let blocks = blocks.trim();
    if blocks.is_empty() {
        return format!("{snapshot}\n");
    }
    match snapshot.find("<instructions scope=") {
        Some(at) => format!(
            "{}\n\n{blocks}\n\n{}\n",
            snapshot[..at].trim_end(),
            snapshot[at..].trim_end()
        ),
        None => format!("{snapshot}\n\n{blocks}\n"),
    }
}

/// What day it is, per turn (10 §2).
///
/// A model has no clock, so without this it either guesses the date or spends a round asking
/// `gantry__clock` for it — and it asks for more than you would think, because half of writing
/// anything is knowing whether "last Tuesday" has happened yet. The date cannot live in the
/// frozen snapshot: a chat opened on Friday and continued on Monday would insist it was still
/// Friday, which is worse than not knowing.
///
/// **The date and not the time**, deliberately. This block sits in the cached prefix, so the
/// time would invalidate the provider's prompt cache on every single turn to tell the model
/// something it almost never needs. It changes once a day, and `gantry__clock` is still there
/// for the questions that are actually about the hour.
#[must_use]
pub fn now_block() -> String {
    let now = chrono::Local::now();
    format!(
        "<gantry_now>
today: {} ({}), UTC{}
</gantry_now>",
        now.format("%Y-%m-%d"),
        chrono::Datelike::weekday(&now),
        now.format("%:z"),
    )
}

/// The connector inventory (03 §9, 04 §9, 10 §2): what this chat can reach, what is installed
/// but not attached, and how to ask for either.
///
/// It is assembled per turn rather than frozen with the snapshot, because installing or
/// attaching a connector happens outside the chat and a frozen list would go on lying about it.
/// It changes only when the connectors change, so the prefix still caches between turns.
#[must_use]
pub fn connector_inventory(
    installed: &[ConnectorInstanceDto],
    attached: &[String],
    can_suggest: bool,
) -> String {
    let is_attached = |i: &ConnectorInstanceDto| attached.iter().any(|n| n == &i.namespace);
    let usable: Vec<&ConnectorInstanceDto> = installed.iter().filter(|i| i.enabled).collect();
    let mut lines = vec!["<gantry_connectors>".to_owned()];
    lines.push(
        match describe(usable.iter().copied().filter(|i| is_attached(i))) {
            Some(list) => format!("attached to this chat: {list}"),
            None => "attached to this chat: none".to_owned(),
        },
    );
    if let Some(list) = describe(usable.iter().copied().filter(|i| !is_attached(i))) {
        lines.push(format!(
            "installed, not attached: {list} — call gantry__request_access to use one. The \
             user answers, unless the chat is in Auto mode, where the answer comes without \
             them."
        ));
    }
    lines.push(if can_suggest {
        "not installed: the catalog holds more. gantry__search_connectors finds them and \
         gantry__suggest_connector offers one to the user, who installs it or does not."
            .to_owned()
    } else {
        "not installed: gantry__search_connectors lists the catalog; the user has turned \
         suggestions off, so tell them what to install instead of offering it."
            .to_owned()
    });
    lines.push("</gantry_connectors>".to_owned());
    lines.join("\n")
}

/// `github (44 tools), supabase (needs sign-in)`, or nothing when the list is empty.
fn describe<'a>(instances: impl Iterator<Item = &'a ConnectorInstanceDto>) -> Option<String> {
    let parts: Vec<String> = instances
        .map(|i| {
            if i.auth_state == AuthState::Authorized {
                format!("{} ({} tools)", i.namespace, i.tools.len())
            } else {
                format!("{} (needs sign-in)", i.namespace)
            }
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// The `SystemNote` a chat is given when a memory it can see changes (12 §B6, 10 §4).
///
/// `carries` is whether this entry is in *this* chat's frozen prompt. It decides which of two
/// sentences gets sent, because they are different facts: a chat holding the old text has to be
/// told to stop using it, and a chat that never had it only needs the new one. A chat that
/// neither carries the entry nor would have been given it — a deleted fact it never saw — is
/// told nothing at all, which is what `None` means.
///
/// The long tail is deliberately absent from all of this: a `fact` or `note` is chosen per
/// message from a live query (§B4), so an edit to one takes effect on the next turn by itself,
/// and announcing it to every open chat would be noise for a change the model will see anyway.
#[must_use]
pub fn memory_note(text: &str, edit: &MemoryEdit, carries: bool) -> Option<String> {
    let text = text.trim();
    match edit {
        MemoryEdit::Changed { was } if carries => Some(format!(
            "Memory updated. This is no longer true and should not be used again: \"{}\". \
             It now reads: \"{text}\".",
            was.trim()
        )),
        MemoryEdit::Forgotten if carries => Some(format!(
            "Memory updated. Forget this and do not rely on it again: \"{text}\"."
        )),
        MemoryEdit::Forgotten | MemoryEdit::Refiled => None,
        // Added, Restored, and an edit to something this chat never held: one sentence, the
        // one that is true now.
        _ => Some(format!(
            "Memory updated. Remember this from here on: \"{text}\"."
        )),
    }
}

/// The `SystemNote` appended when a chat's permission mode changes (04 §3, 10 §4).
#[must_use]
pub fn mode_note(mode: Mode) -> String {
    format!(
        "Permission mode is now {}.\n{}",
        mode_label(mode),
        mode_text(mode).trim()
    )
}

/// The `SystemNote` appended when a chat's own instructions change (10 §4). A chat that has
/// already spoken keeps the prompt it was answering, so the note carries the whole new layer
/// rather than a description of the edit: the model has to know what the layer *is* now, and
/// "the user changed their instructions" tells it nothing it can act on.
#[must_use]
pub fn chat_instructions_note(text: &str) -> String {
    let text: String = text
        .trim()
        .chars()
        .take(CHAT_INSTRUCTIONS_MAX_CHARS)
        .collect();
    if text.is_empty() {
        "The user removed this chat's own instructions; earlier <instructions scope=\"chat\"> no longer apply."
            .to_owned()
    } else {
        format!(
            "Updated this chat's instructions (replacing any earlier ones):\n<instructions scope=\"chat\">\n{text}\n</instructions>"
        )
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Manual => "Manual",
        Mode::AutoEdit => "Auto-edit",
        Mode::Plan => "Plan",
        Mode::Auto => "Auto",
    }
}

fn mode_text(mode: Mode) -> &'static str {
    match mode {
        Mode::Manual => MODE_MANUAL,
        Mode::AutoEdit => MODE_AUTO_EDIT,
        Mode::Plan => MODE_PLAN,
        Mode::Auto => MODE_AUTO,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_placeholder_is_filled_and_layers_are_ordered() {
        let prompt = SystemPromptBuilder::new(
            Mode::Plan,
            PromptContext {
                platform: "linux".into(),
                app_version: "0.1.0".into(),
                workspace_roots: vec!["~/dev/gantry".into()],
                project_name: None,
            },
        )
        .global_instructions("  Answer in Norwegian.  ")
        .build();
        assert!(!prompt.contains("{{"));
        assert!(prompt.starts_with(&format!("<gantry_core version=\"{CORE_VERSION}\">")));
        let core = prompt.find("</gantry_core>").unwrap();
        let ctx = prompt.find("<gantry_context>").unwrap();
        let instr = prompt.find("<instructions scope=\"global\">").unwrap();
        assert!(core < ctx && ctx < instr);
        assert!(prompt.contains("<mode name=\"plan\">\nPermission mode: Plan."));
        assert!(prompt.contains("workspace: ~/dev/gantry"));
        assert!(prompt.contains("\nAnswer in Norwegian.\n</instructions>"));
    }

    #[test]
    fn empty_layers_leave_no_empty_tags() {
        let prompt = SystemPromptBuilder::new(Mode::Manual, PromptContext::default()).build();
        assert!(!prompt.contains("<instructions scope="));
        assert!(
            prompt.ends_with("</gantry_context>\n"),
            "no empty layer after the context"
        );
    }

    #[test]
    fn the_folders_are_rewritten_for_the_turn_that_is_starting() {
        let frozen =
            "<gantry_context>\nplatform: linux\nworkspace: none attached\n</gantry_context>\n";
        let attached = with_roots(frozen, &["/home/o/dev/site".to_owned()]);
        assert!(
            attached.contains("workspace: /home/o/dev/site"),
            "{attached}"
        );
        assert!(!attached.contains("none attached"), "{attached}");
        assert!(attached.ends_with("</gantry_context>\n"), "{attached}");
        // Detaching the last one says so again, and a prompt without the line is left alone.
        assert!(with_roots(&attached, &[]).contains("workspace: none attached"));
        assert_eq!(with_roots("no context here", &[]), "no context here");
    }

    /// The date is a per-turn block, not part of the frozen snapshot: a chat opened on Friday
    /// and continued on Monday must not go on insisting it is Friday. The time is left out on
    /// purpose — it would invalidate the cached prefix on every turn.
    #[test]
    fn the_now_block_carries_a_date_and_no_clock_time() {
        let block = now_block();
        assert!(block.starts_with("<gantry_now>"));
        assert!(block.ends_with("</gantry_now>"));
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(block.contains(&today), "{block}");
        assert!(
            !block.contains(&chrono::Local::now().format("%H:%M").to_string()),
            "the hour belongs to gantry__clock, not to the cached prefix: {block}"
        );
        let frozen = SystemPromptBuilder::new(
            Mode::Plan,
            PromptContext {
                platform: "linux".into(),
                app_version: "0.1.0".into(),
                workspace_roots: Vec::new(),
                project_name: None,
            },
        )
        .build();
        assert!(!frozen.contains("<gantry_now>"));
    }

    /// The user's standing instructions are the last thing in the system prompt, after every
    /// block the turn assembles. They are the most specific layer (10 §2) and the one a model
    /// is most often asked why it ignored; ending the prompt on a connector list and a date
    /// puts machinery where the user's own words should be.
    #[test]
    fn the_turn_blocks_go_above_the_user_instructions() {
        let snapshot = SystemPromptBuilder::new(
            Mode::AutoEdit,
            PromptContext {
                platform: "linux".into(),
                app_version: "0.1.0".into(),
                workspace_roots: Vec::new(),
                project_name: None,
            },
        )
        .global_instructions("Always answer in the language the question was written in.")
        .build();
        let assembled = with_turn_blocks(
            &snapshot,
            "<gantry_connectors>\nnone\n</gantry_connectors>\n\n<gantry_now>\ntoday: 2026-09-13\n</gantry_now>",
        );

        let connectors = assembled.find("<gantry_connectors>").unwrap();
        let now = assembled.find("<gantry_now>").unwrap();
        let instructions = assembled.find("<instructions scope=").unwrap();
        assert!(connectors < now, "the date stays last of the turn blocks");
        assert!(
            now < instructions,
            "the user's instructions come after all of it"
        );
        assert!(
            assembled.trim_end().ends_with("</instructions>"),
            "the prompt ends on the user's own words: {assembled}"
        );
        assert!(assembled.contains("Always answer in the language"));
    }

    /// Most chats have no instructions, and then there is nothing for the blocks to come
    /// before: appending is right, and is what happened before this rule existed.
    #[test]
    fn a_snapshot_without_instructions_just_gets_the_blocks_appended() {
        let snapshot = SystemPromptBuilder::new(
            Mode::AutoEdit,
            PromptContext {
                platform: "linux".into(),
                app_version: "0.1.0".into(),
                workspace_roots: Vec::new(),
                project_name: None,
            },
        )
        .build();
        let assembled =
            with_turn_blocks(&snapshot, "<gantry_now>\ntoday: 2026-09-13\n</gantry_now>");
        assert!(assembled.trim_end().ends_with("</gantry_now>"));
        assert!(!assembled.contains("<instructions scope="));
    }
}
