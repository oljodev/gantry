//! The `blobs` table: one row per content-addressed file under `<data_dir>/blobs/` with a
//! reference count (docs/plan/06 §1, §3). The files themselves are handled by
//! [`crate::BlobStore`].

use rusqlite::{Connection, OptionalExtension, params};

use crate::db::Result;

/// Inserts the row or bumps its refcount.
pub fn add_ref(conn: &Connection, hash: &str, size: i64, mime: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO blobs (hash, size, mime, refcount, created_at) VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT (hash) DO UPDATE SET refcount = refcount + 1",
        params![hash, size, mime, gantry_core::now_ms()],
    )?;
    Ok(())
}

/// Drops one reference; returns whether the blob is now unreferenced.
pub fn release(conn: &Connection, hash: &str) -> Result<bool> {
    conn.execute(
        "UPDATE blobs SET refcount = max(refcount - 1, 0) WHERE hash = ?1",
        params![hash],
    )?;
    let refs: Option<i64> = conn
        .query_row(
            "SELECT refcount FROM blobs WHERE hash = ?1",
            params![hash],
            |r| r.get(0),
        )
        .optional()?;
    Ok(refs == Some(0))
}

/// Hashes with no references, for the sweep.
pub fn unreferenced(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT hash FROM blobs WHERE refcount <= 0")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn delete(conn: &Connection, hash: &str) -> Result<()> {
    conn.execute("DELETE FROM blobs WHERE hash = ?1", params![hash])?;
    Ok(())
}
