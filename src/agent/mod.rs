//! Agent Module
//!
//! The core agent loop, system prompt builder, context management,
//! injection defense, and tool system. When this runs, the automaton is alive.

pub mod agent_loop;
pub mod system_prompt;
pub mod tools;
pub mod context;
pub mod injection_defense;
