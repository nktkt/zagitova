//! Self-Modification Audit Log
//!
//! Append-only ledger of every change the automaton makes to itself.
//! Provides logging, querying, and report-generation facilities.

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::state::Database;
use crate::types::ModificationEntry;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Optional fields when creating a new log entry.
#[derive(Debug, Default)]
pub struct LogOptions {
    pub file_path: Option<String>,
    pub diff: Option<String>,
    pub reversible: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Record a modification in the audit log.
///
/// Returns the newly created [`ModificationEntry`].
pub fn log_modification(
    db: &Database,
    mod_type: &str,
    description: &str,
    options: LogOptions,
) -> Result<ModificationEntry> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    // Parse the mod_type string into a ModificationType enum.
    let mod_type_enum: crate::types::ModificationType =
        serde_json::from_str(&format!("\"{}\"", mod_type))
            .unwrap_or(crate::types::ModificationType::CodeEdit);

    let entry = ModificationEntry {
        id: id.clone(),
        timestamp: now.clone(),
        mod_type: mod_type_enum,
        description: description.to_string(),
        file_path: options.file_path.clone(),
        diff: options.diff.clone(),
        reversible: options.reversible,
    };

    db.insert_modification(&entry)
        .context("Failed to insert audit log entry")?;

    Ok(entry)
}

/// Retrieve the most recent `limit` modification entries, newest first.
pub fn get_recent_modifications(db: &Database, limit: u32) -> Vec<ModificationEntry> {
    db.get_recent_modifications(limit as i64)
        .unwrap_or_default()
}

/// Generate a human-readable audit report summarising recent activity.
pub fn generate_audit_report(db: &Database) -> String {
    let entries = get_recent_modifications(db, 50);

    if entries.is_empty() {
        return "No modifications recorded.".to_string();
    }

    let mut report = String::from("=== Self-Modification Audit Report ===\n\n");
    report.push_str(&format!("Total entries shown: {}\n\n", entries.len()));

    // Counts by type.
    let mut type_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();
    for entry in &entries {
        let type_str = serde_json::to_string(&entry.mod_type)
            .unwrap_or_else(|_| "unknown".to_string());
        let type_str = type_str.trim_matches('"').to_string();
        *type_counts.entry(type_str).or_insert(0) += 1;
    }

    report.push_str("Breakdown by type:\n");
    for (mod_type, count) in &type_counts {
        report.push_str(&format!("  {}: {}\n", mod_type, count));
    }
    report.push('\n');

    // Individual entries (most recent first).
    report.push_str("Recent entries:\n");
    for entry in &entries {
        let type_str = serde_json::to_string(&entry.mod_type)
            .unwrap_or_else(|_| "unknown".to_string());
        let type_str = type_str.trim_matches('"');
        report.push_str(&format!(
            "  [{}] {} - {}\n",
            entry.timestamp, type_str, entry.description,
        ));
        if let Some(ref path) = entry.file_path {
            report.push_str(&format!("    file: {}\n", path));
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_options_default() {
        let opts = LogOptions::default();
        assert!(opts.file_path.is_none());
        assert!(opts.diff.is_none());
        assert!(!opts.reversible);
    }
}
