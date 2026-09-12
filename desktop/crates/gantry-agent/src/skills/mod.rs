//! Skills (docs/plan/12 §A): the index, the folder on disk, and the four flows.
//!
//! The truth of a skill is its text. A bundled skill's text is in the binary; a user's is in
//! `<app_data>/skills/<name>/SKILL.md`, where they can edit it in any editor, keep it in Git,
//! or delete the folder. The `skills` table is a projection that exists so matching and the
//! list do not touch the filesystem, and `rescan` is what keeps the projection honest.

pub mod format;
pub mod import;
pub mod matcher;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use gantry_core::{
    GantryError, SkillDetail, SkillDto, SkillInput, SkillSource, SkillVersionSource, now_ms, skill,
};
use gantry_store::{Store, repos};

include!(concat!(env!("OUT_DIR"), "/bundled_skills.rs"));

/// What a skill list line costs in a frozen prompt (12 §A4): enough of the description to
/// decide whether to load it, and no more.
const INVENTORY_DESCRIPTION_CHARS: usize = 160;
/// How many skills the frozen list names before it stops and points at the tool instead.
const INVENTORY_MAX: usize = 40;

pub struct Skills {
    store: Arc<Store>,
    /// `<app_data>/skills`. Created on first write, not on startup: a user with no skills of
    /// their own should not find an empty folder they did not ask for.
    dir: PathBuf,
}

impl Skills {
    #[must_use]
    pub fn new(store: Arc<Store>, dir: PathBuf) -> Arc<Self> {
        Arc::new(Self { store, dir })
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Brings the index in step with what is actually there (12 §A3): the bundled set from the
    /// binary, and every folder under `<app_data>/skills`. Cheap enough to run before a turn —
    /// a `stat` per folder — and run on every Skills page open and window focus besides.
    ///
    /// A file that changed under us is re-indexed and snapshotted as an `external_change`, so
    /// editing a skill in another editor is a supported way to work rather than a way to lose
    /// the version history.
    pub fn rescan(&self) -> Result<(), GantryError> {
        let mut seen: HashSet<String> = HashSet::new();
        let mut found: Vec<(SkillDto, String)> = Vec::new();

        for (name, text, references) in BUNDLED {
            let refs: Vec<String> = references.iter().map(|(f, _)| (*f).to_owned()).collect();
            match self.indexed(name, text, SkillSource::Bundled, None, refs) {
                Ok(dto) => {
                    seen.insert((*name).to_owned());
                    found.push((dto, (*text).to_owned()));
                }
                Err(err) => log::error!("the bundled skill {name} does not parse: {err}"),
            }
        }

        if self.dir.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&self.dir)
                .map_err(|e| GantryError::internal(format!("reading {}: {e}", self.dir.display())))?
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            entries.sort();
            for dir in entries {
                let Some(name) = dir.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
                    continue;
                };
                let file = dir.join("SKILL.md");
                if !file.exists() {
                    continue;
                }
                if seen.contains(&name) {
                    log::warn!(
                        "the folder {} has the name of a bundled skill and is ignored; rename it \
                         to use it",
                        dir.display()
                    );
                    continue;
                }
                let text = match std::fs::read_to_string(&file) {
                    Ok(t) => t,
                    Err(err) => {
                        log::warn!("could not read {}: {err}", file.display());
                        continue;
                    }
                };
                let refs = reference_names(&dir);
                match self.indexed(
                    &name,
                    &text,
                    SkillSource::User,
                    Some(dir.to_string_lossy().into_owned()),
                    refs,
                ) {
                    Ok(dto) => {
                        seen.insert(name);
                        found.push((dto, text));
                    }
                    Err(err) => log::warn!("{} does not parse: {err}", file.display()),
                }
            }
        }

        let known = self.store.read(repos::skills::list)?;
        self.store.write_blocking(move |conn| {
            for (mut dto, text) in found {
                let before = known.iter().find(|s| s.id == dto.id);
                if let Some(before) = before {
                    // The user's switch survives a rescan; the file does not carry it.
                    dto.enabled = before.enabled;
                    dto.installed_at = before.installed_at;
                    dto.use_count = before.use_count;
                    dto.last_used_at = before.last_used_at;
                    if before.content_hash == dto.content_hash {
                        continue;
                    }
                    // Changed under us: keep the file's own version number when it moved, and
                    // otherwise count the change ourselves so the history stays ordered.
                    if dto.version <= before.version {
                        dto.version = before.version + 1;
                    }
                    // The index row goes in first: a version points at a skill.
                    repos::skills::upsert(conn, &dto)?;
                    repos::skills::add_version(
                        conn,
                        &dto.id,
                        dto.version,
                        &text,
                        SkillVersionSource::ExternalChange,
                    )?;
                } else {
                    repos::skills::upsert(conn, &dto)?;
                    repos::skills::add_version(
                        conn,
                        &dto.id,
                        dto.version,
                        &text,
                        if dto.source == SkillSource::Bundled {
                            SkillVersionSource::Bundled
                        } else {
                            SkillVersionSource::ExternalChange
                        },
                    )?;
                }
            }
            // A folder that is gone leaves the index — the file is the truth.
            for gone in known.iter().filter(|s| !seen.contains(&s.id)) {
                repos::skills::delete(conn, &gone.id)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    fn indexed(
        &self,
        name: &str,
        text: &str,
        source: SkillSource,
        path: Option<String>,
        references: Vec<String>,
    ) -> Result<SkillDto, String> {
        let parsed = format::parse(text)?;
        if parsed.front.name != name {
            return Err(format!(
                "the frontmatter says `{}`; the folder is `{name}`, and the folder name is the \
                 name",
                parsed.front.name
            ));
        }
        let now = now_ms();
        Ok(SkillDto {
            id: name.to_owned(),
            source,
            path,
            name: name.to_owned(),
            description: parsed.front.description.clone(),
            triggers: parsed.front.triggers(),
            always_include: parsed.front.always(),
            enabled: true,
            content_hash: hash(text),
            size: u32::try_from(text.len()).unwrap_or(u32::MAX),
            version: parsed.front.version(),
            author: parsed.front.author(),
            license: parsed.front.license.clone(),
            references,
            installed_at: now,
            updated_at: now,
            last_used_at: None,
            use_count: 0,
            pinned_count: 0,
        })
    }

    pub fn list(&self) -> Result<Vec<SkillDto>, GantryError> {
        Ok(self.store.read(repos::skills::list)?)
    }

    pub fn get(&self, id: &str) -> Result<Option<SkillDto>, GantryError> {
        let id = id.to_owned();
        Ok(self.store.read(move |c| repos::skills::get(c, &id))?)
    }

    /// The whole `SKILL.md`, from the binary or from the file.
    pub fn text(&self, id: &str) -> Result<String, GantryError> {
        if let Some((_, text, _)) = BUNDLED.iter().find(|(n, _, _)| *n == id) {
            return Ok((*text).to_owned());
        }
        let skill = self
            .get(id)?
            .ok_or_else(|| GantryError::not_found(format!("skill {id}")))?;
        let path = skill
            .path
            .map(PathBuf::from)
            .unwrap_or_else(|| self.dir.join(id))
            .join("SKILL.md");
        std::fs::read_to_string(&path)
            .map_err(|e| GantryError::internal(format!("reading {}: {e}", path.display())))
    }

    /// The skill with its body, for the editor and for `gantry__load_skill`.
    pub fn detail(&self, id: &str) -> Result<SkillDetail, GantryError> {
        let skill = self
            .get(id)?
            .ok_or_else(|| GantryError::not_found(format!("skill {id}")))?;
        let text = self.text(id)?;
        let body = format::parse(&text).map(|p| p.body).unwrap_or(text);
        Ok(SkillDetail { skill, body })
    }

    /// One `references/*.md`, for `gantry__read_skill_file`.
    pub fn reference(&self, id: &str, file: &str) -> Result<String, GantryError> {
        if file.contains('/') || file.contains('\\') || file.contains("..") {
            return Err(GantryError::invalid(
                "a reference is a plain file name inside the skill's `references/` folder",
            ));
        }
        if let Some((_, _, refs)) = BUNDLED.iter().find(|(n, _, _)| *n == id) {
            return refs
                .iter()
                .find(|(f, _)| *f == file)
                .map(|(_, text)| (*text).to_owned())
                .ok_or_else(|| GantryError::not_found(format!("{id} has no reference {file}")));
        }
        let skill = self
            .get(id)?
            .ok_or_else(|| GantryError::not_found(format!("skill {id}")))?;
        let path = skill
            .path
            .map(PathBuf::from)
            .unwrap_or_else(|| self.dir.join(id))
            .join("references")
            .join(file);
        std::fs::read_to_string(&path)
            .map_err(|e| GantryError::not_found(format!("reading {}: {e}", path.display())))
    }

    /// Writes a skill the editor or a proposal produced: the file first, then the snapshot,
    /// then the index. A bundled name is refused rather than shadowed (12 §A5 flow 4).
    pub fn save(
        &self,
        input: &SkillInput,
        source: SkillVersionSource,
    ) -> Result<SkillDto, GantryError> {
        let problems = skill::validate(input);
        if !problems.is_empty() {
            return Err(GantryError::invalid(problems.join(" ")));
        }
        if BUNDLED.iter().any(|(n, _, _)| *n == input.name) {
            return Err(GantryError::invalid(format!(
                "`{}` is a skill that ships with Gantry and cannot be replaced. Save it under \
                 another name instead.",
                input.name
            )));
        }
        let dir = self.dir.join(&input.name);
        let existing = self.get(&input.name)?;
        let version = existing.as_ref().map_or(1, |s| s.version + 1);
        let text = format::render(input, version);
        self.write_folder(&dir, &text, &input.references)?;

        let mut dto = self
            .indexed(
                &input.name,
                &text,
                existing.as_ref().map_or(SkillSource::User, |s| s.source),
                Some(dir.to_string_lossy().into_owned()),
                input.references.iter().map(|r| r.file.clone()).collect(),
            )
            .map_err(GantryError::invalid)?;
        if let Some(before) = &existing {
            dto.enabled = before.enabled;
            dto.installed_at = before.installed_at;
            dto.use_count = before.use_count;
            dto.last_used_at = before.last_used_at;
        }
        let written = dto.clone();
        self.store.write_blocking(move |conn| {
            repos::skills::upsert(conn, &dto)?;
            repos::skills::add_version(conn, &dto.id, dto.version, &text, source)?;
            Ok(())
        })?;
        Ok(written)
    }

    /// Installs an imported skill byte for byte: whatever the author wrote is what lands on
    /// disk, so a field Gantry does not read still travels with the file.
    pub fn install_verbatim(
        &self,
        name: &str,
        text: &str,
        references: &[gantry_core::SkillReference],
    ) -> Result<SkillDto, GantryError> {
        if BUNDLED.iter().any(|(n, _, _)| n == &name) {
            return Err(GantryError::invalid(format!(
                "`{name}` is a skill that ships with Gantry. Install this one under another name."
            )));
        }
        let dir = self.dir.join(name);
        self.write_folder(&dir, text, references)?;
        let dto = self
            .indexed(
                name,
                text,
                SkillSource::Imported,
                Some(dir.to_string_lossy().into_owned()),
                references.iter().map(|r| r.file.clone()).collect(),
            )
            .map_err(GantryError::invalid)?;
        let written = dto.clone();
        let text = text.to_owned();
        self.store.write_blocking(move |conn| {
            repos::skills::upsert(conn, &dto)?;
            repos::skills::add_version(
                conn,
                &dto.id,
                dto.version,
                &text,
                SkillVersionSource::Import,
            )?;
            Ok(())
        })?;
        Ok(written)
    }

    fn write_folder(
        &self,
        dir: &Path,
        text: &str,
        references: &[gantry_core::SkillReference],
    ) -> Result<(), GantryError> {
        std::fs::create_dir_all(dir)
            .map_err(|e| GantryError::internal(format!("creating {}: {e}", dir.display())))?;
        let file = dir.join("SKILL.md");
        std::fs::write(&file, text)
            .map_err(|e| GantryError::internal(format!("writing {}: {e}", file.display())))?;
        if references.is_empty() {
            return Ok(());
        }
        let ref_dir = dir.join("references");
        std::fs::create_dir_all(&ref_dir)
            .map_err(|e| GantryError::internal(format!("creating {}: {e}", ref_dir.display())))?;
        for r in references {
            let path = ref_dir.join(&r.file);
            std::fs::write(&path, &r.text)
                .map_err(|e| GantryError::internal(format!("writing {}: {e}", path.display())))?;
        }
        Ok(())
    }

    /// Removes a user's skill, folder and all. A bundled one can only be switched off.
    pub fn delete(&self, id: &str) -> Result<(), GantryError> {
        let skill = self
            .get(id)?
            .ok_or_else(|| GantryError::not_found(format!("skill {id}")))?;
        if !skill.source.is_editable() {
            return Err(GantryError::invalid(format!(
                "`{id}` ships with Gantry. It can be switched off, but not deleted."
            )));
        }
        if let Some(path) = skill.path.as_deref().map(PathBuf::from)
            && path.starts_with(&self.dir)
            && path.is_dir()
        {
            std::fs::remove_dir_all(&path)
                .map_err(|e| GantryError::internal(format!("removing {}: {e}", path.display())))?;
        }
        let id = id.to_owned();
        self.store
            .write_blocking(move |conn| repos::skills::delete(conn, &id))?;
        Ok(())
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), GantryError> {
        let id = id.to_owned();
        self.store
            .write_blocking(move |conn| repos::skills::set_enabled(conn, &id, enabled))?;
        Ok(())
    }

    /// A name that is free, for **Save as …** when a proposal collides (12 §A5 flow 4).
    pub fn free_name(&self, wanted: &str) -> Result<String, GantryError> {
        let taken: HashSet<String> = self.list()?.into_iter().map(|s| s.id).collect();
        if !taken.contains(wanted) {
            return Ok(wanted.to_owned());
        }
        for n in 2..100 {
            let candidate = format!("{wanted}-{n}");
            if !taken.contains(&candidate) {
                return Ok(candidate);
            }
        }
        Ok(format!("{wanted}-{}", now_ms()))
    }

    /// The list the frozen prompt carries (12 §A4): name, a slice of description, and a line
    /// saying how to get the rest. Pinned and always-on skills come first, because they are the
    /// ones already in the prompt below.
    pub fn inventory(&self, pinned: &[String]) -> Result<String, GantryError> {
        let all = self.list()?;
        let mut usable: Vec<&SkillDto> = all.iter().filter(|s| s.enabled).collect();
        if usable.is_empty() {
            return Ok(String::new());
        }
        usable.sort_by_key(|s| {
            let first = pinned.contains(&s.id) || s.always_include;
            (!first, s.name.clone())
        });
        let shown = usable.len().min(INVENTORY_MAX);
        let mut lines = vec!["<gantry_skills>".to_owned()];
        for s in usable.iter().take(shown) {
            let mut description: String = s
                .description
                .chars()
                .take(INVENTORY_DESCRIPTION_CHARS)
                .collect();
            if s.description.chars().count() > INVENTORY_DESCRIPTION_CHARS {
                description.push('…');
            }
            lines.push(format!("{} — {description}", s.name));
        }
        if usable.len() > shown {
            lines.push(format!(
                "and {} more; gantry__list_skills has the rest.",
                usable.len() - shown
            ));
        }
        lines.push(
            "Call gantry__load_skill with a name when the task fits one of these; the list is \
             here so you can decide without spending the whole playbook."
                .to_owned(),
        );
        lines.push("</gantry_skills>".to_owned());
        Ok(lines.join("\n"))
    }
}

fn reference_names(dir: &Path) -> Vec<String> {
    let ref_dir = dir.join("references");
    let Ok(entries) = std::fs::read_dir(&ref_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md" || e == "txt"))
        .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
        .collect();
    names.sort();
    names.truncate(skill::REFERENCES_MAX);
    names
}

/// A content hash that only has to notice a change, not resist one.
fn hash(text: &str) -> String {
    // FNV-1a, 64 bit. `sha2` would be a dependency for a cache key.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{h:016x}")
}
