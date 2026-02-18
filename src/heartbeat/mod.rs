//! Heartbeat Module
//!
//! Periodic task execution daemon for the automaton.
//! Handles scheduled checks, pings, and maintenance tasks
//! using cron-based scheduling.

pub mod config;
pub mod daemon;
pub mod tasks;

pub use config::{
    load_heartbeat_config, save_heartbeat_config, sync_heartbeat_to_db,
    write_default_heartbeat_config, DEFAULT_HEARTBEAT_CONFIG,
};
pub use daemon::{create_heartbeat_daemon, HeartbeatDaemon};
pub use tasks::{HeartbeatTaskResult, BUILTIN_TASKS};
