//! Assembling a chat's system prompt from the fixed core and the additive layers, in the order
//! of docs/plan/10 §2. Built once per chat and frozen as its `system_snapshot`.

use gantry_core::Mode;

/// Bumped whenever `assets/prompts/core.md` or a mode fragment changes meaning.
pub const CORE_VERSION: u32 = 1;

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
    global_instructions: String,
}

impl SystemPromptBuilder {
    #[must_use]
    pub fn new(mode: Mode, context: PromptContext) -> Self {
        Self {
            mode,
            context,
            global_instructions: String::new(),
        }
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

    #[must_use]
    pub fn build(&self) -> String {
        let mut blocks: Vec<String> = Vec::new();
        blocks.push(
            CORE.trim_end()
                .replace("{{mode}}", self.mode.as_str())
                .replace("{{mode_text}}", mode_text(self.mode).trim()),
        );
        blocks.push(self.context_block());
        if !self.global_instructions.is_empty() {
            blocks.push(format!(
                "<instructions scope=\"global\">\n{}\n</instructions>",
                self.global_instructions
            ));
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
        lines.push("connectors: none attached".to_owned());
        lines.push("</gantry_context>".to_owned());
        lines.join("\n")
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
        assert!(prompt.starts_with("<gantry_core version=\"1\">"));
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
}
