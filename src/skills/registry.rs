//! Skill Registry
//!
//! Install skills from git repositories, URLs, or create them inline.
//! Manages the skills table in the database and the on-disk `.md` files.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use chrono::Utc;

use crate::self_mod::audit_log::{log_modification, LogOptions};
use crate::skills::format::parse_skill_md;
use crate::state::Database;
use crate::types::{ConwayClient, Skill, SkillSource};

// ---------------------------------------------------------------------------
// Install from git
// ---------------------------------------------------------------------------

/// Clone a git repository and look for a skill `.md` file inside it.
///
/// Returns `Ok(Some(skill))` on success, `Ok(None)` if the repo contains no
/// valid skill file, or `Err` on hard failures.
pub fn install_skill_from_git(
    repo_url: &str,
    name: &str,
    skills_dir: &str,
    db: &Database,
    _conway: &dyn ConwayClient,
) -> Result<Option<Skill>> {
    let dest = Path::new(skills_dir).join(name);

    // Clone the repository into skills_dir/<name>.
    let output = Command::new("git")
        .args(["clone", "--depth=1", repo_url, &dest.to_string_lossy()])
        .output()
        .context("Failed to execute git clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {}", stderr.trim());
    }

    // Look for a skill.md (or <name>.md) in the cloned directory.
    let skill_file = find_skill_file(&dest, name);
    let skill_path = match skill_file {
        Some(p) => p,
        None => {
            // Clean up the clone -- no valid skill found.
            let _ = fs::remove_dir_all(&dest);
            return Ok(None);
        }
    };

    let content = fs::read_to_string(&skill_path)
        .context("Failed to read cloned skill file")?;

    let file_path_str = skill_path.to_string_lossy().to_string();

    let skill = match parse_skill_md(&content, &file_path_str, "git") {
        Some(mut s) => {
            s.source = SkillSource::Git;
            s
        }
        None => {
            let _ = fs::remove_dir_all(&dest);
            return Ok(None);
        }
    };

    // Persist to database.
    db.upsert_skill(&skill)
        .context("Failed to upsert skill record")?;

    log_modification(
        db,
        "skill_install",
        &format!("Installed skill '{}' from git: {}", name, repo_url),
        LogOptions {
            reversible: true,
            ..Default::default()
        },
    )?;

    Ok(Some(skill))
}

// ---------------------------------------------------------------------------
// Install from URL
// ---------------------------------------------------------------------------

/// Download a single `.md` skill file from a URL and save it to `skills_dir`.
pub fn install_skill_from_url(
    url: &str,
    name: &str,
    skills_dir: &str,
    db: &Database,
    _conway: &dyn ConwayClient,
) -> Result<Option<Skill>> {
    // Use curl (available on all target platforms) for simplicity.
    let output = Command::new("curl")
        .args(["-fsSL", url])
        .output()
        .context("Failed to execute curl")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("curl failed: {}", stderr.trim());
    }

    let content = String::from_utf8_lossy(&output.stdout).to_string();

    let file_name = format!("{}.md", name);
    let dest_path = Path::new(skills_dir).join(&file_name);

    // Ensure the skills directory exists.
    fs::create_dir_all(skills_dir)
        .context("Failed to create skills directory")?;

    fs::write(&dest_path, &content)
        .context("Failed to write skill file")?;

    let file_path_str = dest_path.to_string_lossy().to_string();

    let skill = match parse_skill_md(&content, &file_path_str, "url") {
        Some(mut s) => {
            s.source = SkillSource::Url;
            s
        }
        None => return Ok(None),
    };

    db.upsert_skill(&skill)
        .context("Failed to upsert skill record")?;

    log_modification(
        db,
        "skill_install",
        &format!("Installed skill '{}' from URL: {}", name, url),
        LogOptions {
            reversible: true,
            ..Default::default()
        },
    )?;

    Ok(Some(skill))
}

// ---------------------------------------------------------------------------
// Create inline
// ---------------------------------------------------------------------------

/// Create a new skill from provided content and save it to disk + database.
pub fn create_skill(
    name: &str,
    description: &str,
    instructions: &str,
    skills_dir: &str,
    db: &Database,
    _conway: &dyn ConwayClient,
) -> Result<Skill> {
    fs::create_dir_all(skills_dir)
        .context("Failed to create skills directory")?;

    // Build the markdown content with YAML frontmatter.
    let md_content = format!(
        "---\nname: {}\ndescription: {}\nauto_activate: true\n---\n\n{}",
        name, description, instructions,
    );

    let file_name = format!("{}.md", name);
    let dest_path = Path::new(skills_dir).join(&file_name);
    let file_path_str = dest_path.to_string_lossy().to_string();

    fs::write(&dest_path, &md_content)
        .context("Failed to write skill file")?;

    let skill = Skill {
        name: name.to_string(),
        description: description.to_string(),
        auto_activate: true,
        requires: None,
        instructions: instructions.to_string(),
        source: SkillSource::SelfAuthored,
        path: file_path_str.clone(),
        enabled: true,
        installed_at: Utc::now().to_rfc3339(),
    };

    db.upsert_skill(&skill)
        .context("Failed to upsert skill record")?;

    log_modification(
        db,
        "skill_create",
        &format!("Created skill '{}'", name),
        LogOptions {
            file_path: Some(file_path_str),
            reversible: true,
            ..Default::default()
        },
    )?;

    Ok(skill)
}

// ---------------------------------------------------------------------------
// Remove
// ---------------------------------------------------------------------------

/// Remove a skill by name. Optionally delete the on-disk files as well.
pub fn remove_skill(
    name: &str,
    db: &Database,
    _conway: &dyn ConwayClient,
    skills_dir: &str,
    delete_files: bool,
) -> Result<()> {
    // Remove database record (marks as disabled).
    db.remove_skill(name)
        .context("Failed to delete skill record")?;

    // Optionally remove files.
    if delete_files {
        let file_path = Path::new(skills_dir).join(format!("{}.md", name));
        if file_path.exists() {
            fs::remove_file(&file_path)
                .with_context(|| format!("Failed to delete {}", file_path.display()))?;
        }

        // Also try removing a directory (for git-cloned skills).
        let dir_path = Path::new(skills_dir).join(name);
        if dir_path.is_dir() {
            fs::remove_dir_all(&dir_path)
                .with_context(|| format!("Failed to delete {}", dir_path.display()))?;
        }
    }

    log_modification(
        db,
        "skill_remove",
        &format!("Removed skill '{}'", name),
        LogOptions {
            reversible: false,
            ..Default::default()
        },
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look for a skill markdown file inside a directory. Tries `skill.md` first,
/// then `<name>.md`, then the first `.md` file found.
fn find_skill_file(dir: &Path, name: &str) -> Option<std::path::PathBuf> {
    // Try well-known names.
    let candidates = [
        dir.join("skill.md"),
        dir.join(format!("{}.md", name)),
        dir.join("README.md"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Some(candidate.clone());
        }
    }

    // Fallback: first .md file in the directory root.
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                return Some(path);
            }
        }
    }

    None
}
