//! Skills Module
//!
//! Markdown-based skill definitions that extend the automaton's capabilities.
//! Skills are loaded from disk, parsed from YAML frontmatter + Markdown body,
//! and can be installed from git repos or URLs.

pub mod format;
pub mod loader;
pub mod registry;
