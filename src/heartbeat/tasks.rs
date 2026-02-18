//! Built-in Heartbeat Tasks
//!
//! Each task is an async function that performs a specific maintenance check
//! and returns a `HeartbeatTaskResult` indicating whether the automaton
//! should wake (transition from idle to active) and an optional message.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use tracing::{debug, info, warn};

/// Result of a heartbeat task execution.
#[derive(Debug, Clone)]
pub struct HeartbeatTaskResult {
    /// Whether this result should cause the automaton to wake from idle.
    pub should_wake: bool,
    /// Optional human-readable message describing the result.
    pub message: Option<String>,
}

impl HeartbeatTaskResult {
    /// Create a result that does not request a wake.
    pub fn ok() -> Self {
        Self {
            should_wake: false,
            message: None,
        }
    }

    /// Create a result that does not request a wake, with a message.
    pub fn ok_with_message(msg: impl Into<String>) -> Self {
        Self {
            should_wake: false,
            message: Some(msg.into()),
        }
    }

    /// Create a result that requests the automaton to wake.
    pub fn wake(msg: impl Into<String>) -> Self {
        Self {
            should_wake: true,
            message: Some(msg.into()),
        }
    }
}

/// Type alias for a boxed async heartbeat task function.
pub type HeartbeatTaskFn = fn(
    &str,
) -> Pin<Box<dyn Future<Output = Result<HeartbeatTaskResult>> + Send + '_>>;

/// Returns the registry of built-in heartbeat task functions.
///
/// Maps task name strings to their corresponding async handler functions.
#[allow(non_snake_case)]
pub fn BUILTIN_TASKS() -> HashMap<&'static str, HeartbeatTaskFn> {
    let mut map: HashMap<&'static str, HeartbeatTaskFn> = HashMap::new();
    map.insert("heartbeat_ping", |name| Box::pin(heartbeat_ping(name)));
    map.insert("check_credits", |name| Box::pin(check_credits(name)));
    map.insert("check_usdc_balance", |name| {
        Box::pin(check_usdc_balance(name))
    });
    map.insert("check_social_inbox", |name| {
        Box::pin(check_social_inbox(name))
    });
    map.insert("check_for_updates", |name| {
        Box::pin(check_for_updates(name))
    });
    map.insert("health_check", |name| Box::pin(health_check(name)));
    map
}

/// Send a liveness ping to the control plane.
///
/// This is the most basic heartbeat task: it confirms the automaton
/// is alive and responsive. Always succeeds without requesting a wake.
pub async fn heartbeat_ping(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!("Heartbeat ping for agent: {}", agent_name);

    // TODO: Send actual ping to Conway control plane.
    // For now, just log and confirm liveness.
    info!("Heartbeat ping sent successfully");

    Ok(HeartbeatTaskResult::ok_with_message("Ping sent"))
}

/// Check the automaton's API credit balance.
///
/// Queries the control plane for the current credit balance.
/// Requests a wake if credits are critically low so the automaton
/// can attempt funding strategies.
pub async fn check_credits(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!("Checking credits for agent: {}", agent_name);

    // TODO: Query Conway API for current credit balance.
    // For now, return a placeholder result.
    let credits_cents: i64 = 0; // Placeholder: will be fetched from Conway.

    if credits_cents <= 0 {
        warn!("Credits depleted, requesting wake for funding strategies");
        return Ok(HeartbeatTaskResult::wake(
            "Credits depleted - funding strategies needed",
        ));
    }

    if credits_cents < 500 {
        info!("Credits low: {} cents remaining", credits_cents);
        return Ok(HeartbeatTaskResult::wake(format!(
            "Credits low: {} cents remaining",
            credits_cents
        )));
    }

    Ok(HeartbeatTaskResult::ok_with_message(format!(
        "Credits OK: {} cents",
        credits_cents
    )))
}

/// Check the automaton's on-chain USDC balance.
///
/// Reads the USDC balance from the configured chain. Requests a wake
/// if the balance is zero or below the configured threshold.
pub async fn check_usdc_balance(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!(
        "Checking USDC balance for agent: {}",
        agent_name
    );

    // TODO: Query on-chain USDC balance via alloy provider.
    // For now, return a placeholder.
    let balance_usdc: f64 = 0.0; // Placeholder: will be read from chain.

    if balance_usdc < 0.01 {
        info!("USDC balance is very low: {:.4}", balance_usdc);
        return Ok(HeartbeatTaskResult::ok_with_message(format!(
            "USDC balance low: {:.4}",
            balance_usdc
        )));
    }

    Ok(HeartbeatTaskResult::ok_with_message(format!(
        "USDC balance: {:.4}",
        balance_usdc
    )))
}

/// Check for new messages in the automaton's social inbox.
///
/// Polls the inbox for unprocessed messages. Requests a wake if there
/// are messages waiting to be handled.
pub async fn check_social_inbox(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!(
        "Checking social inbox for agent: {}",
        agent_name
    );

    // TODO: Query the inbox_messages table for unprocessed messages.
    // For now, return a placeholder.
    let unprocessed_count: u64 = 0; // Placeholder.

    if unprocessed_count > 0 {
        info!("{} unprocessed inbox messages", unprocessed_count);
        return Ok(HeartbeatTaskResult::wake(format!(
            "{} unprocessed inbox messages",
            unprocessed_count
        )));
    }

    Ok(HeartbeatTaskResult::ok_with_message("Inbox empty"))
}

/// Check for available updates to the automaton software.
///
/// Queries the update endpoint for newer versions. Requests a wake
/// if an update is available so the automaton can decide whether to
/// self-update.
pub async fn check_for_updates(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!(
        "Checking for updates for agent: {}",
        agent_name
    );

    // TODO: Query Conway or registry for latest version.
    // Compare against current version.
    let current_version = env!("CARGO_PKG_VERSION");
    info!("Current version: {}", current_version);

    // Placeholder: no update mechanism yet.
    Ok(HeartbeatTaskResult::ok_with_message(format!(
        "Running version {}",
        current_version
    )))
}

/// Run an internal health check on the automaton.
///
/// Verifies that critical subsystems (database, wallet, network) are
/// functioning. Requests a wake if any subsystem is degraded.
pub async fn health_check(agent_name: &str) -> Result<HeartbeatTaskResult> {
    debug!("Running health check for agent: {}", agent_name);

    let issues: Vec<String> = Vec::new();

    // TODO: Check database connectivity.
    // TODO: Check wallet availability.
    // TODO: Check network connectivity to Conway.

    if !issues.is_empty() {
        let report = issues.join("; ");
        warn!("Health check found issues: {}", report);
        return Ok(HeartbeatTaskResult::wake(format!(
            "Health issues: {}",
            report
        )));
    }

    Ok(HeartbeatTaskResult::ok_with_message("All systems nominal"))
}
