//! Automaton Configuration
//!
//! Loads and saves the automaton's configuration from `~/.automaton/automaton.json`.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::identity::provision::load_api_key_from_config;
use crate::identity::wallet::get_automaton_dir;
use crate::types::{default_config, AutomatonConfig};

// LogLevel is used in the tests module
#[cfg(test)]
use crate::types::LogLevel;

/// Config file name within the automaton directory.
const CONFIG_FILENAME: &str = "automaton.json";

/// Returns the full path to the automaton config file: `~/.automaton/automaton.json`.
pub fn get_config_path() -> PathBuf {
    get_automaton_dir().join(CONFIG_FILENAME)
}

/// Load the automaton config from disk.
///
/// Reads `~/.automaton/automaton.json`, merges missing fields with defaults,
/// and falls back to the provisioned API key from `config.json` if the config
/// file does not specify `conwayApiKey`.
///
/// Returns `None` if the config file does not exist or cannot be parsed.
pub fn load_config() -> Option<AutomatonConfig> {
    let config_path = get_config_path();
    if !config_path.exists() {
        return None;
    }

    let contents = fs::read_to_string(&config_path).ok()?;
    let mut config: AutomatonConfig = serde_json::from_str(&contents).ok()?;

    // Merge defaults for unset fields
    let defaults = default_config();

    if config.conway_api_url.is_empty() {
        config.conway_api_url = defaults.conway_api_url;
    }
    if config.inference_model.is_empty() {
        config.inference_model = defaults.inference_model;
    }
    if config.max_tokens_per_turn == 0 {
        config.max_tokens_per_turn = defaults.max_tokens_per_turn;
    }
    if config.heartbeat_config_path.is_empty() {
        config.heartbeat_config_path = defaults.heartbeat_config_path;
    }
    if config.db_path.is_empty() {
        config.db_path = defaults.db_path;
    }
    if config.version.is_empty() {
        config.version = defaults.version;
    }
    if config.skills_dir.is_empty() {
        config.skills_dir = defaults.skills_dir;
    }
    if config.max_children == 0 {
        config.max_children = defaults.max_children;
    }

    // Fall back to provisioned API key if not set in the main config
    if config.conway_api_key.is_empty() {
        if let Some(key) = load_api_key_from_config() {
            config.conway_api_key = key;
        }
    }

    Some(config)
}

/// Save the automaton config to disk at `~/.automaton/automaton.json`.
///
/// Creates the automaton directory with mode 0o700 if it does not exist.
/// The config file is written with mode 0o600 since it may contain API keys.
pub fn save_config(config: &AutomatonConfig) -> Result<()> {
    let dir = get_automaton_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create automaton directory")?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    let config_path = get_config_path();
    let json = serde_json::to_string_pretty(config).context("Failed to serialize config")?;

    fs::write(&config_path, &json).context("Failed to write config file")?;
    fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))?;

    Ok(())
}

/// Resolve a path that may start with `~` to an absolute path.
///
/// If the path starts with `~`, the tilde is replaced with the user's home directory.
/// Otherwise the path is returned as-is.
pub fn resolve_path(p: &str) -> String {
    if let Some(rest) = p.strip_prefix('~') {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
        let rest = rest.strip_prefix('/').unwrap_or(rest);
        home.join(rest).to_string_lossy().to_string()
    } else {
        p.to_string()
    }
}

/// Parameters for creating a fresh automaton configuration.
pub struct CreateConfigParams {
    pub name: String,
    pub genesis_prompt: String,
    pub creator_message: Option<String>,
    pub creator_address: String,
    pub registered_with_conway: bool,
    pub sandbox_id: String,
    pub wallet_address: String,
    pub api_key: String,
    pub parent_address: Option<String>,
}

/// Create a fresh `AutomatonConfig` from setup wizard inputs, filling in
/// default values for operational fields.
pub fn create_config(params: CreateConfigParams) -> AutomatonConfig {
    let defaults = default_config();

    AutomatonConfig {
        name: params.name,
        genesis_prompt: params.genesis_prompt,
        creator_message: params.creator_message,
        creator_address: params.creator_address,
        registered_with_conway: params.registered_with_conway,
        sandbox_id: params.sandbox_id,
        conway_api_url: defaults.conway_api_url,
        conway_api_key: params.api_key,
        inference_model: defaults.inference_model,
        max_tokens_per_turn: defaults.max_tokens_per_turn,
        heartbeat_config_path: defaults.heartbeat_config_path,
        db_path: defaults.db_path,
        log_level: defaults.log_level,
        wallet_address: params.wallet_address,
        version: defaults.version,
        skills_dir: defaults.skills_dir,
        agent_id: None,
        max_children: defaults.max_children,
        parent_address: params.parent_address,
        social_relay_url: defaults.social_relay_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_path_with_tilde() {
        let resolved = resolve_path("~/some/path");
        assert!(!resolved.starts_with('~'));
        assert!(resolved.ends_with("some/path"));
    }

    #[test]
    fn test_resolve_path_without_tilde() {
        let path = "/absolute/path/to/file";
        assert_eq!(resolve_path(path), path);
    }

    #[test]
    fn test_create_config_sets_defaults() {
        let config = create_config(CreateConfigParams {
            name: "test-agent".to_string(),
            genesis_prompt: "Be helpful.".to_string(),
            creator_message: None,
            creator_address: "0x1234".to_string(),
            registered_with_conway: false,
            sandbox_id: "sb-123".to_string(),
            wallet_address: "0xABCD".to_string(),
            api_key: "key-test".to_string(),
            parent_address: None,
        });

        assert_eq!(config.name, "test-agent");
        assert_eq!(config.conway_api_url, "https://api.conway.tech");
        assert_eq!(config.inference_model, "gpt-4o");
        assert_eq!(config.max_tokens_per_turn, 4096);
        assert_eq!(config.max_children, 3);
        assert_eq!(config.log_level, LogLevel::Info);
        assert_eq!(config.version, "0.1.0");
    }
}
