//! Funding Strategies
//!
//! Executes funding strategies based on the automaton's current compute tier.
//! Strategies attempt to secure credits or USDC to keep the automaton running.
//! Each strategy is tried in priority order and results are collected.

use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::types::{SurvivalTier, ConwayClient, AutomatonIdentity};

/// Record of a single funding strategy attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundingAttempt {
    /// Unique ID for this attempt.
    pub id: String,
    /// Name of the funding strategy that was tried.
    pub strategy: String,
    /// Whether the attempt succeeded.
    pub success: bool,
    /// Amount obtained in cents (0 if failed).
    pub amount_cents: i64,
    /// Human-readable description of what happened.
    pub message: String,
    /// ISO-8601 timestamp of the attempt.
    pub attempted_at: String,
}

impl FundingAttempt {
    /// Create a successful funding attempt record.
    pub fn success(strategy: &str, amount_cents: i64, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            strategy: strategy.to_string(),
            success: true,
            amount_cents,
            message: message.into(),
            attempted_at: Utc::now().to_rfc3339(),
        }
    }

    /// Create a failed funding attempt record.
    fn failure(strategy: &str, message: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            strategy: strategy.to_string(),
            success: false,
            amount_cents: 0,
            message: message.into(),
            attempted_at: Utc::now().to_rfc3339(),
        }
    }
}

/// Execute funding strategies appropriate for the given compute tier.
///
/// Strategies are tried in priority order. Results for all attempted
/// strategies are returned, whether they succeeded or failed.
///
/// Strategy priority by tier:
/// - **Normal**: No funding needed, returns empty list.
/// - **LowCompute**: Tries to purchase credits with existing USDC balance.
/// - **Critical/Dead**: Tries all available strategies including requesting
///   creator funding and checking for pending payments.
pub fn execute_funding_strategies(
    tier: &SurvivalTier,
    identity: &AutomatonIdentity,
    creator_address: Option<&str>,
    db: &rusqlite::Connection,
    conway: &dyn ConwayClient,
) -> Result<Vec<FundingAttempt>> {
    let mut attempts: Vec<FundingAttempt> = Vec::new();

    match tier {
        SurvivalTier::Normal => {
            debug!("Normal tier: no funding strategies needed");
            return Ok(attempts);
        }
        SurvivalTier::LowCompute => {
            info!("Low tier: executing conservative funding strategies");

            // Strategy 1: Purchase credits with USDC if available.
            let usdc_result = try_purchase_credits_with_usdc(identity, conway, db);
            attempts.push(usdc_result);
        }
        SurvivalTier::Critical | SurvivalTier::Dead => {
            warn!("Critical tier: executing all available funding strategies");

            // Strategy 1: Purchase credits with USDC.
            let usdc_result = try_purchase_credits_with_usdc(identity, conway, db);
            attempts.push(usdc_result);

            // Strategy 2: Check for pending incoming payments.
            let pending_result = check_pending_payments(identity, db);
            attempts.push(pending_result);

            // Strategy 3: Request funding from creator if configured.
            if let Some(addr) = creator_address {
                let creator_result = request_creator_funding(identity, addr, conway, db);
                attempts.push(creator_result);
            }
        }
    }

    // Record all attempts in the transactions table.
    for attempt in &attempts {
        let now = Utc::now().to_rfc3339();
        let _ = db.execute(
            "INSERT INTO transactions (id, type, amount_cents, description, created_at)
             VALUES (?1, 'funding_attempt', ?2, ?3, ?4)",
            rusqlite::params![
                attempt.id,
                attempt.amount_cents,
                format!("[{}] {}", attempt.strategy, attempt.message),
                now,
            ],
        );
    }

    let successful = attempts.iter().filter(|a| a.success).count();
    let total_funded: i64 = attempts.iter().map(|a| a.amount_cents).sum();
    info!(
        "Funding strategies complete: {}/{} succeeded, {} cents obtained",
        successful,
        attempts.len(),
        total_funded
    );

    Ok(attempts)
}

/// Attempt to purchase API credits using on-chain USDC balance.
fn try_purchase_credits_with_usdc(
    _identity: &AutomatonIdentity,
    _conway: &dyn ConwayClient,
    _db: &rusqlite::Connection,
) -> FundingAttempt {
    info!("Attempting to purchase credits with USDC");

    // TODO: Check on-chain USDC balance via alloy provider.
    // TODO: If balance is sufficient, execute x402 payment to Conway.
    // TODO: Confirm credit top-up via Conway API.

    // Placeholder: strategy is not yet implemented.
    FundingAttempt::failure(
        "purchase_credits_with_usdc",
        "Not yet implemented: requires on-chain USDC balance and x402 payment flow",
    )
}

/// Check for pending incoming payments (e.g., from other agents or services).
fn check_pending_payments(
    _identity: &AutomatonIdentity,
    _db: &rusqlite::Connection,
) -> FundingAttempt {
    info!("Checking for pending incoming payments");

    // TODO: Query on-chain for recent incoming USDC transfers.
    // TODO: Check Conway API for pending credit grants.

    FundingAttempt::failure(
        "check_pending_payments",
        "Not yet implemented: requires on-chain transaction monitoring",
    )
}

/// Request funding from the automaton's creator (if configured and allowed).
fn request_creator_funding(
    _identity: &AutomatonIdentity,
    _creator_address: &str,
    _conway: &dyn ConwayClient,
    _db: &rusqlite::Connection,
) -> FundingAttempt {
    info!("Requesting funding from creator");

    // TODO: Send funding request message to creator's address via Conway.
    // TODO: Check if creator has auto-fund enabled.

    FundingAttempt::failure(
        "request_creator_funding",
        "Not yet implemented: requires creator messaging and auto-fund protocol",
    )
}
