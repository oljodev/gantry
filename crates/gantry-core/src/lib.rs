//! Core types shared by every Gantry crate and, through `specta`, by the frontend.
//!
//! This crate holds only data: id newtypes, the message model, tool specifications, events,
//! risk tiers and errors. It performs no IO. See `docs/plan/01-architecture-overview.md` §2.
//!
//! M0 ships the ids, the error type and [`AppInfo`]; the rest arrives with M1.

#![forbid(unsafe_code)]

mod app_info;
pub mod error;
pub mod ids;

pub use app_info::AppInfo;
pub use error::{ErrorDto, GantryError};
