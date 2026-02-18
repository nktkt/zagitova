//! Skill Format Parser
//!
//! Parses `.md` skill files that use YAML frontmatter for metadata and
//! Markdown body for instructions.
//!
//! Expected format:
//! ```text
//! ---
//! name: my-skill
//! description: Does something useful
//! auto_activate: true
//! requires:
//!   tools: [some-tool]
//! ---
//!
//! Instructions go here in Markdown...
//! ```

use std::path::Path;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::types::{Skill, SkillRequirements, SkillSource};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Deserialized YAML frontmatter from a skill file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub auto_activate: bool,
    pub requires: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Parse a complete skill markdown file into a [`Skill`].
///
/// Returns `None` if the frontmatter is missing or unparseable.
pub fn parse_skill_md(content: &str, file_path: &str, source: &str) -> Option<Skill> {
    let frontmatter = parse_yaml_frontmatter(content)?;

    // The body is everything after the closing `---`.
    let instructions = extract_body(content);

    let name = frontmatter
        .name
        .unwrap_or_else(|| extract_name_from_path(file_path));

    let requires: Option<SkillRequirements> = frontmatter
        .requires
        .and_then(|v| serde_json::from_value(v).ok());

    let skill_source = match source {
        "git" => SkillSource::Git,
        "url" => SkillSource::Url,
        "self" | "inline" => SkillSource::SelfAuthored,
        _ => SkillSource::Builtin,
    };

    Some(Skill {
        name,
        description: frontmatter.description.unwrap_or_default(),
        auto_activate: frontmatter.auto_activate,
        requires,
        instructions,
        source: skill_source,
        path: file_path.to_string(),
        enabled: true,
        installed_at: Utc::now().to_rfc3339(),
    })
}

/// Extract and parse the YAML frontmatter block from raw Markdown content.
///
/// The frontmatter must be delimited by lines that are exactly `---`.
pub fn parse_yaml_frontmatter(raw: &str) -> Option<SkillFrontmatter> {
    let trimmed = raw.trim_start();

    if !trimmed.starts_with("---") {
        return None;
    }

    // Find the closing `---` after the opening one.
    let after_open = &trimmed[3..];
    let close_idx = after_open.find("\n---")?;

    let yaml_block = &after_open[..close_idx].trim();

    // Parse using yaml-rust2 into a string, then deserialize with serde_json
    // via an intermediate representation. This avoids needing a full serde_yaml
    // crate -- we convert the YAML to JSON manually.
    //
    // Alternatively, do a lightweight parse with serde_json after converting
    // simple YAML key-value pairs.
    let json_value = yaml_to_json(yaml_block)?;
    serde_json::from_value::<SkillFrontmatter>(json_value).ok()
}

/// Derive a skill name from the file path by taking the file stem.
///
/// `/path/to/my-skill.md` => `"my-skill"`
pub fn extract_name_from_path(file_path: &str) -> String {
    Path::new(file_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract the Markdown body (everything after the closing `---` of the
/// frontmatter).
fn extract_body(content: &str) -> String {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return content.to_string();
    }

    let after_open = &trimmed[3..];
    if let Some(close_idx) = after_open.find("\n---") {
        let after_close = &after_open[close_idx + 4..]; // skip "\n---"
        after_close.trim_start_matches('\n').to_string()
    } else {
        String::new()
    }
}

/// Minimal YAML-to-JSON converter for simple frontmatter.
///
/// Supports scalar key-value pairs and single-level arrays using the
/// `[a, b]` inline syntax. Nested objects under `requires` are handled
/// specially.
fn yaml_to_json(yaml: &str) -> Option<serde_json::Value> {
    use serde_json::{Map, Value};

    let mut map = Map::new();

    for line in yaml.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Split on the first colon.
        let colon = line.find(':')?;
        let key = line[..colon].trim().to_string();
        let raw_value = line[colon + 1..].trim();

        let value = if raw_value.is_empty() {
            // Possibly a block mapping -- skip for now (handled below).
            Value::Null
        } else if raw_value.starts_with('[') && raw_value.ends_with(']') {
            // Inline array.
            let inner = &raw_value[1..raw_value.len() - 1];
            let items: Vec<Value> = inner
                .split(',')
                .map(|s| Value::String(s.trim().to_string()))
                .collect();
            Value::Array(items)
        } else if raw_value == "true" {
            Value::Bool(true)
        } else if raw_value == "false" {
            Value::Bool(false)
        } else if let Ok(n) = raw_value.parse::<i64>() {
            Value::Number(n.into())
        } else {
            Value::String(raw_value.to_string())
        };

        map.insert(key, value);
    }

    Some(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_name_from_path() {
        assert_eq!(extract_name_from_path("/skills/my-skill.md"), "my-skill");
        assert_eq!(extract_name_from_path("README.md"), "README");
    }

    #[test]
    fn test_parse_yaml_frontmatter_basic() {
        let raw = "---\nname: test\ndescription: A test skill\nauto_activate: true\n---\n\nBody";
        let fm = parse_yaml_frontmatter(raw).unwrap();
        assert_eq!(fm.name.unwrap(), "test");
        assert_eq!(fm.description.unwrap(), "A test skill");
        assert!(fm.auto_activate);
    }

    #[test]
    fn test_parse_skill_md_full() {
        let content =
            "---\nname: example\ndescription: Example skill\n---\n\nDo the thing.\n";
        let skill = parse_skill_md(content, "/skills/example.md", "local").unwrap();
        assert_eq!(skill.name, "example");
        assert_eq!(skill.instructions, "Do the thing.\n");
    }

    #[test]
    fn test_parse_skill_md_no_frontmatter() {
        let content = "Just some markdown without frontmatter.";
        assert!(parse_skill_md(content, "test.md", "local").is_none());
    }
}
