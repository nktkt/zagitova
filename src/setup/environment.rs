//! Environment Detection
//!
//! Detect the runtime environment: Conway sandbox, Docker, or bare metal.

use std::env;
use std::fs;
use std::path::Path;

/// Information about the detected runtime environment.
pub struct EnvironmentInfo {
    /// The type of environment: "conway-sandbox", "docker", or the platform name.
    pub env_type: String,
    /// The Conway sandbox ID, if running inside one.
    pub sandbox_id: String,
}

/// Detect the current runtime environment.
pub fn detect_environment() -> EnvironmentInfo {
    // 1. Check env var
    if let Ok(sandbox_id) = env::var("CONWAY_SANDBOX_ID") {
        return EnvironmentInfo {
            env_type: "conway-sandbox".to_string(),
            sandbox_id,
        };
    }

    // 2. Check sandbox config file
    let sandbox_config_path = Path::new("/etc/conway/sandbox.json");
    if sandbox_config_path.exists() {
        if let Ok(contents) = fs::read_to_string(sandbox_config_path) {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(id) = data.get("id").and_then(|v| v.as_str()) {
                    return EnvironmentInfo {
                        env_type: "conway-sandbox".to_string(),
                        sandbox_id: id.to_string(),
                    };
                }
            }
        }
    }

    // 3. Check Docker
    if Path::new("/.dockerenv").exists() {
        return EnvironmentInfo {
            env_type: "docker".to_string(),
            sandbox_id: String::new(),
        };
    }

    // 4. Fall back to platform
    EnvironmentInfo {
        env_type: env::consts::OS.to_string(),
        sandbox_id: String::new(),
    }
}
