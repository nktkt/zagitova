//! Tools Manager
//!
//! Install, list, and remove npm packages and MCP server configurations.
//! Every mutation is audit-logged.

use std::collections::HashMap;
use std::process::Command;

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use crate::self_mod::audit_log::{log_modification, LogOptions};
use crate::state::Database;
use crate::types::{ConwayClient, InstalledTool, InstalledToolType};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Outcome of a tool installation attempt.
#[derive(Debug)]
pub struct InstallResult {
    pub tool_id: String,
    pub name: String,
    pub success: bool,
    pub message: String,
}

// ---------------------------------------------------------------------------
// NPM packages
// ---------------------------------------------------------------------------

/// Install a global npm package and record it in the database.
///
/// `_conway` is reserved for future Conway confirmation workflows.
pub fn install_npm_package(
    _conway: &dyn ConwayClient,
    db: &Database,
    package_name: &str,
) -> Result<InstallResult> {
    let tool_id = Uuid::new_v4().to_string();

    // Run `npm install -g <package>`.
    let output = Command::new("npm")
        .args(["install", "-g", package_name])
        .output()
        .context("Failed to execute npm install")?;

    let success = output.status.success();
    let message = if success {
        format!("Successfully installed npm package '{}'", package_name)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        format!("npm install failed: {}", stderr.trim())
    };

    if success {
        let config = serde_json::json!({
            "type": "npm",
            "package": package_name,
        });

        let tool = InstalledTool {
            id: tool_id.clone(),
            name: package_name.to_string(),
            tool_type: InstalledToolType::Custom,
            config: Some(config),
            installed_at: Utc::now().to_rfc3339(),
            enabled: true,
        };

        db.install_tool(&tool)
            .context("Failed to record installed npm package")?;

        log_modification(
            db,
            "tool_install",
            &format!("Installed npm package: {}", package_name),
            LogOptions {
                reversible: true,
                ..Default::default()
            },
        )?;
    }

    Ok(InstallResult {
        tool_id,
        name: package_name.to_string(),
        success,
        message,
    })
}

// ---------------------------------------------------------------------------
// MCP servers
// ---------------------------------------------------------------------------

/// Register an MCP server as an installed tool.
///
/// `command` is the binary to invoke, `args` are its CLI arguments, and `env`
/// provides any environment variables the server needs.
pub fn install_mcp_server(
    _conway: &dyn ConwayClient,
    db: &Database,
    name: &str,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> Result<InstallResult> {
    let tool_id = Uuid::new_v4().to_string();

    let config = serde_json::json!({
        "type": "mcp",
        "command": command,
        "args": args,
        "env": env,
    });

    let tool = InstalledTool {
        id: tool_id.clone(),
        name: name.to_string(),
        tool_type: InstalledToolType::Mcp,
        config: Some(config),
        installed_at: Utc::now().to_rfc3339(),
        enabled: true,
    };

    db.install_tool(&tool)
        .context("Failed to record installed MCP server")?;

    log_modification(
        db,
        "tool_install",
        &format!("Installed MCP server: {}", name),
        LogOptions {
            reversible: true,
            ..Default::default()
        },
    )?;

    Ok(InstallResult {
        tool_id,
        name: name.to_string(),
        success: true,
        message: format!("MCP server '{}' registered (command: {})", name, command),
    })
}

// ---------------------------------------------------------------------------
// Listing / removal
// ---------------------------------------------------------------------------

/// Return every tool recorded in the database.
pub fn list_installed_tools(db: &Database) -> Vec<InstalledTool> {
    db.get_installed_tools().unwrap_or_default()
}

/// Remove a tool by its id. Deletes the database row.
pub fn remove_tool(db: &Database, tool_id: &str) -> Result<()> {
    // Grab the name before removal for the audit log.
    let tools = db.get_installed_tools().unwrap_or_default();
    let name = tools
        .iter()
        .find(|t| t.id == tool_id)
        .map(|t| t.name.clone())
        .unwrap_or_else(|| tool_id.to_string());

    db.remove_tool(tool_id)
        .context("Failed to delete tool")?;

    log_modification(
        db,
        "tool_remove",
        &format!("Removed tool: {}", name),
        LogOptions {
            reversible: false,
            ..Default::default()
        },
    )?;

    Ok(())
}
