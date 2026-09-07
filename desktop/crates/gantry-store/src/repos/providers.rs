//! The `providers` table (docs/plan/06 §3).

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub base_url: Option<String>,
    pub enabled: bool,
    pub credential_id: Option<String>,
    pub default_model: Option<String>,
    pub options_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, kind, label, base_url, enabled, credential_id, default_model, options_json, created_at, updated_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<ProviderRecord> {
    Ok(ProviderRecord {
        id: r.get(0)?,
        kind: r.get(1)?,
        label: r.get(2)?,
        base_url: r.get(3)?,
        enabled: r.get::<_, i64>(4)? != 0,
        credential_id: r.get(5)?,
        default_model: r.get(6)?,
        options_json: r.get(7)?,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn list(conn: &Connection) -> Result<Vec<ProviderRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM providers ORDER BY created_at"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<ProviderRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM providers WHERE id = ?1"),
            params![id],
            from_row,
        )
        .optional()?)
}

/// Inserts the row when it does not exist; an existing row is left alone.
pub fn ensure(
    conn: &Connection,
    id: &str,
    kind: &str,
    label: &str,
    base_url: Option<&str>,
) -> Result<()> {
    let now = gantry_core::now_ms();
    conn.execute(
        "INSERT OR IGNORE INTO providers (id, kind, label, base_url, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![id, kind, label, base_url, now],
    )?;
    Ok(())
}

pub fn set_credential(conn: &Connection, id: &str, credential_id: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE providers SET credential_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, credential_id, gantry_core::now_ms()],
    )?;
    Ok(())
}

pub fn set_default_model(conn: &Connection, id: &str, model: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE providers SET default_model = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, model, gantry_core::now_ms()],
    )?;
    Ok(())
}

pub fn set_base_url(conn: &Connection, id: &str, base_url: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE providers SET base_url = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, base_url, gantry_core::now_ms()],
    )?;
    Ok(())
}

/// Removes a provider row; its cached models cascade, its credential row is the vault's to
/// delete first.
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM providers WHERE id = ?1", params![id])?;
    Ok(())
}
