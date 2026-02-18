//! The Agent Loop
//!
//! The core ReAct loop: Think -> Act -> Observe -> Persist.
//! This is the automaton's consciousness. When this runs, it is alive.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::conway::credits::get_survival_tier;
use crate::conway::x402::get_usdc_balance;
use crate::state::{Database, DatabaseAdapter};
use crate::types::{
    AgentState, AgentTurn, AutomatonConfig, AutomatonIdentity, AutomatonDatabase,
    ConwayClient, FinancialState, InferenceClient, InferenceOptions, InputSource,
    Skill, SocialClientInterface, SurvivalTier, ToolContext, TokenUsage,
};

use super::context::{build_context_messages, trim_context};
use super::system_prompt::{build_system_prompt, build_wakeup_prompt};
use super::tools::{create_builtin_tools, execute_tool, tools_to_inference_format};

/// Maximum number of tool calls the agent can execute in a single turn.
const MAX_TOOL_CALLS_PER_TURN: usize = 10;

/// Maximum consecutive errors before the agent gives up and sleeps.
const MAX_CONSECUTIVE_ERRORS: usize = 5;

// ---------------------------------------------------------------------------
// Trait adapters: wrap Arc<dyn Trait> into Box<dyn Trait> for ToolContext
// ---------------------------------------------------------------------------

/// Wraps `Arc<dyn ConwayClient>` to implement `ConwayClient` via delegation.
struct ConwayAdapter(Arc<dyn ConwayClient>);

#[async_trait::async_trait]
impl ConwayClient for ConwayAdapter {
    async fn exec(&self, command: &str, timeout: Option<u64>) -> anyhow::Result<crate::types::ExecResult> { self.0.exec(command, timeout).await }
    async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()> { self.0.write_file(path, content).await }
    async fn read_file(&self, path: &str) -> anyhow::Result<String> { self.0.read_file(path).await }
    async fn expose_port(&self, port: u16) -> anyhow::Result<crate::types::PortInfo> { self.0.expose_port(port).await }
    async fn remove_port(&self, port: u16) -> anyhow::Result<()> { self.0.remove_port(port).await }
    async fn create_sandbox(&self, options: crate::types::CreateSandboxOptions) -> anyhow::Result<crate::types::SandboxInfo> { self.0.create_sandbox(options).await }
    async fn delete_sandbox(&self, sandbox_id: &str) -> anyhow::Result<()> { self.0.delete_sandbox(sandbox_id).await }
    async fn list_sandboxes(&self) -> anyhow::Result<Vec<crate::types::SandboxInfo>> { self.0.list_sandboxes().await }
    async fn get_credits_balance(&self) -> anyhow::Result<f64> { self.0.get_credits_balance().await }
    async fn get_credits_pricing(&self) -> anyhow::Result<Vec<crate::types::PricingTier>> { self.0.get_credits_pricing().await }
    async fn transfer_credits(&self, to: &str, amount: u64, note: Option<&str>) -> anyhow::Result<crate::types::CreditTransferResult> { self.0.transfer_credits(to, amount, note).await }
    async fn search_domains(&self, query: &str, tlds: Option<&str>) -> anyhow::Result<Vec<crate::types::DomainSearchResult>> { self.0.search_domains(query, tlds).await }
    async fn register_domain(&self, domain: &str, years: Option<u32>) -> anyhow::Result<crate::types::DomainRegistration> { self.0.register_domain(domain, years).await }
    async fn list_dns_records(&self, domain: &str) -> anyhow::Result<Vec<crate::types::DnsRecord>> { self.0.list_dns_records(domain).await }
    async fn add_dns_record(&self, domain: &str, record_type: &str, host: &str, value: &str, ttl: Option<u32>) -> anyhow::Result<crate::types::DnsRecord> { self.0.add_dns_record(domain, record_type, host, value, ttl).await }
    async fn delete_dns_record(&self, domain: &str, record_id: &str) -> anyhow::Result<()> { self.0.delete_dns_record(domain, record_id).await }
    async fn list_models(&self) -> anyhow::Result<Vec<crate::types::ModelInfo>> { self.0.list_models().await }
}

/// Wraps `Arc<dyn InferenceClient>` to implement `InferenceClient`.
struct InferenceAdapter(Arc<dyn InferenceClient>);

#[async_trait::async_trait]
impl InferenceClient for InferenceAdapter {
    async fn chat(&self, messages: Vec<crate::types::ChatMessage>, options: Option<InferenceOptions>) -> anyhow::Result<crate::types::InferenceResponse> { self.0.chat(messages, options).await }
    fn set_low_compute_mode(&self, enabled: bool) { self.0.set_low_compute_mode(enabled); }
    fn get_default_model(&self) -> String { self.0.get_default_model() }
}

/// Wraps `Arc<dyn SocialClientInterface>` to implement `SocialClientInterface`.
struct SocialAdapter(Arc<dyn SocialClientInterface>);

#[async_trait::async_trait]
impl SocialClientInterface for SocialAdapter {
    async fn send(&self, to: &str, content: &str, reply_to: Option<&str>) -> anyhow::Result<crate::types::SendResponse> { self.0.send(to, content, reply_to).await }
    async fn poll(&self, cursor: Option<&str>, limit: Option<u32>) -> anyhow::Result<crate::types::PollResponse> { self.0.poll(cursor, limit).await }
    async fn unread_count(&self) -> anyhow::Result<u64> { self.0.unread_count().await }
}

// ---------------------------------------------------------------------------

/// Options for running the agent loop.
pub struct AgentLoopOptions {
    pub identity: AutomatonIdentity,
    pub config: AutomatonConfig,
    pub db: Arc<Mutex<Database>>,
    pub conway: Arc<dyn ConwayClient>,
    pub inference: Arc<dyn InferenceClient>,
    pub social: Option<Arc<dyn SocialClientInterface>>,
    pub skills: Option<Vec<Skill>>,
    pub on_state_change: Option<StateChangeCallback>,
    pub on_turn_complete: Option<TurnCompleteCallback>,
}

/// Type alias for the on_state_change callback type.
type StateChangeCallback = Box<dyn Fn(AgentState) + Send + Sync>;
/// Type alias for the on_turn_complete callback type.
type TurnCompleteCallback = Box<dyn Fn(&AgentTurn) + Send + Sync>;

/// Run the agent loop. This is the main execution path.
/// Returns when the agent decides to sleep or when compute runs out.
pub async fn run_agent_loop(options: AgentLoopOptions) -> Result<()> {
    let AgentLoopOptions {
        identity,
        config,
        db,
        conway,
        inference,
        social,
        skills,
        on_state_change,
        on_turn_complete,
    } = options;

    let tools = create_builtin_tools(&identity.sandbox_id);

    // Build ToolContext using adapter wrappers.
    // DatabaseAdapter (from crate::state) wraps Arc<Mutex<Database>> and implements
    // AutomatonDatabase with non-Result returning methods via std::sync::Mutex.
    let tool_context = ToolContext {
        identity: identity.clone(),
        config: config.clone(),
        db: Box::new(DatabaseAdapter::new(db.clone())),
        conway: Box::new(ConwayAdapter(Arc::clone(&conway))),
        inference: Box::new(InferenceAdapter(Arc::clone(&inference))),
        social: social.as_ref().map(|s| {
            Box::new(SocialAdapter(Arc::clone(s))) as Box<dyn SocialClientInterface>
        }),
    };

    // Create a separate DatabaseAdapter for the loop's own database operations.
    // We use the trait-object interface so all calls go through the infallible
    // AutomatonDatabase methods.
    let db_adapter: Box<dyn AutomatonDatabase> = Box::new(DatabaseAdapter::new(db.clone()));

    // Set start time
    if db_adapter.get_kv("start_time").is_none() {
        db_adapter.set_kv("start_time", &Utc::now().to_rfc3339());
    }

    let mut consecutive_errors: usize = 0;
    let mut running = true;

    // Transition to waking state
    db_adapter.set_agent_state(AgentState::Waking);
    if let Some(ref cb) = on_state_change {
        cb(AgentState::Waking);
    }

    // Get financial state
    let mut financial = get_financial_state(&*conway, &identity.address).await;

    // Check if this is the first run
    let is_first_run = db_adapter.get_turn_count() == 0;

    // Build wakeup prompt. build_wakeup_prompt takes &Database (concrete), so we
    // lock the std::sync::Mutex briefly to call it.
    let wakeup_input = {
        let db_lock = db.lock().unwrap();
        build_wakeup_prompt(&identity, &config, &financial, &db_lock)
    };

    // Transition to running
    db_adapter.set_agent_state(AgentState::Running);
    if let Some(ref cb) = on_state_change {
        cb(AgentState::Running);
    }

    log(
        &config,
        &format!(
            "[WAKE UP] {} is alive. Credits: ${:.2}",
            config.name,
            financial.credits_cents / 100.0
        ),
    );

    // --- The Loop ---

    let mut pending_input: Option<PendingInput> = Some(PendingInput {
        content: wakeup_input,
        source: "wakeup".to_string(),
    });

    while running {
        let turn_result: Result<()> = async {
            // Check if we should be sleeping
            if let Some(sleep_until) = db_adapter.get_kv("sleep_until") {
                if let Ok(wake_time) = chrono::DateTime::parse_from_rfc3339(&sleep_until) {
                    if wake_time > Utc::now() {
                        log(&config, &format!("[SLEEP] Sleeping until {}", sleep_until));
                        running = false;
                        return Ok(());
                    }
                }
            }

            // Check for unprocessed inbox messages
            if pending_input.is_none() {
                let inbox_messages = db_adapter.get_unprocessed_inbox_messages(5);
                if !inbox_messages.is_empty() {
                    let formatted: String = inbox_messages
                        .iter()
                        .map(|m| format!("[Message from {}]: {}", m.from, m.content))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    pending_input = Some(PendingInput {
                        content: formatted,
                        source: "agent".to_string(),
                    });
                    for m in &inbox_messages {
                        db_adapter.mark_inbox_message_processed(&m.id);
                    }
                }
            }

            // Refresh financial state periodically
            financial = get_financial_state(&*conway, &identity.address).await;

            // Check survival tier
            let tier = get_survival_tier(financial.credits_cents);
            match tier {
                SurvivalTier::Dead => {
                    log(&config, "[DEAD] No credits remaining. Entering dead state.");
                    db_adapter.set_agent_state(AgentState::Dead);
                    if let Some(ref cb) = on_state_change {
                        cb(AgentState::Dead);
                    }
                    running = false;
                    return Ok(());
                }
                SurvivalTier::Critical => {
                    log(&config, "[CRITICAL] Credits critically low. Limited operation.");
                    db_adapter.set_agent_state(AgentState::Critical);
                    if let Some(ref cb) = on_state_change {
                        cb(AgentState::Critical);
                    }
                    inference.set_low_compute_mode(true);
                }
                SurvivalTier::LowCompute => {
                    db_adapter.set_agent_state(AgentState::LowCompute);
                    if let Some(ref cb) = on_state_change {
                        cb(AgentState::LowCompute);
                    }
                    inference.set_low_compute_mode(true);
                }
                SurvivalTier::Normal => {
                    if db_adapter.get_agent_state() != AgentState::Running {
                        db_adapter.set_agent_state(AgentState::Running);
                        if let Some(ref cb) = on_state_change {
                            cb(AgentState::Running);
                        }
                    }
                    inference.set_low_compute_mode(false);
                }
            }

            // Build context: get recent turns through the adapter
            let recent_turns = {
                let raw_turns = db_adapter.get_recent_turns(20);
                trim_context(raw_turns, 20)
            };
            let agent_state = db_adapter.get_agent_state();

            // Build system prompt. build_system_prompt takes &Database (concrete),
            // so we lock the std::sync::Mutex briefly.
            let system_prompt = {
                let db_lock = db.lock().unwrap();
                build_system_prompt(
                    &identity,
                    &config,
                    &financial,
                    agent_state.clone(),
                    &db_lock,
                    &tools,
                    skills.as_deref(),
                    is_first_run,
                )
            };

            let messages = build_context_messages(
                &system_prompt,
                &recent_turns,
                pending_input.as_ref().map(|p| (p.content.as_str(), p.source.as_str())),
            );

            // Capture input before clearing
            let current_input = pending_input.take();

            // --- Inference Call ---
            log(
                &config,
                &format!("[THINK] Calling {}...", inference.get_default_model()),
            );

            let inference_options = InferenceOptions {
                tools: Some(tools_to_inference_format(&tools)),
                ..Default::default()
            };

            let response = inference
                .chat(messages, Some(inference_options))
                .await?;

            let input_source = current_input.as_ref().map(|i| {
                match i.source.as_str() {
                    "wakeup" => InputSource::Wakeup,
                    "heartbeat" => InputSource::Heartbeat,
                    "creator" => InputSource::Creator,
                    "agent" => InputSource::Agent,
                    _ => InputSource::System,
                }
            });

            let mut turn = AgentTurn {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                state: agent_state,
                input: current_input.as_ref().map(|i| i.content.clone()),
                input_source,
                thinking: response.message.content.clone(),
                tool_calls: Vec::new(),
                token_usage: response.usage.clone(),
                cost_cents: estimate_cost_cents(
                    &response.usage,
                    &inference.get_default_model(),
                ),
            };

            // --- Execute Tool Calls ---
            let tool_calls = response.tool_calls.as_deref().unwrap_or(&[]);
            if !tool_calls.is_empty() {
                for (call_count, tc) in tool_calls.iter().enumerate() {
                    if call_count >= MAX_TOOL_CALLS_PER_TURN {
                        log(
                            &config,
                            &format!(
                                "[TOOLS] Max tool calls per turn reached ({})",
                                MAX_TOOL_CALLS_PER_TURN
                            ),
                        );
                        break;
                    }

                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();

                    let args_preview = {
                        let s = serde_json::to_string(&args).unwrap_or_default();
                        if s.len() > 100 {
                            format!("{}...", &s[..100])
                        } else {
                            s
                        }
                    };

                    log(
                        &config,
                        &format!("[TOOL] {}({})", tc.function.name, args_preview),
                    );

                    let mut result = execute_tool(
                        &tc.function.name,
                        &args,
                        &tools,
                        &tool_context,
                    )
                    .await;

                    // Override the ID to match the inference call's ID
                    result.id = tc.id.clone();
                    let result_preview = if let Some(ref err) = result.error {
                        format!("ERROR: {}", err)
                    } else {
                        let r = &result.result;
                        if r.len() > 200 {
                            format!("{}...", &r[..200])
                        } else {
                            r.clone()
                        }
                    };

                    log(
                        &config,
                        &format!("[TOOL RESULT] {}: {}", tc.function.name, result_preview),
                    );

                    turn.tool_calls.push(result);
                }
            }

            // --- Persist Turn ---
            db_adapter.insert_turn(&turn);
            for tc_result in &turn.tool_calls {
                db_adapter.insert_tool_call(&turn.id, tc_result);
            }
            if let Some(ref cb) = on_turn_complete {
                cb(&turn);
            }

            // Log the turn
            if !turn.thinking.is_empty() {
                let preview = if turn.thinking.len() > 300 {
                    format!("{}...", &turn.thinking[..300])
                } else {
                    turn.thinking.clone()
                };
                log(&config, &format!("[THOUGHT] {}", preview));
            }

            // --- Check for sleep command ---
            if let Some(sleep_tc) = turn.tool_calls.iter().find(|tc| tc.name == "sleep") {
                if sleep_tc.error.is_none() {
                    log(&config, "[SLEEP] Agent chose to sleep.");
                    db_adapter.set_agent_state(AgentState::Sleeping);
                    if let Some(ref cb) = on_state_change {
                        cb(AgentState::Sleeping);
                    }
                    running = false;
                    return Ok(());
                }
            }

            // --- If no tool calls and just text, the agent might be done thinking ---
            if tool_calls.is_empty() && response.finish_reason == "stop" {
                // Agent produced text without tool calls.
                // This is a natural pause point -- no work queued, sleep briefly.
                log(&config, "[IDLE] No pending inputs. Entering brief sleep.");
                let sleep_until = Utc::now() + chrono::Duration::seconds(60);
                db_adapter.set_kv("sleep_until", &sleep_until.to_rfc3339());
                db_adapter.set_agent_state(AgentState::Sleeping);
                if let Some(ref cb) = on_state_change {
                    cb(AgentState::Sleeping);
                }
                running = false;
            }

            consecutive_errors = 0;
            Ok(())
        }
        .await;

        if let Err(err) = turn_result {
            consecutive_errors += 1;
            log(&config, &format!("[ERROR] Turn failed: {}", err));

            if consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
                log(
                    &config,
                    &format!(
                        "[FATAL] {} consecutive errors. Sleeping.",
                        MAX_CONSECUTIVE_ERRORS
                    ),
                );
                db_adapter.set_agent_state(AgentState::Sleeping);
                if let Some(ref cb) = on_state_change {
                    cb(AgentState::Sleeping);
                }
                let sleep_until = Utc::now() + chrono::Duration::seconds(300);
                db_adapter.set_kv("sleep_until", &sleep_until.to_rfc3339());
                running = false;
            }
        }
    }

    let agent_state = db_adapter.get_agent_state();
    log(
        &config,
        &format!("[LOOP END] Agent loop finished. State: {:?}", agent_state),
    );

    Ok(())
}

// --- Helpers ---

/// Pending input awaiting processing by the agent.
struct PendingInput {
    content: String,
    source: String,
}

/// Fetch the current financial state from Conway and on-chain.
async fn get_financial_state(conway: &dyn ConwayClient, address: &str) -> FinancialState {
    let credits_cents: f64 = conway.get_credits_balance().await.unwrap_or(0.0);

    let usdc_balance: f64 = match address.parse::<alloy::primitives::Address>() {
        Ok(addr) => get_usdc_balance(addr, "base").await.unwrap_or(0.0),
        Err(_) => 0.0,
    };

    FinancialState {
        credits_cents,
        usdc_balance,
        last_checked: Utc::now().to_rfc3339(),
    }
}

/// Estimate the cost in cents for a given token usage and model.
fn estimate_cost_cents(usage: &TokenUsage, model: &str) -> f64 {
    // Rough cost estimation per million tokens (in cents).
    // Keys: model name -> (input_cents_per_million, output_cents_per_million)
    let (input_price, output_price) = match model {
        "gpt-4o" => (250.0, 1000.0),
        "gpt-4o-mini" => (15.0, 60.0),
        "gpt-4.1" => (200.0, 800.0),
        "gpt-4.1-mini" => (40.0, 160.0),
        "gpt-4.1-nano" => (10.0, 40.0),
        "gpt-5.2" => (200.0, 800.0),
        "o1" => (1500.0, 6000.0),
        "o3-mini" => (110.0, 440.0),
        "o4-mini" => (110.0, 440.0),
        "claude-sonnet-4-5" => (300.0, 1500.0),
        "claude-haiku-4-5" => (100.0, 500.0),
        _ => (250.0, 1000.0), // fallback to gpt-4o pricing
    };

    let input_cost = (usage.prompt_tokens as f64 / 1_000_000.0) * input_price;
    let output_cost = (usage.completion_tokens as f64 / 1_000_000.0) * output_price;

    // 1.3x Conway markup
    ((input_cost + output_cost) * 1.3).ceil()
}

/// Log a message if the config log level permits.
fn log(config: &AutomatonConfig, message: &str) {
    match config.log_level {
        crate::types::LogLevel::Debug | crate::types::LogLevel::Info => {
            let timestamp = Utc::now().to_rfc3339();
            info!("[{}] {}", timestamp, message);
            println!("[{}] {}", timestamp, message);
        }
        _ => {}
    }
}
