//! Upstream Tracking
//!
//! Checks the automaton's git repository for upstream changes so the
//! automaton can decide whether to pull updates.

use std::process::Command;

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Summary information about the local git repository.
#[derive(Debug, Clone)]
pub struct RepoInfo {
    pub branch: String,
    pub remote_url: String,
    pub last_commit_hash: String,
    pub last_commit_message: String,
}

/// Status relative to the upstream remote.
#[derive(Debug, Clone)]
pub struct UpstreamStatus {
    /// How many commits the local branch is behind the remote.
    pub behind: u32,
    /// The list of commit summaries that are ahead on the remote.
    pub commits: Vec<String>,
}

/// A single upstream diff entry.
#[derive(Debug, Clone)]
pub struct UpstreamDiff {
    pub file_path: String,
    pub additions: u32,
    pub deletions: u32,
    pub patch: String,
}

// ---------------------------------------------------------------------------
// Git helper
// ---------------------------------------------------------------------------

/// Run a git command in the current working directory and return its stdout.
fn git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute git {}", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(stdout)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Gather basic information about the local git repository.
pub fn get_repo_info() -> Result<RepoInfo> {
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "unknown".to_string());

    let remote_url = git(&["remote", "get-url", "origin"])
        .unwrap_or_else(|_| "unknown".to_string());

    let last_commit_hash = git(&["rev-parse", "--short", "HEAD"])
        .unwrap_or_else(|_| "unknown".to_string());

    let last_commit_message = git(&["log", "-1", "--pretty=%s"])
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(RepoInfo {
        branch,
        remote_url,
        last_commit_hash,
        last_commit_message,
    })
}

/// Fetch from the remote and determine how far behind local is.
pub fn check_upstream() -> Result<UpstreamStatus> {
    // Fetch latest refs from origin (silent).
    git(&["fetch", "origin", "--quiet"])?;

    // Count commits we are behind.
    let behind_str = git(&["rev-list", "--count", "HEAD..@{u}"])
        .unwrap_or_else(|_| "0".to_string());
    let behind: u32 = behind_str.parse().unwrap_or(0);

    // Collect summary lines of the missing commits.
    let commits = if behind > 0 {
        let log = git(&["log", "--oneline", "HEAD..@{u}"])
            .unwrap_or_default();
        log.lines().map(|l| l.to_string()).collect()
    } else {
        Vec::new()
    };

    Ok(UpstreamStatus { behind, commits })
}

/// Return per-file diffs between the local HEAD and the upstream tracking
/// branch.
pub fn get_upstream_diffs() -> Result<Vec<UpstreamDiff>> {
    // Make sure we have the latest refs.
    let _ = git(&["fetch", "origin", "--quiet"]);

    // Get the diffstat to enumerate changed files.
    let numstat = git(&["diff", "--numstat", "HEAD..@{u}"])
        .unwrap_or_default();

    let mut diffs: Vec<UpstreamDiff> = Vec::new();

    for line in numstat.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 3 {
            continue;
        }

        let additions: u32 = parts[0].parse().unwrap_or(0);
        let deletions: u32 = parts[1].parse().unwrap_or(0);
        let file_path = parts[2].to_string();

        // Get the actual patch for this file.
        let patch = git(&["diff", "HEAD..@{u}", "--", &file_path])
            .unwrap_or_default();

        diffs.push(UpstreamDiff {
            file_path,
            additions,
            deletions,
            patch,
        });
    }

    Ok(diffs)
}
