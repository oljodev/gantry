//! SQLite for everything the app owns (docs/plan/06). One writer actor, a small read pool,
//! forward-only migrations, typed repositories. No SQL lives outside this crate.

#![forbid(unsafe_code)]

mod blob_store;
mod db;
pub mod repos;

pub use blob_store::BlobStore;
pub use db::{Store, StoreError};
pub use rusqlite::Connection;

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/06-data-model.md";
