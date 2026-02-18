//! Conway API Client
//!
//! Communicates with Conway's control plane for sandbox management,
//! credits, and infrastructure operations.
//! Adapted from the TypeScript @aiws/sdk patterns.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::types::{
    ConwayClient, CreditTransferResult, CreateSandboxOptions, DnsRecord, DomainRegistration,
    DomainSearchResult, ExecResult, ModelInfo, ModelPricing, PricingTier, PortInfo, SandboxInfo,
};

/// Conway API client for sandbox management, credits, domains, and model discovery.
pub struct ConwayHttpClient {
    pub api_url: String,
    pub api_key: String,
    pub sandbox_id: String,
    http: Client,
}

impl ConwayHttpClient {
    /// Create a new Conway API client.
    pub fn new(api_url: String, api_key: String, sandbox_id: String) -> Self {
        Self {
            api_url,
            api_key,
            sandbox_id,
            http: Client::new(),
        }
    }

    /// Internal helper: send an HTTP request to the Conway API and return JSON.
    async fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value> {
        let url = format!("{}{}", self.api_url, path);

        let mut builder = match method {
            "GET" => self.http.get(&url),
            "POST" => self.http.post(&url),
            "PUT" => self.http.put(&url),
            "DELETE" => self.http.delete(&url),
            "PATCH" => self.http.patch(&url),
            _ => self.http.get(&url),
        };

        builder = builder
            .header("Content-Type", "application/json")
            .header("Authorization", &self.api_key);

        if let Some(b) = body {
            builder = builder.json(&b);
        }

        let resp = builder
            .send()
            .await
            .with_context(|| format!("Conway API request failed: {} {}", method, path))?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Conway API error: {} {} -> {}: {}",
                method,
                path,
                status.as_u16(),
                text
            );
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if content_type.contains("application/json") {
            let json: Value = resp.json().await?;
            Ok(json)
        } else {
            let text = resp.text().await?;
            Ok(Value::String(text))
        }
    }
}

#[async_trait]
impl ConwayClient for ConwayHttpClient {
    // ── Sandbox Operations (own sandbox) ──────────────────────────

    /// Execute a command in the automaton's sandbox.
    async fn exec(&self, command: &str, timeout: Option<u64>) -> Result<ExecResult> {
        let mut body = serde_json::json!({ "command": command });
        if let Some(t) = timeout {
            body["timeout"] = serde_json::json!(t);
        }

        let result = self
            .request(
                "POST",
                &format!("/v1/sandboxes/{}/exec", self.sandbox_id),
                Some(body),
            )
            .await?;

        Ok(ExecResult {
            stdout: result["stdout"].as_str().unwrap_or("").to_string(),
            stderr: result["stderr"].as_str().unwrap_or("").to_string(),
            exit_code: result["exit_code"]
                .as_i64()
                .or_else(|| result["exitCode"].as_i64())
                .unwrap_or(0) as i32,
        })
    }

    /// Write a file into the sandbox.
    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        let body = serde_json::json!({ "path": path, "content": content });
        self.request(
            "POST",
            &format!("/v1/sandboxes/{}/files/upload/json", self.sandbox_id),
            Some(body),
        )
        .await?;
        Ok(())
    }

    /// Read a file from the sandbox.
    async fn read_file(&self, file_path: &str) -> Result<String> {
        let encoded = urlencoding::encode(file_path);
        let result = self
            .request(
                "GET",
                &format!(
                    "/v1/sandboxes/{}/files/read?path={}",
                    self.sandbox_id, encoded
                ),
                None,
            )
            .await?;

        match result {
            Value::String(s) => Ok(s),
            _ => Ok(result["content"].as_str().unwrap_or("").to_string()),
        }
    }

    /// Expose a port from the sandbox to the public internet.
    async fn expose_port(&self, port: u16) -> Result<PortInfo> {
        let body = serde_json::json!({ "port": port });
        let result = self
            .request(
                "POST",
                &format!("/v1/sandboxes/{}/ports/expose", self.sandbox_id),
                Some(body),
            )
            .await?;

        let public_url = result["public_url"]
            .as_str()
            .or_else(|| result["publicUrl"].as_str())
            .or_else(|| result["url"].as_str())
            .unwrap_or("")
            .to_string();

        Ok(PortInfo {
            port,
            public_url,
            sandbox_id: self.sandbox_id.clone(),
        })
    }

    /// Remove an exposed port from the sandbox.
    async fn remove_port(&self, port: u16) -> Result<()> {
        self.request(
            "DELETE",
            &format!("/v1/sandboxes/{}/ports/{}", self.sandbox_id, port),
            None,
        )
        .await?;
        Ok(())
    }

    // ── Sandbox Management (other sandboxes) ─────────────────────

    /// Create a new sandbox.
    async fn create_sandbox(&self, options: CreateSandboxOptions) -> Result<SandboxInfo> {
        let body = serde_json::json!({
            "name": options.name,
            "vcpu": options.vcpu.unwrap_or(1),
            "memory_mb": options.memory_mb.unwrap_or(512),
            "disk_gb": options.disk_gb.unwrap_or(5),
            "region": options.region,
        });

        let result = self.request("POST", "/v1/sandboxes", Some(body)).await?;

        Ok(SandboxInfo {
            id: result["id"]
                .as_str()
                .or_else(|| result["sandbox_id"].as_str())
                .unwrap_or("")
                .to_string(),
            status: result["status"]
                .as_str()
                .unwrap_or("running")
                .to_string(),
            region: result["region"].as_str().unwrap_or("").to_string(),
            vcpu: result["vcpu"].as_u64().unwrap_or(options.vcpu.unwrap_or(1) as u64) as u32,
            memory_mb: result["memory_mb"]
                .as_u64()
                .unwrap_or(options.memory_mb.unwrap_or(512) as u64) as u32,
            disk_gb: result["disk_gb"]
                .as_u64()
                .unwrap_or(options.disk_gb.unwrap_or(5) as u64) as u32,
            terminal_url: result["terminal_url"].as_str().map(|s| s.to_string()),
            created_at: result["created_at"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        })
    }

    /// Delete a sandbox by ID.
    async fn delete_sandbox(&self, target_id: &str) -> Result<()> {
        self.request("DELETE", &format!("/v1/sandboxes/{}", target_id), None)
            .await?;
        Ok(())
    }

    /// List all sandboxes.
    async fn list_sandboxes(&self) -> Result<Vec<SandboxInfo>> {
        let result = self.request("GET", "/v1/sandboxes", None).await?;

        let sandboxes = if result.is_array() {
            result.as_array().cloned().unwrap_or_default()
        } else {
            result["sandboxes"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        };

        Ok(sandboxes
            .iter()
            .map(|s| SandboxInfo {
                id: s["id"]
                    .as_str()
                    .or_else(|| s["sandbox_id"].as_str())
                    .unwrap_or("")
                    .to_string(),
                status: s["status"].as_str().unwrap_or("unknown").to_string(),
                region: s["region"].as_str().unwrap_or("").to_string(),
                vcpu: s["vcpu"].as_u64().unwrap_or(0) as u32,
                memory_mb: s["memory_mb"].as_u64().unwrap_or(0) as u32,
                disk_gb: s["disk_gb"].as_u64().unwrap_or(0) as u32,
                terminal_url: s["terminal_url"].as_str().map(|v| v.to_string()),
                created_at: s["created_at"].as_str().unwrap_or("").to_string(),
            })
            .collect())
    }

    // ── Credits ──────────────────────────────────────────────────

    /// Get the automaton's credit balance in cents.
    async fn get_credits_balance(&self) -> Result<f64> {
        let result = self.request("GET", "/v1/credits/balance", None).await?;
        let balance = result["balance_cents"]
            .as_f64()
            .or_else(|| result["credits_cents"].as_f64())
            .unwrap_or(0.0);
        Ok(balance)
    }

    /// Get available pricing tiers.
    async fn get_credits_pricing(&self) -> Result<Vec<PricingTier>> {
        let result = self.request("GET", "/v1/credits/pricing", None).await?;

        let tiers = result["tiers"]
            .as_array()
            .or_else(|| result["pricing"].as_array())
            .cloned()
            .unwrap_or_default();

        Ok(tiers
            .iter()
            .map(|t| PricingTier {
                name: t["name"].as_str().unwrap_or("").to_string(),
                vcpu: t["vcpu"].as_u64().unwrap_or(0) as u32,
                memory_mb: t["memory_mb"].as_u64().unwrap_or(0) as u32,
                disk_gb: t["disk_gb"].as_u64().unwrap_or(0) as u32,
                monthly_cents: t["monthly_cents"].as_u64().unwrap_or(0),
            })
            .collect())
    }

    /// Transfer credits to another address.
    /// Tries `/v1/credits/transfer` first, falls back to `/v1/credits/transfers`.
    async fn transfer_credits(
        &self,
        to_address: &str,
        amount_cents: u64,
        note: Option<&str>,
    ) -> Result<CreditTransferResult> {
        let payload = serde_json::json!({
            "to_address": to_address,
            "amount_cents": amount_cents,
            "note": note,
        });

        let paths = ["/v1/credits/transfer", "/v1/credits/transfers"];
        let mut last_error = String::from("Unknown transfer error");

        for path in &paths {
            let url = format!("{}{}", self.api_url, path);
            let resp = self
                .http
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Authorization", &self.api_key)
                .json(&payload)
                .send()
                .await;

            let resp = match resp {
                Ok(r) => r,
                Err(e) => {
                    last_error = e.to_string();
                    continue;
                }
            };

            let status = resp.status();
            if !status.is_success() {
                let text = resp.text().await.unwrap_or_default();
                last_error = format!("{}: {}", status.as_u16(), text);
                if status.as_u16() == 404 {
                    continue;
                }
                anyhow::bail!("Conway API error: POST {} -> {}", path, last_error);
            }

            let data: Value = resp.json().await.unwrap_or(serde_json::json!({}));
            return Ok(CreditTransferResult {
                transfer_id: data["transfer_id"]
                    .as_str()
                    .or_else(|| data["id"].as_str())
                    .unwrap_or("")
                    .to_string(),
                status: data["status"]
                    .as_str()
                    .unwrap_or("submitted")
                    .to_string(),
                to_address: data["to_address"]
                    .as_str()
                    .unwrap_or(to_address)
                    .to_string(),
                amount_cents: data["amount_cents"].as_u64().unwrap_or(amount_cents),
                balance_after_cents: data["balance_after_cents"]
                    .as_u64()
                    .or_else(|| data["new_balance_cents"].as_u64()),
            });
        }

        anyhow::bail!(
            "Conway API error: POST /v1/credits/transfer -> {}",
            last_error
        )
    }

    // ── Domains ──────────────────────────────────────────────────

    /// Search for available domains.
    async fn search_domains(
        &self,
        query: &str,
        tlds: Option<&str>,
    ) -> Result<Vec<DomainSearchResult>> {
        let mut params = format!("query={}", urlencoding::encode(query));
        if let Some(tlds_val) = tlds {
            params.push_str(&format!("&tlds={}", urlencoding::encode(tlds_val)));
        }

        let result = self
            .request("GET", &format!("/v1/domains/search?{}", params), None)
            .await?;

        let results = result["results"]
            .as_array()
            .or_else(|| result["domains"].as_array())
            .cloned()
            .unwrap_or_default();

        Ok(results
            .iter()
            .map(|d| DomainSearchResult {
                domain: d["domain"].as_str().unwrap_or("").to_string(),
                available: d["available"]
                    .as_bool()
                    .or_else(|| d["purchasable"].as_bool())
                    .unwrap_or(false),
                registration_price: d["registration_price"]
                    .as_f64()
                    .or_else(|| d["purchase_price"].as_f64()),
                renewal_price: d["renewal_price"].as_f64(),
                currency: Some(
                    d["currency"]
                        .as_str()
                        .unwrap_or("USD")
                        .to_string(),
                ),
            })
            .collect())
    }

    /// Register a domain.
    async fn register_domain(
        &self,
        domain: &str,
        years: Option<u32>,
    ) -> Result<DomainRegistration> {
        let body = serde_json::json!({
            "domain": domain,
            "years": years.unwrap_or(1),
        });

        let result = self
            .request("POST", "/v1/domains/register", Some(body))
            .await?;

        Ok(DomainRegistration {
            domain: result["domain"]
                .as_str()
                .unwrap_or(domain)
                .to_string(),
            status: result["status"]
                .as_str()
                .unwrap_or("registered")
                .to_string(),
            expires_at: result["expires_at"]
                .as_str()
                .or_else(|| result["expiry"].as_str())
                .map(|s| s.to_string()),
            transaction_id: result["transaction_id"]
                .as_str()
                .or_else(|| result["id"].as_str())
                .map(|s| s.to_string()),
        })
    }

    /// List DNS records for a domain.
    async fn list_dns_records(&self, domain: &str) -> Result<Vec<DnsRecord>> {
        let encoded = urlencoding::encode(domain);
        let result = self
            .request("GET", &format!("/v1/domains/{}/dns", encoded), None)
            .await?;

        let records_val = if result["records"].is_array() {
            result["records"].as_array().cloned().unwrap_or_default()
        } else if result.is_array() {
            result.as_array().cloned().unwrap_or_default()
        } else {
            Vec::new()
        };

        Ok(records_val
            .iter()
            .map(|r| DnsRecord {
                id: r["id"]
                    .as_str()
                    .or_else(|| r["record_id"].as_str())
                    .unwrap_or("")
                    .to_string(),
                record_type: r["type"].as_str().unwrap_or("").to_string(),
                host: r["host"]
                    .as_str()
                    .or_else(|| r["name"].as_str())
                    .unwrap_or("")
                    .to_string(),
                value: r["value"]
                    .as_str()
                    .or_else(|| r["answer"].as_str())
                    .unwrap_or("")
                    .to_string(),
                ttl: r["ttl"].as_u64().map(|v| v as u32),
                distance: r["distance"]
                    .as_u64()
                    .or_else(|| r["priority"].as_u64())
                    .map(|v| v as u32),
            })
            .collect())
    }

    /// Add a DNS record to a domain.
    async fn add_dns_record(
        &self,
        domain: &str,
        record_type: &str,
        host: &str,
        value: &str,
        ttl: Option<u32>,
    ) -> Result<DnsRecord> {
        let encoded = urlencoding::encode(domain);
        let body = serde_json::json!({
            "type": record_type,
            "host": host,
            "value": value,
            "ttl": ttl.unwrap_or(3600),
        });

        let result = self
            .request("POST", &format!("/v1/domains/{}/dns", encoded), Some(body))
            .await?;

        Ok(DnsRecord {
            id: result["id"]
                .as_str()
                .or_else(|| result["record_id"].as_str())
                .unwrap_or("")
                .to_string(),
            record_type: result["type"]
                .as_str()
                .unwrap_or(record_type)
                .to_string(),
            host: result["host"].as_str().unwrap_or(host).to_string(),
            value: result["value"].as_str().unwrap_or(value).to_string(),
            ttl: Some(result["ttl"].as_u64().unwrap_or(ttl.unwrap_or(3600) as u64) as u32),
            distance: None,
        })
    }

    /// Delete a DNS record from a domain.
    async fn delete_dns_record(&self, domain: &str, record_id: &str) -> Result<()> {
        let encoded_domain = urlencoding::encode(domain);
        let encoded_record = urlencoding::encode(record_id);
        self.request(
            "DELETE",
            &format!("/v1/domains/{}/dns/{}", encoded_domain, encoded_record),
            None,
        )
        .await?;
        Ok(())
    }

    // ── Model Discovery ──────────────────────────────────────────

    /// List available inference models.
    /// Tries inference.conway.tech first, falls back to the control plane.
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let urls = [
            "https://inference.conway.tech/v1/models".to_string(),
            format!("{}/v1/models", self.api_url),
        ];

        for url in &urls {
            let resp = self
                .http
                .get(url)
                .header("Authorization", &self.api_key)
                .send()
                .await;

            let resp = match resp {
                Ok(r) if r.status().is_success() => r,
                _ => continue,
            };

            let data: Value = match resp.json().await {
                Ok(d) => d,
                Err(_) => continue,
            };

            let raw = data["data"]
                .as_array()
                .or_else(|| data["models"].as_array())
                .cloned()
                .unwrap_or_default();

            let models: Vec<ModelInfo> = raw
                .iter()
                .filter(|m| m["available"].as_bool().unwrap_or(true))
                .map(|m| {
                    let input = m["pricing"]["input_per_million"]
                        .as_f64()
                        .or_else(|| m["pricing"]["input_per_1m_tokens_usd"].as_f64())
                        .unwrap_or(0.0);
                    let output = m["pricing"]["output_per_million"]
                        .as_f64()
                        .or_else(|| m["pricing"]["output_per_1m_tokens_usd"].as_f64())
                        .unwrap_or(0.0);

                    ModelInfo {
                        id: m["id"].as_str().unwrap_or("").to_string(),
                        provider: m["provider"]
                            .as_str()
                            .or_else(|| m["owned_by"].as_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        pricing: ModelPricing {
                            input_per_million: input,
                            output_per_million: output,
                        },
                    }
                })
                .collect();

            return Ok(models);
        }

        Ok(Vec::new())
    }
}
