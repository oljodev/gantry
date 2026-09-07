//! Typed access to each table. Functions take a connection so they compose inside one
//! `Store::write` or `Store::read` closure.

pub mod credentials;
pub mod models;
pub mod providers;
pub mod settings;
