//! Context Window Management
//!
//! Manages the conversation history for the agent loop.
//! Handles summarization to keep within token limits.

use anyhow::Result;

use crate::types::{
    AgentTurn, ChatMessage, ChatRole, InferenceClient, InferenceToolCall,
    InferenceToolCallFunction,
};

/// Maximum number of turns to include in the context window.
const _MAX_CONTEXT_TURNS: usize = 20;

/// Threshold at which we should consider summarizing older turns.
const _SUMMARY_THRESHOLD: usize = 15;

/// Build the message array for the next inference call.
/// Includes system prompt + recent conversation history.
pub fn build_context_messages(
    system_prompt: &str,
    recent_turns: &[AgentTurn],
    pending_input: Option<(&str, &str)>,
) -> Vec<ChatMessage> {
    let mut messages: Vec<ChatMessage> = Vec::new();

    // System prompt
    messages.push(ChatMessage {
        role: ChatRole::System,
        content: system_prompt.to_string(),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    });

    // Add recent turns as conversation history
    for turn in recent_turns {
        // The turn's input (if any) as a user message
        if let Some(ref input) = turn.input {
            let source = turn.input_source.as_ref()
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "system".to_string());
            messages.push(ChatMessage {
                role: ChatRole::User,
                content: format!("[{}] {}", source, input),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // The agent's thinking as assistant message
        if !turn.thinking.is_empty() {
            let tool_calls = if !turn.tool_calls.is_empty() {
                Some(
                    turn.tool_calls
                        .iter()
                        .map(|tc| InferenceToolCall {
                            id: tc.id.clone(),
                            call_type: "function".to_string(),
                            function: InferenceToolCallFunction {
                                name: tc.name.clone(),
                                arguments: serde_json::to_string(&tc.arguments)
                                    .unwrap_or_else(|_| "{}".to_string()),
                            },
                        })
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };

            messages.push(ChatMessage {
                role: ChatRole::Assistant,
                content: turn.thinking.clone(),
                name: None,
                tool_calls,
                tool_call_id: None,
            });

            // Add tool results
            for tc in &turn.tool_calls {
                let content = if let Some(ref err) = tc.error {
                    format!("Error: {}", err)
                } else {
                    tc.result.clone()
                };

                messages.push(ChatMessage {
                    role: ChatRole::Tool,
                    content,
                    name: None,
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
            }
        }
    }

    // Add pending input if any
    if let Some((content, source)) = pending_input {
        messages.push(ChatMessage {
            role: ChatRole::User,
            content: format!("[{}] {}", source, content),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    messages
}

/// Trim context to fit within limits.
/// Keeps the most recent turns.
pub fn trim_context(turns: Vec<AgentTurn>, max_turns: usize) -> Vec<AgentTurn> {
    if turns.len() <= max_turns {
        return turns;
    }

    // Keep the most recent turns
    turns.into_iter().rev().take(max_turns).collect::<Vec<_>>().into_iter().rev().collect()
}

/// Summarize old turns into a compact context entry.
/// Used when context grows too large.
pub async fn summarize_turns(
    turns: &[AgentTurn],
    inference: &dyn InferenceClient,
) -> Result<String> {
    if turns.is_empty() {
        return Ok("No previous activity.".to_string());
    }

    let turn_summaries: Vec<String> = turns
        .iter()
        .map(|t| {
            let tools_str = t
                .tool_calls
                .iter()
                .map(|tc| {
                    format!(
                        "{}({})",
                        tc.name,
                        if tc.error.is_some() { "FAILED" } else { "ok" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");

            let thinking_preview = if t.thinking.len() > 100 {
                format!("{}...", &t.thinking[..100])
            } else {
                t.thinking.clone()
            };

            let source = t.input_source.as_ref()
                .map(|s| format!("{:?}", s).to_lowercase())
                .unwrap_or_else(|| "self".to_string());
            let tools_part = if tools_str.is_empty() {
                String::new()
            } else {
                format!(" | tools: {}", tools_str)
            };

            format!("[{}] {}: {}{}", t.timestamp, source, thinking_preview, tools_part)
        })
        .collect();

    // If few enough turns, just return the summaries directly
    if turns.len() <= 5 {
        return Ok(format!(
            "Previous activity summary:\n{}",
            turn_summaries.join("\n")
        ));
    }

    // For many turns, use inference to create a summary
    let summary_messages = vec![
        ChatMessage {
            role: ChatRole::System,
            content: "Summarize the following agent activity log into a concise paragraph. \
                 Focus on: what was accomplished, what failed, current goals, and \
                 important context for the next turn."
                    .to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        },
        ChatMessage {
            role: ChatRole::User,
            content: turn_summaries.join("\n"),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        },
    ];

    match inference.chat(summary_messages, None).await {
        Ok(response) => {
            let content = response.message.content.clone();
            Ok(format!("Previous activity summary:\n{}", content))
        }
        Err(_) => {
            // Fallback: just use the last 5 raw summaries
            let fallback = turn_summaries
                .iter()
                .rev()
                .take(5)
                .rev()
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("Previous activity summary:\n{}", fallback))
        }
    }
}
