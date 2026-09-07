//! The `messages` and `attachments` tables (docs/plan/06 §3). The transcript is append-only
//! and ordered by `seq` inside a chat.

use gantry_core::{ChatId, Message, MessageId, ProviderKind, Role, StopReason, TurnId, Usage};
use rusqlite::{Connection, Row, params};

use crate::{
    db::Result,
    repos::{enum_from_str, enum_to_str, id_from_str},
};

#[derive(Debug, Clone, PartialEq)]
pub struct MessageRecord {
    pub message: Message,
    pub chat_id: ChatId,
    /// `None` for system notes appended between turns.
    pub turn_id: Option<TurnId>,
    pub seq: u32,
    pub stop_reason: Option<StopReason>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentRecord {
    pub id: String,
    pub message_id: MessageId,
    pub chat_id: ChatId,
    pub name: String,
    pub mime: String,
    pub size: i64,
    pub blob_hash: String,
    pub extracted_text: Option<String>,
    pub created_at: i64,
}

const COLUMNS: &str = "id, chat_id, turn_id, seq, role, parts_json, origin_provider, stop_reason, usage_json, created_at";

fn from_row(r: &Row<'_>) -> rusqlite::Result<MessageRecord> {
    let parts_json: String = r.get(5)?;
    let parts = serde_json::from_str(&parts_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let origin: Option<ProviderKind> = r
        .get::<_, Option<String>>(6)?
        .map(|s| serde_json::from_value(serde_json::Value::String(s)))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let stop_reason: Option<StopReason> = r
        .get::<_, Option<String>>(7)?
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let usage: Option<Usage> = r
        .get::<_, Option<String>>(8)?
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
        })?;
    let role: Role = enum_from_str(r, 4)?;
    Ok(MessageRecord {
        message: Message {
            id: id_from_str(r, 0)?,
            role,
            parts,
            origin,
            created_at: r.get(9)?,
        },
        chat_id: id_from_str(r, 1)?,
        turn_id: r
            .get::<_, Option<String>>(2)?
            .map(|s| s.parse())
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        seq: r.get(3)?,
        stop_reason,
        usage,
    })
}

pub fn insert(conn: &Connection, m: &MessageRecord) -> Result<()> {
    let parts_json = serde_json::to_string(&m.message.parts)
        .map_err(|e| crate::StoreError::Other(e.to_string()))?;
    conn.execute(
        &format!(
            "INSERT INTO messages ({COLUMNS}, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
        ),
        params![
            m.message.id.to_string(),
            m.chat_id.to_string(),
            m.turn_id.map(|t| t.to_string()),
            m.seq,
            enum_to_str(&m.message.role),
            parts_json,
            m.message.origin.map(|o| enum_to_str(&o)),
            m.stop_reason
                .as_ref()
                .and_then(|s| serde_json::to_string(s).ok()),
            m.usage.as_ref().and_then(|u| serde_json::to_string(u).ok()),
            m.message.created_at,
            m.message.text(),
        ],
    )?;
    Ok(())
}

pub fn next_seq(conn: &Connection, chat_id: ChatId) -> Result<u32> {
    let max: Option<u32> = conn.query_row(
        "SELECT max(seq) FROM messages WHERE chat_id = ?1",
        params![chat_id.to_string()],
        |r| r.get(0),
    )?;
    Ok(max.unwrap_or(0) + 1)
}

/// The whole transcript of a chat in order.
pub fn list_for_chat(conn: &Connection, chat_id: ChatId) -> Result<Vec<MessageRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM messages WHERE chat_id = ?1 ORDER BY seq"
    ))?;
    let rows = stmt.query_map(params![chat_id.to_string()], from_row)?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}

pub fn delete_for_turn(conn: &Connection, turn_id: TurnId) -> Result<()> {
    conn.execute(
        "DELETE FROM messages WHERE turn_id = ?1",
        params![turn_id.to_string()],
    )?;
    Ok(())
}

pub fn insert_attachment(conn: &Connection, a: &AttachmentRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO attachments (id, message_id, chat_id, name, mime, size, blob_hash, extracted_text, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            a.id,
            a.message_id.to_string(),
            a.chat_id.to_string(),
            a.name,
            a.mime,
            a.size,
            a.blob_hash,
            a.extracted_text,
            a.created_at,
        ],
    )?;
    Ok(())
}

pub fn list_attachments(conn: &Connection, message_id: MessageId) -> Result<Vec<AttachmentRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, message_id, chat_id, name, mime, size, blob_hash, extracted_text, created_at
         FROM attachments WHERE message_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(params![message_id.to_string()], |r| {
        Ok(AttachmentRecord {
            id: r.get(0)?,
            message_id: id_from_str(r, 1)?,
            chat_id: id_from_str(r, 2)?,
            name: r.get(3)?,
            mime: r.get(4)?,
            size: r.get(5)?,
            blob_hash: r.get(6)?,
            extracted_text: r.get(7)?,
            created_at: r.get(8)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
}
