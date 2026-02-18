//! Replication Module
//!
//! Spawn child automatons, generate genesis configs, and track lineage.
//! The parent creates new sandboxes, installs the runtime,
//! writes a genesis config, funds the child, and starts it.

pub mod spawn;
pub mod genesis;
pub mod lineage;
