//! The `models` cache (docs/plan/06 §3): what a provider's model list said, and when.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRecord {
    pub provider_id: String,
    pub model_id: String,
    pub display_name: String,
    pub capabilities_json: String,
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub pricing_json: Option<String>,
    /// When the provider says the model was released, in Unix seconds; `None` when it never said.
    pub created_at: Option<i64>,
    pub fetched_at: i64,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<ModelRecord> {
    Ok(ModelRecord {
        provider_id: r.get(0)?,
        model_id: r.get(1)?,
        display_name: r.get(2)?,
        capabilities_json: r.get(3)?,
        context_window: r.get(4)?,
        max_output: r.get(5)?,
        pricing_json: r.get(6)?,
        created_at: r.get(7)?,
        fetched_at: r.get(8)?,
    })
}

pub fn list_for(conn: &Connection, provider_id: &str) -> Result<Vec<ModelRecord>> {
    let mut stmt = conn.prepare(
        "SELECT provider_id, model_id, display_name, capabilities_json, context_window, max_output, pricing_json, created_at, fetched_at
         FROM models WHERE provider_id = ?1 ORDER BY model_id",
    )?;
    let rows = stmt.query_map(params![provider_id], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

/// When the provider's list was last fetched, if ever.
pub fn fetched_at(conn: &Connection, provider_id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT max(fetched_at) FROM models WHERE provider_id = ?1",
            params![provider_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten())
}

/// Replaces the provider's cached list in one transaction.
pub fn replace_for(conn: &mut Connection, provider_id: &str, models: &[ModelRecord]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM models WHERE provider_id = ?1",
        params![provider_id],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO models (provider_id, model_id, display_name, capabilities_json, context_window, max_output, pricing_json, created_at, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for m in models {
            stmt.execute(params![
                provider_id,
                m.model_id,
                m.display_name,
                m.capabilities_json,
                m.context_window,
                m.max_output,
                m.pricing_json,
                m.created_at,
                m.fetched_at
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}
