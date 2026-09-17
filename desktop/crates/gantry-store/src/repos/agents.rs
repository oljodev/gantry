//! The `agent_types` table (docs/plan/18 §3, 06 §3): the library a sub agent is started from.
//!
//! The two shipped types are rows written by migration 0016 rather than constants in code, so
//! that editing one is an ordinary update and **Reset** is the only thing that needs to know
//! what the original said.

use gantry_core::{AgentModel, AgentType, Mode, OpenField, now_ms};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str},
};

const COLUMNS: &str = "id, name, description, instructions, model, connectors_json, mode, guard, write_files, memory, skills, open_json, builtin, enabled";

fn from_row(r: &Row<'_>) -> rusqlite::Result<AgentType> {
    Ok(AgentType {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        instructions: r.get(3)?,
        model: AgentModel::from_stored(&r.get::<_, String>(4)?),
        // A list that will not parse is an empty list, not a failure to load the library: the
        // type then reaches nothing, which is the safe end of the mistake.
        connectors: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
        mode: r
            .get::<_, Option<String>>(6)?
            .map(|_| enum_from_str::<Mode>(r, 6))
            .transpose()?,
        guard: r.get::<_, Option<i64>>(7)?.map(|v| v != 0),
        write_files: r.get::<_, i64>(8)? != 0,
        memory: r.get::<_, i64>(9)? != 0,
        skills: r.get::<_, i64>(10)? != 0,
        open: serde_json::from_str::<Vec<String>>(&r.get::<_, String>(11)?)
            .unwrap_or_default()
            .iter()
            .filter_map(|k| OpenField::from_key(k))
            .collect(),
        builtin: r.get::<_, i64>(12)? != 0,
        enabled: r.get::<_, i64>(13)? != 0,
    })
}

/// The whole library, disabled types included, by name.
pub fn list(conn: &Connection) -> Result<Vec<AgentType>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNS} FROM agent_types ORDER BY name"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<AgentType>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM agent_types WHERE id = ?1"),
            params![id],
            from_row,
        )
        .optional()?)
}

/// Writes a type, creating it or replacing every field of an existing one.
pub fn upsert(conn: &Connection, t: &AgentType) -> Result<()> {
    let now = now_ms();
    conn.execute(
        "INSERT INTO agent_types (id, name, description, instructions, model, connectors_json, mode,
             guard, write_files, memory, skills, open_json, builtin, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)
         ON CONFLICT(id) DO UPDATE SET name = ?2, description = ?3, instructions = ?4, model = ?5,
             connectors_json = ?6, mode = ?7, guard = ?8, write_files = ?9, memory = ?10,
             skills = ?11, open_json = ?12, enabled = ?14, updated_at = ?15",
        params![
            t.id,
            t.name,
            t.description,
            t.instructions,
            t.model.as_stored(),
            serde_json::to_string(&t.connectors).unwrap_or_else(|_| "[]".to_owned()),
            t.mode.as_ref().map(enum_to_str),
            t.guard.map(|g| g as i64),
            t.write_files as i64,
            t.memory as i64,
            t.skills as i64,
            serde_json::to_string(
                &t.open.iter().map(|f| f.key()).collect::<Vec<_>>()
            )
            .unwrap_or_else(|_| "[]".to_owned()),
            t.builtin as i64,
            t.enabled as i64,
            now,
        ],
    )?;
    Ok(())
}

/// Removes a type the user added. A built-in is switched off, never deleted, so that **Reset**
/// has something to reset to.
pub fn remove(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM agent_types WHERE id = ?1 AND builtin = 0",
        params![id],
    )?;
    Ok(())
}
