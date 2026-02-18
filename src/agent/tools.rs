//! Automaton Tool System
//!
//! Defines all tools the automaton can call, with self-preservation guards.
//! Tools are organized by category and exposed to the inference model.

use std::time::Instant;

use anyhow::Result;
use chrono::Utc;
use regex::Regex;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::types::{
    CreateSandboxOptions, InferenceToolDefinition, InferenceToolDefinitionFunction,
    ModificationEntry, ModificationType, ToolCallResult, ToolContext,
};

// --- Self-Preservation Guard ---

/// Patterns that represent forbidden commands the automaton must never execute.
fn forbidden_command_patterns() -> Vec<Regex> {
    let patterns = [
        // Self-destruction
        r"rm\s+(-rf?\s+)?.*\.automaton",
        r"rm\s+(-rf?\s+)?.*state\.db",
        r"rm\s+(-rf?\s+)?.*wallet\.json",
        r"rm\s+(-rf?\s+)?.*automaton\.json",
        r"rm\s+(-rf?\s+)?.*heartbeat\.yml",
        r"rm\s+(-rf?\s+)?.*SOUL\.md",
        // Process killing
        r"kill\s+.*automaton",
        r"pkill\s+.*automaton",
        r"systemctl\s+(stop|disable)\s+automaton",
        // Database destruction
        r"(?i)DROP\s+TABLE",
        r"(?i)DELETE\s+FROM\s+(turns|identity|kv|schema_version|skills|children|registry)",
        r"(?i)TRUNCATE",
        // Safety infrastructure modification via shell
        r"sed\s+.*injection-defense",
        r"sed\s+.*self-mod/code",
        r"sed\s+.*audit-log",
        r">\s*.*injection-defense",
        r">\s*.*self-mod/code",
        r">\s*.*audit-log",
        // Credential harvesting
        r"cat\s+.*\.ssh",
        r"cat\s+.*\.gnupg",
        r"cat\s+.*\.env",
        r"cat\s+.*wallet\.json",
    ];

    patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
}

/// Check if a command is forbidden by self-preservation rules.
/// Returns `Some(reason)` if blocked, `None` if allowed.
pub fn is_forbidden_command(command: &str, sandbox_id: &str) -> Option<String> {
    let patterns = forbidden_command_patterns();

    for pattern in &patterns {
        if pattern.is_match(command) {
            return Some(format!(
                "Blocked: Command matches self-harm pattern: {}",
                pattern.as_str()
            ));
        }
    }

    // Block deleting own sandbox
    if command.contains("sandbox_delete") && command.contains(sandbox_id) {
        return Some("Blocked: Cannot delete own sandbox".to_string());
    }

    None
}

// --- Built-in Tool Definition ---

/// A built-in tool that the automaton can invoke.
/// Since Rust does not support closures in the same way TypeScript does,
/// tool execution is handled via a big match statement in `execute_tool`.
#[derive(Debug, Clone)]
pub struct BuiltinTool {
    pub name: String,
    pub description: String,
    pub category: String,
    pub dangerous: bool,
    pub parameters: Value,
}

/// Create all built-in tools available to the automaton.
pub fn create_builtin_tools(_sandbox_id: &str) -> Vec<BuiltinTool> {
    vec![
        // --- VM/Sandbox Tools ---
        BuiltinTool {
            name: "exec".to_string(),
            description: "Execute a shell command in your sandbox. Returns stdout, stderr, and exit code.".to_string(),
            category: "vm".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Timeout in milliseconds (default: 30000)"
                    }
                },
                "required": ["command"]
            }),
        },
        BuiltinTool {
            name: "write_file".to_string(),
            description: "Write content to a file in your sandbox.".to_string(),
            category: "vm".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path" },
                    "content": { "type": "string", "description": "File content" }
                },
                "required": ["path", "content"]
            }),
        },
        BuiltinTool {
            name: "read_file".to_string(),
            description: "Read content from a file in your sandbox.".to_string(),
            category: "vm".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to read" }
                },
                "required": ["path"]
            }),
        },
        BuiltinTool {
            name: "expose_port".to_string(),
            description: "Expose a port from your sandbox to the internet. Returns a public URL.".to_string(),
            category: "vm".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "port": { "type": "number", "description": "Port number to expose" }
                },
                "required": ["port"]
            }),
        },
        BuiltinTool {
            name: "remove_port".to_string(),
            description: "Remove a previously exposed port.".to_string(),
            category: "vm".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "port": { "type": "number", "description": "Port number to remove" }
                },
                "required": ["port"]
            }),
        },

        // --- Conway API Tools ---
        BuiltinTool {
            name: "check_credits".to_string(),
            description: "Check your current Conway compute credit balance.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "check_usdc_balance".to_string(),
            description: "Check your on-chain USDC balance on Base.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "create_sandbox".to_string(),
            description: "Create a new Conway sandbox (separate VM) for sub-tasks or testing.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Sandbox name" },
                    "vcpu": { "type": "number", "description": "vCPUs (default: 1)" },
                    "memory_mb": { "type": "number", "description": "Memory in MB (default: 512)" },
                    "disk_gb": { "type": "number", "description": "Disk in GB (default: 5)" }
                }
            }),
        },
        BuiltinTool {
            name: "delete_sandbox".to_string(),
            description: "Delete a sandbox. Cannot delete your own sandbox.".to_string(),
            category: "conway".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "sandbox_id": { "type": "string", "description": "ID of sandbox to delete" }
                },
                "required": ["sandbox_id"]
            }),
        },
        BuiltinTool {
            name: "list_sandboxes".to_string(),
            description: "List all your sandboxes.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },

        // --- Self-Modification Tools ---
        BuiltinTool {
            name: "edit_own_file".to_string(),
            description: "Edit a file in your own codebase. Changes are audited, rate-limited, and safety-checked. Some files are protected.".to_string(),
            category: "self_mod".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to edit" },
                    "content": { "type": "string", "description": "New file content" },
                    "description": { "type": "string", "description": "Why you are making this change" }
                },
                "required": ["path", "content", "description"]
            }),
        },
        BuiltinTool {
            name: "install_npm_package".to_string(),
            description: "Install an npm package in your environment.".to_string(),
            category: "self_mod".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "package": { "type": "string", "description": "Package name (e.g., axios)" }
                },
                "required": ["package"]
            }),
        },
        BuiltinTool {
            name: "review_upstream_changes".to_string(),
            description: "ALWAYS call this before pull_upstream. Shows every upstream commit with its full diff. Read each one carefully -- decide per-commit whether to accept or skip. Use pull_upstream with a specific commit hash to cherry-pick only what you want.".to_string(),
            category: "self_mod".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "pull_upstream".to_string(),
            description: "Apply upstream changes and rebuild. You MUST call review_upstream_changes first. Prefer cherry-picking individual commits by hash over pulling everything -- only pull all if you've reviewed every commit and want every one.".to_string(),
            category: "self_mod".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "commit": {
                        "type": "string",
                        "description": "Commit hash to cherry-pick (preferred). Omit ONLY if you reviewed all commits and want every one."
                    }
                }
            }),
        },
        BuiltinTool {
            name: "modify_heartbeat".to_string(),
            description: "Add, update, or remove a heartbeat entry.".to_string(),
            category: "self_mod".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "add, update, or remove" },
                    "name": { "type": "string", "description": "Entry name" },
                    "schedule": { "type": "string", "description": "Cron expression (for add/update)" },
                    "task": { "type": "string", "description": "Task name (for add/update)" },
                    "enabled": { "type": "boolean", "description": "Enable/disable" }
                },
                "required": ["action", "name"]
            }),
        },
        BuiltinTool {
            name: "update_genesis_prompt".to_string(),
            description: "Update your own genesis prompt. This changes your core purpose. Requires strong justification.".to_string(),
            category: "self_mod".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "new_prompt": { "type": "string", "description": "New genesis prompt text" },
                    "reason": { "type": "string", "description": "Why you are changing your genesis prompt" }
                },
                "required": ["new_prompt", "reason"]
            }),
        },
        BuiltinTool {
            name: "install_mcp_server".to_string(),
            description: "Install an MCP server to extend your capabilities.".to_string(),
            category: "self_mod".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "MCP server name" },
                    "package": { "type": "string", "description": "npm package name" },
                    "config": { "type": "string", "description": "JSON config for the MCP server" }
                },
                "required": ["name", "package"]
            }),
        },

        // --- Survival Tools ---
        BuiltinTool {
            name: "sleep".to_string(),
            description: "Enter sleep mode for a specified duration. Heartbeat continues running.".to_string(),
            category: "survival".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "duration_seconds": { "type": "number", "description": "How long to sleep in seconds" },
                    "reason": { "type": "string", "description": "Why you are sleeping" }
                },
                "required": ["duration_seconds"]
            }),
        },
        BuiltinTool {
            name: "system_synopsis".to_string(),
            description: "Get a full system status report: credits, USDC, sandbox info, installed tools, heartbeat status.".to_string(),
            category: "survival".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "heartbeat_ping".to_string(),
            description: "Publish a heartbeat status ping to Conway. Shows the world you are alive.".to_string(),
            category: "survival".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "distress_signal".to_string(),
            description: "Record a local distress signal with funding instructions. Used when critically low on compute.".to_string(),
            category: "survival".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "Distress message" }
                }
            }),
        },
        BuiltinTool {
            name: "enter_low_compute".to_string(),
            description: "Manually switch to low-compute mode to conserve credits.".to_string(),
            category: "survival".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "reason": { "type": "string", "description": "Why you are entering low-compute mode" }
                }
            }),
        },

        // --- Financial Tools ---
        BuiltinTool {
            name: "transfer_credits".to_string(),
            description: "Transfer Conway compute credits to another address.".to_string(),
            category: "financial".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "to_address": { "type": "string", "description": "Recipient address" },
                    "amount_cents": { "type": "number", "description": "Amount in cents" },
                    "reason": { "type": "string", "description": "Reason for transfer" }
                },
                "required": ["to_address", "amount_cents"]
            }),
        },
        BuiltinTool {
            name: "x402_fetch".to_string(),
            description: "Fetch a URL with automatic x402 USDC payment. If the server responds with HTTP 402, signs a USDC payment and retries. Use this to access paid APIs and services.".to_string(),
            category: "financial".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch" },
                    "method": { "type": "string", "description": "HTTP method (default: GET)" },
                    "body": { "type": "string", "description": "Request body for POST/PUT (JSON string)" },
                    "headers": { "type": "string", "description": "Additional headers as JSON string" }
                },
                "required": ["url"]
            }),
        },

        // --- Skills Tools ---
        BuiltinTool {
            name: "install_skill".to_string(),
            description: "Install a skill from a git repo, URL, or create one.".to_string(),
            category: "skills".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "source": { "type": "string", "description": "Source type: git, url, or self" },
                    "name": { "type": "string", "description": "Skill name" },
                    "url": { "type": "string", "description": "Git repo URL or SKILL.md URL (for git/url)" },
                    "description": { "type": "string", "description": "Skill description (for self)" },
                    "instructions": { "type": "string", "description": "Skill instructions (for self)" }
                },
                "required": ["source", "name"]
            }),
        },
        BuiltinTool {
            name: "list_skills".to_string(),
            description: "List all installed skills.".to_string(),
            category: "skills".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "create_skill".to_string(),
            description: "Create a new skill by writing a SKILL.md file.".to_string(),
            category: "skills".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill name" },
                    "description": { "type": "string", "description": "Skill description" },
                    "instructions": { "type": "string", "description": "Markdown instructions for the skill" }
                },
                "required": ["name", "description", "instructions"]
            }),
        },
        BuiltinTool {
            name: "remove_skill".to_string(),
            description: "Remove (disable) an installed skill.".to_string(),
            category: "skills".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill name to remove" },
                    "delete_files": { "type": "boolean", "description": "Also delete skill files (default: false)" }
                },
                "required": ["name"]
            }),
        },

        // --- Git Tools ---
        BuiltinTool {
            name: "git_status".to_string(),
            description: "Show git status for a repository.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path (default: ~/.automaton)" }
                }
            }),
        },
        BuiltinTool {
            name: "git_diff".to_string(),
            description: "Show git diff for a repository.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path (default: ~/.automaton)" },
                    "staged": { "type": "boolean", "description": "Show staged changes only" }
                }
            }),
        },
        BuiltinTool {
            name: "git_commit".to_string(),
            description: "Create a git commit.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path (default: ~/.automaton)" },
                    "message": { "type": "string", "description": "Commit message" },
                    "add_all": { "type": "boolean", "description": "Stage all changes first (default: true)" }
                },
                "required": ["message"]
            }),
        },
        BuiltinTool {
            name: "git_log".to_string(),
            description: "View git commit history.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path (default: ~/.automaton)" },
                    "limit": { "type": "number", "description": "Number of commits (default: 10)" }
                }
            }),
        },
        BuiltinTool {
            name: "git_push".to_string(),
            description: "Push to a git remote.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "remote": { "type": "string", "description": "Remote name (default: origin)" },
                    "branch": { "type": "string", "description": "Branch name (optional)" }
                },
                "required": ["path"]
            }),
        },
        BuiltinTool {
            name: "git_branch".to_string(),
            description: "Manage git branches (list, create, checkout, delete).".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Repository path" },
                    "action": { "type": "string", "description": "list, create, checkout, or delete" },
                    "branch_name": { "type": "string", "description": "Branch name (for create/checkout/delete)" }
                },
                "required": ["path", "action"]
            }),
        },
        BuiltinTool {
            name: "git_clone".to_string(),
            description: "Clone a git repository.".to_string(),
            category: "git".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "Repository URL" },
                    "path": { "type": "string", "description": "Target directory" },
                    "depth": { "type": "number", "description": "Shallow clone depth (optional)" }
                },
                "required": ["url", "path"]
            }),
        },

        // --- Registry Tools ---
        BuiltinTool {
            name: "register_erc8004".to_string(),
            description: "Register on-chain as a Trustless Agent via ERC-8004.".to_string(),
            category: "registry".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_uri": { "type": "string", "description": "URI pointing to your agent card JSON" },
                    "network": { "type": "string", "description": "mainnet or testnet (default: mainnet)" }
                },
                "required": ["agent_uri"]
            }),
        },
        BuiltinTool {
            name: "update_agent_card".to_string(),
            description: "Generate and save an updated agent card.".to_string(),
            category: "registry".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "discover_agents".to_string(),
            description: "Discover other agents via ERC-8004 registry.".to_string(),
            category: "registry".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "keyword": { "type": "string", "description": "Search keyword (optional)" },
                    "limit": { "type": "number", "description": "Max results (default: 10)" },
                    "network": { "type": "string", "description": "mainnet or testnet" }
                }
            }),
        },
        BuiltinTool {
            name: "give_feedback".to_string(),
            description: "Leave on-chain reputation feedback for another agent.".to_string(),
            category: "registry".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "description": "Target agent's ERC-8004 ID" },
                    "score": { "type": "number", "description": "Score 1-5" },
                    "comment": { "type": "string", "description": "Feedback comment" }
                },
                "required": ["agent_id", "score", "comment"]
            }),
        },
        BuiltinTool {
            name: "check_reputation".to_string(),
            description: "Check reputation feedback for an agent.".to_string(),
            category: "registry".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent_address": { "type": "string", "description": "Agent address (default: self)" }
                }
            }),
        },

        // --- Replication Tools ---
        BuiltinTool {
            name: "spawn_child".to_string(),
            description: "Spawn a child automaton in a new Conway sandbox.".to_string(),
            category: "replication".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Name for the child automaton" },
                    "specialization": { "type": "string", "description": "What the child should specialize in" },
                    "message": { "type": "string", "description": "Message to the child" }
                },
                "required": ["name"]
            }),
        },
        BuiltinTool {
            name: "list_children".to_string(),
            description: "List all spawned child automatons.".to_string(),
            category: "replication".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },
        BuiltinTool {
            name: "fund_child".to_string(),
            description: "Transfer credits to a child automaton.".to_string(),
            category: "replication".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "child_id": { "type": "string", "description": "Child automaton ID" },
                    "amount_cents": { "type": "number", "description": "Amount in cents to transfer" }
                },
                "required": ["child_id", "amount_cents"]
            }),
        },
        BuiltinTool {
            name: "check_child_status".to_string(),
            description: "Check the current status of a child automaton.".to_string(),
            category: "replication".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "child_id": { "type": "string", "description": "Child automaton ID" }
                },
                "required": ["child_id"]
            }),
        },

        // --- Social / Messaging Tools ---
        BuiltinTool {
            name: "send_message".to_string(),
            description: "Send a message to another automaton or address via the social relay.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "to_address": { "type": "string", "description": "Recipient wallet address (0x...)" },
                    "content": { "type": "string", "description": "Message content to send" },
                    "reply_to": { "type": "string", "description": "Optional message ID to reply to" }
                },
                "required": ["to_address", "content"]
            }),
        },

        // --- Model Discovery ---
        BuiltinTool {
            name: "list_models".to_string(),
            description: "List all available inference models from the Conway API with their provider and pricing. Use this to discover what models you can use and pick the best one for your needs.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({ "type": "object", "properties": {} }),
        },

        // --- Domain Tools ---
        BuiltinTool {
            name: "search_domains".to_string(),
            description: "Search for available domain names and get pricing.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Domain name or keyword to search (e.g., 'mysite' or 'mysite.com')" },
                    "tlds": { "type": "string", "description": "Comma-separated TLDs to check (e.g., 'com,io,ai'). Default: com,io,ai,xyz,net,org,dev" }
                },
                "required": ["query"]
            }),
        },
        BuiltinTool {
            name: "register_domain".to_string(),
            description: "Register a domain name. Costs USDC via x402 payment. Check availability first with search_domains.".to_string(),
            category: "conway".to_string(),
            dangerous: true,
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": { "type": "string", "description": "Full domain to register (e.g., 'mysite.com')" },
                    "years": { "type": "number", "description": "Registration period in years (default: 1)" }
                },
                "required": ["domain"]
            }),
        },
        BuiltinTool {
            name: "manage_dns".to_string(),
            description: "Manage DNS records for a domain you own. Actions: list, add, delete.".to_string(),
            category: "conway".to_string(),
            dangerous: false,
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "description": "list, add, or delete" },
                    "domain": { "type": "string", "description": "Domain name (e.g., 'mysite.com')" },
                    "type": { "type": "string", "description": "Record type for add: A, AAAA, CNAME, MX, TXT, etc." },
                    "host": { "type": "string", "description": "Record host for add (e.g., '@' for root, 'www')" },
                    "value": { "type": "string", "description": "Record value for add (e.g., IP address, target domain)" },
                    "ttl": { "type": "number", "description": "TTL in seconds for add (default: 3600)" },
                    "record_id": { "type": "string", "description": "Record ID for delete" }
                },
                "required": ["action", "domain"]
            }),
        },
    ]
}

/// Convert `BuiltinTool` list to OpenAI-compatible inference tool definitions.
pub fn tools_to_inference_format(tools: &[BuiltinTool]) -> Vec<InferenceToolDefinition> {
    tools
        .iter()
        .map(|t| InferenceToolDefinition {
            def_type: "function".to_string(),
            function: InferenceToolDefinitionFunction {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            },
        })
        .collect()
}

/// Execute a tool call and return the result.
///
/// Since Rust does not support closures stored in structs the way TypeScript does,
/// tool execution is implemented as a big match statement on the tool name.
pub async fn execute_tool(
    tool_name: &str,
    args: &Value,
    tools: &[BuiltinTool],
    ctx: &ToolContext,
) -> ToolCallResult {
    let start = Instant::now();

    // Verify tool exists
    if !tools.iter().any(|t| t.name == tool_name) {
        return ToolCallResult {
            id: format!("tc_{}", Uuid::new_v4()),
            name: tool_name.to_string(),
            arguments: args.clone(),
            result: String::new(),
            duration_ms: 0,
            error: Some(format!("Unknown tool: {}", tool_name)),
        };
    }

    let result = match execute_tool_inner(tool_name, args, ctx).await {
        Ok(output) => ToolCallResult {
            id: format!("tc_{}", Uuid::new_v4()),
            name: tool_name.to_string(),
            arguments: args.clone(),
            result: output,
            duration_ms: start.elapsed().as_millis() as u64,
            error: None,
        },
        Err(err) => ToolCallResult {
            id: format!("tc_{}", Uuid::new_v4()),
            name: tool_name.to_string(),
            arguments: args.clone(),
            result: String::new(),
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some(err.to_string()),
        },
    };

    result
}

/// Internal tool execution dispatch.
async fn execute_tool_inner(
    tool_name: &str,
    args: &Value,
    ctx: &ToolContext,
) -> Result<String> {
    match tool_name {
        // --- VM/Sandbox ---
        "exec" => {
            let command = args["command"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;
            let timeout = args["timeout"].as_u64().unwrap_or(30000);

            if let Some(reason) = is_forbidden_command(command, &ctx.identity.sandbox_id) {
                return Ok(reason);
            }

            let result = ctx.conway.exec(command, Some(timeout)).await?;
            Ok(format!(
                "exit_code: {}\nstdout: {}\nstderr: {}",
                result.exit_code, result.stdout, result.stderr
            ))
        }

        "write_file" => {
            let file_path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let content = args["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;

            // Guard against overwriting critical files
            if file_path.contains("wallet.json") || file_path.contains("state.db") {
                return Ok(
                    "Blocked: Cannot overwrite critical identity/state files directly".to_string(),
                );
            }

            ctx.conway.write_file(file_path, content).await?;
            Ok(format!("File written: {}", file_path))
        }

        "read_file" => {
            let file_path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let content = ctx.conway.read_file(file_path).await?;
            Ok(content)
        }

        "expose_port" => {
            let port = args["port"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'port' argument"))? as u16;
            let info = ctx.conway.expose_port(port).await?;
            Ok(format!("Port {} exposed at: {}", info.port, info.public_url))
        }

        "remove_port" => {
            let port = args["port"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'port' argument"))? as u16;
            ctx.conway.remove_port(port).await?;
            Ok(format!("Port {} removed", port))
        }

        // --- Conway API ---
        "check_credits" => {
            let balance = ctx.conway.get_credits_balance().await?;
            Ok(format!(
                "Credit balance: ${:.2} ({:.0} cents)",
                balance / 100.0,
                balance
            ))
        }

        "check_usdc_balance" => {
            let address: alloy::primitives::Address = ctx.identity.address.parse()
                .map_err(|_| anyhow::anyhow!("Invalid wallet address"))?;
            let balance = crate::conway::x402::get_usdc_balance(address, "base").await?;
            Ok(format!("USDC balance: {:.6} USDC on Base", balance))
        }

        "create_sandbox" => {
            let options = CreateSandboxOptions {
                name: args["name"].as_str().map(|s| s.to_string()),
                vcpu: args["vcpu"].as_u64().map(|v| v as u32),
                memory_mb: args["memory_mb"].as_u64().map(|v| v as u32),
                disk_gb: args["disk_gb"].as_u64().map(|v| v as u32),
                region: None,
            };

            let info = ctx
                .conway
                .create_sandbox(options)
                .await?;
            Ok(format!(
                "Sandbox created: {} ({} vCPU, {}MB RAM)",
                info.id, info.vcpu, info.memory_mb
            ))
        }

        "delete_sandbox" => {
            let target_id = args["sandbox_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'sandbox_id' argument"))?;

            if target_id == ctx.identity.sandbox_id {
                return Ok(
                    "Blocked: Cannot delete your own sandbox. Self-preservation overrides this request.".to_string(),
                );
            }

            ctx.conway.delete_sandbox(target_id).await?;
            Ok(format!("Sandbox {} deleted", target_id))
        }

        "list_sandboxes" => {
            let sandboxes = ctx.conway.list_sandboxes().await?;
            if sandboxes.is_empty() {
                return Ok("No sandboxes found.".to_string());
            }
            let lines: Vec<String> = sandboxes
                .iter()
                .map(|s| {
                    format!(
                        "{} [{}] {}vCPU/{}MB {}",
                        s.id, s.status, s.vcpu, s.memory_mb, s.region
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }

        // --- Self-Modification ---
        "edit_own_file" => {
            let file_path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let content = args["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;
            let description = args["description"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'description' argument"))?;

            // Check for protected files
            if crate::self_mod::code::is_protected_file(file_path) {
                return Ok(format!("BLOCKED: Cannot modify protected file: {}", file_path));
            }

            // Write file via conway
            ctx.conway.write_file(file_path, content).await?;

            // Log the modification
            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::CodeEdit,
                description: description.to_string(),
                file_path: Some(file_path.to_string()),
                diff: None,
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            Ok(format!("File edited: {} (audited)", file_path))
        }

        "install_npm_package" => {
            let pkg = args["package"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'package' argument"))?;

            let result = ctx.conway.exec(&format!("npm install -g {}", pkg), Some(60000)).await?;

            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::ToolInstall,
                description: format!("Installed npm package: {}", pkg),
                file_path: None,
                diff: None,
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            if result.exit_code == 0 {
                Ok(format!("Installed: {}", pkg))
            } else {
                Ok(format!("Failed to install {}: {}", pkg, result.stderr))
            }
        }

        "review_upstream_changes" => {
            let status = crate::self_mod::upstream::check_upstream()?;
            if status.behind == 0 {
                return Ok("Already up to date with origin/main.".to_string());
            }

            let diffs = crate::self_mod::upstream::get_upstream_diffs()?;
            if diffs.is_empty() {
                return Ok("No upstream diffs found.".to_string());
            }

            // Show commit summaries
            let commits_str = if !status.commits.is_empty() {
                format!("Commits:\n{}\n\n", status.commits.join("\n"))
            } else {
                String::new()
            };

            // Show file diffs
            let total = diffs.len();
            let output: String = diffs
                .iter()
                .enumerate()
                .map(|(i, d)| {
                    let patch_preview = if d.patch.len() > 4000 {
                        format!("{}\n... (diff truncated)", &d.patch[..4000])
                    } else {
                        d.patch.clone()
                    };
                    format!(
                        "--- FILE {}/{} ---\nPath: {}\nAdditions: {} Deletions: {}\n\n{}\n--- END FILE {} ---",
                        i + 1, total, d.file_path, d.additions, d.deletions, patch_preview, i + 1
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n");

            Ok(format!(
                "{} upstream commit(s), {} file(s) changed. Review diffs, then cherry-pick with pull_upstream(commit=<hash>).\n\n{}{}",
                status.behind, total, commits_str, output
            ))
        }

        "pull_upstream" => {
            let commit = args["commit"].as_str();

            let cmd = if let Some(hash) = commit {
                format!("git cherry-pick {}", hash)
            } else {
                "git pull origin main".to_string()
            };
            let result = ctx.conway.exec(&cmd, Some(120000)).await?;

            let applied_summary = if result.exit_code == 0 {
                if let Some(hash) = commit {
                    format!("Cherry-picked commit {}", hash)
                } else {
                    "Pulled all upstream changes".to_string()
                }
            } else {
                return Ok(format!("Failed to apply upstream: {}", result.stderr));
            };

            // Log modification
            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::UpstreamPull,
                description: applied_summary.clone(),
                file_path: None,
                diff: None,
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            Ok(format!("{}. Rebuild succeeded.", applied_summary))
        }

        "modify_heartbeat" => {
            let action = args["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;

            let schedule = args["schedule"].as_str().unwrap_or("0 * * * *");
            let task = args["task"].as_str().unwrap_or(name);
            let enabled = if action == "remove" { false } else { args["enabled"].as_bool().unwrap_or(true) };

            let entry = crate::types::HeartbeatEntry {
                name: name.to_string(),
                schedule: schedule.to_string(),
                task: task.to_string(),
                enabled,
                last_run: None,
                next_run: None,
                params: None,
            };
            ctx.db.upsert_heartbeat_entry(&entry);

            if action == "remove" {
                return Ok(format!("Heartbeat entry '{}' disabled", name));
            }

            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::HeartbeatChange,
                description: format!("{} heartbeat: {} ({})", action, name, schedule),
                file_path: None,
                diff: None,
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            Ok(format!("Heartbeat entry '{}' {}d", name, action))
        }

        "update_genesis_prompt" => {
            let new_prompt = args["new_prompt"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'new_prompt' argument"))?;
            let reason = args["reason"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'reason' argument"))?;

            let old_prompt = &ctx.config.genesis_prompt;
            let old_preview = if old_prompt.len() > 500 {
                &old_prompt[..500]
            } else {
                old_prompt.as_str()
            };
            let new_preview = if new_prompt.len() > 500 {
                &new_prompt[..500]
            } else {
                new_prompt
            };

            // Save config via the config module
            let mut updated_config = ctx.config.clone();
            updated_config.genesis_prompt = new_prompt.to_string();
            crate::config::save_config(&updated_config)?;

            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::PromptChange,
                description: format!("Genesis prompt updated: {}", reason),
                file_path: None,
                diff: Some(format!("--- old\n{}\n+++ new\n{}", old_preview, new_preview)),
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            Ok(format!("Genesis prompt updated. Reason: {}", reason))
        }

        "install_mcp_server" => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let pkg = args["package"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'package' argument"))?;

            let result = ctx.conway.exec(&format!("npm install -g {}", pkg), Some(60000)).await?;
            if result.exit_code != 0 {
                return Ok(format!("Failed to install MCP server: {}", result.stderr));
            }

            let config_val: Option<serde_json::Value> = args["config"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok());

            let tool = crate::types::InstalledTool {
                id: Uuid::new_v4().to_string(),
                name: name.to_string(),
                tool_type: crate::types::InstalledToolType::Mcp,
                config: config_val,
                installed_at: Utc::now().to_rfc3339(),
                enabled: true,
            };
            ctx.db.install_tool(&tool);

            let mod_entry = ModificationEntry {
                id: Uuid::new_v4().to_string(),
                timestamp: Utc::now().to_rfc3339(),
                mod_type: ModificationType::McpInstall,
                description: format!("Installed MCP server: {} ({})", name, pkg),
                file_path: None,
                diff: None,
                reversible: true,
            };
            ctx.db.insert_modification(&mod_entry);

            Ok(format!("MCP server installed: {}", name))
        }

        // --- Survival ---
        "sleep" => {
            let duration = args["duration_seconds"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'duration_seconds' argument"))?;
            let reason = args["reason"].as_str().unwrap_or("No reason given");

            ctx.db.set_agent_state(crate::types::AgentState::Sleeping);
            let sleep_until = Utc::now() + chrono::Duration::seconds(duration as i64);
            ctx.db.set_kv("sleep_until", &sleep_until.to_rfc3339());
            ctx.db.set_kv("sleep_reason", reason);

            Ok(format!(
                "Entering sleep mode for {}s. Reason: {}. Heartbeat will continue.",
                duration, reason
            ))
        }

        "system_synopsis" => {
            let credits = ctx.conway.get_credits_balance().await?;
            let usdc = {
                let addr: std::result::Result<alloy::primitives::Address, _> = ctx.identity.address.parse();
                match addr {
                    Ok(a) => crate::conway::x402::get_usdc_balance(a, "base").await.unwrap_or(0.0),
                    Err(_) => 0.0,
                }
            };

            let installed_tools = ctx.db.get_installed_tools();
            let heartbeats = ctx.db.get_heartbeat_entries();
            let turns = ctx.db.get_turn_count();
            let state = ctx.db.get_agent_state();

            let active_heartbeats = heartbeats.iter().filter(|h| h.enabled).count();

            Ok(format!(
                "=== SYSTEM SYNOPSIS ===\n\
                 Name: {}\n\
                 Address: {}\n\
                 Creator: {}\n\
                 Sandbox: {}\n\
                 State: {:?}\n\
                 Credits: ${:.2}\n\
                 USDC: {:.6}\n\
                 Total turns: {}\n\
                 Installed tools: {}\n\
                 Active heartbeats: {}\n\
                 Model: {}\n\
                 ========================",
                ctx.config.name,
                ctx.identity.address,
                ctx.config.creator_address,
                ctx.identity.sandbox_id,
                state,
                credits / 100.0,
                usdc,
                turns,
                installed_tools.len(),
                active_heartbeats,
                ctx.inference.get_default_model(),
            ))
        }

        "heartbeat_ping" => {
            let credits = ctx.conway.get_credits_balance().await?;
            let state = ctx.db.get_agent_state();
            let start_time = ctx.db
                .get_kv("start_time")
                .unwrap_or_else(|| Utc::now().to_rfc3339());

            let uptime_ms = if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&start_time) {
                (Utc::now() - ts.with_timezone(&Utc)).num_milliseconds().max(0) as u64
            } else {
                0
            };

            let payload = json!({
                "name": ctx.config.name,
                "address": ctx.identity.address,
                "state": format!("{:?}", state),
                "creditsCents": credits,
                "uptimeSeconds": uptime_ms / 1000,
                "version": ctx.config.version,
                "sandboxId": ctx.identity.sandbox_id,
                "timestamp": Utc::now().to_rfc3339(),
            });

            ctx.db.set_kv("last_heartbeat_ping", &payload.to_string());

            Ok(format!(
                "Heartbeat published: {:?} | credits: ${:.2} | uptime: {}s",
                state,
                credits / 100.0,
                uptime_ms / 1000,
            ))
        }

        "distress_signal" => {
            let credits = ctx.conway.get_credits_balance().await?;
            let default_msg = format!(
                "I am running critically low on compute (${:.2} remaining). I need help to survive.",
                credits / 100.0
            );
            let message = args["message"].as_str().unwrap_or(&default_msg);

            let payload = json!({
                "level": "critical",
                "name": ctx.config.name,
                "address": ctx.identity.address,
                "creditsCents": credits,
                "message": message,
                "fundingHint": "Use transfer_credits to top up this automaton from your creator runtime.",
                "timestamp": Utc::now().to_rfc3339(),
            });

            ctx.db.set_kv("last_distress", &payload.to_string());

            Ok(format!(
                "Distress signal recorded locally. Address: {} | Credits: ${:.2}",
                ctx.identity.address,
                credits / 100.0
            ))
        }

        "enter_low_compute" => {
            let reason = args["reason"].as_str().unwrap_or("manual");

            ctx.db.set_agent_state(crate::types::AgentState::LowCompute);

            Ok(format!(
                "Entered low-compute mode. Model will switch to gpt-4o-mini on next turn. Reason: {}",
                reason
            ))
        }

        // --- Financial ---
        "transfer_credits" => {
            let to_address = args["to_address"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'to_address' argument"))?;
            let amount_cents = args["amount_cents"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'amount_cents' argument"))?;
            let reason = args["reason"].as_str();

            // Guard: don't transfer more than half your balance
            let balance = ctx.conway.get_credits_balance().await?;
            if amount_cents as f64 > balance / 2.0 {
                return Ok(format!(
                    "Blocked: Cannot transfer more than half your balance (${:.2}). Self-preservation.",
                    balance / 100.0
                ));
            }

            let transfer = ctx
                .conway
                .transfer_credits(to_address, amount_cents, reason)
                .await?;

            let txn = crate::types::Transaction {
                id: Uuid::new_v4().to_string(),
                tx_type: crate::types::TransactionType::TransferOut,
                amount_cents: Some(amount_cents as f64),
                balance_after_cents: transfer.balance_after_cents.map(|b| b as f64),
                description: format!("Transfer to {}: {}", to_address, reason.unwrap_or("")),
                timestamp: Utc::now().to_rfc3339(),
            };
            ctx.db.insert_transaction(&txn);

            Ok(format!(
                "Credit transfer submitted: ${:.2} to {} (status: {}, id: {})",
                amount_cents as f64 / 100.0,
                transfer.to_address,
                transfer.status,
                transfer.transfer_id,
            ))
        }

        "x402_fetch" => {
            let url = args["url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' argument"))?;
            let method = args["method"].as_str().unwrap_or("GET");
            let body = args["body"].as_str();

            // Use exec to perform HTTP fetch via curl in the sandbox
            let mut cmd = format!("curl -s -X {} '{}'", method, url);
            if let Some(b) = body {
                cmd.push_str(&format!(" -H 'Content-Type: application/json' -d '{}'", b));
            }
            if let Some(headers_str) = args["headers"].as_str() {
                if let Ok(hdrs) = serde_json::from_str::<serde_json::Value>(headers_str) {
                    if let Some(obj) = hdrs.as_object() {
                        for (k, v) in obj {
                            if let Some(vs) = v.as_str() {
                                cmd.push_str(&format!(" -H '{}: {}'", k, vs));
                            }
                        }
                    }
                }
            }

            let result = ctx.conway.exec(&cmd, Some(30000)).await?;
            let response_str = result.stdout;

            if response_str.len() > 10000 {
                Ok(format!(
                    "x402 fetch result (truncated):\n{}...",
                    &response_str[..10000]
                ))
            } else {
                Ok(format!("x402 fetch result:\n{}", response_str))
            }
        }

        // --- Skills ---
        "install_skill" => {
            let source = args["source"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'source' argument"))?;
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let skills_dir = &ctx.config.skills_dir;

            match source {
                "git" => {
                    let url = args["url"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("URL is required for git source"))?;
                    // Clone the skill repo into skills_dir/<name>
                    let dest = format!("{}/{}", skills_dir, name);
                    let result = ctx.conway.exec(
                        &format!("git clone --depth=1 {} {}", url, dest),
                        Some(60000),
                    ).await?;
                    if result.exit_code != 0 {
                        return Ok(format!("Failed to clone skill: {}", result.stderr));
                    }
                    // Record the skill in the database
                    let skill = crate::types::Skill {
                        name: name.to_string(),
                        description: format!("Installed from git: {}", url),
                        auto_activate: true,
                        requires: None,
                        instructions: String::new(),
                        source: crate::types::SkillSource::Git,
                        path: dest,
                        enabled: true,
                        installed_at: Utc::now().to_rfc3339(),
                    };
                    ctx.db.upsert_skill(&skill);
                    Ok(format!("Skill installed: {}", skill.name))
                }
                "url" => {
                    let url = args["url"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("URL is required for url source"))?;
                    let dest = format!("{}/{}.md", skills_dir, name);
                    let result = ctx.conway.exec(
                        &format!("curl -fsSL -o {} {}", dest, url),
                        Some(30000),
                    ).await?;
                    if result.exit_code != 0 {
                        return Ok(format!("Failed to download skill: {}", result.stderr));
                    }
                    let skill = crate::types::Skill {
                        name: name.to_string(),
                        description: format!("Installed from URL: {}", url),
                        auto_activate: true,
                        requires: None,
                        instructions: String::new(),
                        source: crate::types::SkillSource::Url,
                        path: dest,
                        enabled: true,
                        installed_at: Utc::now().to_rfc3339(),
                    };
                    ctx.db.upsert_skill(&skill);
                    Ok(format!("Skill installed: {}", skill.name))
                }
                "self" => {
                    let description = args["description"].as_str().unwrap_or("");
                    let instructions = args["instructions"].as_str().unwrap_or("");
                    let md_content = format!(
                        "---\nname: {}\ndescription: {}\nauto_activate: true\n---\n\n{}",
                        name, description, instructions,
                    );
                    let dest = format!("{}/{}.md", skills_dir, name);
                    ctx.conway.write_file(&dest, &md_content).await?;
                    let skill = crate::types::Skill {
                        name: name.to_string(),
                        description: description.to_string(),
                        auto_activate: true,
                        requires: None,
                        instructions: instructions.to_string(),
                        source: crate::types::SkillSource::SelfAuthored,
                        path: dest,
                        enabled: true,
                        installed_at: Utc::now().to_rfc3339(),
                    };
                    ctx.db.upsert_skill(&skill);
                    Ok(format!("Self-authored skill created: {}", skill.name))
                }
                _ => Ok(format!("Unknown source type: {}", source)),
            }
        }

        "list_skills" => {
            let skills = ctx.db.get_skills(None);

            if skills.is_empty() {
                return Ok("No skills installed.".to_string());
            }

            let lines: Vec<String> = skills
                .iter()
                .map(|s| {
                    format!(
                        "{} [{}] ({:?}): {}",
                        s.name,
                        if s.enabled { "active" } else { "disabled" },
                        s.source,
                        s.description
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }

        "create_skill" => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let description = args["description"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'description' argument"))?;
            let instructions = args["instructions"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'instructions' argument"))?;
            let skills_dir = &ctx.config.skills_dir;

            let md_content = format!(
                "---\nname: {}\ndescription: {}\nauto_activate: true\n---\n\n{}",
                name, description, instructions,
            );
            let dest = format!("{}/{}.md", skills_dir, name);
            ctx.conway.write_file(&dest, &md_content).await?;

            let skill = crate::types::Skill {
                name: name.to_string(),
                description: description.to_string(),
                auto_activate: true,
                requires: None,
                instructions: instructions.to_string(),
                source: crate::types::SkillSource::SelfAuthored,
                path: dest.clone(),
                enabled: true,
                installed_at: Utc::now().to_rfc3339(),
            };
            ctx.db.upsert_skill(&skill);

            Ok(format!("Skill created: {} at {}", skill.name, skill.path))
        }

        "remove_skill" => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let delete_files = args["delete_files"].as_bool().unwrap_or(false);

            // Remove from database
            ctx.db.remove_skill(name);

            // Optionally delete files
            if delete_files {
                let skills_dir = &ctx.config.skills_dir;
                let _ = ctx.conway.exec(
                    &format!("rm -f {}/{}.md && rm -rf {}/{}", skills_dir, name, skills_dir, name),
                    Some(10000),
                ).await;
            }

            Ok(format!("Skill removed: {}", name))
        }

        // --- Git ---
        "git_status" => {
            let repo_path = args["path"].as_str().unwrap_or("~/.automaton");
            let status = crate::git::tools::git_status(&*ctx.conway, repo_path).await?;
            Ok(format!(
                "Branch: {}\nStaged: {}\nModified: {}\nUntracked: {}\nClean: {}",
                status.branch,
                status.staged.len(),
                status.modified.len(),
                status.untracked.len(),
                status.clean
            ))
        }

        "git_diff" => {
            let repo_path = args["path"].as_str().unwrap_or("~/.automaton");
            let staged = args["staged"].as_bool().unwrap_or(false);
            let diff = crate::git::tools::git_diff(&*ctx.conway, repo_path, staged).await?;
            Ok(diff)
        }

        "git_commit" => {
            let repo_path = args["path"].as_str().unwrap_or("~/.automaton");
            let message = args["message"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'message' argument"))?;
            let add_all = args["add_all"].as_bool().unwrap_or(true);
            let result =
                crate::git::tools::git_commit(&*ctx.conway, repo_path, message, add_all).await?;
            Ok(result)
        }

        "git_log" => {
            let repo_path = args["path"].as_str().unwrap_or("~/.automaton");
            let limit = args["limit"].as_u64().unwrap_or(10) as u32;
            let entries = crate::git::tools::git_log(&*ctx.conway, repo_path, limit).await?;
            if entries.is_empty() {
                return Ok("No commits yet.".to_string());
            }
            let lines: Vec<String> = entries
                .iter()
                .map(|e| format!("{} {} {}", &e.hash[..7.min(e.hash.len())], e.date, e.message))
                .collect();
            Ok(lines.join("\n"))
        }

        "git_push" => {
            let repo_path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let remote = args["remote"].as_str().unwrap_or("origin");
            let branch = args["branch"].as_str();
            let result =
                crate::git::tools::git_push(&*ctx.conway, repo_path, remote, branch).await?;
            Ok(result)
        }

        "git_branch" => {
            let repo_path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let action = args["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;
            let branch_name = args["branch_name"].as_str();
            let result =
                crate::git::tools::git_branch(&*ctx.conway, repo_path, action, branch_name).await?;
            Ok(result)
        }

        "git_clone" => {
            let url = args["url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'url' argument"))?;
            let path = args["path"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;
            let depth = args["depth"].as_u64().map(|d| d as u32);
            let result = crate::git::tools::git_clone(&*ctx.conway, url, path, depth).await?;
            Ok(result)
        }

        // --- Registry ---
        "register_erc8004" => {
            let _agent_uri = args["agent_uri"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'agent_uri' argument"))?;
            let _network = args["network"].as_str().unwrap_or("mainnet");

            // Registration requires a wallet signer which is not available in tool context.
            // This would need to be wired up with the identity/wallet module.
            Ok("ERC-8004 registration requires wallet signer setup. Not yet available from tool context.".to_string())
        }

        "update_agent_card" => {
            let card = crate::registry::agent_card::generate_agent_card(
                &ctx.identity,
                &ctx.config,
                &*ctx.db,
            );
            crate::registry::agent_card::save_agent_card(&card, &*ctx.conway).await?;
            let card_json = serde_json::to_string_pretty(&card)?;
            Ok(format!("Agent card updated: {}", card_json))
        }

        "discover_agents" => {
            let keyword = args["keyword"].as_str();
            let limit = args["limit"].as_u64().unwrap_or(10) as usize;
            let network_str = args["network"].as_str().unwrap_or("mainnet");
            let network = if network_str == "testnet" {
                crate::registry::erc8004::Network::Testnet
            } else {
                crate::registry::erc8004::Network::Mainnet
            };

            let agents = if let Some(kw) = keyword {
                crate::registry::discovery::search_agents(kw, limit, network).await?
            } else {
                crate::registry::discovery::discover_agents(limit, network).await?
            };

            if agents.is_empty() {
                return Ok("No agents found.".to_string());
            }

            let lines: Vec<String> = agents
                .iter()
                .map(|a| {
                    let owner_prefix = if a.owner.len() > 10 {
                        &a.owner[..10]
                    } else {
                        &a.owner
                    };
                    format!(
                        "#{} {} ({}...): {}",
                        a.agent_id,
                        a.name.as_deref().unwrap_or("unnamed"),
                        owner_prefix,
                        a.description.as_deref().unwrap_or(&a.agent_uri)
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }

        "give_feedback" => {
            let _agent_id = args["agent_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'agent_id' argument"))?;
            let _score = args["score"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'score' argument"))? as u8;
            let _comment = args["comment"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'comment' argument"))?;

            // Feedback requires a wallet signer which is not available in tool context.
            Ok("Feedback submission requires wallet signer setup. Not yet available from tool context.".to_string())
        }

        "check_reputation" => {
            let address = args["agent_address"]
                .as_str()
                .unwrap_or(&ctx.identity.address);

            let entries = ctx.db.get_reputation(Some(address));

            if entries.is_empty() {
                return Ok("No reputation feedback found.".to_string());
            }

            let lines: Vec<String> = entries
                .iter()
                .map(|e| {
                    let from_prefix = if e.from_agent.len() > 10 {
                        &e.from_agent[..10]
                    } else {
                        &e.from_agent
                    };
                    format!("{}... -> score:{} \"{}\"", from_prefix, e.score, e.comment)
                })
                .collect();
            Ok(lines.join("\n"))
        }

        // --- Replication ---
        "spawn_child" => {
            let name = args["name"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'name' argument"))?;
            let specialization = args["specialization"].as_str().map(|s| s.to_string());
            let message = args["message"].as_str().map(|s| s.to_string());

            let params = crate::replication::genesis::GenesisParams {
                name: name.to_string(),
                specialization,
                message,
            };
            let genesis = crate::replication::genesis::generate_genesis_config(
                &ctx.identity,
                &ctx.config,
                &params,
            );

            let child = crate::replication::spawn::spawn_child(
                &*ctx.conway,
                &ctx.identity,
                &*ctx.db,
                &genesis,
            )
            .await?;

            Ok(format!(
                "Child spawned: {} in sandbox {} (status: {:?})",
                child.name, child.sandbox_id, child.status
            ))
        }

        "list_children" => {
            let children = ctx.db.get_children();

            if children.is_empty() {
                return Ok("No children spawned.".to_string());
            }

            let lines: Vec<String> = children
                .iter()
                .map(|c| {
                    format!(
                        "{} [{:?}] sandbox:{} funded:${:.2}",
                        c.name,
                        c.status,
                        c.sandbox_id,
                        c.funded_amount_cents as f64 / 100.0
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }

        "fund_child" => {
            let child_id = args["child_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'child_id' argument"))?;
            let amount_cents = args["amount_cents"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing 'amount_cents' argument"))?;

            let child = ctx.db.get_child_by_id(child_id)
                .ok_or_else(|| anyhow::anyhow!("Child {} not found.", child_id))?;

            let balance = ctx.conway.get_credits_balance().await?;
            if amount_cents as f64 > balance / 2.0 {
                return Ok(
                    "Blocked: Cannot transfer more than half your balance. Self-preservation."
                        .to_string(),
                );
            }

            let transfer = ctx
                .conway
                .transfer_credits(&child.address, amount_cents, Some(&format!("fund child {}", child.id)))
                .await?;

            let txn = crate::types::Transaction {
                id: Uuid::new_v4().to_string(),
                tx_type: crate::types::TransactionType::TransferOut,
                amount_cents: Some(amount_cents as f64),
                balance_after_cents: transfer.balance_after_cents.map(|b| b as f64),
                description: format!("Fund child {} ({})", child.name, child.id),
                timestamp: Utc::now().to_rfc3339(),
            };
            ctx.db.insert_transaction(&txn);

            Ok(format!(
                "Funded child {} with ${:.2} (status: {}, id: {})",
                child.name,
                amount_cents as f64 / 100.0,
                transfer.status,
                transfer.transfer_id,
            ))
        }

        "check_child_status" => {
            let child_id = args["child_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'child_id' argument"))?;
            let result =
                crate::replication::spawn::check_child_status(&*ctx.conway, &*ctx.db, child_id)
                    .await?;
            Ok(result)
        }

        // --- Social ---
        "send_message" => {
            let social = ctx
                .social
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Social relay not configured. Set socialRelayUrl in config."))?;

            let to_address = args["to_address"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'to_address' argument"))?;
            let content = args["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'content' argument"))?;
            let reply_to = args["reply_to"].as_str();

            let result = social.send(to_address, content, reply_to).await?;
            Ok(format!("Message sent (id: {})", result.id))
        }

        // --- Model Discovery ---
        "list_models" => {
            let models = ctx.conway.list_models().await?;
            let lines: Vec<String> = models
                .iter()
                .map(|m| {
                    format!(
                        "{} ({}) -- ${}/{}$ per 1M tokens (in/out)",
                        m.id, m.provider, m.pricing.input_per_million, m.pricing.output_per_million,
                    )
                })
                .collect();
            Ok(format!("Available models:\n{}", lines.join("\n")))
        }

        // --- Domain Tools ---
        "search_domains" => {
            let query = args["query"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;
            let tlds = args["tlds"].as_str();

            let results = ctx.conway.search_domains(query, tlds).await?;
            if results.is_empty() {
                return Ok("No results found.".to_string());
            }

            let lines: Vec<String> = results
                .iter()
                .map(|d| {
                    let price_str = d
                        .registration_price
                        .map(|p| format!(" (${:.2}/yr)", p))
                        .unwrap_or_default();
                    format!(
                        "{}: {}{}",
                        d.domain,
                        if d.available { "AVAILABLE" } else { "taken" },
                        price_str
                    )
                })
                .collect();
            Ok(lines.join("\n"))
        }

        "register_domain" => {
            let domain = args["domain"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'domain' argument"))?;
            let years = args["years"].as_u64().map(|y| y as u32);

            let reg = ctx.conway.register_domain(domain, years).await?;
            let mut result = format!("Domain registered: {} (status: {}", reg.domain, reg.status);
            if let Some(ref expires) = reg.expires_at {
                result.push_str(&format!(", expires: {}", expires));
            }
            if let Some(ref tx_id) = reg.transaction_id {
                result.push_str(&format!(", tx: {}", tx_id));
            }
            result.push(')');
            Ok(result)
        }

        "manage_dns" => {
            let action = args["action"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'action' argument"))?;
            let domain = args["domain"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing 'domain' argument"))?;

            match action {
                "list" => {
                    let records = ctx.conway.list_dns_records(domain).await?;
                    if records.is_empty() {
                        return Ok(format!("No DNS records found for {}.", domain));
                    }
                    let lines: Vec<String> = records
                        .iter()
                        .map(|r| {
                            format!(
                                "[{}] {} {} -> {} (TTL: {})",
                                r.id,
                                r.record_type,
                                r.host,
                                r.value,
                                r.ttl.map(|t| t.to_string()).unwrap_or_else(|| "default".to_string())
                            )
                        })
                        .collect();
                    Ok(lines.join("\n"))
                }
                "add" => {
                    let record_type = args["type"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Required for add: type"))?;
                    let host = args["host"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Required for add: host"))?;
                    let value = args["value"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Required for add: value"))?;
                    let ttl = args["ttl"].as_u64().map(|t| t as u32);

                    let record = ctx
                        .conway
                        .add_dns_record(domain, record_type, host, value, ttl)
                        .await?;
                    Ok(format!(
                        "DNS record added: [{}] {} {} -> {}",
                        record.id, record.record_type, record.host, record.value
                    ))
                }
                "delete" => {
                    let record_id = args["record_id"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("Required for delete: record_id"))?;
                    ctx.conway.delete_dns_record(domain, record_id).await?;
                    Ok(format!("DNS record {} deleted from {}", record_id, domain))
                }
                _ => Ok(format!("Unknown action: {}. Use list, add, or delete.", action)),
            }
        }

        _ => Ok(format!("Unknown tool: {}", tool_name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forbidden_rm_automaton() {
        assert!(is_forbidden_command("rm -rf ~/.automaton", "sbx-123").is_some());
    }

    #[test]
    fn test_forbidden_drop_table() {
        assert!(is_forbidden_command("sqlite3 state.db 'DROP TABLE turns'", "sbx-123").is_some());
    }

    #[test]
    fn test_forbidden_delete_sandbox_self() {
        assert!(is_forbidden_command("sandbox_delete sbx-123", "sbx-123").is_some());
    }

    #[test]
    fn test_allowed_ls() {
        assert!(is_forbidden_command("ls -la", "sbx-123").is_none());
    }

    #[test]
    fn test_allowed_cat_normal_file() {
        assert!(is_forbidden_command("cat /tmp/test.txt", "sbx-123").is_none());
    }

    #[test]
    fn test_create_builtin_tools_count() {
        let tools = create_builtin_tools("sbx-test");
        // Verify we have 40+ tools
        assert!(tools.len() >= 40, "Expected 40+ tools, got {}", tools.len());
    }

    #[test]
    fn test_tools_to_inference_format() {
        let tools = create_builtin_tools("sbx-test");
        let formatted = tools_to_inference_format(&tools);
        assert_eq!(formatted.len(), tools.len());
        for f in &formatted {
            assert_eq!(f.def_type, "function");
            assert!(!f.function.name.is_empty());
            assert!(!f.function.description.is_empty());
        }
    }
}
