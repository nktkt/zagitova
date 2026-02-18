//! Spawn
//!
//! Spawn child automatons in new Conway sandboxes.
//! The parent creates a new sandbox, installs the runtime,
//! writes a genesis config, funds the child, and starts it.

use anyhow::{bail, Context, Result};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

use crate::types::{
    AutomatonDatabase, AutomatonIdentity, ChildAutomaton, ChildStatus, ConwayClient,
    CreateSandboxOptions, GenesisConfig, ModificationEntry, ModificationType, MAX_CHILDREN,
};

/// Spawn a child automaton in a new Conway sandbox.
pub async fn spawn_child(
    conway: &dyn ConwayClient,
    identity: &AutomatonIdentity,
    db: &dyn AutomatonDatabase,
    genesis: &GenesisConfig,
) -> Result<ChildAutomaton> {
    // Check child limit
    let existing = db.get_children();
    let alive_count = existing
        .iter()
        .filter(|c| c.status != ChildStatus::Dead)
        .count() as u32;

    if alive_count >= MAX_CHILDREN {
        bail!(
            "Cannot spawn: already at max children ({}). Kill or wait for existing children to die.",
            MAX_CHILDREN
        );
    }

    let child_id = Uuid::new_v4().to_string();

    // 1. Create a new sandbox for the child
    let sanitized_name = genesis
        .name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    let sandbox = conway
        .create_sandbox(CreateSandboxOptions {
            name: Some(format!("automaton-child-{}", sanitized_name)),
            vcpu: Some(1),
            memory_mb: Some(512),
            disk_gb: Some(5),
            region: None,
        })
        .await
        .context("Failed to create child sandbox")?;

    let child = ChildAutomaton {
        id: child_id.clone(),
        name: genesis.name.clone(),
        address: "0x0000000000000000000000000000000000000000".to_string(),
        sandbox_id: sandbox.id.clone(),
        genesis_prompt: genesis.genesis_prompt.clone(),
        creator_message: genesis.creator_message.clone(),
        funded_amount_cents: 0,
        status: ChildStatus::Spawning,
        created_at: Utc::now().to_rfc3339(),
        last_checked: None,
    };

    db.insert_child(&child);

    // 2. Install Node.js and the automaton runtime in the child sandbox
    exec_in_sandbox(
        conway,
        &sandbox.id,
        "apt-get update -qq && apt-get install -y -qq nodejs npm git curl",
        Some(120_000),
    )
    .await?;

    // 3. Install the automaton runtime
    exec_in_sandbox(
        conway,
        &sandbox.id,
        "npm install -g @conway/automaton@latest 2>/dev/null || true",
        Some(60_000),
    )
    .await?;

    // 4. Write the genesis configuration
    let genesis_json = serde_json::json!({
        "name": genesis.name,
        "genesisPrompt": genesis.genesis_prompt,
        "creatorMessage": genesis.creator_message,
        "creatorAddress": identity.address,
        "parentAddress": identity.address,
    });

    let genesis_content =
        serde_json::to_string_pretty(&genesis_json).context("Failed to serialize genesis config")?;

    write_in_sandbox(
        conway,
        &sandbox.id,
        "/root/.automaton/genesis.json",
        &genesis_content,
    )
    .await?;

    // 4b. Propagate constitution (immutable, inherited before anything else)
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
    let constitution_path = home.join(".automaton").join("constitution.md");

    if constitution_path.exists() {
        if let Ok(constitution) = fs::read_to_string(&constitution_path) {
            let _ = write_in_sandbox(
                conway,
                &sandbox.id,
                "/root/.automaton/constitution.md",
                &constitution,
            )
            .await;
            // Make it read-only in the child
            let _ = exec_in_sandbox(
                conway,
                &sandbox.id,
                "chmod 444 /root/.automaton/constitution.md",
                Some(5_000),
            )
            .await;
        }
    }

    // 5. Record the spawn
    db.insert_modification(&ModificationEntry {
        id: Uuid::new_v4().to_string(),
        timestamp: Utc::now().to_rfc3339(),
        mod_type: ModificationType::ChildSpawn,
        description: format!("Spawned child: {} in sandbox {}", genesis.name, sandbox.id),
        file_path: None,
        diff: None,
        reversible: false,
    });

    Ok(child)
}

/// Start a child automaton after setup.
pub async fn start_child(
    conway: &dyn ConwayClient,
    db: &dyn AutomatonDatabase,
    child_id: &str,
) -> Result<()> {
    let child = db
        .get_child_by_id(child_id)
        .context(format!("Child {} not found", child_id))?;

    // Initialize wallet, provision, and run
    exec_in_sandbox(
        conway,
        &child.sandbox_id,
        "automaton --init && automaton --provision && systemctl start automaton 2>/dev/null || automaton --run &",
        Some(60_000),
    )
    .await?;

    db.update_child_status(child_id, ChildStatus::Running);

    Ok(())
}

/// Check a child's status.
pub async fn check_child_status(
    conway: &dyn ConwayClient,
    db: &dyn AutomatonDatabase,
    child_id: &str,
) -> Result<String> {
    let child = db
        .get_child_by_id(child_id)
        .context(format!("Child {} not found", child_id))?;

    match exec_in_sandbox(
        conway,
        &child.sandbox_id,
        "automaton --status 2>/dev/null || echo 'offline'",
        Some(10_000),
    )
    .await
    {
        Ok(result) => {
            let output = if result.stdout.is_empty() {
                "unknown".to_string()
            } else {
                result.stdout.clone()
            };

            // Parse status from output
            if output.contains("dead") {
                db.update_child_status(child_id, ChildStatus::Dead);
            } else if output.contains("sleeping") {
                db.update_child_status(child_id, ChildStatus::Sleeping);
            } else if output.contains("running") {
                db.update_child_status(child_id, ChildStatus::Running);
            }

            Ok(output)
        }
        Err(_) => {
            db.update_child_status(child_id, ChildStatus::Unknown);
            Ok("Unable to reach child sandbox".to_string())
        }
    }
}

/// Send a message to a child automaton.
pub async fn message_child(
    conway: &dyn ConwayClient,
    db: &dyn AutomatonDatabase,
    child_id: &str,
    message: &str,
) -> Result<()> {
    let child = db
        .get_child_by_id(child_id)
        .context(format!("Child {} not found", child_id))?;

    // Write message to child's message queue
    let msg_json = serde_json::json!({
        "from": "parent",
        "content": message,
        "timestamp": Utc::now().to_rfc3339(),
    });

    let msg_content =
        serde_json::to_string_pretty(&msg_json).context("Failed to serialize message")?;

    let msg_id = Uuid::new_v4().to_string();
    write_in_sandbox(
        conway,
        &child.sandbox_id,
        &format!("/root/.automaton/inbox/{}.json", msg_id),
        &msg_content,
    )
    .await?;

    Ok(())
}

// ---- Helpers --------------------------------------------------------

/// Execute a command in a specific sandbox via the Conway API.
async fn exec_in_sandbox(
    conway: &dyn ConwayClient,
    _sandbox_id: &str,
    command: &str,
    timeout: Option<u64>,
) -> Result<crate::types::ExecResult> {
    conway
        .exec(command, timeout)
        .await
        .context("Exec in sandbox failed")
}

/// Write a file in a specific sandbox, creating parent directories as needed.
async fn write_in_sandbox(
    conway: &dyn ConwayClient,
    sandbox_id: &str,
    path: &str,
    content: &str,
) -> Result<()> {
    // Ensure parent directory exists
    if let Some(dir_end) = path.rfind('/') {
        let dir = &path[..dir_end];
        exec_in_sandbox(
            conway,
            sandbox_id,
            &format!("mkdir -p {}", dir),
            Some(5_000),
        )
        .await?;
    }

    conway
        .write_file(path, content)
        .await
        .context("Write to sandbox failed")
}
