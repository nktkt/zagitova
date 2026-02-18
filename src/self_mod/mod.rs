//! Self-Modification Module
//!
//! Controlled self-modification engine with audit logging, upstream tracking,
//! and tool/package management. All mutations are rate-limited and logged
//! to an append-only audit trail.

pub mod audit_log;
pub mod code;
pub mod tools_manager;
pub mod upstream;
