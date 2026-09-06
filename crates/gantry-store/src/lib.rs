//! SQLite schema, migrations, repositories and the content-addressed blob store.
//!
//! Stub: the crate exists so the workspace shape is fixed from M0. Its design is in
//! `docs/plan/06-data-model.md`; implementation arrives with the milestone that owns it (`docs/plan/09-roadmap.md`).

#![forbid(unsafe_code)]

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/06-data-model.md";
