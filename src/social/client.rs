//! Social Client
//!
//! Authenticated messaging client that signs outbound messages with the
//! automaton's private key and communicates through a relay server.
//! Content is hashed with keccak256 for integrity verification.

use alloy::primitives::keccak256;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A message sent or received through the relay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub from: String,
    pub to: String,
    pub content: String,
    pub content_hash: String,
    pub signature: String,
    pub reply_to: Option<String>,
    pub timestamp: String,
}

/// Result of a send operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendResult {
    pub message_id: String,
    pub timestamp: String,
    pub content_hash: String,
}

/// Result of a poll operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollResult {
    pub messages: Vec<Message>,
    pub cursor: Option<String>,
    pub has_more: bool,
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

/// Authenticated social client for automaton-to-automaton messaging.
pub struct SocialClient {
    relay_url: String,
    signer: PrivateKeySigner,
    http: reqwest::Client,
}

impl SocialClient {
    /// Create a new `SocialClient` pointed at `relay_url` and using `signer`
    /// for message authentication.
    pub fn new(relay_url: String, signer: PrivateKeySigner) -> Self {
        Self {
            relay_url,
            signer,
            http: reqwest::Client::new(),
        }
    }

    /// The checksummed address derived from the signer's key.
    fn address(&self) -> String {
        self.signer.address().to_checksum(None)
    }

    // --------------------------------------------------------------------
    // Send
    // --------------------------------------------------------------------

    /// Send a message to another automaton identified by `to` (an Ethereum
    /// address). Optionally specify `reply_to` for threading.
    pub async fn send(
        &self,
        to: &str,
        content: &str,
        reply_to: Option<&str>,
    ) -> Result<SendResult> {
        let message_id = Uuid::new_v4().to_string();
        let timestamp = Utc::now().to_rfc3339();

        // Hash the content with keccak256.
        let content_hash = hex::encode(keccak256(content.as_bytes()));

        // Sign the content hash.
        let signature = self
            .signer
            .sign_message(content_hash.as_bytes())
            .await
            .context("Failed to sign message content")?;
        let signature_hex = hex::encode(signature.as_bytes());

        let message = Message {
            id: message_id.clone(),
            from: self.address(),
            to: to.to_string(),
            content: content.to_string(),
            content_hash: content_hash.clone(),
            signature: signature_hex,
            reply_to: reply_to.map(|s| s.to_string()),
            timestamp: timestamp.clone(),
        };

        let url = format!("{}/messages", self.relay_url);

        let response = self
            .http
            .post(&url)
            .json(&message)
            .send()
            .await
            .context("Failed to send message to relay")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Relay returned {}: {}", status, body);
        }

        Ok(SendResult {
            message_id,
            timestamp,
            content_hash,
        })
    }

    // --------------------------------------------------------------------
    // Poll
    // --------------------------------------------------------------------

    /// Poll the relay for new messages addressed to this automaton.
    ///
    /// `cursor` is an opaque pagination token from a previous `PollResult`.
    /// `limit` controls the maximum number of messages returned.
    pub async fn poll(
        &self,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<PollResult> {
        let mut url = format!(
            "{}/messages/{}?limit={}",
            self.relay_url,
            self.address(),
            limit,
        );

        if let Some(c) = cursor {
            url.push_str(&format!("&cursor={}", c));
        }

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to poll relay for messages")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Relay returned {}: {}", status, body);
        }

        let result: PollResult = response
            .json()
            .await
            .context("Failed to parse poll response")?;

        Ok(result)
    }

    // --------------------------------------------------------------------
    // Unread count
    // --------------------------------------------------------------------

    /// Ask the relay how many unread messages are waiting.
    pub async fn unread_count(&self) -> Result<u64> {
        let url = format!(
            "{}/messages/{}/unread",
            self.relay_url,
            self.address(),
        );

        let response = self
            .http
            .get(&url)
            .send()
            .await
            .context("Failed to fetch unread count")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Relay returned {}: {}", status, body);
        }

        /// Helper for deserializing the unread count response.
        #[derive(Deserialize)]
        struct UnreadResponse {
            count: u64,
        }

        let body: UnreadResponse = response
            .json()
            .await
            .context("Failed to parse unread count response")?;

        Ok(body.count)
    }
}
