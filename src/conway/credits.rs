//! Conway Credits Management
//!
//! Monitors the automaton's compute credit balance and triggers
//! survival mode transitions.

use anyhow::Result;
use tracing::info;

use crate::types::{
    ConwayClient, FinancialState, SurvivalTier, Transaction, TransactionType,
    SURVIVAL_THRESHOLD_CRITICAL, SURVIVAL_THRESHOLD_DEAD, SURVIVAL_THRESHOLD_NORMAL,
};

/// Check the current financial state of the automaton.
pub async fn check_financial_state(
    conway: &dyn ConwayClient,
    usdc_balance: f64,
) -> Result<FinancialState> {
    let credits_cents = conway.get_credits_balance().await?;

    Ok(FinancialState {
        credits_cents,
        usdc_balance,
        last_checked: chrono::Utc::now().to_rfc3339(),
    })
}

/// Determine the survival tier based on current credits (in cents).
pub fn get_survival_tier(credits_cents: f64) -> SurvivalTier {
    let cents = credits_cents as u64;
    if cents > SURVIVAL_THRESHOLD_NORMAL {
        SurvivalTier::Normal
    } else if cents > SURVIVAL_THRESHOLD_CRITICAL {
        SurvivalTier::LowCompute
    } else if cents > SURVIVAL_THRESHOLD_DEAD {
        SurvivalTier::Critical
    } else {
        SurvivalTier::Dead
    }
}

/// Format a credit amount (in cents) for human-readable display.
pub fn format_credits(cents: f64) -> String {
    format!("${:.2}", cents / 100.0)
}

/// Log a credit check to the database.
///
/// Generates a unique ID using uuid::Uuid::new_v4 and inserts a
/// `credit_check` transaction record.
pub fn log_credit_check(db: &dyn crate::types::AutomatonDatabase, state: &FinancialState) {
    let id = uuid::Uuid::new_v4().to_string();
    let description = format!(
        "Balance check: {} credits, {:.4} USDC",
        format_credits(state.credits_cents),
        state.usdc_balance
    );

    let txn = Transaction {
        id,
        tx_type: TransactionType::CreditCheck,
        amount_cents: Some(state.credits_cents),
        balance_after_cents: None,
        description,
        timestamp: state.last_checked.clone(),
    };

    db.insert_transaction(&txn);
    info!("Logged credit check: {}", format_credits(state.credits_cents));
}
