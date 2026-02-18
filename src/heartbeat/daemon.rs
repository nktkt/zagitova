//! Heartbeat Daemon
//!
//! Runs a background loop that checks cron schedules and executes
//! due heartbeat tasks. Uses `tokio::time::interval` for the tick
//! loop and `Arc<AtomicBool>` for graceful shutdown signaling.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use cron::Schedule;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use crate::types::HeartbeatEntry;

use super::tasks::{HeartbeatTaskResult, BUILTIN_TASKS};

/// Options for creating a heartbeat daemon.
pub struct HeartbeatDaemonOptions {
    /// Tick interval in seconds. Defaults to 30.
    pub tick_interval_secs: u64,
    /// Heartbeat entries to schedule.
    pub entries: Vec<HeartbeatEntry>,
}

impl Default for HeartbeatDaemonOptions {
    fn default() -> Self {
        Self {
            tick_interval_secs: 30,
            entries: Vec::new(),
        }
    }
}

/// The heartbeat daemon. Runs a background tokio task that periodically
/// checks all registered heartbeat entries and executes those that are due.
pub struct HeartbeatDaemon {
    /// Atomic flag indicating whether the daemon is running.
    running: Arc<AtomicBool>,
    /// Handle to the spawned background task.
    interval_handle: Option<JoinHandle<()>>,
    /// Tick interval in seconds.
    tick_interval_secs: u64,
    /// Registered heartbeat entries.
    entries: Arc<tokio::sync::RwLock<Vec<HeartbeatEntry>>>,
}

/// Create a new heartbeat daemon from the given options.
pub fn create_heartbeat_daemon(options: HeartbeatDaemonOptions) -> HeartbeatDaemon {
    HeartbeatDaemon {
        running: Arc::new(AtomicBool::new(false)),
        interval_handle: None,
        tick_interval_secs: options.tick_interval_secs,
        entries: Arc::new(tokio::sync::RwLock::new(options.entries)),
    }
}

impl HeartbeatDaemon {
    /// Start the heartbeat daemon background loop.
    ///
    /// Spawns a tokio task that ticks at the configured interval,
    /// checking all entries and executing those that are due.
    pub fn start(&mut self, agent_name: String) {
        if self.running.load(Ordering::SeqCst) {
            warn!("Heartbeat daemon is already running");
            return;
        }

        self.running.store(true, Ordering::SeqCst);
        info!(
            "Starting heartbeat daemon with {}s tick interval",
            self.tick_interval_secs
        );

        let running = Arc::clone(&self.running);
        let entries = Arc::clone(&self.entries);
        let tick_secs = self.tick_interval_secs;

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(tick_secs));

            loop {
                interval.tick().await;

                if !running.load(Ordering::SeqCst) {
                    info!("Heartbeat daemon stopping");
                    break;
                }

                if let Err(e) = tick(&entries, &agent_name).await {
                    error!("Heartbeat tick error: {:#}", e);
                }
            }
        });

        self.interval_handle = Some(handle);
    }

    /// Stop the heartbeat daemon gracefully.
    pub fn stop(&mut self) {
        if !self.running.load(Ordering::SeqCst) {
            debug!("Heartbeat daemon is not running");
            return;
        }

        info!("Stopping heartbeat daemon");
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.interval_handle.take() {
            handle.abort();
        }
    }

    /// Returns whether the daemon is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Force-run a specific task by name, regardless of its schedule.
    pub async fn force_run(
        &self,
        task_name: &str,
        agent_name: &str,
    ) -> Result<HeartbeatTaskResult> {
        let entries = self.entries.read().await;
        let entry = entries
            .iter()
            .find(|e| e.name == task_name)
            .cloned()
            .with_context(|| format!("No heartbeat entry found with name '{}'", task_name))?;
        drop(entries);

        info!("Force-running heartbeat task: {}", task_name);
        execute_task(&entry, agent_name).await
    }
}

/// Check whether a heartbeat entry is due for execution based on its cron schedule.
///
/// Parses the entry's schedule string using the `cron` crate and checks whether
/// the current time falls within the next expected execution window.
pub fn is_due(entry: &HeartbeatEntry) -> bool {
    if !entry.enabled {
        return false;
    }

    let schedule: Schedule = match entry.schedule.parse() {
        Ok(s) => s,
        Err(e) => {
            warn!(
                "Invalid cron schedule '{}' for entry '{}': {}",
                entry.schedule, entry.name, e
            );
            return false;
        }
    };

    let now = Utc::now();

    // If there is a last_run timestamp, check if a new scheduled time has arrived since then.
    if let Some(ref last_run_str) = entry.last_run {
        if let Ok(last_run) = last_run_str.parse::<chrono::DateTime<Utc>>() {
            // Find the next scheduled time after the last run.
            if let Some(next) = schedule.after(&last_run).next() {
                return now >= next;
            }
        }
    }

    // No last_run recorded; the task is due immediately.
    true
}

/// Execute a single heartbeat task entry.
///
/// Looks up the task name in the built-in task registry and executes it.
/// Returns the task result indicating whether the automaton should wake.
pub async fn execute_task(
    entry: &HeartbeatEntry,
    agent_name: &str,
) -> Result<HeartbeatTaskResult> {
    let builtin_tasks = BUILTIN_TASKS();
    let task_fn = builtin_tasks.get(entry.task.as_str()).with_context(|| {
        format!(
            "No built-in task function found for task '{}'",
            entry.task
        )
    })?;

    info!("Executing heartbeat task: {} (task={})", entry.name, entry.task);
    let result = task_fn(agent_name).await;

    match &result {
        Ok(ref r) => {
            if r.should_wake {
                info!(
                    "Task '{}' requests wake: {}",
                    entry.name,
                    r.message.as_deref().unwrap_or("(no message)")
                );
            } else {
                debug!("Task '{}' completed (no wake)", entry.name);
            }
        }
        Err(ref e) => {
            error!("Task '{}' failed: {:#}", entry.name, e);
        }
    }

    result
}

/// Perform a single tick: iterate over all entries, check which are due,
/// and execute them.
async fn tick(
    entries: &tokio::sync::RwLock<Vec<HeartbeatEntry>>,
    agent_name: &str,
) -> Result<()> {
    let current_entries = entries.read().await.clone();
    let mut executed: HashMap<String, String> = HashMap::new();

    for entry in &current_entries {
        if is_due(entry) {
            match execute_task(entry, agent_name).await {
                Ok(_result) => {
                    let now = Utc::now().to_rfc3339();
                    executed.insert(entry.name.clone(), now);
                }
                Err(e) => {
                    error!("Failed to execute heartbeat task '{}': {:#}", entry.name, e);
                }
            }
        }
    }

    // Update last_run timestamps for executed tasks.
    if !executed.is_empty() {
        let mut writable = entries.write().await;
        for entry in writable.iter_mut() {
            if let Some(timestamp) = executed.get(&entry.name) {
                entry.last_run = Some(timestamp.clone());
            }
        }
    }

    Ok(())
}
