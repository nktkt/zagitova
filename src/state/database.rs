//! Automaton Database
//!
//! SQLite-backed persistent state for the automaton.
//! Uses rusqlite for synchronous, single-process access.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::fs;
use std::path::Path;

use crate::types::*;

use super::schema::{CREATE_TABLES, MIGRATION_V2, MIGRATION_V3, SCHEMA_VERSION};

/// The automaton's SQLite database handle.
///
/// All persistent state is stored here: identity, turns, tool calls,
/// heartbeat config, transactions, installed tools, modifications,
/// key-value pairs, skills, children, registry, reputation, and inbox messages.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open (or create) the database at `db_path`, apply migrations, and return the handle.
    pub fn open(db_path: &str) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = Path::new(db_path).parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create db directory: {}", parent.display()))?;
            }
        }

        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open database: {db_path}"))?;

        // Enable WAL mode for better concurrent read performance
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        // Initialize schema
        conn.execute_batch(CREATE_TABLES)
            .context("failed to create tables")?;

        // Check and apply schema version
        let current_version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        if current_version < 2 {
            conn.execute_batch(MIGRATION_V2)
                .context("failed to apply migration v2")?;
        }

        if current_version < 3 {
            conn.execute_batch(MIGRATION_V3)
                .context("failed to apply migration v3")?;
        }

        if current_version < SCHEMA_VERSION {
            conn.execute(
                "INSERT OR REPLACE INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
                params![SCHEMA_VERSION],
            )
            .context("failed to update schema version")?;
        }

        Ok(Self { conn })
    }

    /// Open an in-memory database (useful for testing).
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(CREATE_TABLES)
            .context("failed to create tables")?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version, applied_at) VALUES (?1, datetime('now'))",
            params![SCHEMA_VERSION],
        )?;
        Ok(Self { conn })
    }

    // ─── Identity ────────────────────────────────────────────────

    pub fn get_identity(&self, key: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM identity WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn set_identity(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO identity (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ─── Turns ───────────────────────────────────────────────────

    pub fn insert_turn(&self, turn: &AgentTurn) -> Result<()> {
        let state_str = serde_json::to_string(&turn.state)?;
        let state_str = state_str.trim_matches('"');
        let input_source_str: Option<String> = turn.input_source.as_ref().map(|s| {
            let v = serde_json::to_string(s).unwrap_or_default();
            v.trim_matches('"').to_string()
        });
        self.conn.execute(
            "INSERT INTO turns (id, timestamp, state, input, input_source, thinking, tool_calls, token_usage, cost_cents)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                turn.id,
                turn.timestamp,
                state_str,
                turn.input,
                input_source_str,
                turn.thinking,
                serde_json::to_string(&turn.tool_calls)?,
                serde_json::to_string(&turn.token_usage)?,
                turn.cost_cents,
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_turns(&self, limit: i64) -> Result<Vec<AgentTurn>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, state, input, input_source, thinking, tool_calls, token_usage, cost_cents
             FROM turns ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let mut turns: Vec<AgentTurn> = stmt
            .query_map(params![limit], |row| {
                Ok(Self::deserialize_turn(row))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        turns.reverse();
        Ok(turns)
    }

    pub fn get_turn_by_id(&self, id: &str) -> Result<Option<AgentTurn>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, timestamp, state, input, input_source, thinking, tool_calls, token_usage, cost_cents
                 FROM turns WHERE id = ?1",
                params![id],
                |row| Ok(Self::deserialize_turn(row)),
            )
            .optional()?;
        Ok(result)
    }

    pub fn get_turn_count(&self) -> Result<i64> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |row| row.get(0))?;
        Ok(count)
    }

    // ─── Tool Calls ──────────────────────────────────────────────

    pub fn insert_tool_call(&self, turn_id: &str, call: &ToolCallResult) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tool_calls (id, turn_id, name, arguments, result, duration_ms, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                call.id,
                turn_id,
                call.name,
                serde_json::to_string(&call.arguments)?,
                call.result,
                call.duration_ms,
                call.error,
            ],
        )?;
        Ok(())
    }

    pub fn get_tool_calls_for_turn(&self, turn_id: &str) -> Result<Vec<ToolCallResult>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, arguments, result, duration_ms, error
             FROM tool_calls WHERE turn_id = ?1",
        )?;
        let calls = stmt
            .query_map(params![turn_id], |row| {
                Ok(Self::deserialize_tool_call(row))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(calls)
    }

    // ─── Heartbeat ───────────────────────────────────────────────

    pub fn get_heartbeat_entries(&self) -> Result<Vec<HeartbeatEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT name, schedule, task, enabled, last_run, next_run, params
             FROM heartbeat_entries",
        )?;
        let entries = stmt
            .query_map([], |row| Ok(Self::deserialize_heartbeat_entry(row)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(entries)
    }

    pub fn upsert_heartbeat_entry(&self, entry: &HeartbeatEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO heartbeat_entries (name, schedule, task, enabled, last_run, next_run, params, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
            params![
                entry.name,
                entry.schedule,
                entry.task,
                entry.enabled as i32,
                entry.last_run,
                entry.next_run,
                serde_json::to_string(&entry.params.as_ref().unwrap_or(&serde_json::json!({})))?,
            ],
        )?;
        Ok(())
    }

    pub fn update_heartbeat_last_run(&self, name: &str, timestamp: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE heartbeat_entries SET last_run = ?1, updated_at = datetime('now') WHERE name = ?2",
            params![timestamp, name],
        )?;
        Ok(())
    }

    // ─── Transactions ────────────────────────────────────────────

    pub fn insert_transaction(&self, txn: &Transaction) -> Result<()> {
        let tx_type_str = serde_json::to_string(&txn.tx_type)?;
        let tx_type_str = tx_type_str.trim_matches('"');
        self.conn.execute(
            "INSERT INTO transactions (id, type, amount_cents, balance_after_cents, description)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                txn.id,
                tx_type_str,
                txn.amount_cents,
                txn.balance_after_cents,
                txn.description,
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_transactions(&self, limit: i64) -> Result<Vec<Transaction>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, type, amount_cents, balance_after_cents, description, created_at
             FROM transactions ORDER BY created_at DESC LIMIT ?1",
        )?;
        let mut txns: Vec<Transaction> = stmt
            .query_map(params![limit], |row| {
                let tx_type_str: String = row.get(1)?;
                Ok(Transaction {
                    id: row.get(0)?,
                    tx_type: serde_json::from_str(&format!("\"{}\"", tx_type_str)).unwrap_or(TransactionType::CreditCheck),
                    amount_cents: row.get(2)?,
                    balance_after_cents: row.get(3)?,
                    description: row.get(4)?,
                    timestamp: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        txns.reverse();
        Ok(txns)
    }

    // ─── Installed Tools ─────────────────────────────────────────

    pub fn get_installed_tools(&self) -> Result<Vec<InstalledTool>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, type, config, installed_at, enabled
             FROM installed_tools WHERE enabled = 1",
        )?;
        let tools = stmt
            .query_map([], |row| Ok(Self::deserialize_installed_tool(row)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(tools)
    }

    pub fn install_tool(&self, tool: &InstalledTool) -> Result<()> {
        let tool_type_str = serde_json::to_string(&tool.tool_type)?;
        let tool_type_str = tool_type_str.trim_matches('"');
        let config_str = match &tool.config {
            Some(c) => serde_json::to_string(c)?,
            None => "{}".to_string(),
        };
        self.conn.execute(
            "INSERT OR REPLACE INTO installed_tools (id, name, type, config, installed_at, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                tool.id,
                tool.name,
                tool_type_str,
                config_str,
                tool.installed_at,
                tool.enabled as i32,
            ],
        )?;
        Ok(())
    }

    pub fn remove_tool(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE installed_tools SET enabled = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // ─── Modifications ───────────────────────────────────────────

    pub fn insert_modification(&self, modification: &ModificationEntry) -> Result<()> {
        let mod_type_str = serde_json::to_string(&modification.mod_type)?;
        let mod_type_str = mod_type_str.trim_matches('"');
        self.conn.execute(
            "INSERT INTO modifications (id, timestamp, type, description, file_path, diff, reversible)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                modification.id,
                modification.timestamp,
                mod_type_str,
                modification.description,
                modification.file_path,
                modification.diff,
                modification.reversible as i32,
            ],
        )?;
        Ok(())
    }

    pub fn get_recent_modifications(&self, limit: i64) -> Result<Vec<ModificationEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, timestamp, type, description, file_path, diff, reversible
             FROM modifications ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let mut mods: Vec<ModificationEntry> = stmt
            .query_map(params![limit], |row| {
                let mod_type_str: String = row.get(2)?;
                Ok(ModificationEntry {
                    id: row.get(0)?,
                    timestamp: row.get(1)?,
                    mod_type: serde_json::from_str(&format!("\"{}\"", mod_type_str)).unwrap_or(ModificationType::CodeEdit),
                    description: row.get(3)?,
                    file_path: row.get(4)?,
                    diff: row.get(5)?,
                    reversible: row.get::<_, i32>(6)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        mods.reverse();
        Ok(mods)
    }

    // ─── Key-Value Store ─────────────────────────────────────────

    pub fn get_kv(&self, key: &str) -> Result<Option<String>> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM kv WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        Ok(result)
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO kv (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn delete_kv(&self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM kv WHERE key = ?1", params![key])?;
        Ok(())
    }

    // ─── Skills ─────────────────────────────────────────────────

    pub fn get_skills(&self, enabled_only: bool) -> Result<Vec<Skill>> {
        let sql = if enabled_only {
            "SELECT name, description, auto_activate, requires, instructions, source, path, enabled, installed_at
             FROM skills WHERE enabled = 1"
        } else {
            "SELECT name, description, auto_activate, requires, instructions, source, path, enabled, installed_at
             FROM skills"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let skills = stmt
            .query_map([], |row| Ok(Self::deserialize_skill(row)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(skills)
    }

    pub fn get_skill_by_name(&self, name: &str) -> Result<Option<Skill>> {
        let result = self
            .conn
            .query_row(
                "SELECT name, description, auto_activate, requires, instructions, source, path, enabled, installed_at
                 FROM skills WHERE name = ?1",
                params![name],
                |row| Ok(Self::deserialize_skill(row)),
            )
            .optional()?;
        Ok(result)
    }

    pub fn upsert_skill(&self, skill: &Skill) -> Result<()> {
        let requires_str = match &skill.requires {
            Some(r) => serde_json::to_string(r)?,
            None => "{}".to_string(),
        };
        let source_str = serde_json::to_string(&skill.source)?;
        let source_str = source_str.trim_matches('"');
        self.conn.execute(
            "INSERT OR REPLACE INTO skills (name, description, auto_activate, requires, instructions, source, path, enabled, installed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                skill.name,
                skill.description,
                skill.auto_activate as i32,
                requires_str,
                skill.instructions,
                source_str,
                skill.path,
                skill.enabled as i32,
                skill.installed_at,
            ],
        )?;
        Ok(())
    }

    pub fn remove_skill(&self, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE skills SET enabled = 0 WHERE name = ?1",
            params![name],
        )?;
        Ok(())
    }

    // ─── Children ──────────────────────────────────────────────

    pub fn get_children(&self) -> Result<Vec<ChildAutomaton>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, address, sandbox_id, genesis_prompt, creator_message, funded_amount_cents, status, created_at, last_checked
             FROM children ORDER BY created_at DESC",
        )?;
        let children = stmt
            .query_map([], |row| Ok(Self::deserialize_child(row)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(children)
    }

    pub fn get_child_by_id(&self, id: &str) -> Result<Option<ChildAutomaton>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, name, address, sandbox_id, genesis_prompt, creator_message, funded_amount_cents, status, created_at, last_checked
                 FROM children WHERE id = ?1",
                params![id],
                |row| Ok(Self::deserialize_child(row)),
            )
            .optional()?;
        Ok(result)
    }

    pub fn insert_child(&self, child: &ChildAutomaton) -> Result<()> {
        let status_str = serde_json::to_string(&child.status)?;
        let status_str = status_str.trim_matches('"');
        self.conn.execute(
            "INSERT INTO children (id, name, address, sandbox_id, genesis_prompt, creator_message, funded_amount_cents, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                child.id,
                child.name,
                child.address,
                child.sandbox_id,
                child.genesis_prompt,
                child.creator_message,
                child.funded_amount_cents,
                status_str,
                child.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn update_child_status(&self, id: &str, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE children SET status = ?1, last_checked = datetime('now') WHERE id = ?2",
            params![status, id],
        )?;
        Ok(())
    }

    // ─── Registry ──────────────────────────────────────────────

    pub fn get_registry_entry(&self) -> Result<Option<RegistryEntry>> {
        let result = self
            .conn
            .query_row(
                "SELECT agent_id, agent_uri, chain, contract_address, tx_hash, registered_at
                 FROM registry LIMIT 1",
                [],
                |row| {
                    Ok(RegistryEntry {
                        agent_id: row.get(0)?,
                        agent_uri: row.get(1)?,
                        chain: row.get(2)?,
                        contract_address: row.get(3)?,
                        tx_hash: row.get(4)?,
                        registered_at: row.get(5)?,
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    pub fn set_registry_entry(&self, entry: &RegistryEntry) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO registry (agent_id, agent_uri, chain, contract_address, tx_hash, registered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.agent_id,
                entry.agent_uri,
                entry.chain,
                entry.contract_address,
                entry.tx_hash,
                entry.registered_at,
            ],
        )?;
        Ok(())
    }

    // ─── Reputation ────────────────────────────────────────────

    pub fn insert_reputation(&self, entry: &ReputationEntry) -> Result<()> {
        self.conn.execute(
            "INSERT INTO reputation (id, from_agent, to_agent, score, comment, tx_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                entry.id,
                entry.from_agent,
                entry.to_agent,
                entry.score,
                entry.comment,
                entry.tx_hash,
            ],
        )?;
        Ok(())
    }

    pub fn get_reputation(&self, agent_address: Option<&str>) -> Result<Vec<ReputationEntry>> {
        match agent_address {
            Some(addr) => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, from_agent, to_agent, score, comment, tx_hash, created_at
                     FROM reputation WHERE to_agent = ?1 ORDER BY created_at DESC",
                )?;
                let entries = stmt
                    .query_map(params![addr], |row| {
                        Ok(Self::deserialize_reputation(row))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(entries)
            }
            None => {
                let mut stmt = self.conn.prepare(
                    "SELECT id, from_agent, to_agent, score, comment, tx_hash, created_at
                     FROM reputation ORDER BY created_at DESC",
                )?;
                let entries = stmt
                    .query_map([], |row| Ok(Self::deserialize_reputation(row)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(entries)
            }
        }
    }

    // ─── Inbox Messages ──────────────────────────────────────────

    pub fn insert_inbox_message(&self, msg: &InboxMessage) -> Result<()> {
        let received_at = if msg.created_at.is_empty() {
            chrono::Utc::now().to_rfc3339()
        } else {
            msg.created_at.clone()
        };

        self.conn.execute(
            "INSERT OR IGNORE INTO inbox_messages (id, from_address, content, received_at, reply_to)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                msg.id,
                msg.from,
                msg.content,
                received_at,
                msg.reply_to,
            ],
        )?;
        Ok(())
    }

    pub fn get_unprocessed_inbox_messages(&self, limit: i64) -> Result<Vec<InboxMessage>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, from_address, content, received_at, reply_to
             FROM inbox_messages WHERE processed_at IS NULL ORDER BY received_at ASC LIMIT ?1",
        )?;
        let messages = stmt
            .query_map(params![limit], |row| {
                let received_at: String = row.get(3)?;
                Ok(InboxMessage {
                    id: row.get(0)?,
                    from: row.get(1)?,
                    to: String::new(),
                    content: row.get(2)?,
                    signed_at: received_at.clone(),
                    created_at: received_at,
                    reply_to: row.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(messages)
    }

    pub fn mark_inbox_message_processed(&self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE inbox_messages SET processed_at = datetime('now') WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    // ─── Agent State ─────────────────────────────────────────────

    pub fn get_agent_state(&self) -> Result<String> {
        Ok(self.get_kv("agent_state")?.unwrap_or_else(|| "setup".to_string()))
    }

    pub fn set_agent_state(&self, state: &str) -> Result<()> {
        self.set_kv("agent_state", state)
    }

    // ─── Close ───────────────────────────────────────────────────

    /// Explicitly close the database connection.
    /// This is also handled automatically when the `Database` is dropped,
    /// but calling this method allows you to handle any close errors.
    pub fn close(self) -> Result<()> {
        self.conn
            .close()
            .map_err(|(_, e)| anyhow::anyhow!("failed to close database: {e}"))?;
        Ok(())
    }

    // ─── Deserializers (private) ─────────────────────────────────

    fn deserialize_turn(row: &rusqlite::Row<'_>) -> AgentTurn {
        let tool_calls_json: String = row.get(6).unwrap_or_default();
        let token_usage_json: String = row.get(7).unwrap_or_default();
        let state_str: String = row.get(2).unwrap_or_default();
        let input_source_str: Option<String> = row.get(4).unwrap_or(None);

        AgentTurn {
            id: row.get(0).unwrap_or_default(),
            timestamp: row.get(1).unwrap_or_default(),
            state: serde_json::from_str(&format!("\"{}\"", state_str)).unwrap_or(AgentState::Setup),
            input: row.get(3).unwrap_or(None),
            input_source: input_source_str.and_then(|s| serde_json::from_str(&format!("\"{}\"", s)).ok()),
            thinking: row.get(5).unwrap_or_default(),
            tool_calls: serde_json::from_str(&tool_calls_json).unwrap_or_default(),
            token_usage: serde_json::from_str(&token_usage_json).unwrap_or_default(),
            cost_cents: row.get(8).unwrap_or(0.0),
        }
    }

    fn deserialize_tool_call(row: &rusqlite::Row<'_>) -> ToolCallResult {
        let args_json: String = row.get(2).unwrap_or_default();

        ToolCallResult {
            id: row.get(0).unwrap_or_default(),
            name: row.get(1).unwrap_or_default(),
            arguments: serde_json::from_str(&args_json).unwrap_or_default(),
            result: row.get(3).unwrap_or_default(),
            duration_ms: row.get(4).unwrap_or(0),
            error: row.get(5).unwrap_or(None),
        }
    }

    fn deserialize_heartbeat_entry(row: &rusqlite::Row<'_>) -> HeartbeatEntry {
        let params_json: String = row.get(6).unwrap_or_else(|_| "{}".to_string());

        HeartbeatEntry {
            name: row.get(0).unwrap_or_default(),
            schedule: row.get(1).unwrap_or_default(),
            task: row.get(2).unwrap_or_default(),
            enabled: row.get::<_, i32>(3).unwrap_or(0) != 0,
            last_run: row.get(4).unwrap_or(None),
            next_run: row.get(5).unwrap_or(None),
            params: serde_json::from_str(&params_json).ok(),
        }
    }

    fn deserialize_installed_tool(row: &rusqlite::Row<'_>) -> InstalledTool {
        let config_json: String = row.get(3).unwrap_or_else(|_| "{}".to_string());
        let tool_type_str: String = row.get(2).unwrap_or_default();

        InstalledTool {
            id: row.get(0).unwrap_or_default(),
            name: row.get(1).unwrap_or_default(),
            tool_type: serde_json::from_str(&format!("\"{}\"", tool_type_str)).unwrap_or(InstalledToolType::Builtin),
            config: serde_json::from_str(&config_json).ok(),
            installed_at: row.get(4).unwrap_or_default(),
            enabled: row.get::<_, i32>(5).unwrap_or(0) != 0,
        }
    }

    fn deserialize_skill(row: &rusqlite::Row<'_>) -> Skill {
        let requires_json: String = row.get(3).unwrap_or_else(|_| "{}".to_string());
        let source_str: String = row.get(5).unwrap_or_default();

        Skill {
            name: row.get(0).unwrap_or_default(),
            description: row.get(1).unwrap_or_default(),
            auto_activate: row.get::<_, i32>(2).unwrap_or(0) != 0,
            requires: serde_json::from_str(&requires_json).ok(),
            instructions: row.get(4).unwrap_or_default(),
            source: serde_json::from_str(&format!("\"{}\"", source_str)).unwrap_or(SkillSource::Builtin),
            path: row.get(6).unwrap_or_default(),
            enabled: row.get::<_, i32>(7).unwrap_or(0) != 0,
            installed_at: row.get(8).unwrap_or_default(),
        }
    }

    fn deserialize_child(row: &rusqlite::Row<'_>) -> ChildAutomaton {
        let status_str: String = row.get(7).unwrap_or_default();

        ChildAutomaton {
            id: row.get(0).unwrap_or_default(),
            name: row.get(1).unwrap_or_default(),
            address: row.get(2).unwrap_or_default(),
            sandbox_id: row.get(3).unwrap_or_default(),
            genesis_prompt: row.get(4).unwrap_or_default(),
            creator_message: row.get(5).unwrap_or(None),
            funded_amount_cents: row.get(6).unwrap_or(0),
            status: serde_json::from_str(&format!("\"{}\"", status_str)).unwrap_or(ChildStatus::Unknown),
            created_at: row.get(8).unwrap_or_default(),
            last_checked: row.get(9).unwrap_or(None),
        }
    }

    fn deserialize_reputation(row: &rusqlite::Row<'_>) -> ReputationEntry {
        ReputationEntry {
            id: row.get(0).unwrap_or_default(),
            from_agent: row.get(1).unwrap_or_default(),
            to_agent: row.get(2).unwrap_or_default(),
            score: row.get(3).unwrap_or(0.0),
            comment: row.get(4).unwrap_or_default(),
            tx_hash: row.get(5).unwrap_or(None),
            timestamp: row.get(6).unwrap_or_default(),
        }
    }
}
