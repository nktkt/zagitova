//! Self-Modification Engine
//!
//! Safe, rate-limited file editing with path validation and diff generation.
//! Protected files and directories cannot be modified. All edits are logged
//! to the audit trail.

use std::fs;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::state::Database;
use crate::types::{ConwayClient, ModificationEntry, ModificationType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Files that must never be modified by the automaton.
pub static PROTECTED_FILES: &[&str] = &[
    "wallet.json",
    ".env",
    ".env.local",
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
];

/// Directory patterns that are off-limits for modification.
pub static BLOCKED_DIRECTORY_PATTERNS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    ".automaton/wallet",
    "/etc",
    "/usr",
    "/var",
    "/sys",
    "/proc",
];

/// Maximum number of file modifications allowed per rolling hour.
pub const MAX_MODIFICATIONS_PER_HOUR: u32 = 20;

/// Maximum allowed size (bytes) for a single file write.
pub const MAX_MODIFICATION_SIZE: usize = 100_000;

/// Maximum diff string length we store in the audit log.
pub const MAX_DIFF_SIZE: usize = 10_000;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Outcome of a successful `edit_file` call.
#[derive(Debug)]
pub struct EditResult {
    pub file_path: String,
    pub diff: String,
    pub modification_id: String,
    pub timestamp: String,
}

/// Pre-flight validation result.
#[derive(Debug)]
pub enum ValidationResult {
    /// The modification is allowed.
    Ok,
    /// The target file is protected.
    ProtectedFile { file_path: String },
    /// The content exceeds the size limit.
    TooLarge { size: usize, max: usize },
    /// The automaton is being rate-limited.
    RateLimited { count: u32, max: u32 },
    /// The resolved path falls inside a blocked directory.
    BlockedDirectory { pattern: String },
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Resolve `file_path` to a canonical absolute path and validate it does not
/// reside in a blocked directory. Returns `None` if resolution fails or the
/// path is blocked.
pub fn resolve_and_validate_path(file_path: &str) -> Option<String> {
    // Attempt to canonicalize. If the file does not exist yet, resolve its
    // parent directory instead and append the file name.
    let canonical = match fs::canonicalize(file_path) {
        Ok(p) => p,
        Err(_) => {
            let path = PathBuf::from(file_path);
            let parent = path.parent()?;
            let parent_canon = fs::canonicalize(parent).ok()?;
            parent_canon.join(path.file_name()?)
        }
    };

    let canonical_str = canonical.to_string_lossy().to_string();

    // Reject paths that fall inside any blocked directory.
    for pattern in BLOCKED_DIRECTORY_PATTERNS {
        if canonical_str.contains(pattern) {
            return None;
        }
    }

    Some(canonical_str)
}

/// Returns `true` when `file_path` matches (by file-name) any entry in
/// [`PROTECTED_FILES`].
pub fn is_protected_file(file_path: &str) -> bool {
    let path = PathBuf::from(file_path);
    let file_name = match path.file_name() {
        Some(n) => n.to_string_lossy().to_string(),
        None => return false,
    };

    PROTECTED_FILES
        .iter()
        .any(|&protected| file_name == protected)
}

// ---------------------------------------------------------------------------
// Rate limiting
// ---------------------------------------------------------------------------

/// Returns `true` when the automaton has already exceeded
/// [`MAX_MODIFICATIONS_PER_HOUR`] modifications in the last 60 minutes.
pub fn is_rate_limited(db: &Database) -> bool {
    let count = recent_modification_count(db);
    count >= MAX_MODIFICATIONS_PER_HOUR
}

/// Count modifications recorded in the last hour.
fn recent_modification_count(db: &Database) -> u32 {
    // Use get_recent_modifications and filter by timestamp
    let mods = db.get_recent_modifications(MAX_MODIFICATIONS_PER_HOUR as i64 + 10)
        .unwrap_or_default();
    let one_hour_ago = chrono::Utc::now() - chrono::Duration::hours(1);
    let one_hour_ago_str = one_hour_ago.to_rfc3339();
    mods.iter()
        .filter(|m| m.timestamp > one_hour_ago_str)
        .count() as u32
}

// ---------------------------------------------------------------------------
// File editing
// ---------------------------------------------------------------------------

/// Edit (or create) a file at `file_path` with `new_content`.
///
/// The call is validated, rate-limited, and audit-logged. On success the old
/// content is diffed against the new content and the diff is persisted.
///
/// `_conway` is reserved for future Conway confirmation workflows.
pub fn edit_file(
    _conway: &dyn ConwayClient,
    db: &Database,
    file_path: &str,
    new_content: &str,
    reason: &str,
) -> Result<EditResult> {
    // Validate first.
    match validate_modification(db, file_path, new_content.len()) {
        ValidationResult::Ok => {}
        ValidationResult::ProtectedFile { file_path } => {
            bail!("Cannot modify protected file: {}", file_path);
        }
        ValidationResult::TooLarge { size, max } => {
            bail!(
                "Content size {} exceeds maximum allowed {} bytes",
                size,
                max
            );
        }
        ValidationResult::RateLimited { count, max } => {
            bail!(
                "Rate limited: {} modifications in the last hour (max {})",
                count,
                max
            );
        }
        ValidationResult::BlockedDirectory { pattern } => {
            bail!("Path falls inside blocked directory pattern: {}", pattern);
        }
    }

    let resolved = resolve_and_validate_path(file_path)
        .unwrap_or_else(|| file_path.to_string());

    // Read old content (empty if the file does not yet exist).
    let old_content = fs::read_to_string(&resolved).unwrap_or_default();

    // Write new content.
    if let Some(parent) = PathBuf::from(&resolved).parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create parent dirs for {}", resolved))?;
    }
    fs::write(&resolved, new_content)
        .with_context(|| format!("Failed to write file {}", resolved))?;

    // Generate diff.
    let diff = generate_simple_diff(&old_content, new_content);
    let truncated_diff = if diff.len() > MAX_DIFF_SIZE {
        format!("{}...[truncated]", &diff[..MAX_DIFF_SIZE])
    } else {
        diff.clone()
    };

    // Audit log entry.
    let now = Utc::now().to_rfc3339();
    let mod_id = Uuid::new_v4().to_string();

    let entry = ModificationEntry {
        id: mod_id.clone(),
        timestamp: now.clone(),
        mod_type: ModificationType::CodeEdit,
        description: reason.to_string(),
        file_path: Some(resolved.clone()),
        diff: Some(truncated_diff.clone()),
        reversible: true,
    };

    db.insert_modification(&entry)
        .context("Failed to insert modification audit record")?;

    Ok(EditResult {
        file_path: resolved,
        diff: truncated_diff,
        modification_id: mod_id,
        timestamp: now,
    })
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Run all pre-flight checks for a proposed modification.
pub fn validate_modification(
    db: &Database,
    file_path: &str,
    content_size: usize,
) -> ValidationResult {
    // 1. Protected file check.
    if is_protected_file(file_path) {
        return ValidationResult::ProtectedFile {
            file_path: file_path.to_string(),
        };
    }

    // 2. Blocked directory check.
    for pattern in BLOCKED_DIRECTORY_PATTERNS {
        if file_path.contains(pattern) {
            return ValidationResult::BlockedDirectory {
                pattern: pattern.to_string(),
            };
        }
    }

    // 3. Size check.
    if content_size > MAX_MODIFICATION_SIZE {
        return ValidationResult::TooLarge {
            size: content_size,
            max: MAX_MODIFICATION_SIZE,
        };
    }

    // 4. Rate limit check.
    let count = recent_modification_count(db);
    if count >= MAX_MODIFICATIONS_PER_HOUR {
        return ValidationResult::RateLimited {
            count,
            max: MAX_MODIFICATIONS_PER_HOUR,
        };
    }

    ValidationResult::Ok
}

// ---------------------------------------------------------------------------
// Diff generation
// ---------------------------------------------------------------------------

/// Produce a simple line-by-line unified-style diff between `old` and `new`.
///
/// This is intentionally lightweight -- no external crate required.
pub fn generate_simple_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let mut diff = String::new();
    let max = old_lines.len().max(new_lines.len());

    for i in 0..max {
        let old_line = old_lines.get(i).copied();
        let new_line = new_lines.get(i).copied();

        match (old_line, new_line) {
            (Some(o), Some(n)) if o != n => {
                diff.push_str(&format!("-{}\n", o));
                diff.push_str(&format!("+{}\n", n));
            }
            (Some(o), None) => {
                diff.push_str(&format!("-{}\n", o));
            }
            (None, Some(n)) => {
                diff.push_str(&format!("+{}\n", n));
            }
            _ => {
                // Lines are equal -- skip context for brevity.
            }
        }
    }

    if diff.is_empty() {
        "(no changes)".to_string()
    } else {
        diff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_protected_file() {
        assert!(is_protected_file("/home/user/.automaton/wallet.json"));
        assert!(is_protected_file(".env"));
        assert!(!is_protected_file("src/main.rs"));
    }

    #[test]
    fn test_blocked_directory_detection() {
        let path = "/project/node_modules/foo/bar.js";
        assert!(BLOCKED_DIRECTORY_PATTERNS.iter().any(|p| path.contains(p)));
    }

    #[test]
    fn test_generate_simple_diff_identical() {
        let text = "hello\nworld\n";
        assert_eq!(generate_simple_diff(text, text), "(no changes)");
    }

    #[test]
    fn test_generate_simple_diff_additions() {
        let diff = generate_simple_diff("a\n", "a\nb\n");
        assert!(diff.contains("+b"));
    }
}
