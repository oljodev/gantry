//! Typed access to each table. Functions take a connection so they compose inside one
//! `Store::write` or `Store::read` closure.

pub mod blobs;
pub mod chats;
pub mod credentials;
pub mod events;
pub mod interactions;
pub mod messages;
pub mod models;
pub mod projections;
pub mod providers;
pub mod recovery;
pub mod search;
pub mod settings;
pub mod tool_calls;
pub mod turns;

use std::str::FromStr;

use rusqlite::Row;

/// A snake_case enum as its serde name (`auto_edit`), for TEXT columns.
pub(crate) fn enum_to_str<T: serde::Serialize>(v: &T) -> String {
    match serde_json::to_value(v) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

pub(crate) fn enum_from_str<T: serde::de::DeserializeOwned>(
    r: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<T> {
    let s: String = r.get(idx)?;
    serde_json::from_value(serde_json::Value::String(s)).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

pub(crate) fn id_from_str<T>(r: &Row<'_>, idx: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    let s: String = r.get(idx)?;
    s.parse().map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}
