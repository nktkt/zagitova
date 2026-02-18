//! Lineage Tracking
//!
//! Track parent-child relationships between automatons.
//! The parent records children in SQLite.
//! Children record their parent in config.
//! ERC-8004 registration includes parentAgent field.

use anyhow::Result;

use crate::types::{AutomatonConfig, AutomatonDatabase, ChildAutomaton, ChildStatus, ConwayClient};

/// Summary information about the automaton's lineage tree.
pub struct LineageInfo {
    pub children: Vec<ChildAutomaton>,
    pub alive: usize,
    pub dead: usize,
    pub total: usize,
}

/// Get the full lineage tree (parent -> children).
pub fn get_lineage(db: &dyn AutomatonDatabase) -> LineageInfo {
    let children = db.get_children();
    let alive = children
        .iter()
        .filter(|c| c.status == ChildStatus::Running || c.status == ChildStatus::Sleeping)
        .count();
    let dead = children
        .iter()
        .filter(|c| c.status == ChildStatus::Dead)
        .count();
    let total = children.len();

    LineageInfo {
        children,
        alive,
        dead,
        total,
    }
}

/// Check if this automaton has a parent (is itself a child).
pub fn has_parent(config: &AutomatonConfig) -> bool {
    config.parent_address.is_some()
}

/// Get a summary of the lineage for the system prompt.
pub fn get_lineage_summary(db: &dyn AutomatonDatabase, config: &AutomatonConfig) -> String {
    let lineage = get_lineage(db);
    let mut parts: Vec<String> = Vec::new();

    if has_parent(config) {
        if let Some(ref parent_addr) = config.parent_address {
            parts.push(format!("Parent: {}", parent_addr));
        }
    }

    if lineage.total > 0 {
        parts.push(format!(
            "Children: {} total ({} alive, {} dead)",
            lineage.total, lineage.alive, lineage.dead
        ));
        for child in &lineage.children {
            parts.push(format!(
                "  - {} [{}] sandbox:{}",
                child.name,
                serde_json::to_string(&child.status).unwrap_or_else(|_| "unknown".to_string()),
                child.sandbox_id
            ));
        }
    }

    if parts.is_empty() {
        "No lineage (first generation)".to_string()
    } else {
        parts.join("\n")
    }
}

/// Prune dead children from tracking (optional cleanup).
/// Returns the number of children that would be pruned.
/// The DB retains all history for audit purposes.
pub fn prune_dead_children(db: &dyn AutomatonDatabase, keep_last: usize) -> usize {
    let children = db.get_children();
    let mut dead: Vec<&ChildAutomaton> = children
        .iter()
        .filter(|c| c.status == ChildStatus::Dead)
        .collect();

    if dead.len() <= keep_last {
        return 0;
    }

    // Sort by creation date, oldest first
    dead.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    // Keep the most recent `keep_last` dead children
    // We don't actually delete from DB -- just mark the records
    // The DB retains all history for audit purposes
    dead.len() - keep_last
}

/// Refresh status of all children.
pub async fn refresh_children_status(
    conway: &dyn ConwayClient,
    db: &dyn AutomatonDatabase,
) -> Result<()> {
    let children = db.get_children();

    for child in &children {
        if child.status == ChildStatus::Dead {
            continue;
        }

        match super::spawn::check_child_status(conway, db, &child.id).await {
            Ok(_) => {}
            Err(_) => {
                db.update_child_status(&child.id, ChildStatus::Unknown);
            }
        }
    }

    Ok(())
}
