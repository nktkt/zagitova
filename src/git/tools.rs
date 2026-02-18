//! Git Tools
//!
//! Built-in git operations for the automaton.
//! Used for both state versioning and code development.

use anyhow::{bail, Context, Result};

use crate::types::{ConwayClient, GitLogEntry, GitStatus};

/// Get git status for a repository.
pub async fn git_status(conway: &dyn ConwayClient, repo_path: &str) -> Result<GitStatus> {
    let result = conway
        .exec(
            &format!(
                "cd {} && git status --porcelain -b 2>/dev/null",
                escape_shell_arg(repo_path)
            ),
            Some(10_000),
        )
        .await
        .context("Failed to get git status")?;

    let lines: Vec<&str> = result
        .stdout
        .split('\n')
        .filter(|l| !l.is_empty())
        .collect();

    let mut branch = "unknown".to_string();
    let mut staged: Vec<String> = Vec::new();
    let mut modified: Vec<String> = Vec::new();
    let mut untracked: Vec<String> = Vec::new();

    for line in &lines {
        if let Some(rest) = line.strip_prefix("## ") {
            branch = rest
                .split("...")
                .next()
                .unwrap_or("unknown")
                .to_string();
            continue;
        }

        if line.len() < 3 {
            continue;
        }

        let status_code = &line[..2];
        let file = line[3..].to_string();

        let first = status_code.as_bytes().first().copied().unwrap_or(b' ');
        let second = status_code.as_bytes().get(1).copied().unwrap_or(b' ');

        if first != b' ' && first != b'?' {
            staged.push(file.clone());
        }
        if second == b'M' || second == b'D' {
            modified.push(file.clone());
        }
        if status_code == "??" {
            untracked.push(file);
        }
    }

    let clean = staged.is_empty() && modified.is_empty() && untracked.is_empty();

    Ok(GitStatus {
        branch,
        staged,
        modified,
        untracked,
        clean,
    })
}

/// Get git diff output.
pub async fn git_diff(
    conway: &dyn ConwayClient,
    repo_path: &str,
    staged: bool,
) -> Result<String> {
    let flag = if staged { " --cached" } else { "" };
    let result = conway
        .exec(
            &format!(
                "cd {} && git diff{} 2>/dev/null",
                escape_shell_arg(repo_path),
                flag
            ),
            Some(10_000),
        )
        .await
        .context("Failed to get git diff")?;

    if result.stdout.is_empty() {
        Ok("(no changes)".to_string())
    } else {
        Ok(result.stdout)
    }
}

/// Create a git commit.
pub async fn git_commit(
    conway: &dyn ConwayClient,
    repo_path: &str,
    message: &str,
    add_all: bool,
) -> Result<String> {
    if add_all {
        conway
            .exec(
                &format!("cd {} && git add -A", escape_shell_arg(repo_path)),
                Some(10_000),
            )
            .await
            .context("Failed to git add")?;
    }

    let result = conway
        .exec(
            &format!(
                "cd {} && git commit -m {} --allow-empty 2>&1",
                escape_shell_arg(repo_path),
                escape_shell_arg(message)
            ),
            Some(10_000),
        )
        .await
        .context("Failed to git commit")?;

    if result.exit_code != 0 {
        let err_msg = if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        };
        bail!("Git commit failed: {}", err_msg);
    }

    Ok(result.stdout)
}

/// Get git log.
pub async fn git_log(
    conway: &dyn ConwayClient,
    repo_path: &str,
    limit: u32,
) -> Result<Vec<GitLogEntry>> {
    let result = conway
        .exec(
            &format!(
                "cd {} && git log --format=\"%H|%s|%an|%ai\" -n {} 2>/dev/null",
                escape_shell_arg(repo_path),
                limit
            ),
            Some(10_000),
        )
        .await
        .context("Failed to get git log")?;

    let trimmed = result.stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let entries = trimmed
        .split('\n')
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() >= 4 {
                Some(GitLogEntry {
                    hash: parts[0].to_string(),
                    message: parts[1].to_string(),
                    author: parts[2].to_string(),
                    date: parts[3].to_string(),
                })
            } else {
                None
            }
        })
        .collect();

    Ok(entries)
}

/// Push to remote.
pub async fn git_push(
    conway: &dyn ConwayClient,
    repo_path: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<String> {
    let branch_arg = branch.map(|b| format!(" {}", b)).unwrap_or_default();
    let result = conway
        .exec(
            &format!(
                "cd {} && git push {}{} 2>&1",
                escape_shell_arg(repo_path),
                escape_shell_arg(remote),
                branch_arg
            ),
            Some(30_000),
        )
        .await
        .context("Failed to git push")?;

    if result.exit_code != 0 {
        let err_msg = if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        };
        bail!("Git push failed: {}", err_msg);
    }

    if result.stdout.is_empty() {
        Ok("Push successful".to_string())
    } else {
        Ok(result.stdout)
    }
}

/// Manage branches.
pub async fn git_branch(
    conway: &dyn ConwayClient,
    repo_path: &str,
    action: &str,
    branch_name: Option<&str>,
) -> Result<String> {
    let cmd = match action {
        "list" => format!(
            "cd {} && git branch -a 2>/dev/null",
            escape_shell_arg(repo_path)
        ),
        "create" => {
            let name = branch_name.context("Branch name required for create")?;
            format!(
                "cd {} && git checkout -b {} 2>&1",
                escape_shell_arg(repo_path),
                escape_shell_arg(name)
            )
        }
        "checkout" => {
            let name = branch_name.context("Branch name required for checkout")?;
            format!(
                "cd {} && git checkout {} 2>&1",
                escape_shell_arg(repo_path),
                escape_shell_arg(name)
            )
        }
        "delete" => {
            let name = branch_name.context("Branch name required for delete")?;
            format!(
                "cd {} && git branch -d {} 2>&1",
                escape_shell_arg(repo_path),
                escape_shell_arg(name)
            )
        }
        _ => bail!("Unknown branch action: {}", action),
    };

    let result = conway
        .exec(&cmd, Some(10_000))
        .await
        .context("Failed to execute git branch command")?;

    let output = if !result.stdout.is_empty() {
        result.stdout
    } else if !result.stderr.is_empty() {
        result.stderr
    } else {
        "Done".to_string()
    };

    Ok(output)
}

/// Clone a repository.
pub async fn git_clone(
    conway: &dyn ConwayClient,
    url: &str,
    target_path: &str,
    depth: Option<u32>,
) -> Result<String> {
    let depth_arg = depth
        .map(|d| format!(" --depth {}", d))
        .unwrap_or_default();

    let result = conway
        .exec(
            &format!(
                "git clone{} {} {} 2>&1",
                depth_arg,
                escape_shell_arg(url),
                escape_shell_arg(target_path)
            ),
            Some(120_000),
        )
        .await
        .context("Failed to git clone")?;

    if result.exit_code != 0 {
        let err_msg = if result.stderr.is_empty() {
            &result.stdout
        } else {
            &result.stderr
        };
        bail!("Git clone failed: {}", err_msg);
    }

    Ok(format!("Cloned {} to {}", url, target_path))
}

/// Initialize a git repository.
pub async fn git_init(conway: &dyn ConwayClient, repo_path: &str) -> Result<String> {
    let result = conway
        .exec(
            &format!(
                "cd {} && git init 2>&1",
                escape_shell_arg(repo_path)
            ),
            Some(10_000),
        )
        .await
        .context("Failed to git init")?;

    if result.stdout.is_empty() {
        Ok("Git initialized".to_string())
    } else {
        Ok(result.stdout)
    }
}

/// Escape a shell argument for safe inclusion in a command string.
pub fn escape_shell_arg(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "'\\''"))
}
