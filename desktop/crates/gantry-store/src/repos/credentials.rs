//! The `credentials` table (docs/plan/06 §5): ciphertext only. Encryption is `gantry-secrets`'
//! business; this module never sees a plaintext.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::db::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRecord {
    pub id: String,
    pub kind: String,
    pub owner_kind: String,
    pub owner_id: String,
    pub label: Option<String>,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub expires_at: Option<i64>,
    pub meta_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

const COLUMNS: &str = "id, kind, owner_kind, owner_id, label, ciphertext, nonce, expires_at, meta_json, created_at, updated_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<CredentialRecord> {
    Ok(CredentialRecord {
        id: r.get(0)?,
        kind: r.get(1)?,
        owner_kind: r.get(2)?,
        owner_id: r.get(3)?,
        label: r.get(4)?,
        ciphertext: r.get(5)?,
        nonce: r.get(6)?,
        expires_at: r.get(7)?,
        meta_json: r.get(8)?,
        created_at: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<CredentialRecord>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM credentials WHERE id = ?1"),
            params![id],
            from_row,
        )
        .optional()?)
}

pub fn list_for_owner(
    conn: &Connection,
    owner_kind: &str,
    owner_id: &str,
) -> Result<Vec<CredentialRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM credentials WHERE owner_kind = ?1 AND owner_id = ?2 ORDER BY created_at"
    ))?;
    let rows = stmt.query_map(params![owner_kind, owner_id], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn list_all(conn: &Connection) -> Result<Vec<CredentialRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM credentials ORDER BY created_at"
    ))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn insert(conn: &Connection, c: &CredentialRecord) -> Result<()> {
    conn.execute(
        &format!("INSERT INTO credentials ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
        params![
            c.id,
            c.kind,
            c.owner_kind,
            c.owner_id,
            c.label,
            c.ciphertext,
            c.nonce,
            c.expires_at,
            c.meta_json,
            c.created_at,
            c.updated_at
        ],
    )?;
    Ok(())
}

/// Rewrites the ciphertext of an existing row (key rotation).
pub fn update_ciphertext(
    conn: &Connection,
    id: &str,
    ciphertext: &[u8],
    nonce: &[u8],
) -> Result<()> {
    conn.execute(
        "UPDATE credentials SET ciphertext = ?2, nonce = ?3, updated_at = ?4 WHERE id = ?1",
        params![id, ciphertext, nonce, gantry_core::now_ms()],
    )?;
    Ok(())
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM credentials WHERE id = ?1", params![id])?;
    Ok(())
}
