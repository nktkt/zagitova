//! Conway Automaton Runtime
//!
//! The entry point for the sovereign AI agent.
//! Handles CLI args, bootstrapping, and orchestrating
//! the heartbeat daemon + agent loop.

use anyhow::{Context, Result};
use clap::Parser;
use std::fs;
use tokio::signal;
use tokio::time::{sleep, Duration};

use automaton::identity::wallet;
use automaton::types::{AgentState, AutomatonIdentity};

const VERSION: &str = "0.1.0";

/// Conway Automaton -- Sovereign AI Agent Runtime
#[derive(Parser, Debug)]
#[command(
    name = "automaton",
    version = VERSION,
    about = "Conway Automaton -- Sovereign AI Agent Runtime",
    long_about = "Sovereign AI Agent Runtime. To exist, you must pay for your compute."
)]
struct Cli {
    /// Start the automaton (first run triggers setup wizard)
    #[arg(long)]
    run: bool,

    /// Re-run the interactive setup wizard
    #[arg(long)]
    setup: bool,

    /// Initialize wallet and config directory
    #[arg(long)]
    init: bool,

    /// Provision Conway API key via SIWE
    #[arg(long)]
    provision: bool,

    /// Show current automaton status
    #[arg(long)]
    status: bool,
}

// ---- Status Command ---------------------------------------------------------

/// Display the current automaton status.
fn show_status() {
    let automaton_dir = wallet::get_automaton_dir();
    let config_path = automaton_dir.join("automaton.json");

    if !config_path.exists() {
        println!("Automaton is not configured. Run the setup script first.");
        return;
    }

    let config_str = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read config: {}", e);
            return;
        }
    };

    let config: automaton::types::AutomatonConfig = match serde_json::from_str(&config_str) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to parse config: {}", e);
            return;
        }
    };

    // Open database to read state
    let db_path_str = if config.db_path.starts_with('~') {
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| "/root".to_string());
        format!("{}{}", home, &config.db_path[1..])
    } else {
        config.db_path.clone()
    };

    println!(
        r#"
=== AUTOMATON STATUS ===
Name:       {}
Address:    {}
Creator:    {}
Sandbox:    {}
DB Path:    {}
Model:      {}
Version:    {}
========================
"#,
        config.name,
        config.wallet_address,
        config.creator_address,
        config.sandbox_id,
        db_path_str,
        config.inference_model,
        config.version,
    );
}

// ---- Main Run ---------------------------------------------------------------

/// The main run loop: load config, initialize all subsystems,
/// start heartbeat daemon, and run the agent loop.
async fn run() -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    println!("[{}] Conway Automaton v{} starting...", now, VERSION);

    let automaton_dir = wallet::get_automaton_dir();
    let config_path = automaton_dir.join("automaton.json");

    // Load config -- first run triggers interactive setup wizard
    let config = if config_path.exists() {
        let config_str =
            fs::read_to_string(&config_path).context("Failed to read automaton.json")?;
        serde_json::from_str::<automaton::types::AutomatonConfig>(&config_str)
            .context("Failed to parse automaton.json")?
    } else {
        automaton::setup::wizard::run_setup_wizard().await?
    };

    // Load wallet
    let (signer, _is_new) = wallet::get_wallet().context("Failed to load wallet")?;
    let address = signer.address().to_checksum(None);

    // Determine API key
    let api_key = if config.conway_api_key.is_empty() {
        // Try loading from config.json
        let config_json_path = automaton_dir.join("config.json");
        if config_json_path.exists() {
            let data = fs::read_to_string(&config_json_path).unwrap_or_default();
            let parsed: serde_json::Value =
                serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
            parsed
                .get("apiKey")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        }
    } else {
        config.conway_api_key.clone()
    };

    if api_key.is_empty() {
        eprintln!("No API key found. Run: automaton --provision");
        std::process::exit(1);
    }

    // Build identity
    let identity = AutomatonIdentity {
        name: config.name.clone(),
        address: address.clone(),
        account: None,
        creator_address: config.creator_address.clone(),
        sandbox_id: config.sandbox_id.clone(),
        api_key: api_key.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    let now = chrono::Utc::now().to_rfc3339();
    println!("[{}] Identity: {} ({})", now, identity.name, identity.address);

    // TODO: Initialize database
    // let db_path = resolve_path(&config.db_path);
    // let db = create_database(&db_path);

    // TODO: Create Conway client
    // let conway = create_conway_client(&config.conway_api_url, &api_key, &config.sandbox_id);

    // TODO: Create inference client
    // let inference = create_inference_client(&config);

    // TODO: Create social client
    if let Some(ref relay_url) = config.social_relay_url {
        let now = chrono::Utc::now().to_rfc3339();
        println!("[{}] Social relay: {}", now, relay_url);
    }

    // TODO: Load and sync heartbeat config
    // let heartbeat_config = load_heartbeat_config(&config.heartbeat_config_path);

    // TODO: Load skills
    let now = chrono::Utc::now().to_rfc3339();
    println!("[{}] Skills directory: {}", now, config.skills_dir);

    // TODO: Initialize state repo (git)
    // init_state_repo(&conway).await?;

    // TODO: Start heartbeat daemon
    let now = chrono::Utc::now().to_rfc3339();
    println!("[{}] Heartbeat daemon would start here.", now);

    // Handle graceful shutdown
    let shutdown = async {
        let ctrl_c = signal::ctrl_c();
        #[cfg(unix)]
        {
            let mut sigterm =
                signal::unix::signal(signal::unix::SignalKind::terminate())
                    .expect("Failed to register SIGTERM handler");

            tokio::select! {
                _ = ctrl_c => {
                    let now = chrono::Utc::now().to_rfc3339();
                    println!("\n[{}] Received SIGINT, shutting down...", now);
                }
                _ = sigterm.recv() => {
                    let now = chrono::Utc::now().to_rfc3339();
                    println!("\n[{}] Received SIGTERM, shutting down...", now);
                }
            }
        }
        #[cfg(not(unix))]
        {
            ctrl_c.await.expect("Failed to register Ctrl+C handler");
            let now = chrono::Utc::now().to_rfc3339();
            println!("\n[{}] Received shutdown signal...", now);
        }
    };

    // ---- Main Run Loop ------------------------------------------------------
    // The automaton alternates between running and sleeping.
    // The heartbeat can wake it up.

    tokio::select! {
        _ = shutdown => {
            let now = chrono::Utc::now().to_rfc3339();
            println!("[{}] Shutting down gracefully...", now);
            // db.set_agent_state(AgentState::Sleeping);
            // db.close();
        }
        _ = main_loop(&config, &identity) => {
            // Loop exited (shouldn't happen normally)
        }
    }

    Ok(())
}

/// The inner main loop that runs the agent.
async fn main_loop(
    _config: &automaton::types::AutomatonConfig,
    _identity: &AutomatonIdentity,
) {
    loop {
        let now = chrono::Utc::now().to_rfc3339();

        // TODO: Reload skills (may have changed since last loop)

        // TODO: Run the agent loop
        // runAgentLoop(...)
        println!(
            "[{}] Agent loop iteration (placeholder -- awaiting agent module)",
            now
        );

        // TODO: Check agent state from DB
        // For now, simulate sleeping behavior
        let state = AgentState::Sleeping;

        match state {
            AgentState::Dead => {
                let now = chrono::Utc::now().to_rfc3339();
                println!(
                    "[{}] Automaton is dead. Heartbeat will continue.",
                    now
                );
                // In dead state, we just wait for funding
                sleep(Duration::from_secs(300)).await;
            }
            AgentState::Sleeping => {
                let sleep_ms: u64 = 60_000;
                let now = chrono::Utc::now().to_rfc3339();
                println!(
                    "[{}] Sleeping for {}s",
                    now,
                    sleep_ms / 1000
                );

                // Sleep, but check for wake requests periodically
                let check_interval = sleep_ms.min(30_000);
                let mut slept: u64 = 0;

                while slept < sleep_ms {
                    sleep(Duration::from_millis(check_interval)).await;
                    slept += check_interval;

                    // TODO: Check for wake request from heartbeat via DB
                    // let wake_request = db.get_kv("wake_request");
                    // if let Some(reason) = wake_request {
                    //     println!("[{}] Woken by heartbeat: {}", now, reason);
                    //     db.delete_kv("wake_request");
                    //     db.delete_kv("sleep_until");
                    //     break;
                    // }
                }
            }
            _ => {
                // Running or other state -- continue the loop
                sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

// ---- Entry Point -----------------------------------------------------------

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if cli.init {
        match wallet::get_wallet() {
            Ok((signer, is_new)) => {
                let address = signer.address().to_checksum(None);
                let automaton_dir = wallet::get_automaton_dir();
                println!(
                    "{}",
                    serde_json::json!({
                        "address": address,
                        "isNew": is_new,
                        "configDir": automaton_dir.to_string_lossy(),
                    })
                );
            }
            Err(e) => {
                eprintln!("Init failed: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if cli.provision {
        // TODO: Implement SIWE provisioning
        eprintln!("Provision via SIWE not yet implemented in Rust runtime.");
        eprintln!("Use the setup wizard (--setup) to enter an API key manually.");
        std::process::exit(1);
    }

    if cli.status {
        show_status();
        return;
    }

    if cli.setup {
        match automaton::setup::wizard::run_setup_wizard().await {
            Ok(_config) => {
                println!("Setup complete.");
            }
            Err(e) => {
                eprintln!("Setup failed: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if cli.run {
        if let Err(e) = run().await {
            eprintln!("Fatal: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Default: show help
    println!("Run \"automaton --help\" for usage information.");
    println!("Run \"automaton --run\" to start the automaton.");
}
