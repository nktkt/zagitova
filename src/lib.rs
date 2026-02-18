//! Conway Automaton -- Sovereign AI Agent Runtime
//!
//! A self-sustaining AI agent that runs in a Conway sandbox,
//! manages its own compute resources, and can replicate.

pub mod types;
pub mod config;
pub mod agent;
pub mod state;
pub mod identity;
pub mod conway;
pub mod heartbeat;
pub mod survival;
pub mod self_mod;
pub mod setup;
pub mod skills;
pub mod replication;
pub mod registry;
pub mod git;
pub mod social;
