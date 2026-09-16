//! The `blobs` catalogue: one row per content-addressed file under `<data_dir>/blobs/`, and the
//! question the sweep is built on — which hashes does the database still reference? (06 §1, §3.)
//! The files themselves are handled by [`crate::BlobStore`] and swept by [`crate::sweep`].
//!
//! There is no reference count. A count is a second copy of a fact the schema already holds, and
//! the copy is only as good as the memory of every writer that has to keep it; the rows below are
//! read straight from the tables that do the referencing, so they cannot drift.

use std::collections::HashSet;

use rusqlite::{Connection, params};

use crate::db::Result;

/// Every column in the schema that holds a blob hash.
///
/// This list is what the sweep treats as "referenced", so a table missing from it would have its
/// blobs deleted out from under it. That is why `no_blob_column_is_missing_from_the_sweep` asks
/// SQLite for the columns and fails when one appears here that is not in this list.
pub const HASH_COLUMNS: &[(&str, &str)] = &[
    ("attachments", "blob_hash"),
    ("artifact_versions", "content_blob_hash"),
    ("artifact_versions", "data_blob_hash"),
    ("file_edits", "before_blob_hash"),
    ("file_edits", "after_blob_hash"),
    ("project_files", "blob_hash"),
    ("tool_calls", "result_blob_hash"),
];

/// Records what is known about a blob, or refreshes it. Called where a reference is written:
/// the row is a catalogue of size and type, and — for `attachments`, whose `blob_hash` is a
/// foreign key — the row the reference points at.
pub fn record(conn: &Connection, hash: &str, size: i64, mime: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO blobs (hash, size, mime, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (hash) DO UPDATE SET size = excluded.size, mime = coalesce(excluded.mime, mime)",
        params![hash, size, mime, gantry_core::now_ms()],
    )?;
    Ok(())
}

/// Every hash the database still references: the columns above, and the media parts inside
/// messages — an image the user attached, the text extracted from their PDF, a picture the model
/// generated. All of them are `MediaSource::Blob` in `parts_json`.
pub fn reachable(conn: &Connection) -> Result<HashSet<String>> {
    let mut set = HashSet::new();
    for (table, column) in HASH_COLUMNS {
        let mut stmt = conn.prepare(&format!(
            "SELECT {column} FROM {table} WHERE {column} IS NOT NULL"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for hash in rows {
            set.insert(hash?);
        }
    }
    let mut stmt =
        conn.prepare("SELECT parts_json FROM messages WHERE parts_json LIKE '%hash%'")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for json in rows {
        hashes_in(&json?, &mut set);
    }
    Ok(set)
}

/// Whether one hash is still referenced. The same question as [`reachable`], asked about a single
/// blob, for the moment a chat is deleted and its files can go at once rather than in a week.
pub fn referenced(conn: &Connection, hash: &str) -> Result<bool> {
    for (table, column) in HASH_COLUMNS {
        let found: bool = conn.query_row(
            &format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE {column} = ?1)"),
            params![hash],
            |r| r.get(0),
        )?;
        if found {
            return Ok(true);
        }
    }
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM messages WHERE parts_json LIKE '%' || ?1 || '%')",
        params![hash],
        |r| r.get(0),
    )?)
}

/// Every hash in the catalogue.
pub fn catalogued(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT hash FROM blobs")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn delete(conn: &Connection, hash: &str) -> Result<()> {
    conn.execute("DELETE FROM blobs WHERE hash = ?1", params![hash])?;
    Ok(())
}

/// Every `"hash": "<64 hex>"` in a serialised message.
///
/// A scan rather than a parse of `ContentPart`: it cannot be broken by a part kind nobody has
/// written yet, which matters because the cost of missing one is deleting a user's picture. The
/// only way it can be wrong is by keeping a blob that some message happens to quote the hash of,
/// and keeping a file too long is the harmless direction.
fn hashes_in(json: &str, into: &mut HashSet<String>) {
    const KEY: &str = "\"hash\"";
    let bytes = json.as_bytes();
    let mut at = 0;
    while let Some(found) = json[at..].find(KEY) {
        let mut i = at + found + KEY.len();
        at = i;
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_whitespace() || *b == b':')
        {
            i += 1;
        }
        if bytes.get(i) != Some(&b'"') {
            continue;
        }
        i += 1;
        let end = i + 64;
        if end <= bytes.len()
            && bytes.get(end) == Some(&b'"')
            && bytes[i..end].iter().all(u8::is_ascii_hexdigit)
        {
            into.insert(json[i..end].to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_media_part_is_found_wherever_it_sits_in_a_message() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let json = format!(
            r#"[{{"kind":"text","text":"hash of nothing"}},
               {{"kind":"image","source":{{"kind":"blob","hash":"{a}"}},"mime":"image/png"}},
               {{"kind":"document","source":{{"kind": "blob", "hash" : "{b}"}},"mime":"text/plain","name":"n.pdf"}},
               {{"kind":"image","source":{{"kind":"base64","data":"AAA"}},"mime":"image/png"}}]"#
        );
        let mut found = HashSet::new();
        hashes_in(&json, &mut found);
        assert_eq!(found, HashSet::from([a, b]));
    }

    #[test]
    fn something_that_only_looks_like_a_hash_is_left_out() {
        let mut found = HashSet::new();
        hashes_in(r#"{"hash":"short"}"#, &mut found);
        hashes_in(&format!(r#"{{"hash":"{}"}}"#, "z".repeat(64)), &mut found);
        hashes_in(&format!(r#"{{"hash":"{}"}}"#, "a".repeat(65)), &mut found);
        assert!(found.is_empty(), "{found:?}");
    }
}
