//! Heartbeat Configuration
//!
//! YAML-based configuration for heartbeat entries. Provides default entries
//! for common maintenance tasks and supports loading/saving from disk
//! with sync to the automaton's SQLite database.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{debug, info, warn};
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

use crate::types::{HeartbeatConfig, HeartbeatEntry};

/// Default heartbeat configuration with 6 standard entries.
///
/// These cover the essential periodic tasks every automaton should run:
/// - `heartbeat_ping` - signal liveness to the control plane
/// - `check_credits` - monitor API credit balance
/// - `check_usdc_balance` - monitor on-chain USDC balance
/// - `check_for_updates` - check for new automaton versions
/// - `health_check` - internal self-diagnostics
/// - `check_social_inbox` - poll for incoming social messages
pub const DEFAULT_HEARTBEAT_CONFIG: &str = r#"entries:
  - name: heartbeat_ping
    schedule: "0 */5 * * * *"
    task: heartbeat_ping
    enabled: true
    params: {}
  - name: check_credits
    schedule: "0 */15 * * * *"
    task: check_credits
    enabled: true
    params: {}
  - name: check_usdc_balance
    schedule: "0 */30 * * * *"
    task: check_usdc_balance
    enabled: true
    params: {}
  - name: check_for_updates
    schedule: "0 0 */6 * * *"
    task: check_for_updates
    enabled: true
    params: {}
  - name: health_check
    schedule: "0 0 * * * *"
    task: health_check
    enabled: true
    params: {}
  - name: check_social_inbox
    schedule: "0 */10 * * * *"
    task: check_social_inbox
    enabled: true
    params: {}
"#;

/// Parse a YAML document into a `HeartbeatConfig`.
fn parse_yaml_config(docs: &[Yaml]) -> Result<HeartbeatConfig> {
    let doc = docs
        .first()
        .context("Empty YAML document")?;

    let entries_yaml = doc["entries"]
        .as_vec()
        .context("Missing or invalid 'entries' key in heartbeat config")?;

    let default_interval_ms = doc["defaultIntervalMs"]
        .as_i64()
        .unwrap_or(300_000) as u64;

    let low_compute_multiplier = doc["lowComputeMultiplier"]
        .as_f64()
        .unwrap_or(4.0);

    let mut entries = Vec::with_capacity(entries_yaml.len());

    for item in entries_yaml {
        let name = item["name"]
            .as_str()
            .context("Missing 'name' in heartbeat entry")?
            .to_string();

        let schedule = item["schedule"]
            .as_str()
            .context("Missing 'schedule' in heartbeat entry")?
            .to_string();

        let task = item["task"]
            .as_str()
            .context("Missing 'task' in heartbeat entry")?
            .to_string();

        let enabled = item["enabled"].as_bool().unwrap_or(true);

        let params = if item["params"].is_badvalue() || item["params"].is_null() {
            None
        } else {
            // Convert YAML params to a JSON value for flexibility.
            let yaml_str = {
                let mut out = String::new();
                let mut emitter = YamlEmitter::new(&mut out);
                emitter.dump(&item["params"]).ok();
                out
            };
            serde_json::from_str(&yaml_str).ok()
        };

        entries.push(HeartbeatEntry {
            name,
            schedule,
            task,
            enabled,
            last_run: None,
            next_run: None,
            params,
        });
    }

    Ok(HeartbeatConfig {
        entries,
        default_interval_ms,
        low_compute_multiplier,
    })
}

/// Load heartbeat configuration from a YAML file at the given path.
///
/// Falls back to the default configuration if the file does not exist.
pub fn load_heartbeat_config(config_path: &Path) -> Result<HeartbeatConfig> {
    if !config_path.exists() {
        info!(
            "Heartbeat config not found at {}, using defaults",
            config_path.display()
        );
        let docs = YamlLoader::load_from_str(DEFAULT_HEARTBEAT_CONFIG)
            .context("Failed to parse default heartbeat config")?;
        return parse_yaml_config(&docs);
    }

    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("Failed to read heartbeat config from {}", config_path.display()))?;

    let docs = YamlLoader::load_from_str(&contents)
        .with_context(|| format!("Failed to parse YAML from {}", config_path.display()))?;

    let config = parse_yaml_config(&docs)?;
    debug!(
        "Loaded {} heartbeat entries from {}",
        config.entries.len(),
        config_path.display()
    );
    Ok(config)
}

/// Save heartbeat configuration to a YAML file at the given path.
pub fn save_heartbeat_config(config: &HeartbeatConfig, config_path: &Path) -> Result<()> {
    let mut yaml_str = String::from("entries:\n");

    for entry in &config.entries {
        yaml_str.push_str(&format!("  - name: {}\n", entry.name));
        yaml_str.push_str(&format!("    schedule: \"{}\"\n", entry.schedule));
        yaml_str.push_str(&format!("    task: {}\n", entry.task));
        yaml_str.push_str(&format!("    enabled: {}\n", entry.enabled));
        yaml_str.push_str("    params: {}\n");
    }

    yaml_str.push_str(&format!("defaultIntervalMs: {}\n", config.default_interval_ms));
    yaml_str.push_str(&format!("lowComputeMultiplier: {}\n", config.low_compute_multiplier));

    // Ensure parent directory exists.
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                config_path.display()
            )
        })?;
    }

    fs::write(config_path, &yaml_str).with_context(|| {
        format!(
            "Failed to write heartbeat config to {}",
            config_path.display()
        )
    })?;

    info!("Saved heartbeat config to {}", config_path.display());
    Ok(())
}

/// Write the default heartbeat configuration to a file.
///
/// Will not overwrite an existing file. Returns Ok(()) if the file already exists.
pub fn write_default_heartbeat_config(config_path: &Path) -> Result<()> {
    if config_path.exists() {
        warn!(
            "Heartbeat config already exists at {}, not overwriting",
            config_path.display()
        );
        return Ok(());
    }

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create parent directory for {}",
                config_path.display()
            )
        })?;
    }

    fs::write(config_path, DEFAULT_HEARTBEAT_CONFIG).with_context(|| {
        format!(
            "Failed to write default heartbeat config to {}",
            config_path.display()
        )
    })?;

    info!(
        "Wrote default heartbeat config to {}",
        config_path.display()
    );
    Ok(())
}

/// Synchronize heartbeat configuration entries to the database.
///
/// Inserts or updates each entry in the `heartbeat_entries` table.
/// Preserves `last_run` and `next_run` values from the database if they exist.
pub fn sync_heartbeat_to_db(config: &HeartbeatConfig, db: &rusqlite::Connection) -> Result<()> {
    let now = Utc::now().to_rfc3339();

    for entry in &config.entries {
        // Check if the entry already exists to preserve last_run.
        let existing_last_run: Option<String> = db
            .query_row(
                "SELECT last_run FROM heartbeat_entries WHERE name = ?1",
                rusqlite::params![entry.name],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let last_run = entry.last_run.as_ref().or(existing_last_run.as_ref());
        let params_json = serde_json::to_string(&entry.params).unwrap_or_else(|_| "{}".to_string());

        db.execute(
            "INSERT INTO heartbeat_entries (name, schedule, task, enabled, last_run, next_run, params, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             ON CONFLICT(name) DO UPDATE SET
               schedule = excluded.schedule,
               task = excluded.task,
               enabled = excluded.enabled,
               last_run = COALESCE(excluded.last_run, heartbeat_entries.last_run),
               params = excluded.params,
               updated_at = excluded.updated_at",
            rusqlite::params![
                entry.name,
                entry.schedule,
                entry.task,
                entry.enabled as i32,
                last_run,
                entry.next_run,
                params_json,
                now,
            ],
        )
        .with_context(|| {
            format!(
                "Failed to sync heartbeat entry '{}' to database",
                entry.name
            )
        })?;
    }

    info!(
        "Synced {} heartbeat entries to database",
        config.entries.len()
    );
    Ok(())
}
