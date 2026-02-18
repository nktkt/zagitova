//! Skill Loader
//!
//! Discovers `.md` skill files on disk, checks prerequisites, and builds
//! the active instruction set for the agent's system prompt.

use std::fs;
use std::path::Path;

use crate::skills::format::parse_skill_md;
use crate::state::Database;
use crate::types::Skill;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Scan `skills_dir` for `.md` files and return all successfully parsed skills.
///
/// Each file is parsed via [`parse_skill_md`]. Skills whose database record
/// has `enabled = 0` are excluded.
pub fn load_skills(skills_dir: &str, db: &Database) -> Vec<Skill> {
    let dir = Path::new(skills_dir);
    if !dir.is_dir() {
        return Vec::new();
    }

    let mut skills: Vec<Skill> = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Only consider markdown files.
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext != "md" {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let file_path = path.to_string_lossy().to_string();

        if let Some(skill) = parse_skill_md(&content, &file_path, "local") {
            // Check the database to see if this skill is disabled.
            if is_skill_enabled(db, &skill.name) {
                skills.push(skill);
            }
        }
    }

    skills
}

/// Returns `true` if the skill is enabled or has no database record (default
/// enabled).
fn is_skill_enabled(db: &Database, name: &str) -> bool {
    match db.get_skill_by_name(name) {
        Ok(Some(skill)) => skill.enabled,
        _ => true, // Not in DB => default enabled
    }
}

/// Check whether all external requirements declared by a skill are satisfied.
///
/// Requirements are stored as a `SkillRequirements` struct in the skill's `requires` field.
/// Currently checks for:
/// - `bins`: a list of binary names that must be available.
pub fn check_requirements(skill: &Skill, db: &Database) -> bool {
    let requires = match &skill.requires {
        Some(r) => r,
        None => return true,
    };

    // Check required binaries.
    if let Some(bins) = &requires.bins {
        for bin in bins {
            // Check if the tool exists in the installed tools list.
            let tools = db.get_installed_tools().unwrap_or_default();
            let exists = tools.iter().any(|t| t.name == *bin && t.enabled);
            if !exists {
                return false;
            }
        }
    }

    true
}

/// Build a combined instruction string from all active (enabled & requirements
/// met) skills, suitable for injection into the agent's system prompt.
pub fn get_active_skill_instructions(skills: &[Skill], db: &Database) -> String {
    let mut sections: Vec<String> = Vec::new();

    for skill in skills {
        if !check_requirements(skill, db) {
            continue;
        }

        let header = format!("## Skill: {}", skill.name);
        let body = skill.instructions.trim();

        if !body.is_empty() {
            sections.push(format!("{}\n\n{}", header, body));
        }
    }

    if sections.is_empty() {
        String::new()
    } else {
        sections.join("\n\n---\n\n")
    }
}
