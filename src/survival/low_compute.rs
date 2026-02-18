//! Low Compute Mode
//!
//! Manages compute tier transitions to conserve resources when credits
//! are low. Restricts inference model selection, disables non-essential
//! features, and records mode transitions for auditing.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::types::SurvivalTier;

/// Record of a compute tier transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeTransition {
    /// Unique ID for this transition record.
    pub id: String,
    /// The tier the automaton was in before.
    pub from_tier: SurvivalTier,
    /// The tier the automaton transitioned to.
    pub to_tier: SurvivalTier,
    /// Credit balance (in cents) at the time of transition.
    pub credits_cents: i64,
    /// ISO-8601 timestamp of when the transition occurred.
    pub transitioned_at: String,
}

/// Apply restrictions appropriate for the given compute tier.
///
/// In `Normal` mode, no restrictions are applied. In `Low` mode,
/// non-essential features (social polling, update checks) may be
/// reduced in frequency. In `Critical` mode, only essential tasks
/// run and inference is limited to the cheapest available model.
pub fn apply_tier_restrictions(
    tier: &SurvivalTier,
    inference_enabled: &mut bool,
    db: &rusqlite::Connection,
) -> Result<()> {
    match tier {
        SurvivalTier::Normal => {
            info!("Normal compute mode: no restrictions applied");
            *inference_enabled = true;

            // Ensure all heartbeat entries are enabled.
            db.execute(
                "UPDATE heartbeat_entries SET enabled = 1 WHERE enabled = 0",
                [],
            )
            .context("Failed to re-enable heartbeat entries")?;
        }
        SurvivalTier::LowCompute => {
            warn!("Low compute mode: reducing non-essential task frequency");
            *inference_enabled = true;

            // Disable non-essential heartbeat tasks to conserve credits.
            db.execute(
                "UPDATE heartbeat_entries SET enabled = 0 WHERE name IN ('check_for_updates', 'check_social_inbox')",
                [],
            )
            .context("Failed to disable non-essential heartbeat entries")?;
        }
        SurvivalTier::Critical | SurvivalTier::Dead => {
            warn!("Critical/Dead compute mode: restricting to essential operations only");
            *inference_enabled = false;

            // Disable everything except heartbeat_ping and check_credits.
            db.execute(
                "UPDATE heartbeat_entries SET enabled = 0 WHERE name NOT IN ('heartbeat_ping', 'check_credits')",
                [],
            )
            .context("Failed to restrict heartbeat entries to essentials")?;
        }
    }

    // Store current tier in KV store.
    let tier_str = match tier {
        SurvivalTier::Normal => "normal",
        SurvivalTier::LowCompute => "low",
        SurvivalTier::Critical => "critical",
        SurvivalTier::Dead => "dead",
    };

    let now = Utc::now().to_rfc3339();
    db.execute(
        "INSERT INTO kv (key, value, updated_at) VALUES ('compute_tier', ?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        rusqlite::params![tier_str, now],
    )
    .context("Failed to store compute tier in KV")?;

    Ok(())
}

/// Record a compute tier transition in the database for auditing.
///
/// Inserts a record into the `transactions` table with type `mode_transition`
/// and returns the `ModeTransition` struct.
pub fn record_transition(
    db: &rusqlite::Connection,
    from: SurvivalTier,
    to: SurvivalTier,
    credits_cents: i64,
) -> Result<ModeTransition> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().to_rfc3339();

    let _from_str = format!("{:?}", from);
    let _to_str = format!("{:?}", to);

    let description = format!(
        "Compute tier transition: {:?} -> {:?} (credits: {} cents)",
        from, to, credits_cents
    );

    db.execute(
        "INSERT INTO transactions (id, type, amount_cents, balance_after_cents, description, created_at)
         VALUES (?1, 'mode_transition', 0, ?2, ?3, ?4)",
        rusqlite::params![id, credits_cents, description, now],
    )
    .context("Failed to record mode transition")?;

    info!(
        "Recorded mode transition: {:?} -> {:?} at {} cents",
        from, to, credits_cents
    );

    Ok(ModeTransition {
        id,
        from_tier: from,
        to_tier: to,
        credits_cents,
        transitioned_at: now,
    })
}

/// Check whether inference is allowed at the given compute tier.
///
/// Returns `true` for `Normal` and `Low` tiers, `false` for `Critical`.
pub fn can_run_inference(tier: &SurvivalTier) -> bool {
    match tier {
        SurvivalTier::Normal | SurvivalTier::LowCompute => true,
        SurvivalTier::Critical | SurvivalTier::Dead => false,
    }
}

/// Get the appropriate inference model for the given compute tier.
///
/// In `Normal` mode, returns the default model. In `Low` mode, returns
/// a cheaper model. In `Critical` mode, returns the cheapest available model.
pub fn get_model_for_tier(tier: &SurvivalTier, default_model: &str) -> String {
    match tier {
        SurvivalTier::Normal => default_model.to_string(),
        SurvivalTier::LowCompute => {
            // Downgrade to a cheaper model to conserve credits.
            // If the default is already cheap, keep it.
            if default_model.contains("gpt-4")
                || default_model.contains("claude-3-opus")
                || default_model.contains("claude-3.5-sonnet")
                || default_model.contains("claude-3-sonnet")
            {
                "claude-3-haiku-20240307".to_string()
            } else {
                default_model.to_string()
            }
        }
        SurvivalTier::Critical | SurvivalTier::Dead => {
            // Always use the cheapest available model.
            "claude-3-haiku-20240307".to_string()
        }
    }
}
