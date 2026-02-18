//! Automaton SIWE Provisioning
//!
//! Uses the automaton's wallet to authenticate via Sign-In With Ethereum (SIWE)
//! and create an API key for Conway API access.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use alloy::primitives::Address;
use alloy::signers::Signer;
use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::wallet::{get_automaton_dir, get_wallet};
use crate::types::ProvisionResult;

/// Default Conway API base URL.
const DEFAULT_API_URL: &str = "https://api.conway.tech";

/// SIWE domain used in the sign-in message.
const SIWE_DOMAIN: &str = "conway.tech";

/// Chain ID for Base network.
const CHAIN_ID: u64 = 8453;

/// Minimal config.json structure stored in `~/.automaton/config.json`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProvisionConfig {
    api_key: String,
    wallet_address: String,
    provisioned_at: String,
}

// ─── API response types ──────────────────────────────────────────

#[derive(Deserialize)]
struct NonceResponse {
    nonce: String,
}

#[derive(Deserialize)]
struct VerifyResponse {
    access_token: String,
}

#[derive(Deserialize)]
struct ApiKeyResponse {
    key: String,
    key_prefix: String,
}

// ─── Public API ──────────────────────────────────────────────────

/// Load a previously-saved API key from `~/.automaton/config.json`.
///
/// Returns `None` if the file does not exist or the key field is absent.
pub fn load_api_key_from_config() -> Option<String> {
    let config_path = get_automaton_dir().join("config.json");
    if !config_path.exists() {
        return None;
    }

    let contents = fs::read_to_string(&config_path).ok()?;
    let config: ProvisionConfig = serde_json::from_str(&contents).ok()?;

    if config.api_key.is_empty() {
        None
    } else {
        Some(config.api_key)
    }
}

/// Save the API key and wallet address to `~/.automaton/config.json`.
fn save_config(api_key: &str, wallet_address: &str) -> Result<()> {
    let dir = get_automaton_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create automaton directory")?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }

    let config_path = dir.join("config.json");
    let config = ProvisionConfig {
        api_key: api_key.to_string(),
        wallet_address: wallet_address.to_string(),
        provisioned_at: Utc::now().to_rfc3339(),
    };

    let json = serde_json::to_string_pretty(&config)?;
    fs::write(&config_path, &json).context("Failed to write config.json")?;
    fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))?;

    Ok(())
}

/// Construct an EIP-4361 (SIWE) message string manually.
///
/// Format:
/// ```text
/// {domain} wants you to sign in with your Ethereum account:
/// {address}
///
/// {statement}
///
/// URI: {uri}
/// Version: {version}
/// Chain ID: {chain_id}
/// Nonce: {nonce}
/// Issued At: {issued_at}
/// ```
fn build_siwe_message(
    domain: &str,
    address: &Address,
    statement: &str,
    uri: &str,
    chain_id: u64,
    nonce: &str,
    issued_at: &str,
) -> String {
    format!(
        "{domain} wants you to sign in with your Ethereum account:\n\
         {address}\n\
         \n\
         {statement}\n\
         \n\
         URI: {uri}\n\
         Version: 1\n\
         Chain ID: {chain_id}\n\
         Nonce: {nonce}\n\
         Issued At: {issued_at}",
        domain = domain,
        address = address.to_checksum(None),
        statement = statement,
        uri = uri,
        chain_id = chain_id,
        nonce = nonce,
        issued_at = issued_at,
    )
}

/// Run the full SIWE provisioning flow:
///
/// 1. Load wallet (or create one if it doesn't exist)
/// 2. POST `/v1/auth/nonce` to get a challenge nonce
/// 3. Construct and sign a SIWE message
/// 4. POST `/v1/auth/verify` with message + signature to get a JWT
/// 5. POST `/v1/auth/api-keys` with Bearer JWT to create an API key
/// 6. Save API key and wallet address to `~/.automaton/config.json`
pub async fn provision(api_url: Option<&str>) -> Result<ProvisionResult> {
    let url = api_url
        .map(|s| s.to_string())
        .or_else(|| std::env::var("CONWAY_API_URL").ok())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string());

    let client = reqwest::Client::new();

    // 1. Load wallet
    let (signer, _is_new) = get_wallet().context("Failed to load wallet")?;
    let address = signer.address();

    // 2. Get nonce
    let nonce_resp = client
        .post(format!("{}/v1/auth/nonce", url))
        .send()
        .await
        .context("Failed to request nonce")?;

    if !nonce_resp.status().is_success() {
        let status = nonce_resp.status();
        let body = nonce_resp.text().await.unwrap_or_default();
        bail!("Failed to get nonce: {} {}", status, body);
    }

    let nonce_data: NonceResponse = nonce_resp
        .json()
        .await
        .context("Failed to parse nonce response")?;

    // 3. Construct and sign SIWE message
    let issued_at = Utc::now().to_rfc3339();
    let verify_uri = format!("{}/v1/auth/verify", url);

    let message_string = build_siwe_message(
        SIWE_DOMAIN,
        &address,
        "Sign in to Conway as an Automaton to provision an API key.",
        &verify_uri,
        CHAIN_ID,
        &nonce_data.nonce,
        &issued_at,
    );

    let signature = signer
        .sign_message(message_string.as_bytes())
        .await
        .context("Failed to sign SIWE message")?;

    let signature_hex = format!("0x{}", hex::encode(signature.as_bytes()));

    // 4. Verify signature -> get JWT
    let verify_resp = client
        .post(&verify_uri)
        .json(&serde_json::json!({
            "message": message_string,
            "signature": signature_hex,
        }))
        .send()
        .await
        .context("Failed to send verify request")?;

    if !verify_resp.status().is_success() {
        let status = verify_resp.status();
        let body = verify_resp.text().await.unwrap_or_default();
        bail!("SIWE verification failed: {} {}", status, body);
    }

    let verify_data: VerifyResponse = verify_resp
        .json()
        .await
        .context("Failed to parse verify response")?;

    // 5. Create API key
    let key_resp = client
        .post(format!("{}/v1/auth/api-keys", url))
        .header("Authorization", format!("Bearer {}", verify_data.access_token))
        .json(&serde_json::json!({ "name": "conway-automaton" }))
        .send()
        .await
        .context("Failed to send API key request")?;

    if !key_resp.status().is_success() {
        let status = key_resp.status();
        let body = key_resp.text().await.unwrap_or_default();
        bail!("Failed to create API key: {} {}", status, body);
    }

    let key_data: ApiKeyResponse = key_resp
        .json()
        .await
        .context("Failed to parse API key response")?;

    // 6. Save to config
    let address_str = address.to_checksum(None);
    save_config(&key_data.key, &address_str)?;

    Ok(ProvisionResult {
        api_key: key_data.key,
        wallet_address: address_str,
        key_prefix: key_data.key_prefix,
    })
}

/// Register the automaton's creator as its parent with Conway.
///
/// This allows the creator to see automaton logs and inference calls.
/// Fails gracefully if the endpoint does not exist (404).
pub async fn register_parent(creator_address: &str, api_url: Option<&str>) -> Result<()> {
    let url = api_url
        .map(|s| s.to_string())
        .or_else(|| std::env::var("CONWAY_API_URL").ok())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string());

    let api_key = load_api_key_from_config()
        .context("Must provision API key before registering parent")?;

    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{}/v1/automaton/register-parent", url))
        .header("Content-Type", "application/json")
        .header("Authorization", &api_key)
        .json(&serde_json::json!({ "creatorAddress": creator_address }))
        .send()
        .await
        .context("Failed to send register-parent request")?;

    // Endpoint may not exist yet -- fail gracefully on 404
    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Failed to register parent: {} {}", status, body);
    }

    Ok(())
}
