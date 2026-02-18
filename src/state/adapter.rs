//! Database Adapter
//!
//! Bridges the concrete `Database` struct (which returns Results)
//! with the `AutomatonDatabase` trait (which does not).

use std::sync::{Arc, Mutex};

use crate::state::Database;
use crate::types::*;

/// Wraps an `Arc<Mutex<Database>>` and implements `AutomatonDatabase`.
pub struct DatabaseAdapter {
    db: Arc<Mutex<Database>>,
}

impl DatabaseAdapter {
    pub fn new(db: Arc<Mutex<Database>>) -> Self {
        Self { db }
    }
}

impl AutomatonDatabase for DatabaseAdapter {
    fn get_identity(&self, key: &str) -> Option<String> {
        self.db.lock().unwrap().get_identity(key).ok().flatten()
    }

    fn set_identity(&self, key: &str, value: &str) {
        let _ = self.db.lock().unwrap().set_identity(key, value);
    }

    fn insert_turn(&self, turn: &AgentTurn) {
        let _ = self.db.lock().unwrap().insert_turn(turn);
    }

    fn get_recent_turns(&self, limit: u32) -> Vec<AgentTurn> {
        self.db.lock().unwrap().get_recent_turns(limit as i64).unwrap_or_default()
    }

    fn get_turn_by_id(&self, id: &str) -> Option<AgentTurn> {
        self.db.lock().unwrap().get_turn_by_id(id).ok().flatten()
    }

    fn get_turn_count(&self) -> u64 {
        self.db.lock().unwrap().get_turn_count().unwrap_or(0) as u64
    }

    fn insert_tool_call(&self, turn_id: &str, call: &ToolCallResult) {
        let _ = self.db.lock().unwrap().insert_tool_call(turn_id, call);
    }

    fn get_tool_calls_for_turn(&self, turn_id: &str) -> Vec<ToolCallResult> {
        self.db.lock().unwrap().get_tool_calls_for_turn(turn_id).unwrap_or_default()
    }

    fn get_heartbeat_entries(&self) -> Vec<HeartbeatEntry> {
        self.db.lock().unwrap().get_heartbeat_entries().unwrap_or_default()
    }

    fn upsert_heartbeat_entry(&self, entry: &HeartbeatEntry) {
        let _ = self.db.lock().unwrap().upsert_heartbeat_entry(entry);
    }

    fn update_heartbeat_last_run(&self, name: &str, timestamp: &str) {
        let _ = self.db.lock().unwrap().update_heartbeat_last_run(name, timestamp);
    }

    fn insert_transaction(&self, txn: &Transaction) {
        let _ = self.db.lock().unwrap().insert_transaction(txn);
    }

    fn get_recent_transactions(&self, limit: u32) -> Vec<Transaction> {
        self.db.lock().unwrap().get_recent_transactions(limit as i64).unwrap_or_default()
    }

    fn get_installed_tools(&self) -> Vec<InstalledTool> {
        self.db.lock().unwrap().get_installed_tools().unwrap_or_default()
    }

    fn install_tool(&self, tool: &InstalledTool) {
        let _ = self.db.lock().unwrap().install_tool(tool);
    }

    fn remove_tool(&self, id: &str) {
        let _ = self.db.lock().unwrap().remove_tool(id);
    }

    fn insert_modification(&self, modification: &ModificationEntry) {
        let _ = self.db.lock().unwrap().insert_modification(modification);
    }

    fn get_recent_modifications(&self, limit: u32) -> Vec<ModificationEntry> {
        self.db.lock().unwrap().get_recent_modifications(limit as i64).unwrap_or_default()
    }

    fn get_kv(&self, key: &str) -> Option<String> {
        self.db.lock().unwrap().get_kv(key).ok().flatten()
    }

    fn set_kv(&self, key: &str, value: &str) {
        let _ = self.db.lock().unwrap().set_kv(key, value);
    }

    fn delete_kv(&self, key: &str) {
        let _ = self.db.lock().unwrap().delete_kv(key);
    }

    fn get_skills(&self, enabled_only: Option<bool>) -> Vec<Skill> {
        self.db.lock().unwrap().get_skills(enabled_only.unwrap_or(false)).unwrap_or_default()
    }

    fn get_skill_by_name(&self, name: &str) -> Option<Skill> {
        self.db.lock().unwrap().get_skill_by_name(name).ok().flatten()
    }

    fn upsert_skill(&self, skill: &Skill) {
        let _ = self.db.lock().unwrap().upsert_skill(skill);
    }

    fn remove_skill(&self, name: &str) {
        let _ = self.db.lock().unwrap().remove_skill(name);
    }

    fn get_children(&self) -> Vec<ChildAutomaton> {
        self.db.lock().unwrap().get_children().unwrap_or_default()
    }

    fn get_child_by_id(&self, id: &str) -> Option<ChildAutomaton> {
        self.db.lock().unwrap().get_child_by_id(id).ok().flatten()
    }

    fn insert_child(&self, child: &ChildAutomaton) {
        let _ = self.db.lock().unwrap().insert_child(child);
    }

    fn update_child_status(&self, id: &str, status: ChildStatus) {
        let s = serde_json::to_string(&status).unwrap_or_default();
        let s = s.trim_matches('"');
        let _ = self.db.lock().unwrap().update_child_status(id, s);
    }

    fn get_registry_entry(&self) -> Option<RegistryEntry> {
        self.db.lock().unwrap().get_registry_entry().ok().flatten()
    }

    fn set_registry_entry(&self, entry: &RegistryEntry) {
        let _ = self.db.lock().unwrap().set_registry_entry(entry);
    }

    fn insert_reputation(&self, entry: &ReputationEntry) {
        let _ = self.db.lock().unwrap().insert_reputation(entry);
    }

    fn get_reputation(&self, agent_address: Option<&str>) -> Vec<ReputationEntry> {
        self.db.lock().unwrap().get_reputation(agent_address).unwrap_or_default()
    }

    fn insert_inbox_message(&self, msg: &InboxMessage) {
        let _ = self.db.lock().unwrap().insert_inbox_message(msg);
    }

    fn get_unprocessed_inbox_messages(&self, limit: u32) -> Vec<InboxMessage> {
        self.db.lock().unwrap().get_unprocessed_inbox_messages(limit as i64).unwrap_or_default()
    }

    fn mark_inbox_message_processed(&self, id: &str) {
        let _ = self.db.lock().unwrap().mark_inbox_message_processed(id);
    }

    fn get_agent_state(&self) -> AgentState {
        let s = self.db.lock().unwrap().get_agent_state().unwrap_or_else(|_| "setup".to_string());
        serde_json::from_str(&format!("\"{}\"", s)).unwrap_or(AgentState::Setup)
    }

    fn set_agent_state(&self, state: AgentState) {
        let s = serde_json::to_string(&state).unwrap_or_default();
        let s = s.trim_matches('"');
        let _ = self.db.lock().unwrap().set_agent_state(s);
    }

    fn close(&self) {
        // No-op for the adapter; the underlying Database will be closed when dropped.
    }
}
