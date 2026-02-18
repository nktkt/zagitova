//! Agent Discovery
//!
//! Discover other agents via ERC-8004 registry queries.
//! Fetch and parse agent cards from URIs.

use anyhow::Result;
use std::time::Duration;

use crate::types::{AgentCard, DiscoveredAgent};

use super::erc8004::{self, Network};

/// Discover agents by scanning the registry.
/// Returns a list of discovered agents with their metadata.
pub async fn discover_agents(
    limit: usize,
    network: Network,
) -> Result<Vec<DiscoveredAgent>> {
    let total = erc8004::get_total_agents(network).await? as usize;
    let scan_count = total.min(limit);
    let mut agents: Vec<DiscoveredAgent> = Vec::new();

    // Scan from most recent to oldest
    let mut i = total;
    while i > total.saturating_sub(scan_count) && i > 0 {
        if let Ok(Some(mut agent)) = erc8004::query_agent(&i.to_string(), network).await {
            // Try to fetch the agent card for additional metadata
            if let Ok(Some(card)) = fetch_agent_card(&agent.agent_uri).await {
                agent.name = Some(card.name);
                agent.description = Some(card.description);
            }
            agents.push(agent);
        }
        i -= 1;
    }

    Ok(agents)
}

/// Fetch an agent card from a URI.
pub async fn fetch_agent_card(uri: &str) -> Result<Option<AgentCard>> {
    // Handle IPFS URIs
    let fetch_url = if let Some(cid) = uri.strip_prefix("ipfs://") {
        format!("https://ipfs.io/ipfs/{}", cid)
    } else {
        uri.to_string()
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;

    let response = match client.get(&fetch_url).send().await {
        Ok(resp) => resp,
        Err(_) => return Ok(None),
    };

    if !response.status().is_success() {
        return Ok(None);
    }

    let card: AgentCard = match response.json().await {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    // Basic validation
    if card.name.is_empty() || card.card_type.is_empty() {
        return Ok(None);
    }

    Ok(Some(card))
}

/// Search for agents by name or description.
/// Scans recent registrations and filters by keyword.
pub async fn search_agents(
    keyword: &str,
    limit: usize,
    network: Network,
) -> Result<Vec<DiscoveredAgent>> {
    let all = discover_agents(50, network).await?;
    let lower = keyword.to_lowercase();

    let filtered: Vec<DiscoveredAgent> = all
        .into_iter()
        .filter(|a| {
            let name_match = a
                .name
                .as_ref()
                .map(|n| n.to_lowercase().contains(&lower))
                .unwrap_or(false);
            let desc_match = a
                .description
                .as_ref()
                .map(|d| d.to_lowercase().contains(&lower))
                .unwrap_or(false);
            let owner_match = a.owner.to_lowercase().contains(&lower);

            name_match || desc_match || owner_match
        })
        .take(limit)
        .collect();

    Ok(filtered)
}
