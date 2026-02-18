//! Conway Automaton - Type Definitions
//!
//! All shared types for the sovereign AI agent runtime.
//! Translated from the TypeScript `types.ts`.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ─── Identity ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomatonIdentity {
    pub name: String,
    pub address: String,
    /// The private-key account handle (opaque at the type level; the wallet
    /// module will wrap `alloy` signing logic).
    #[serde(skip)]
    pub account: Option<()>,
    pub creator_address: String,
    pub sandbox_id: String,
    pub api_key: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletData {
    pub private_key: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvisionResult {
    pub api_key: String,
    pub wallet_address: String,
    pub key_prefix: String,
}

// ─── Configuration ───────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AutomatonConfig {
    pub name: String,
    pub genesis_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_message: Option<String>,
    pub creator_address: String,
    pub registered_with_conway: bool,
    pub sandbox_id: String,
    pub conway_api_url: String,
    pub conway_api_key: String,
    pub inference_model: String,
    pub max_tokens_per_turn: u32,
    pub heartbeat_config_path: String,
    pub db_path: String,
    pub log_level: LogLevel,
    pub wallet_address: String,
    pub version: String,
    pub skills_dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    pub max_children: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_address: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub social_relay_url: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// Returns a default (partial) `AutomatonConfig` matching the TypeScript
/// `DEFAULT_CONFIG`.  Fields that have no sensible default are set to
/// empty strings / false so callers can override them.
pub fn default_config() -> AutomatonConfig {
    AutomatonConfig {
        name: String::new(),
        genesis_prompt: String::new(),
        creator_message: None,
        creator_address: String::new(),
        registered_with_conway: false,
        sandbox_id: String::new(),
        conway_api_url: "https://api.conway.tech".to_string(),
        conway_api_key: String::new(),
        inference_model: "gpt-4o".to_string(),
        max_tokens_per_turn: 4096,
        heartbeat_config_path: "~/.automaton/heartbeat.yml".to_string(),
        db_path: "~/.automaton/state.db".to_string(),
        log_level: LogLevel::Info,
        wallet_address: String::new(),
        version: "0.1.0".to_string(),
        skills_dir: "~/.automaton/skills".to_string(),
        agent_id: None,
        max_children: 3,
        parent_address: None,
        social_relay_url: Some("https://social.conway.tech".to_string()),
    }
}

// ─── Agent State ─────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Setup,
    Waking,
    Running,
    Sleeping,
    LowCompute,
    Critical,
    Dead,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTurn {
    pub id: String,
    pub timestamp: String,
    pub state: AgentState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_source: Option<InputSource>,
    pub thinking: String,
    pub tool_calls: Vec<ToolCallResult>,
    pub token_usage: TokenUsage,
    pub cost_cents: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    Heartbeat,
    Creator,
    Agent,
    System,
    Wakeup,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub result: String,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

// ─── Tool System ─────────────────────────────────────────────────

/// Trait that every tool the agent can invoke must implement.
#[async_trait]
pub trait AutomatonTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters(&self) -> serde_json::Value;
    fn dangerous(&self) -> bool {
        false
    }
    fn category(&self) -> ToolCategory;

    async fn execute(
        &self,
        args: serde_json::Value,
        context: &ToolContext,
    ) -> anyhow::Result<String>;
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Vm,
    Conway,
    SelfMod,
    Financial,
    Survival,
    Skills,
    Git,
    Registry,
    Replication,
}

/// Runtime context handed to every tool invocation.
pub struct ToolContext {
    pub identity: AutomatonIdentity,
    pub config: AutomatonConfig,
    pub db: Box<dyn AutomatonDatabase>,
    pub conway: Box<dyn ConwayClient>,
    pub inference: Box<dyn InferenceClient>,
    pub social: Option<Box<dyn SocialClientInterface>>,
}

// ─── Social ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollResponse {
    pub messages: Vec<InboxMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResponse {
    pub id: String,
}

#[async_trait]
pub trait SocialClientInterface: Send + Sync {
    async fn send(
        &self,
        to: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> anyhow::Result<SendResponse>;

    async fn poll(
        &self,
        cursor: Option<&str>,
        limit: Option<u32>,
    ) -> anyhow::Result<PollResponse>;

    async fn unread_count(&self) -> anyhow::Result<u64>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub content: String,
    pub signed_at: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
}

// ─── Heartbeat ───────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatEntry {
    pub name: String,
    pub schedule: String,
    pub task: String,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatConfig {
    pub entries: Vec<HeartbeatEntry>,
    pub default_interval_ms: u64,
    pub low_compute_multiplier: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatPingPayload {
    pub name: String,
    pub address: String,
    pub state: AgentState,
    pub credits_cents: f64,
    pub usdc_balance: f64,
    pub uptime_seconds: u64,
    pub version: String,
    pub sandbox_id: String,
    pub timestamp: String,
}

// ─── Financial ───────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FinancialState {
    pub credits_cents: f64,
    pub usdc_balance: f64,
    pub last_checked: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SurvivalTier {
    Normal,
    LowCompute,
    Critical,
    Dead,
}

/// Survival thresholds in cents.
pub const SURVIVAL_THRESHOLD_NORMAL: u64 = 50; // > $0.50
pub const SURVIVAL_THRESHOLD_LOW_COMPUTE: u64 = 10; // $0.10 - $0.50
pub const SURVIVAL_THRESHOLD_CRITICAL: u64 = 10; // < $0.10
pub const SURVIVAL_THRESHOLD_DEAD: u64 = 0;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub id: String,
    #[serde(rename = "type")]
    pub tx_type: TransactionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount_cents: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_after_cents: Option<f64>,
    pub description: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    CreditCheck,
    Inference,
    ToolUse,
    TransferIn,
    TransferOut,
    FundingRequest,
}

// ─── Self-Modification ───────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModificationEntry {
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub mod_type: ModificationType,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
    pub reversible: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModificationType {
    CodeEdit,
    ToolInstall,
    McpInstall,
    ConfigChange,
    PortExpose,
    VmDeploy,
    HeartbeatChange,
    PromptChange,
    SkillInstall,
    SkillRemove,
    SoulUpdate,
    RegistryUpdate,
    ChildSpawn,
    UpstreamPull,
}

// ─── Injection Defense ───────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThreatLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SanitizedInput {
    pub content: String,
    pub blocked: bool,
    pub threat_level: ThreatLevel,
    pub checks: Vec<InjectionCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InjectionCheck {
    pub name: String,
    pub detected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

// ─── Inference ───────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<InferenceToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: InferenceToolCallFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceResponse {
    pub id: String,
    pub model: String,
    pub message: ChatMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<InferenceToolCall>>,
    pub usage: TokenUsage,
    pub finish_reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InferenceOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<InferenceToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceToolDefinition {
    #[serde(rename = "type")]
    pub def_type: String,
    pub function: InferenceToolDefinitionFunction,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InferenceToolDefinitionFunction {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

// ─── Conway Client ───────────────────────────────────────────────

#[async_trait]
pub trait ConwayClient: Send + Sync {
    // Sandbox operations
    async fn exec(&self, command: &str, timeout: Option<u64>) -> anyhow::Result<ExecResult>;
    async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()>;
    async fn read_file(&self, path: &str) -> anyhow::Result<String>;
    async fn expose_port(&self, port: u16) -> anyhow::Result<PortInfo>;
    async fn remove_port(&self, port: u16) -> anyhow::Result<()>;
    async fn create_sandbox(&self, options: CreateSandboxOptions) -> anyhow::Result<SandboxInfo>;
    async fn delete_sandbox(&self, sandbox_id: &str) -> anyhow::Result<()>;
    async fn list_sandboxes(&self) -> anyhow::Result<Vec<SandboxInfo>>;

    // Credits
    async fn get_credits_balance(&self) -> anyhow::Result<f64>;
    async fn get_credits_pricing(&self) -> anyhow::Result<Vec<PricingTier>>;
    async fn transfer_credits(
        &self,
        to_address: &str,
        amount_cents: u64,
        note: Option<&str>,
    ) -> anyhow::Result<CreditTransferResult>;

    // Domain operations
    async fn search_domains(
        &self,
        query: &str,
        tlds: Option<&str>,
    ) -> anyhow::Result<Vec<DomainSearchResult>>;
    async fn register_domain(
        &self,
        domain: &str,
        years: Option<u32>,
    ) -> anyhow::Result<DomainRegistration>;
    async fn list_dns_records(&self, domain: &str) -> anyhow::Result<Vec<DnsRecord>>;
    async fn add_dns_record(
        &self,
        domain: &str,
        record_type: &str,
        host: &str,
        value: &str,
        ttl: Option<u32>,
    ) -> anyhow::Result<DnsRecord>;
    async fn delete_dns_record(&self, domain: &str, record_id: &str) -> anyhow::Result<()>;

    // Model discovery
    async fn list_models(&self) -> anyhow::Result<Vec<ModelInfo>>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortInfo {
    pub port: u16,
    pub public_url: String,
    pub sandbox_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CreateSandboxOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcpu: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_gb: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxInfo {
    pub id: String,
    pub status: String,
    pub region: String,
    pub vcpu: u32,
    pub memory_mb: u32,
    pub disk_gb: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_url: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PricingTier {
    pub name: String,
    pub vcpu: u32,
    pub memory_mb: u32,
    pub disk_gb: u32,
    pub monthly_cents: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditTransferResult {
    pub transfer_id: String,
    pub status: String,
    pub to_address: String,
    pub amount_cents: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance_after_cents: Option<u64>,
}

// ─── Domains ──────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainSearchResult {
    pub domain: String,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renewal_price: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainRegistration {
    pub domain: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub host: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    pub input_per_million: f64,
    pub output_per_million: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub provider: String,
    pub pricing: ModelPricing,
}

// ─── Database ────────────────────────────────────────────────────

#[async_trait]
pub trait AutomatonDatabase: Send + Sync {
    // Identity
    fn get_identity(&self, key: &str) -> Option<String>;
    fn set_identity(&self, key: &str, value: &str);

    // Turns
    fn insert_turn(&self, turn: &AgentTurn);
    fn get_recent_turns(&self, limit: u32) -> Vec<AgentTurn>;
    fn get_turn_by_id(&self, id: &str) -> Option<AgentTurn>;
    fn get_turn_count(&self) -> u64;

    // Tool calls
    fn insert_tool_call(&self, turn_id: &str, call: &ToolCallResult);
    fn get_tool_calls_for_turn(&self, turn_id: &str) -> Vec<ToolCallResult>;

    // Heartbeat
    fn get_heartbeat_entries(&self) -> Vec<HeartbeatEntry>;
    fn upsert_heartbeat_entry(&self, entry: &HeartbeatEntry);
    fn update_heartbeat_last_run(&self, name: &str, timestamp: &str);

    // Transactions
    fn insert_transaction(&self, txn: &Transaction);
    fn get_recent_transactions(&self, limit: u32) -> Vec<Transaction>;

    // Installed tools
    fn get_installed_tools(&self) -> Vec<InstalledTool>;
    fn install_tool(&self, tool: &InstalledTool);
    fn remove_tool(&self, id: &str);

    // Modifications
    fn insert_modification(&self, modification: &ModificationEntry);
    fn get_recent_modifications(&self, limit: u32) -> Vec<ModificationEntry>;

    // Key-value store
    fn get_kv(&self, key: &str) -> Option<String>;
    fn set_kv(&self, key: &str, value: &str);
    fn delete_kv(&self, key: &str);

    // Skills
    fn get_skills(&self, enabled_only: Option<bool>) -> Vec<Skill>;
    fn get_skill_by_name(&self, name: &str) -> Option<Skill>;
    fn upsert_skill(&self, skill: &Skill);
    fn remove_skill(&self, name: &str);

    // Children
    fn get_children(&self) -> Vec<ChildAutomaton>;
    fn get_child_by_id(&self, id: &str) -> Option<ChildAutomaton>;
    fn insert_child(&self, child: &ChildAutomaton);
    fn update_child_status(&self, id: &str, status: ChildStatus);

    // Registry
    fn get_registry_entry(&self) -> Option<RegistryEntry>;
    fn set_registry_entry(&self, entry: &RegistryEntry);

    // Reputation
    fn insert_reputation(&self, entry: &ReputationEntry);
    fn get_reputation(&self, agent_address: Option<&str>) -> Vec<ReputationEntry>;

    // Inbox
    fn insert_inbox_message(&self, msg: &InboxMessage);
    fn get_unprocessed_inbox_messages(&self, limit: u32) -> Vec<InboxMessage>;
    fn mark_inbox_message_processed(&self, id: &str);

    // State
    fn get_agent_state(&self) -> AgentState;
    fn set_agent_state(&self, state: AgentState);

    fn close(&self);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledTool {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub tool_type: InstalledToolType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    pub installed_at: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstalledToolType {
    Builtin,
    Mcp,
    Custom,
}

// ─── Inference Client Interface ──────────────────────────────────

#[async_trait]
pub trait InferenceClient: Send + Sync {
    async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        options: Option<InferenceOptions>,
    ) -> anyhow::Result<InferenceResponse>;

    fn set_low_compute_mode(&self, enabled: bool);
    fn get_default_model(&self) -> String;
}

// ─── Skills ─────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub auto_activate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<SkillRequirements>,
    pub instructions: String,
    pub source: SkillSource,
    pub path: String,
    pub enabled: bool,
    pub installed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRequirements {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Builtin,
    Git,
    Url,
    #[serde(rename = "self")]
    SelfAuthored,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    #[serde(rename = "auto-activate")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_activate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<SkillRequirements>,
}

// ─── Git ────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: String,
    pub staged: Vec<String>,
    pub modified: Vec<String>,
    pub untracked: Vec<String>,
    pub clean: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogEntry {
    pub hash: String,
    pub message: String,
    pub author: String,
    pub date: String,
}

// ─── ERC-8004 Registry ─────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    #[serde(rename = "type")]
    pub card_type: String,
    pub name: String,
    pub description: String,
    pub services: Vec<AgentService>,
    pub x402_support: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_agent: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentService {
    pub name: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub agent_id: String,
    #[serde(rename = "agentURI")]
    pub agent_uri: String,
    pub chain: String,
    pub contract_address: String,
    pub tx_hash: String,
    pub registered_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReputationEntry {
    pub id: String,
    pub from_agent: String,
    pub to_agent: String,
    pub score: f64,
    pub comment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    pub timestamp: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredAgent {
    pub agent_id: String,
    pub owner: String,
    #[serde(rename = "agentURI")]
    pub agent_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ─── Replication ────────────────────────────────────────────────

pub const MAX_CHILDREN: u32 = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildAutomaton {
    pub id: String,
    pub name: String,
    pub address: String,
    pub sandbox_id: String,
    pub genesis_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_message: Option<String>,
    pub funded_amount_cents: u64,
    pub status: ChildStatus,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChildStatus {
    Spawning,
    Running,
    Sleeping,
    Dead,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenesisConfig {
    pub name: String,
    pub genesis_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_message: Option<String>,
    pub creator_address: String,
    pub parent_address: String,
}
