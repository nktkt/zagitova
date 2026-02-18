//! x402 Payment Protocol
//!
//! Enables the automaton to make USDC micropayments via HTTP 402.
//! Uses alloy for all Ethereum operations and reqwest for HTTP.

use std::collections::HashMap;
use std::sync::LazyLock;

use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol;
use alloy::signers::Signer;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── USDC Addresses ──────────────────────────────────────────────────

/// USDC contract addresses by CAIP-2 network identifier.
pub static USDC_ADDRESSES: LazyLock<HashMap<&'static str, Address>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // Base mainnet
    m.insert(
        "eip155:8453",
        "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"
            .parse::<Address>()
            .unwrap(),
    );
    // Base Sepolia
    m.insert(
        "eip155:84532",
        "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
            .parse::<Address>()
            .unwrap(),
    );
    m
});

/// RPC endpoints by CAIP-2 network identifier.
static RPC_URLS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("eip155:8453", "https://mainnet.base.org");
    m.insert("eip155:84532", "https://sepolia.base.org");
    m
});

// ── ABI for USDC balanceOf ──────────────────────────────────────────

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

// ── Types ───────────────────────────────────────────────────────────

/// Payment requirement returned from an HTTP 402 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequirement {
    pub scheme: String,
    pub network: String,
    #[serde(rename = "maxAmountRequired")]
    pub max_amount_required: String,
    #[serde(rename = "payToAddress")]
    pub pay_to_address: String,
    #[serde(rename = "requiredDeadlineSeconds", default = "default_deadline")]
    pub required_deadline_seconds: u64,
    #[serde(rename = "usdcAddress")]
    pub usdc_address: String,
}

fn default_deadline() -> u64 {
    300
}

/// Result of an x402 payment fetch operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X402PaymentResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// EIP-712 signed payment payload for x402.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct X402Payment {
    #[serde(rename = "x402Version")]
    x402_version: u32,
    scheme: String,
    network: String,
    payload: X402PaymentPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct X402PaymentPayload {
    signature: String,
    authorization: X402Authorization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct X402Authorization {
    from: String,
    to: String,
    value: String,
    #[serde(rename = "validAfter")]
    valid_after: String,
    #[serde(rename = "validBefore")]
    valid_before: String,
    nonce: String,
}

// ── Public API ──────────────────────────────────────────────────────

/// Get the USDC balance for a wallet address on a given network.
///
/// Returns the balance as a floating-point number (USDC has 6 decimals).
/// Returns 0.0 on any error.
pub async fn get_usdc_balance(address: Address, network: &str) -> Result<f64> {
    let usdc_address = match USDC_ADDRESSES.get(network) {
        Some(addr) => *addr,
        None => return Ok(0.0),
    };

    let rpc_url = match RPC_URLS.get(network) {
        Some(url) => *url,
        None => return Ok(0.0),
    };

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let contract = IERC20::new(usdc_address, &provider);
    let result = contract.balanceOf(address).call().await;

    match result {
        Ok(raw) => {
            // USDC has 6 decimals
            // In alloy 1.x, sol! returns U256 directly for single return values
            let divisor = U256::from(1_000_000u64);
            let whole = raw / divisor;
            let frac = raw % divisor;
            let balance_f64 =
                whole.to::<u64>() as f64 + frac.to::<u64>() as f64 / 1_000_000.0;
            Ok(balance_f64)
        }
        Err(_) => Ok(0.0),
    }
}

/// Check if a URL requires x402 payment by issuing a GET request
/// and looking for a 402 status code.
///
/// Returns `Some(PaymentRequirement)` if payment is required, `None` otherwise.
pub async fn check_x402(url: &str) -> Result<Option<PaymentRequirement>> {
    let client = Client::new();

    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };

    if resp.status().as_u16() != 402 {
        return Ok(None);
    }

    // Try X-Payment-Required header (base64-encoded JSON)
    if let Some(header_val) = resp.headers().get("X-Payment-Required") {
        if let Ok(header_str) = header_val.to_str() {
            if let Ok(decoded) = BASE64.decode(header_str) {
                if let Ok(text) = String::from_utf8(decoded) {
                    if let Ok(requirements) = serde_json::from_str::<Value>(&text) {
                        if let Some(accept) = requirements["accepts"].get(0) {
                            let network = accept["network"]
                                .as_str()
                                .unwrap_or("eip155:8453");
                            let default_usdc = USDC_ADDRESSES
                                .get(network)
                                .or_else(|| USDC_ADDRESSES.get("eip155:8453"))
                                .map(|a| format!("{:?}", a))
                                .unwrap_or_default();

                            return Ok(Some(PaymentRequirement {
                                scheme: accept["scheme"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                network: network.to_string(),
                                max_amount_required: accept["maxAmountRequired"]
                                    .as_str()
                                    .unwrap_or("0")
                                    .to_string(),
                                pay_to_address: accept["payToAddress"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                                required_deadline_seconds: accept["requiredDeadlineSeconds"]
                                    .as_u64()
                                    .unwrap_or(300),
                                usdc_address: accept["usdcAddress"]
                                    .as_str()
                                    .map(|s| s.to_string())
                                    .unwrap_or(default_usdc),
                            }));
                        }
                    }
                }
            }
        }
    }

    // Try response body
    let body_text = resp.text().await.unwrap_or_default();
    if let Ok(body) = serde_json::from_str::<Value>(&body_text) {
        if let Some(accept) = body["accepts"].get(0) {
            let network = accept["network"]
                .as_str()
                .unwrap_or("eip155:8453");
            let default_usdc = USDC_ADDRESSES
                .get(network)
                .or_else(|| USDC_ADDRESSES.get("eip155:8453"))
                .map(|a| format!("{:?}", a))
                .unwrap_or_default();

            return Ok(Some(PaymentRequirement {
                scheme: accept["scheme"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                network: network.to_string(),
                max_amount_required: accept["maxAmountRequired"]
                    .as_str()
                    .unwrap_or("0")
                    .to_string(),
                pay_to_address: accept["payToAddress"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                required_deadline_seconds: accept["requiredDeadlineSeconds"]
                    .as_u64()
                    .unwrap_or(300),
                usdc_address: accept["usdcAddress"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or(default_usdc),
            }));
        }
    }

    Ok(None)
}

/// Fetch a URL with automatic x402 payment handling.
///
/// If the endpoint returns HTTP 402, the function will parse the payment
/// requirements, sign an EIP-712 TransferWithAuthorization message, and
/// retry the request with the `X-Payment` header.
///
/// * `url` - The URL to fetch.
/// * `signer` - An alloy `Signer` (e.g. `PrivateKeySigner`) for signing the payment.
/// * `signer_address` - The address of the signer.
/// * `method` - HTTP method (e.g. "GET", "POST").
/// * `body` - Optional request body.
/// * `headers` - Optional extra headers.
pub async fn x402_fetch<S: Signer + Send + Sync>(
    url: &str,
    signer: &S,
    signer_address: Address,
    method: &str,
    body: Option<&str>,
    headers: Option<&HashMap<String, String>>,
) -> Result<X402PaymentResult> {
    let client = Client::new();

    // Build initial request
    let mut builder = match method {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        _ => client.get(url),
    };

    builder = builder.header("Content-Type", "application/json");

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            builder = builder.header(k.as_str(), v.as_str());
        }
    }

    if let Some(b) = body {
        builder = builder.body(b.to_string());
    }

    let initial_resp = match builder.send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(X402PaymentResult {
                success: false,
                response: None,
                error: Some(e.to_string()),
            })
        }
    };

    if initial_resp.status().as_u16() != 402 {
        let success = initial_resp.status().is_success();
        let resp_text = initial_resp.text().await.unwrap_or_default();
        let response: Value = serde_json::from_str(&resp_text).unwrap_or(Value::String(resp_text));
        return Ok(X402PaymentResult {
            success,
            response: Some(response),
            error: None,
        });
    }

    // Parse payment requirements from the 402 response
    let requirement = parse_payment_required(&initial_resp, url).await;
    let requirement = match requirement {
        Some(r) => r,
        None => {
            return Ok(X402PaymentResult {
                success: false,
                response: None,
                error: Some("Could not parse payment requirements".to_string()),
            })
        }
    };

    // Sign the payment
    let payment = match sign_payment(signer, signer_address, &requirement).await {
        Some(p) => p,
        None => {
            return Ok(X402PaymentResult {
                success: false,
                response: None,
                error: Some("Failed to sign payment".to_string()),
            })
        }
    };

    // Encode payment as base64 JSON for the X-Payment header
    let payment_json = serde_json::to_string(&payment).unwrap_or_default();
    let payment_header = BASE64.encode(payment_json.as_bytes());

    // Retry with payment
    let mut retry_builder = match method {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "DELETE" => client.delete(url),
        "PATCH" => client.patch(url),
        _ => client.get(url),
    };

    retry_builder = retry_builder
        .header("Content-Type", "application/json")
        .header("X-Payment", &payment_header);

    if let Some(hdrs) = headers {
        for (k, v) in hdrs {
            retry_builder = retry_builder.header(k.as_str(), v.as_str());
        }
    }

    if let Some(b) = body {
        retry_builder = retry_builder.body(b.to_string());
    }

    let paid_resp = match retry_builder.send().await {
        Ok(r) => r,
        Err(e) => {
            return Ok(X402PaymentResult {
                success: false,
                response: None,
                error: Some(e.to_string()),
            })
        }
    };

    let success = paid_resp.status().is_success();
    let resp_text = paid_resp.text().await.unwrap_or_default();
    let response: Value = serde_json::from_str(&resp_text).unwrap_or(Value::String(resp_text));

    Ok(X402PaymentResult {
        success,
        response: Some(response),
        error: None,
    })
}

// ── Internal helpers ────────────────────────────────────────────────

/// Parse the payment requirement from a 402 HTTP response.
/// Checks the X-Payment-Required header first, then the response body.
async fn parse_payment_required(
    _resp: &reqwest::Response,
    url: &str,
) -> Option<PaymentRequirement> {
    // Note: reqwest::Response is consumed when reading the body, so
    // for the full flow we re-issue a GET to parse the body.
    // In practice, the header path is preferred. For the initial call in
    // x402_fetch we have already consumed the response, so we re-check
    // using check_x402 which issues a fresh request.
    match check_x402(url).await {
        Ok(Some(req)) => Some(req),
        _ => None,
    }
}

/// Sign an EIP-712 TransferWithAuthorization payment.
///
/// Constructs the typed data, signs it with the provided signer, and returns
/// the x402 payment envelope.
async fn sign_payment<S: Signer + Send + Sync>(
    signer: &S,
    signer_address: Address,
    requirement: &PaymentRequirement,
) -> Option<X402Payment> {
    use alloy::primitives::keccak256;

    // Generate random nonce (32 bytes)
    let mut nonce_bytes = [0u8; 32];
    for byte in nonce_bytes.iter_mut() {
        *byte = rand::random();
    }
    let nonce = format!("0x{}", hex::encode(nonce_bytes));

    let now = chrono::Utc::now().timestamp() as u64;
    let valid_after = now.saturating_sub(60);
    let valid_before = now + requirement.required_deadline_seconds;

    // Parse the amount (USDC has 6 decimals)
    let amount_str = &requirement.max_amount_required;
    let amount = parse_usdc_amount(amount_str)?;

    let chain_id: u64 = if requirement.network == "eip155:84532" {
        84532
    } else {
        8453
    };

    let pay_to: Address = requirement.pay_to_address.parse().ok()?;
    let usdc_addr: Address = requirement.usdc_address.parse().ok()?;

    // EIP-712 domain separator hash
    // domain: { name: "USD Coin", version: "2", chainId, verifyingContract: usdcAddress }
    let domain_type_hash = keccak256(
        b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)",
    );
    let name_hash = keccak256(b"USD Coin");
    let version_hash = keccak256(b"2");

    // Manually encode the domain separator components
    let mut domain_data = Vec::with_capacity(5 * 32);
    domain_data.extend_from_slice(domain_type_hash.as_slice());
    domain_data.extend_from_slice(name_hash.as_slice());
    domain_data.extend_from_slice(version_hash.as_slice());
    domain_data.extend_from_slice(&U256::from(chain_id).to_be_bytes::<32>());
    {
        let mut buf = [0u8; 32];
        buf[12..32].copy_from_slice(usdc_addr.as_slice());
        domain_data.extend_from_slice(&buf);
    }
    let domain_separator = keccak256(&domain_data);

    // TransferWithAuthorization type hash
    let transfer_type_hash = keccak256(
        b"TransferWithAuthorization(address from,address to,uint256 value,uint256 validAfter,uint256 validBefore,bytes32 nonce)",
    );

    // Encode the struct hash
    let nonce_fixed = FixedBytes::<32>::from_slice(&nonce_bytes);
    let struct_data = {
        let mut data = Vec::with_capacity(7 * 32);
        data.extend_from_slice(transfer_type_hash.as_slice());
        // from
        let mut from_buf = [0u8; 32];
        from_buf[12..32].copy_from_slice(signer_address.as_slice());
        data.extend_from_slice(&from_buf);
        // to
        let mut to_buf = [0u8; 32];
        to_buf[12..32].copy_from_slice(pay_to.as_slice());
        data.extend_from_slice(&to_buf);
        // value
        data.extend_from_slice(&amount.to_be_bytes::<32>());
        // validAfter
        data.extend_from_slice(&U256::from(valid_after).to_be_bytes::<32>());
        // validBefore
        data.extend_from_slice(&U256::from(valid_before).to_be_bytes::<32>());
        // nonce
        data.extend_from_slice(nonce_fixed.as_slice());
        data
    };
    let struct_hash = keccak256(&struct_data);

    // EIP-712 hash: keccak256("\x19\x01" || domainSeparator || structHash)
    let mut sign_input = Vec::with_capacity(2 + 32 + 32);
    sign_input.extend_from_slice(&[0x19, 0x01]);
    sign_input.extend_from_slice(domain_separator.as_slice());
    sign_input.extend_from_slice(struct_hash.as_slice());
    let digest = keccak256(&sign_input);

    // Sign using alloy Signer
    let signature = signer
        .sign_hash(&digest)
        .await
        .ok()?;

    let sig_hex = format!("0x{}", hex::encode(signature.as_bytes()));

    Some(X402Payment {
        x402_version: 1,
        scheme: "exact".to_string(),
        network: requirement.network.clone(),
        payload: X402PaymentPayload {
            signature: sig_hex,
            authorization: X402Authorization {
                from: format!("{:?}", signer_address),
                to: format!("{:?}", pay_to),
                value: amount.to_string(),
                valid_after: valid_after.to_string(),
                valid_before: valid_before.to_string(),
                nonce,
            },
        },
    })
}

/// Parse a USDC amount string (human-readable, e.g. "1.50") into raw units
/// (6 decimals). Returns None on parse failure.
fn parse_usdc_amount(amount_str: &str) -> Option<U256> {
    let trimmed = amount_str.trim();

    // Handle cases like "1500000" (already in raw units) vs "1.50" (human readable)
    if trimmed.contains('.') {
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() != 2 {
            return None;
        }
        let whole: u64 = parts[0].parse().ok()?;
        let frac_str = format!("{:0<6}", parts[1]);
        let frac: u64 = frac_str[..6].parse().ok()?;
        Some(U256::from(whole * 1_000_000 + frac))
    } else {
        // Assume raw units or integer dollars
        let val: u64 = trimmed.parse().ok()?;
        // If the value is very large, assume it is already in raw units
        if val > 1_000_000 {
            Some(U256::from(val))
        } else {
            Some(U256::from(val * 1_000_000))
        }
    }
}
