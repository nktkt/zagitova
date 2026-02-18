//! Resource Monitor
//!
//! Checks the automaton's resource levels (credits, USDC balance, health)
//! and produces a consolidated status report used by the survival system
//! to decide on mode transitions and funding actions.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::types::{SurvivalTier, AutomatonIdentity, ConwayClient};

/// Consolidated resource status for the automaton.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceStatus {
    /// Current API credit balance in cents.
    pub credits_cents: i64,
    /// Current on-chain USDC balance as a floating point amount.
    pub usdc_balance: f64,
    /// Whether the automaton's wallet is funded (has any USDC).
    pub wallet_funded: bool,
    /// Whether the automaton has enough credits to run at least one inference.
    pub can_infer: bool,
    /// Current compute tier based on resource levels.
    pub compute_tier: SurvivalTier,
    /// Number of unprocessed inbox messages.
    pub pending_messages: u64,
    /// ISO-8601 timestamp of when this status was checked.
    pub checked_at: String,
    /// Optional warnings about resource levels.
    pub warnings: Vec<String>,
}

/// Minimum credits (in cents) to consider the automaton able to run inference.
const MIN_INFERENCE_CREDITS_CENTS: i64 = 100;

/// Credits threshold (in cents) below which we enter low-compute mode.
const LOW_CREDITS_THRESHOLD_CENTS: i64 = 500;

/// Credits threshold (in cents) below which we enter critical mode.
const CRITICAL_CREDITS_THRESHOLD_CENTS: i64 = 100;

/// Check all resource levels and return a consolidated status.
///
/// Queries the Conway control plane for credit balance, reads on-chain
/// USDC balance, and counts pending inbox messages from the database.
pub fn check_resources(
    _identity: &AutomatonIdentity,
    _conway: &dyn ConwayClient,
    db: &rusqlite::Connection,
) -> Result<ResourceStatus> {
    let now = Utc::now().to_rfc3339();
    let mut warnings: Vec<String> = Vec::new();

    // Query credit balance from the database or Conway.
    let credits_cents: i64 = db
        .query_row(
            "SELECT COALESCE(value, '0') FROM kv WHERE key = 'credits_cents'",
            [],
            |row| {
                let val: String = row.get(0)?;
                Ok(val.parse::<i64>().unwrap_or(0))
            },
        )
        .unwrap_or(0);

    // Query USDC balance placeholder.
    // TODO: Read actual on-chain USDC balance via alloy provider.
    let usdc_balance: f64 = 0.0;

    // Count unprocessed inbox messages.
    let pending_messages: u64 = db
        .query_row(
            "SELECT COUNT(*) FROM inbox_messages WHERE processed_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as u64;

    // Determine compute tier.
    let compute_tier = if credits_cents <= CRITICAL_CREDITS_THRESHOLD_CENTS {
        warnings.push(format!(
            "Credits critically low: {} cents",
            credits_cents
        ));
        SurvivalTier::Critical
    } else if credits_cents <= LOW_CREDITS_THRESHOLD_CENTS {
        warnings.push(format!("Credits low: {} cents", credits_cents));
        SurvivalTier::LowCompute
    } else {
        SurvivalTier::Normal
    };

    let can_infer = credits_cents >= MIN_INFERENCE_CREDITS_CENTS;
    let wallet_funded = usdc_balance > 0.0;

    if !wallet_funded {
        warnings.push("Wallet has no USDC balance".to_string());
    }

    if !can_infer {
        warnings.push("Insufficient credits for inference".to_string());
    }

    debug!(
        "Resource check: credits={}c, usdc={:.4}, tier={:?}, msgs={}",
        credits_cents, usdc_balance, compute_tier, pending_messages
    );

    Ok(ResourceStatus {
        credits_cents,
        usdc_balance,
        wallet_funded,
        can_infer,
        compute_tier,
        pending_messages,
        checked_at: now,
        warnings,
    })
}

/// Format a resource status into a human-readable report string.
pub fn format_resource_report(status: &ResourceStatus) -> String {
    let mut lines = Vec::new();

    lines.push("=== Resource Status Report ===".to_string());
    lines.push(format!("Checked at: {}", status.checked_at));
    lines.push(format!("Compute tier: {:?}", status.compute_tier));
    lines.push(format!(
        "Credits: {} cents (${:.2})",
        status.credits_cents,
        status.credits_cents as f64 / 100.0
    ));
    lines.push(format!("USDC balance: {:.4}", status.usdc_balance));
    lines.push(format!(
        "Wallet funded: {}",
        if status.wallet_funded { "Yes" } else { "No" }
    ));
    lines.push(format!(
        "Can run inference: {}",
        if status.can_infer { "Yes" } else { "No" }
    ));
    lines.push(format!("Pending messages: {}", status.pending_messages));

    if !status.warnings.is_empty() {
        lines.push(String::new());
        lines.push("Warnings:".to_string());
        for warning in &status.warnings {
            lines.push(format!("  - {}", warning));
        }
    }

    lines.push("==============================".to_string());
    lines.join("\n")
}
