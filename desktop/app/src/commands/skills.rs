//! The Skills section of Customize (docs/plan/12 §A5, §A6, 15 A18).
//!
//! Four flows and nothing else: write one, export it, import one, and keep one the model
//! proposed. Every one of them ends at a file in `<app_data>/skills/`, which is the truth; the
//! index catches up through `rescan`.

use gantry_agent::skills::{format, import};
use gantry_core::{
    ChatId, ErrorDto, GantryError, SkillDetail, SkillDto, SkillInput, SkillVersionSource,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::{AppState, events::SkillsChanged};

/// Every skill, bundled and user-written, with the folder rescanned first so a file edited in
/// another editor shows up (12 §A3).
#[tauri::command]
#[specta::specta]
pub fn list_skills(state: State<'_, AppState>) -> Result<Vec<SkillDto>, ErrorDto> {
    state.skills.rescan()?;
    Ok(state.skills.list()?)
}

/// One skill with its body, for the editor.
#[tauri::command]
#[specta::specta]
pub fn get_skill(state: State<'_, AppState>, id: String) -> Result<SkillDetail, ErrorDto> {
    Ok(state.skills.detail(&id)?)
}

/// Saves a skill the user wrote or edited. The name is the folder name, so renaming one is
/// saving a new skill and deleting the old — which the UI says out loud rather than hiding.
#[tauri::command]
#[specta::specta]
pub fn save_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    input: SkillInput,
) -> Result<SkillDto, ErrorDto> {
    let saved = state.skills.save(&input, SkillVersionSource::UserEdit)?;
    let _ = SkillsChanged.emit(&app);
    Ok(saved)
}

/// Deletes a user's skill, folder and all. A bundled one can only be switched off.
#[tauri::command]
#[specta::specta]
pub fn delete_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), ErrorDto> {
    state.skills.delete(&id)?;
    let _ = SkillsChanged.emit(&app);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn set_skill_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), ErrorDto> {
    state.skills.set_enabled(&id, enabled)?;
    let _ = SkillsChanged.emit(&app);
    Ok(())
}

/// What the **Test match** box shows (12 §A5 flow 1): the score a sample message would get,
/// and the words that earned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MatchResult {
    pub score: u32,
    pub qualifies: bool,
    pub hits: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn test_skill_match(
    state: State<'_, AppState>,
    id: String,
    message: String,
) -> Result<MatchResult, ErrorDto> {
    use gantry_agent::skills::matcher;
    let skill = state
        .skills
        .get(&id)?
        .ok_or_else(|| GantryError::not_found(format!("skill {id}")))?;
    let score = matcher::score(&matcher::terms(&message), &skill);
    Ok(MatchResult {
        qualifies: score.score >= matcher::QUALIFYING_SCORE,
        score: score.score,
        hits: score.hits,
    })
}

/// The exact `SKILL.md`, and the name it should be written under (12 §A5 flow 2). The app
/// hands back text rather than writing a file: where it goes is the file dialog's business.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillExport {
    pub filename: String,
    pub text: String,
}

#[tauri::command]
#[specta::specta]
pub fn export_skill(state: State<'_, AppState>, id: String) -> Result<SkillExport, ErrorDto> {
    Ok(SkillExport {
        filename: import::export_filename(&id),
        text: state.skills.text(&id)?,
    })
}

/// The review screen (12 §A5 flow 3). Nothing is written by this; `install_skill` is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SkillReview {
    pub input: SkillInput,
    /// The file as it arrived, installed byte for byte if the user says yes.
    pub text: String,
    pub warnings: Vec<String>,
    /// A review with problems cannot be installed.
    pub problems: Vec<String>,
    /// The name it would take: repaired if it had to be, and free of collisions.
    pub name: String,
    /// Set when `name` is already taken, so the screen can say what it would replace.
    pub replaces: Option<String>,
}

impl SkillReview {
    fn from(review: import::Review, state: &AppState) -> Result<Self, GantryError> {
        let taken = state.skills.get(&review.name)?.map(|s| s.name);
        Ok(Self {
            input: review.input,
            text: review.text,
            warnings: review.warnings,
            problems: review.problems,
            name: review.name,
            replaces: taken,
        })
    }
}

/// Reviews a skill from a file the user picked, a folder, or pasted text.
#[tauri::command]
#[specta::specta]
pub fn review_skill(
    state: State<'_, AppState>,
    path: Option<String>,
    text: Option<String>,
) -> Result<SkillReview, ErrorDto> {
    let review = match (path.as_deref(), text.as_deref()) {
        (Some(path), _) => {
            let p = std::path::Path::new(path);
            if p.is_dir() {
                import::review_folder(p).map_err(GantryError::invalid)?
            } else {
                let text = std::fs::read_to_string(p).map_err(|e| {
                    GantryError::invalid(format!("could not read {}: {e}", p.display()))
                })?;
                import::review_text(&text, Some(path))
            }
        }
        (None, Some(text)) => import::review_text(text, None),
        (None, None) => {
            return Err(GantryError::invalid("give a path or some text").into());
        }
    };
    Ok(SkillReview::from(review, &state)?)
}

/// Fetches a skill from a URL for review (12 §A5 flow 3): HTTPS only, no redirect to another
/// host, text only, and capped — it is somebody else's file, and it is about to be read by a
/// model that trusts its prompt.
#[tauri::command]
#[specta::specta]
pub async fn review_skill_url(
    state: State<'_, AppState>,
    url: String,
) -> Result<SkillReview, ErrorDto> {
    let parsed = url::Url::parse(&url).map_err(|e| GantryError::invalid(format!("{url}: {e}")))?;
    if parsed.scheme() != "https" {
        return Err(GantryError::invalid("only https URLs are fetched").into());
    }
    let host = parsed.host_str().unwrap_or_default().to_owned();
    let response = gantry_providers::http_client(env!("CARGO_PKG_VERSION"))
        .get(parsed.clone())
        .send()
        .await
        .map_err(|e| GantryError::invalid(format!("could not fetch {url}: {e}")))?;
    if response.url().host_str().unwrap_or_default() != host {
        return Err(GantryError::invalid(format!(
            "{url} redirected to another host ({}); fetch it yourself and import the file.",
            response.url().host_str().unwrap_or_default()
        ))
        .into());
    }
    if !response.status().is_success() {
        return Err(GantryError::invalid(format!("{url} answered {}", response.status())).into());
    }
    let text = response
        .text()
        .await
        .map_err(|e| GantryError::invalid(format!("could not read {url}: {e}")))?;
    if text.len() > import::URL_MAX_BYTES {
        return Err(GantryError::invalid(format!(
            "{url} is {} KB; a skill fetched from a URL may be {} KB.",
            text.len() / 1024,
            import::URL_MAX_BYTES / 1024
        ))
        .into());
    }
    Ok(SkillReview::from(
        import::review_text(&text, Some(parsed.path())),
        &state,
    )?)
}

/// Installs a reviewed skill, byte for byte, under `name` — which the user may have changed on
/// the review screen to avoid a collision.
#[tauri::command]
#[specta::specta]
pub fn install_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    text: String,
    references: Vec<gantry_core::SkillReference>,
) -> Result<SkillDto, ErrorDto> {
    // The name on the screen wins over the one in the file, so the frontmatter is rewritten to
    // agree with the folder rather than left contradicting it.
    let text = rename(&text, &name);
    let installed = state.skills.install_verbatim(&name, &text, &references)?;
    let _ = SkillsChanged.emit(&app);
    Ok(installed)
}

/// Rewrites the frontmatter's `name:` line and leaves every other byte alone — including a
/// `name:` inside `metadata:` and one in the body, which are not this field.
fn rename(text: &str, name: &str) -> String {
    match format::parse(text) {
        Ok(parsed) if parsed.front.name == name => text.to_owned(),
        Ok(_) => {
            let mut out = Vec::new();
            let mut in_front = false;
            let mut done = false;
            for (i, line) in text.lines().enumerate() {
                if line.trim_end() == "---" {
                    if i == 0 {
                        in_front = true;
                    } else if in_front {
                        in_front = false;
                    }
                    out.push(line.to_owned());
                    continue;
                }
                if in_front && !done && line.starts_with("name:") {
                    out.push(format!("name: {name}"));
                    done = true;
                    continue;
                }
                out.push(line.to_owned());
            }
            let mut joined = out.join("\n");
            if text.ends_with('\n') {
                joined.push('\n');
            }
            joined
        }
        Err(_) => text.to_owned(),
    }
}

/// The skills pinned to one chat (12 §A6). A pinned skill is in that chat's frozen prompt.
#[tauri::command]
#[specta::specta]
pub fn list_chat_skills(
    state: State<'_, AppState>,
    chat_id: ChatId,
) -> Result<Vec<String>, ErrorDto> {
    Ok(state.turns.chats().pinned_skills(chat_id)?)
}

#[tauri::command]
#[specta::specta]
pub fn pin_skill_to_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    chat_id: ChatId,
    skill_id: String,
    pinned: bool,
) -> Result<(), ErrorDto> {
    state.turns.chats().pin_skill(chat_id, &skill_id, pinned)?;
    let _ = SkillsChanged.emit(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::rename;

    #[test]
    fn installing_under_another_name_rewrites_only_that_line() {
        let text = "---\nname: helper\ndescription: Helps.\nmetadata:\n  name: not this one\n---\nname: not this either\n";
        let out = rename(text, "helper-2");
        assert!(out.contains("\nname: helper-2\n"), "{out}");
        assert!(
            out.contains("  name: not this one"),
            "nested keys are left alone: {out}"
        );
        assert!(
            out.trim_end().ends_with("name: not this either"),
            "the body is left alone: {out}"
        );
    }
}
