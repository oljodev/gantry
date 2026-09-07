//! Exporting one chat as Markdown or JSON (docs/plan/11 §2, Data & privacy).

use gantry_core::{ChatDetail, ContentPart};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Markdown,
    Json,
}

impl ExportFormat {
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Json => "json",
        }
    }
}

/// The chat as a file. JSON is the `ChatDetail` verbatim; Markdown keeps the readable parts.
pub fn render(chat: &ChatDetail, format: ExportFormat) -> String {
    match format {
        ExportFormat::Json => serde_json::to_string_pretty(chat).unwrap_or_default(),
        ExportFormat::Markdown => markdown(chat),
    }
}

fn markdown(chat: &ChatDetail) -> String {
    let mut out = format!("# {}\n\n", chat.title);
    out.push_str(&format!(
        "Model: {} · Mode: {}\n\n",
        chat.model.model,
        chat.mode.as_str()
    ));
    for t in &chat.turns {
        out.push_str("## You\n\n");
        for p in &t.user.parts {
            match p {
                ContentPart::Text { text } => {
                    out.push_str(text);
                    out.push_str("\n\n");
                }
                ContentPart::Document { name, .. } => {
                    out.push_str(&format!("*Attached: {name}*\n\n"))
                }
                ContentPart::Image { mime, .. } => {
                    out.push_str(&format!("*Attached image ({mime})*\n\n"))
                }
                _ => {}
            }
        }
        out.push_str("## Assistant\n\n");
        let mut wrote = false;
        for m in &t.messages {
            for p in &m.parts {
                match p {
                    ContentPart::Text { text } if !text.trim().is_empty() => {
                        out.push_str(text);
                        out.push_str("\n\n");
                        wrote = true;
                    }
                    ContentPart::ToolCall { name, args, .. } => {
                        out.push_str(&format!("*Called `{name}` with `{args}`*\n\n"));
                        wrote = true;
                    }
                    _ => {}
                }
            }
        }
        if !wrote {
            out.push_str("*(no reply)*\n\n");
        }
        if let Some(err) = &t.error {
            out.push_str(&format!("> Error: {err}\n\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use gantry_core::{
        ChatId, Message, Mode, ModelRef, ReasoningEffort, TurnDto, TurnId, TurnStatus,
    };

    use super::*;

    #[test]
    fn markdown_has_the_title_and_both_sides() {
        let chat = ChatDetail {
            id: ChatId::new(),
            title: "T".into(),
            pinned: false,
            archived: false,
            project_id: None,
            created_at: 0,
            last_message_at: 0,
            model: ModelRef::default_model(),
            mode: Mode::Plan,
            guard: true,
            effort: ReasoningEffort::Off,
            active_turn: None,
            turns: vec![TurnDto {
                id: TurnId::new(),
                status: TurnStatus::Completed,
                model: ModelRef::default_model(),
                user: Message::user_text("Q?"),
                messages: vec![{
                    let mut m = Message::user_text("A.");
                    m.role = gantry_core::Role::Assistant;
                    m
                }],
                tool_calls: Vec::new(),
                usage: None,
                stop_reason: None,
                error: None,
                feedback: None,
                started_at: 0,
                ended_at: None,
            }],
        };
        let md = render(&chat, ExportFormat::Markdown);
        assert!(md.starts_with("# T\n"));
        assert!(md.contains("## You\n\nQ?") && md.contains("## Assistant\n\nA."));
        let json = render(&chat, ExportFormat::Json);
        assert!(json.contains("\"title\": \"T\""));
    }
}
