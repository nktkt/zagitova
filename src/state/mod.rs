//! Automaton State Module
//!
//! SQLite-backed persistent state for the automaton.
//! The database IS the automaton's memory.

mod adapter;
mod database;
mod schema;

pub use adapter::DatabaseAdapter;
pub use database::Database;
pub use schema::{CREATE_TABLES, MIGRATION_V2, MIGRATION_V3, SCHEMA_VERSION};
