//! What reaches a prompt, and what it costs (docs/plan/12 §A4, §B4, 10 §2, §5).
//!
//! Two budgets, and they are the reason memory can grow without a turn growing with it. The
//! **core set** is standing behaviour, chosen once at chat creation and frozen into the prompt
//! (~1,500 tokens). The **long tail** is looked up per message (~800 tokens). A store of five
//! hundred facts costs the same per turn as a store of five, which is the whole arithmetic of
//! §B4 and the reason there is no third tier that sends everything.
//!
//! Both budgets are counted in characters at four per token. Nothing here talks to a
//! tokenizer, and a ceiling that is roughly right and always cheap is worth more than one that
//! is exact and costs a model call.

use std::collections::HashSet;

use gantry_core::{
    ContentPart, InjectedContext, InjectedMemory, InjectedSkill, MemoryDto, Message, ProjectId,
    Role, SkillDto, memory,
};
use gantry_store::{Connection, repos};

use crate::skills::matcher;

/// Renders the `<memory>` block of a frozen prompt (10 §2, layer 3), and says which entries it
/// spent, so the chat can record them in `snapshot_memory_ids_json`.
#[must_use]
pub fn core_block(entries: &[MemoryDto]) -> (String, Vec<gantry_core::MemoryId>) {
    let mut lines = Vec::new();
    let mut ids = Vec::new();
    let mut budget = memory::CORE_SET_MAX_CHARS;
    for m in entries {
        let line = format!("- {}: {}", m.kind.as_str(), m.text.trim());
        // The newline that will separate it counts too, or a store of short entries slips the
        // budget one character at a time.
        if line.len() + 1 > budget {
            break;
        }
        budget -= line.len() + 1;
        lines.push(line);
        ids.push(m.id);
    }
    if lines.is_empty() {
        return (String::new(), ids);
    }
    let block = format!("<memory>\n{}\n</memory>", lines.join("\n"));
    (block, ids)
}

/// The skills behind a list of pinned ids, with their bodies, ready for [`pinned_block`].
///
/// A pin that names a skill which is gone, or disabled, is skipped rather than rendered as an
/// empty tag: the pin is a row in another table, and the skill it points at can be uninstalled.
#[must_use]
pub fn bodies(conn: &Connection, ids: &[String]) -> Vec<(SkillDto, String)> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let Ok(Some(skill)) = repos::skills::get(conn, id) else {
            continue;
        };
        if !skill.enabled {
            continue;
        }
        if let Some(body) = skill_body(conn, &skill) {
            out.push((skill, body));
        }
    }
    out
}

/// The pinned skills a frozen prompt carries (10 §2, layer 7).
#[must_use]
pub fn pinned_block(skills: &[(SkillDto, String)]) -> String {
    let mut blocks = Vec::new();
    for (skill, body) in skills {
        let body: String = body
            .chars()
            .take(gantry_core::skill::PINNED_MAX_BYTES)
            .collect();
        blocks.push(format!(
            "<skill name=\"{}\" source=\"{}\">\n{}\n</skill>",
            skill.name,
            skill.source.as_str(),
            body.trim()
        ));
    }
    if blocks.is_empty() {
        return String::new();
    }
    format!("<skills>\n{}\n</skills>", blocks.join("\n\n"))
}

/// What one turn adds beyond the transcript, and the block that carries it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnContext {
    /// The text appended to the user's message, already carrying its own blank line so a
    /// projection can concatenate it without knowing what it is.
    pub text: String,
    pub injected: InjectedContext,
}

impl TurnContext {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The part a user message carries (02 §6: written down, not added at request time).
    #[must_use]
    pub fn part(self) -> ContentPart {
        ContentPart::TurnContext {
            text: self.text,
            injected: self.injected,
        }
    }
}

/// Everything the turn block needs, read from one connection so the selection happens inside
/// the same transaction that records the user's message.
pub struct Selection<'a> {
    pub conn: &'a Connection,
    pub project: Option<ProjectId>,
    /// The transcript so far, for the "not again within six turns" rule.
    pub transcript: &'a [Message],
    /// Skills the user forced with `/name` in the composer (12 §A4 rule 5).
    pub invoked: &'a [String],
    /// Skills pinned to this chat or its project: they are in the frozen prompt already.
    pub pinned: &'a [String],
    /// Off means no memory is injected at all (Settings → Memory).
    pub memory_on: bool,
    /// Off means no skill is injected either, which is how a sub agent whose type says so runs
    /// on its own instructions and nothing else (18 §3).
    pub skills_on: bool,
}

/// Builds the `<gantry_turn_context>` block for one message (10 §5).
pub fn build(message: &str, selection: &Selection<'_>) -> TurnContext {
    let recent = recently_injected(selection.transcript);
    let mut injected = InjectedContext::default();
    let mut blocks: Vec<String> = Vec::new();

    // --- Skills (12 §A4) ---------------------------------------------------------------
    let all = if selection.skills_on {
        repos::skills::enabled(selection.conn).unwrap_or_else(|err| {
            log::warn!("could not read the skills index: {err}");
            Vec::new()
        })
    } else {
        Vec::new()
    };
    let mut chosen: Vec<(SkillDto, &'static str)> = Vec::new();
    // An explicit `/name` wins over everything, including the six-turn rule: the user asked.
    for name in selection.invoked {
        if let Some(skill) = all.iter().find(|s| &s.id == name) {
            chosen.push((skill.clone(), "invoked"));
        }
    }
    let mut skip: HashSet<String> = recent.skills.clone();
    skip.extend(selection.pinned.iter().cloned());
    skip.extend(chosen.iter().map(|(s, _)| s.id.clone()));
    for score in matcher::matches(message, &all, &skip) {
        if let Some(skill) = all.iter().find(|s| s.id == score.id) {
            chosen.push((skill.clone(), "matched"));
        }
    }

    for (skill, how) in &chosen {
        let Some(body) = skill_body(selection.conn, skill) else {
            continue;
        };
        blocks.push(format!(
            "<skill name=\"{}\" source=\"{}\">\n{}\n</skill>",
            skill.name,
            skill.source.as_str(),
            body.trim()
        ));
        injected.skills.push(InjectedSkill {
            name: skill.name.clone(),
            source: skill.source,
            how: (*how).to_owned(),
        });
    }

    // --- The long tail of memory (12 §B4) ----------------------------------------------
    if selection.memory_on {
        let terms = matcher::terms(message);
        let hits = repos::memories::long_tail(
            selection.conn,
            selection.project,
            &terms.raw,
            memory::LONG_TAIL_LIMIT * 2,
        )
        .unwrap_or_else(|err| {
            log::warn!("could not select memories for this message: {err}");
            Vec::new()
        });
        let mut budget = memory::LONG_TAIL_CHARS;
        let mut lines = Vec::new();
        for m in hits {
            if recent.memories.contains(&m.id) {
                continue;
            }
            let line = format!("- {}", m.text.trim());
            if line.len() + 1 > budget || lines.len() >= memory::LONG_TAIL_LIMIT {
                break;
            }
            budget -= line.len() + 1;
            lines.push(line);
            injected.memories.push(InjectedMemory {
                id: m.id,
                kind: m.kind,
                text: m.text,
            });
        }
        if !lines.is_empty() {
            blocks.push(format!("<memory>\n{}\n</memory>", lines.join("\n")));
        }
    }

    if blocks.is_empty() {
        return TurnContext::default();
    }
    TurnContext {
        text: format!(
            "\n\n<gantry_turn_context>\n{}\n</gantry_turn_context>",
            blocks.join("\n")
        ),
        injected,
    }
}

/// The whole `SKILL.md` body for a skill in the index. A bundled skill is read from the last
/// version snapshot rather than from the binary, because this runs where there is a connection
/// and not where there is a `Skills` service — and the snapshot is written from the binary on
/// every scan, so it says the same thing.
fn skill_body(conn: &Connection, skill: &SkillDto) -> Option<String> {
    let text = repos::skills::version_content(conn, &skill.id, skill.version)
        .ok()
        .flatten()
        .or_else(|| {
            skill.path.as_deref().and_then(|p| {
                std::fs::read_to_string(std::path::Path::new(p).join("SKILL.md")).ok()
            })
        })?;
    Some(crate::skills::format::parse(&text).map_or(text, |p| p.body))
}

/// What the last few turns already injected (12 §A4 rule 3, §B4).
#[derive(Debug, Default)]
struct Recent {
    skills: HashSet<String>,
    memories: HashSet<gantry_core::MemoryId>,
}

/// Reads the transcript backwards for the context blocks of the last `RECENT_TURNS` user
/// messages. The transcript is where this lives because the transcript is where the injected
/// text lives: if a copy is still in the model's context, re-sending it buys nothing.
fn recently_injected(transcript: &[Message]) -> Recent {
    let mut recent = Recent::default();
    let mut turns = 0;
    for message in transcript.iter().rev() {
        if message.role != Role::User {
            continue;
        }
        turns += 1;
        if turns > memory::RECENT_TURNS {
            break;
        }
        for part in &message.parts {
            if let ContentPart::TurnContext { injected, .. } = part {
                recent
                    .skills
                    .extend(injected.skills.iter().map(|s| s.name.clone()));
                recent
                    .memories
                    .extend(injected.memories.iter().map(|m| m.id));
            }
        }
    }
    recent
}

#[cfg(test)]
mod tests {
    use gantry_core::{MemoryId, MemoryKind, MessageId, now_ms};

    use super::*;

    fn entry(kind: MemoryKind, text: &str) -> MemoryDto {
        MemoryDto {
            id: MemoryId::new(),
            scope_kind: gantry_core::MemoryScopeKind::Global,
            scope_id: None,
            kind,
            text: text.to_owned(),
            always_include: false,
            source: gantry_core::MemorySource::User,
            origin_chat_id: None,
            origin_message_id: None,
            tags: Vec::new(),
            enabled: true,
            use_count: 0,
            last_used_at: None,
            created_at: 0,
            updated_at: 0,
            archived_at: None,
        }
    }

    #[test]
    fn the_core_block_names_the_kind_and_stops_at_its_budget() {
        let entries = vec![
            entry(MemoryKind::Instruction, "answer in Norwegian"),
            entry(MemoryKind::Preference, "prefer pnpm"),
        ];
        let (block, ids) = core_block(&entries);
        assert!(block.starts_with("<memory>\n- instruction: answer in Norwegian"));
        assert!(block.ends_with("</memory>"));
        assert_eq!(ids.len(), 2);

        let many: Vec<MemoryDto> = (0..500)
            .map(|i| entry(MemoryKind::Preference, &format!("preference number {i}")))
            .collect();
        let (block, ids) = core_block(&many);
        assert!(
            block.len() <= memory::CORE_SET_MAX_CHARS + 64,
            "the frozen prompt cannot grow with the store: {} chars",
            block.len()
        );
        assert!(ids.len() < many.len());
    }

    #[test]
    fn an_empty_core_set_is_no_block_at_all() {
        assert_eq!(core_block(&[]).0, "");
        assert_eq!(pinned_block(&[]), "");
    }

    #[test]
    fn the_last_six_turns_are_what_counts_as_recent() {
        let user = |injected: InjectedContext| Message {
            id: MessageId::new(),
            role: Role::User,
            parts: vec![ContentPart::TurnContext {
                text: String::new(),
                injected,
            }],
            origin: None,
            created_at: now_ms(),
        };
        let with = |name: &str| InjectedContext {
            skills: vec![InjectedSkill {
                name: name.to_owned(),
                source: gantry_core::SkillSource::Bundled,
                how: "matched".to_owned(),
            }],
            memories: Vec::new(),
        };
        let mut transcript = vec![user(with("old-one"))];
        for i in 0..memory::RECENT_TURNS {
            transcript.push(user(with(&format!("recent-{i}"))));
        }
        let recent = recently_injected(&transcript);
        assert!(recent.skills.contains("recent-0"));
        assert!(
            !recent.skills.contains("old-one"),
            "seven turns ago is far enough to send again: {:?}",
            recent.skills
        );
    }
}
