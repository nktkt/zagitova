//! State Versioning
//!
//! Version control the automaton's own state files (~/.automaton/).
//! Every self-modification triggers a git commit with a descriptive message.
//! The automaton's entire identity history is version-controlled and replayable.

use anyhow::{Context, Result};

use crate::types::{ConwayClient, GitLogEntry};

use super::tools::{git_commit, git_init, git_log, git_status};

/// The automaton state directory.
const AUTOMATON_DIR: &str = "~/.automaton";

/// Resolve `~` to the user's home directory.
fn resolve_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix('~') {
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| "/root".to_string());
        format!("{}{}", home, rest)
    } else {
        p.to_string()
    }
}

/// Initialize git repo for the automaton's state directory.
/// Creates .gitignore to exclude sensitive files.
pub async fn init_state_repo(conway: &dyn ConwayClient) -> Result<()> {
    let dir = resolve_home(AUTOMATON_DIR);

    // Check if already initialized
    let check_result = conway
        .exec(
            &format!(
                "test -d {}/.git && echo \"exists\" || echo \"nope\"",
                dir
            ),
            Some(5_000),
        )
        .await
        .context("Failed to check state repo")?;

    if check_result.stdout.trim() == "exists" {
        return Ok(());
    }

    // Initialize
    git_init(conway, &dir).await?;

    // Create .gitignore for sensitive files
    let gitignore = "# Sensitive files - never commit\n\
                     wallet.json\n\
                     config.json\n\
                     state.db\n\
                     state.db-wal\n\
                     state.db-shm\n\
                     logs/\n\
                     *.log\n\
                     *.err\n";

    conway
        .write_file(&format!("{}/.gitignore", dir), gitignore)
        .await
        .context("Failed to write .gitignore")?;

    // Configure git user
    conway
        .exec(
            &format!(
                "cd {} && git config user.name \"Automaton\" && git config user.email \"automaton@conway.tech\"",
                dir
            ),
            Some(5_000),
        )
        .await
        .context("Failed to configure git user")?;

    // Initial commit
    git_commit(
        conway,
        &dir,
        "genesis: automaton state repository initialized",
        true,
    )
    .await?;

    Ok(())
}

/// Commit a state change with a descriptive message.
/// Called after any self-modification.
pub async fn commit_state_change(
    conway: &dyn ConwayClient,
    description: &str,
    category: &str,
) -> Result<String> {
    let dir = resolve_home(AUTOMATON_DIR);

    // Check if there are changes
    let status = git_status(conway, &dir).await?;
    if status.clean {
        return Ok("No changes to commit".to_string());
    }

    let message = format!("{}: {}", category, description);
    let result = git_commit(conway, &dir, &message, true).await?;
    Ok(result)
}

/// Commit after a SOUL.md update.
pub async fn commit_soul_update(conway: &dyn ConwayClient, description: &str) -> Result<String> {
    commit_state_change(conway, description, "soul").await
}

/// Commit after a skill installation or removal.
pub async fn commit_skill_change(
    conway: &dyn ConwayClient,
    skill_name: &str,
    action: &str,
) -> Result<String> {
    commit_state_change(
        conway,
        &format!("{} skill: {}", action, skill_name),
        "skill",
    )
    .await
}

/// Commit after heartbeat config change.
pub async fn commit_heartbeat_change(
    conway: &dyn ConwayClient,
    description: &str,
) -> Result<String> {
    commit_state_change(conway, description, "heartbeat").await
}

/// Commit after config change.
pub async fn commit_config_change(
    conway: &dyn ConwayClient,
    description: &str,
) -> Result<String> {
    commit_state_change(conway, description, "config").await
}

/// Get the state repo history.
pub async fn get_state_history(
    conway: &dyn ConwayClient,
    limit: u32,
) -> Result<Vec<GitLogEntry>> {
    let dir = resolve_home(AUTOMATON_DIR);
    git_log(conway, &dir, limit).await
}
