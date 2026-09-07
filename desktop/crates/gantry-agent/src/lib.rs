//! The turn loop: transcript, permissions, judge, interactions, events, skills, memory, artifacts.
//!
//! Stub: the crate exists so the workspace shape is fixed from M0. Its design is in
//! `docs/plan/01-architecture-overview.md`; implementation arrives with the milestone that owns it (`docs/plan/09-roadmap.md`).

#![forbid(unsafe_code)]

/// The plan document that specifies this crate.
pub const PLAN_DOCUMENT: &str = "docs/plan/01-architecture-overview.md";
