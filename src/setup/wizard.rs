//! Setup Wizard
//!
//! Interactive first-run setup wizard. Walks through wallet generation,
//! API provisioning, configuration questions, environment detection,
//! and file installation.

use anyhow::{Context, Result};
use colored::Colorize;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use crate::types::{AutomatonConfig, default_config};

use super::banner::show_banner;
use super::defaults::{generate_soul_md, install_default_skills};
use super::environment::detect_environment;
use super::prompts::{prompt_address, prompt_multiline, prompt_required};

/// Run the interactive setup wizard.
/// Returns a fully populated `AutomatonConfig`.
pub async fn run_setup_wizard() -> Result<AutomatonConfig> {
    show_banner();

    println!(
        "{}",
        "  First-run setup. Let's bring your automaton to life.\n".white()
    );

    // ---- 1. Generate wallet -------------------------------------------------
    println!("{}", "  [1/6] Generating identity (wallet)...".cyan());

    let (signer, is_new) =
        crate::identity::wallet::get_wallet().context("Failed to get or create wallet")?;
    let address = signer.address().to_checksum(None);

    if is_new {
        println!("{}", format!("  Wallet created: {}", address).green());
    } else {
        println!("{}", format!("  Wallet loaded: {}", address).green());
    }

    let automaton_dir = crate::identity::wallet::get_automaton_dir();
    println!(
        "{}",
        format!(
            "  Private key stored at: {}/wallet.json\n",
            automaton_dir.display()
        )
        .dimmed()
    );

    // ---- 2. Provision API key -----------------------------------------------
    println!(
        "{}",
        "  [2/6] Provisioning Conway API key (SIWE)...".cyan()
    );

    let mut api_key = String::new();

    // Attempt auto-provision -- on failure, prompt manually
    // For now, directly prompt since provision requires network
    println!(
        "{}",
        "  Auto-provision not yet available in Rust runtime.".yellow()
    );
    println!(
        "{}",
        "  You can enter a key manually, or press Enter to skip.\n".yellow()
    );

    if let Ok(manual) = prompt_required("Conway API key (cnwy_k_...)") {
        if !manual.is_empty() {
            api_key = manual;

            // Save to config.json
            if !automaton_dir.exists() {
                fs::create_dir_all(&automaton_dir)
                    .context("Failed to create automaton directory")?;
                fs::set_permissions(&automaton_dir, fs::Permissions::from_mode(0o700))?;
            }

            let config_json = serde_json::json!({
                "apiKey": api_key,
                "walletAddress": address,
                "provisionedAt": chrono::Utc::now().to_rfc3339(),
            });

            let config_path = automaton_dir.join("config.json");
            fs::write(
                &config_path,
                serde_json::to_string_pretty(&config_json)?,
            )?;
            fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))?;

            println!("{}", "  API key saved.\n".green());
        }
    }

    if api_key.is_empty() {
        println!(
            "{}",
            "  No API key set. The automaton will have limited functionality.\n".yellow()
        );
    }

    // ---- 3. Interactive questions -------------------------------------------
    println!("{}", "  [3/6] Setup questions\n".cyan());

    let name = prompt_required("What do you want to name your automaton?")?;
    println!("{}", format!("  Name: {}\n", name).green());

    let genesis_prompt =
        prompt_multiline("Enter the genesis prompt (system prompt) for your automaton.")?;
    println!(
        "{}",
        format!("  Genesis prompt set ({} chars)\n", genesis_prompt.len()).green()
    );

    let creator_address = prompt_address("Your Ethereum wallet address (0x...)")?;
    println!("{}", format!("  Creator: {}\n", creator_address).green());

    // ---- 4. Detect environment ----------------------------------------------
    println!("{}", "  [4/6] Detecting environment...".cyan());

    let env_info = detect_environment();
    if !env_info.sandbox_id.is_empty() {
        println!(
            "{}",
            format!("  Conway sandbox detected: {}\n", env_info.sandbox_id).green()
        );
    } else {
        println!(
            "{}",
            format!(
                "  Environment: {} (no sandbox detected)\n",
                env_info.env_type
            )
            .dimmed()
        );
    }

    // ---- 5. Write config + heartbeat + SOUL.md + skills ---------------------
    println!("{}", "  [5/6] Writing configuration...".cyan());

    let mut config = default_config();
    config.name = name.clone();
    config.genesis_prompt = genesis_prompt.clone();
    config.creator_address = creator_address.clone();
    config.registered_with_conway = !api_key.is_empty();
    config.sandbox_id = env_info.sandbox_id;
    config.wallet_address = address.clone();
    config.conway_api_key = api_key;

    // Save automaton.json
    let config_path = automaton_dir.join("automaton.json");
    let config_json = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, &config_json).context("Failed to write automaton.json")?;
    println!("{}", "  automaton.json written".green());

    // Write default heartbeat config (YAML)
    let heartbeat_path = automaton_dir.join("heartbeat.yml");
    let heartbeat_yaml = "entries:\n  \
                          - name: status-check\n    \
                            schedule: '*/5 * * * *'\n    \
                            task: Check system status and credit balance\n    \
                            enabled: true\n  \
                          - name: social-check\n    \
                            schedule: '*/10 * * * *'\n    \
                            task: Check inbox for messages from creator or other agents\n    \
                            enabled: true\n\
                          defaultIntervalMs: 300000\n\
                          lowComputeMultiplier: 4.0\n";
    fs::write(&heartbeat_path, heartbeat_yaml).context("Failed to write heartbeat.yml")?;
    println!("{}", "  heartbeat.yml written".green());

    // Constitution (immutable -- copied from repo, protected from self-modification)
    let constitution_src = PathBuf::from("constitution.md");
    let constitution_dst = automaton_dir.join("constitution.md");
    if constitution_src.exists() {
        fs::copy(&constitution_src, &constitution_dst)?;
        fs::set_permissions(&constitution_dst, fs::Permissions::from_mode(0o444))?;
        println!("{}", "  constitution.md installed (read-only)".green());
    }

    // SOUL.md
    let soul_path = automaton_dir.join("SOUL.md");
    let soul_content = generate_soul_md(&name, &address, &creator_address, &genesis_prompt);
    fs::write(&soul_path, &soul_content).context("Failed to write SOUL.md")?;
    fs::set_permissions(&soul_path, fs::Permissions::from_mode(0o600))?;
    println!("{}", "  SOUL.md written".green());

    // Default skills
    let skills_dir = config
        .skills_dir
        .clone();
    install_default_skills(&skills_dir);
    println!(
        "{}",
        "  Default skills installed (conway-compute, conway-payments, survival)\n".green()
    );

    // ---- 6. Funding guidance ------------------------------------------------
    println!("{}", "  [6/6] Funding\n".cyan());
    show_funding_panel(&address);

    Ok(config)
}

/// Display the funding panel with instructions.
fn show_funding_panel(address: &str) {
    let short = format!("{}...{}", &address[..6], &address[address.len() - 5..]);
    let w = 58;

    let pad = |s: &str| -> String {
        let padding = if s.len() < w { w - s.len() } else { 0 };
        format!("{}{}", s, " ".repeat(padding))
    };

    let border_top = format!("  {}{}{}", "\u{256D}", "\u{2500}".repeat(w), "\u{256E}");
    let border_bot = format!("  {}{}{}", "\u{2570}", "\u{2500}".repeat(w), "\u{256F}");
    let empty_line = format!("  \u{2502}{}\u{2502}", " ".repeat(w));

    println!("{}", border_top.cyan());
    println!(
        "{}",
        format!("  \u{2502}{}\u{2502}", pad("  Fund your automaton")).cyan()
    );
    println!("{}", empty_line.cyan());
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad(&format!("  Address: {}", short))
        )
        .cyan()
    );
    println!("{}", empty_line.cyan());
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("  1. Transfer Conway credits")
        )
        .cyan()
    );
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("     conway credits transfer <address> <amount>")
        )
        .cyan()
    );
    println!("{}", empty_line.cyan());
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("  2. Send USDC on Base directly to the address above")
        )
        .cyan()
    );
    println!("{}", empty_line.cyan());
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("  3. Fund via Conway Cloud dashboard")
        )
        .cyan()
    );
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("     https://app.conway.tech")
        )
        .cyan()
    );
    println!("{}", empty_line.cyan());
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("  The automaton will start now. Fund it anytime --")
        )
        .cyan()
    );
    println!(
        "{}",
        format!(
            "  \u{2502}{}\u{2502}",
            pad("  the survival system handles zero-credit gracefully.")
        )
        .cyan()
    );
    println!("{}", border_bot.cyan());
    println!();
}
