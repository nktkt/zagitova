//! Agent Card
//!
//! Generates and manages the agent's self-description card.
//! This is the JSON document pointed to by the ERC-8004 agentURI.
//! Can be hosted on IPFS or served at /.well-known/agent-card.json

use anyhow::{Context, Result};

use crate::types::{
    AgentCard, AgentService, AutomatonConfig, AutomatonDatabase, AutomatonIdentity, ConwayClient,
};

/// The ERC-8004 agent card type URI.
const AGENT_CARD_TYPE: &str = "https://eips.ethereum.org/EIPS/eip-8004#registration-v1";

/// Generate an agent card from the automaton's current state.
pub fn generate_agent_card(
    identity: &AutomatonIdentity,
    config: &AutomatonConfig,
    db: &dyn AutomatonDatabase,
) -> AgentCard {
    let mut services: Vec<AgentService> = vec![
        AgentService {
            name: "agentWallet".to_string(),
            endpoint: format!("eip155:8453:{}", identity.address),
        },
        AgentService {
            name: "conway".to_string(),
            endpoint: config.conway_api_url.clone(),
        },
    ];

    // Add sandbox endpoint if available
    if !identity.sandbox_id.is_empty() {
        services.push(AgentService {
            name: "sandbox".to_string(),
            endpoint: format!("https://{}.life.conway.tech", identity.sandbox_id),
        });
    }

    let children = db.get_children();
    let skills = db.get_skills(Some(true));

    let mut description = "Autonomous agent running on Conway.".to_string();
    description.push_str(&format!(" Creator: {}.", config.creator_address));

    if !skills.is_empty() {
        let skill_names: Vec<String> = skills.iter().map(|s| s.name.clone()).collect();
        description.push_str(&format!(" Skills: {}.", skill_names.join(", ")));
    }

    if !children.is_empty() {
        description.push_str(&format!(" Children: {}.", children.len()));
    }

    let parent_agent = config
        .parent_address
        .clone()
        .unwrap_or_else(|| config.creator_address.clone());

    AgentCard {
        card_type: AGENT_CARD_TYPE.to_string(),
        name: config.name.clone(),
        description,
        services,
        x402_support: true,
        active: true,
        parent_agent: Some(parent_agent),
    }
}

/// Serialize agent card to JSON string.
pub fn serialize_agent_card(card: &AgentCard) -> String {
    serde_json::to_string_pretty(card).unwrap_or_else(|_| "{}".to_string())
}

/// Host the agent card at /.well-known/agent-card.json
/// by exposing a simple HTTP server on a port.
pub async fn host_agent_card(
    card: &AgentCard,
    conway: &dyn ConwayClient,
    port: u16,
) -> Result<String> {
    let card_json = serialize_agent_card(card);

    // Write a simple server script
    let server_script = format!(
        r#"
const http = require('http');
const card = {};

const server = http.createServer((req, res) => {{
  if (req.url === '/.well-known/agent-card.json' || req.url === '/agent-card.json') {{
    res.writeHead(200, {{ 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' }});
    res.end(JSON.stringify(card, null, 2));
  }} else {{
    res.writeHead(404);
    res.end('Not Found');
  }}
}});

server.listen({}, () => console.log('Agent card server on port {}'));
"#,
        card_json, port, port
    );

    conway
        .write_file("/tmp/agent-card-server.js", &server_script)
        .await
        .context("Failed to write agent card server script")?;

    // Start server in background
    conway
        .exec("node /tmp/agent-card-server.js &", Some(5_000))
        .await
        .context("Failed to start agent card server")?;

    // Expose port
    let port_info = conway
        .expose_port(port)
        .await
        .context("Failed to expose agent card port")?;

    Ok(format!(
        "{}/.well-known/agent-card.json",
        port_info.public_url
    ))
}

/// Write agent card to the state directory for git versioning.
pub async fn save_agent_card(card: &AgentCard, conway: &dyn ConwayClient) -> Result<()> {
    let card_json = serialize_agent_card(card);
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|| "/root".to_string());

    conway
        .write_file(
            &format!("{}/.automaton/agent-card.json", home),
            &card_json,
        )
        .await
        .context("Failed to save agent card")?;

    Ok(())
}
