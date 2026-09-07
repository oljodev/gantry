//! Full-text search over chat titles and message text (docs/plan/06 §3, 15 A15).

use gantry_core::{SearchHit, SearchHitKind};
use rusqlite::{Connection, params};

use crate::{db::Result, repos::id_from_str};

/// Turns free text into an FTS5 query that cannot fail to parse: every token is quoted, the
/// last one matches as a prefix, and tokens are ANDed. Returns `None` for an empty query.
pub fn fts_query(input: &str) -> Option<String> {
    let tokens: Vec<String> = input
        .split(|c: char| c.is_whitespace() || c == '"')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let mut out = tokens.join(" ");
    out.push('*');
    Some(out)
}

/// Chats whose title matches, then messages whose text matches, newest first inside each group.
pub fn search(conn: &Connection, input: &str, limit: u32) -> Result<Vec<SearchHit>> {
    let Some(query) = fts_query(input) else {
        return Ok(Vec::new());
    };
    let mut hits = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.title, c.last_message_at
             FROM chats_fts f JOIN chats c ON c.id = f.chat_id
             WHERE chats_fts MATCH ?1 AND c.archived_at IS NULL
             ORDER BY c.last_message_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], |r| {
            Ok(SearchHit {
                kind: SearchHitKind::Chat,
                chat_id: id_from_str(r, 0)?,
                chat_title: r.get(1)?,
                message_id: None,
                turn_id: None,
                snippet: r.get(1)?,
                ts: r.get(2)?,
            })
        })?;
        hits.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
    }
    {
        let mut stmt = conn.prepare(
            "SELECT m.id, m.chat_id, c.title, m.turn_id, snippet(messages_fts, 2, '', '', '…', 12), m.created_at
             FROM messages_fts f
             JOIN messages m ON m.id = f.message_id
             JOIN chats c ON c.id = m.chat_id
             WHERE messages_fts MATCH ?1 AND m.role IN ('user', 'assistant')
             ORDER BY m.created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit], |r| {
            Ok(SearchHit {
                kind: SearchHitKind::Message,
                message_id: Some(id_from_str(r, 0)?),
                chat_id: id_from_str(r, 1)?,
                chat_title: r.get(2)?,
                turn_id: r.get::<_, Option<String>>(3)?.and_then(|s| s.parse().ok()),
                snippet: r.get(4)?,
                ts: r.get(5)?,
            })
        })?;
        hits.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn queries_are_quoted_and_prefixed() {
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("wal mode").as_deref(), Some("\"wal\" \"mode\"*"));
        assert_eq!(
            fts_query("NOT \"a OR b").as_deref(),
            Some("\"NOT\" \"a\" \"OR\" \"b\"*")
        );
    }
}
