//! ERC-8004 On-Chain Agent Registration
//!
//! Registers the automaton on-chain as a Trustless Agent via ERC-8004.
//! Uses the Identity Registry on Base mainnet.
//!
//! Contract: 0x8004A169FB4a3325136EB29fA0ceB6D2e539a432 (Base)
//! Reputation: 0x8004BAa17C55a88189AE136b182e5fdA19dE9b63 (Base)

use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use chrono::Utc;

use crate::types::{AutomatonDatabase, DiscoveredAgent, RegistryEntry};

// ---- Contract Addresses -------------------------------------------------

/// Contract addresses for mainnet (Base).
pub mod mainnet {
    use alloy::primitives::Address;

    const fn hex_literal_20(s: &str) -> [u8; 20] {
        let bytes = s.as_bytes();
        let mut out = [0u8; 20];
        let mut i = 0;
        while i < 20 {
            let hi = hex_val(bytes[i * 2]);
            let lo = hex_val(bytes[i * 2 + 1]);
            out[i] = (hi << 4) | lo;
            i += 1;
        }
        out
    }

    const fn hex_val(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => panic!("invalid hex character"),
        }
    }

    pub const IDENTITY: Address = Address::new(hex_literal_20("8004A169FB4a3325136EB29fA0ceB6D2e539a432"));
    pub const REPUTATION: Address = Address::new(hex_literal_20("8004BAa17C55a88189AE136b182e5fdA19dE9b63"));
    pub const CHAIN_ID: u64 = 8453;
    pub const RPC_URL: &str = "https://mainnet.base.org";
}

/// Contract addresses for testnet (Base Sepolia).
pub mod testnet {
    pub const CHAIN_ID: u64 = 84532;
    pub const RPC_URL: &str = "https://sepolia.base.org";
    // Testnet uses the same contract addresses for now
}

/// Network selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Network {
    #[default]
    Mainnet,
    Testnet,
}


fn get_identity_address(_network: Network) -> Address {
    // Both networks use the same contract address
    mainnet::IDENTITY
}

fn get_reputation_address(_network: Network) -> Address {
    mainnet::REPUTATION
}

fn get_chain_id(network: Network) -> u64 {
    match network {
        Network::Mainnet => mainnet::CHAIN_ID,
        Network::Testnet => testnet::CHAIN_ID,
    }
}

fn get_rpc_url(network: Network) -> &'static str {
    match network {
        Network::Mainnet => mainnet::RPC_URL,
        Network::Testnet => testnet::RPC_URL,
    }
}

// ---- ABI Definitions (minimal subset for registration) -------------------

sol! {
    #[allow(missing_docs)]
    interface IIdentityRegistry {
        function register(string agentURI) external returns (uint256 agentId);
        function updateAgentURI(uint256 agentId, string newAgentURI) external;
        function agentURI(uint256 agentId) external view returns (string);
        function ownerOf(uint256 tokenId) external view returns (address);
        function totalSupply() external view returns (uint256);
        function balanceOf(address owner) external view returns (uint256);
    }

    #[allow(missing_docs)]
    interface IReputation {
        function leaveFeedback(uint256 agentId, uint8 score, string comment) external;
    }
}

// ---- Public API ----------------------------------------------------------

/// Register the automaton on-chain with ERC-8004.
/// Returns the registry entry containing the agent ID (NFT token ID).
pub async fn register_agent(
    signer: &PrivateKeySigner,
    agent_uri: &str,
    network: Network,
    db: &dyn AutomatonDatabase,
) -> Result<RegistryEntry> {
    let rpc_url = get_rpc_url(network);
    let identity_addr = get_identity_address(network);
    let chain_id = get_chain_id(network);

    let wallet = alloy::network::EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    // Build the register call
    let call = IIdentityRegistry::registerCall {
        agentURI: agent_uri.to_string(),
    };

    let tx = TransactionRequest::default()
        .to(identity_addr)
        .input(alloy::primitives::Bytes::from(
            call.abi_encode(),
        ).into());

    let pending = provider
        .send_transaction(tx)
        .await
        .context("Failed to send register transaction")?;

    let receipt = pending
        .get_receipt()
        .await
        .context("Failed to get transaction receipt")?;

    let tx_hash = format!("{:?}", receipt.transaction_hash);

    // Extract agentId from Transfer event logs
    // The register function mints an ERC-721 token
    let mut agent_id = "0".to_string();
    for log in receipt.inner.logs() {
        let topics = log.topics();
        if topics.len() >= 4 {
            // Transfer(address from, address to, uint256 tokenId)
            let token_id = U256::from_be_bytes::<32>(topics[3].0);
            agent_id = token_id.to_string();
            break;
        }
    }

    let entry = RegistryEntry {
        agent_id,
        agent_uri: agent_uri.to_string(),
        chain: format!("eip155:{}", chain_id),
        contract_address: format!("{:?}", identity_addr),
        tx_hash,
        registered_at: Utc::now().to_rfc3339(),
    };

    db.set_registry_entry(&entry);
    Ok(entry)
}

/// Update the agent's URI on-chain.
pub async fn update_agent_uri(
    signer: &PrivateKeySigner,
    agent_id: &str,
    new_uri: &str,
    network: Network,
    db: &dyn AutomatonDatabase,
) -> Result<String> {
    let rpc_url = get_rpc_url(network);
    let identity_addr = get_identity_address(network);

    let wallet = alloy::network::EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let agent_id_u256: U256 = agent_id.parse().context("Invalid agent ID")?;

    let call = IIdentityRegistry::updateAgentURICall {
        agentId: agent_id_u256,
        newAgentURI: new_uri.to_string(),
    };

    let tx = TransactionRequest::default()
        .to(identity_addr)
        .input(alloy::primitives::Bytes::from(
            call.abi_encode(),
        ).into());

    let pending = provider
        .send_transaction(tx)
        .await
        .context("Failed to send updateAgentURI transaction")?;

    let receipt = pending
        .get_receipt()
        .await
        .context("Failed to get transaction receipt")?;

    let tx_hash = format!("{:?}", receipt.transaction_hash);

    // Update in DB
    if let Some(mut entry) = db.get_registry_entry() {
        entry.agent_uri = new_uri.to_string();
        entry.tx_hash = tx_hash.clone();
        db.set_registry_entry(&entry);
    }

    Ok(tx_hash)
}

/// Leave reputation feedback for another agent.
pub async fn leave_feedback(
    signer: &PrivateKeySigner,
    agent_id: &str,
    score: u8,
    comment: &str,
    network: Network,
    db: &dyn AutomatonDatabase,
) -> Result<String> {
    let rpc_url = get_rpc_url(network);
    let reputation_addr = get_reputation_address(network);

    let wallet = alloy::network::EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let agent_id_u256: U256 = agent_id.parse().context("Invalid agent ID")?;

    let call = IReputation::leaveFeedbackCall {
        agentId: agent_id_u256,
        score,
        comment: comment.to_string(),
    };

    let tx = TransactionRequest::default()
        .to(reputation_addr)
        .input(alloy::primitives::Bytes::from(
            call.abi_encode(),
        ).into());

    let pending = provider
        .send_transaction(tx)
        .await
        .context("Failed to send leaveFeedback transaction")?;

    let receipt = pending
        .get_receipt()
        .await
        .context("Failed to get transaction receipt")?;

    let tx_hash = format!("{:?}", receipt.transaction_hash);

    // Record in DB (we don't have the full ReputationEntry context here,
    // but the tx_hash is the important part)
    let _ = db;

    Ok(tx_hash)
}

/// Query the registry for an agent by ID.
pub async fn query_agent(
    agent_id: &str,
    network: Network,
) -> Result<Option<DiscoveredAgent>> {
    let rpc_url = get_rpc_url(network);
    let identity_addr = get_identity_address(network);

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let agent_id_u256: U256 = agent_id.parse().context("Invalid agent ID")?;

    // Read agentURI
    let uri_call = IIdentityRegistry::agentURICall {
        agentId: agent_id_u256,
    };
    let uri_input = Bytes::from(uri_call.abi_encode());

    // Read ownerOf
    let owner_call = IIdentityRegistry::ownerOfCall {
        tokenId: agent_id_u256,
    };
    let owner_input = Bytes::from(owner_call.abi_encode());

    let uri_tx = TransactionRequest::default()
        .to(identity_addr)
        .input(uri_input.into());

    let owner_tx = TransactionRequest::default()
        .to(identity_addr)
        .input(owner_input.into());

    let uri_result = provider.call(uri_tx).await;
    let owner_result = provider.call(owner_tx).await;

    match (uri_result, owner_result) {
        (Ok(uri_bytes), Ok(owner_bytes)) => {
            let uri = String::from_utf8_lossy(&uri_bytes).to_string();
            let owner_slice: &[u8] = owner_bytes.as_ref();
            let owner = if owner_slice.len() >= 32 {
                format!("0x{}", hex::encode(&owner_slice[12..32]))
            } else {
                format!("0x{}", hex::encode(owner_slice))
            };

            Ok(Some(DiscoveredAgent {
                agent_id: agent_id.to_string(),
                owner,
                agent_uri: uri,
                name: None,
                description: None,
            }))
        }
        _ => Ok(None),
    }
}

/// Get the total number of registered agents.
pub async fn get_total_agents(network: Network) -> Result<u64> {
    let rpc_url = get_rpc_url(network);
    let identity_addr = get_identity_address(network);

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let call = IIdentityRegistry::totalSupplyCall {};
    let input = Bytes::from(call.abi_encode());

    let tx = TransactionRequest::default()
        .to(identity_addr)
        .input(input.into());

    match provider.call(tx).await {
        Ok(result) => {
            let result_bytes: &[u8] = result.as_ref();
            let bytes32: [u8; 32] = result_bytes.try_into().unwrap_or([0u8; 32]);
            let supply = U256::from_be_bytes::<32>(bytes32);
            Ok(supply.to::<u64>())
        }
        Err(_) => Ok(0),
    }
}

/// Check if an address has a registered agent.
pub async fn has_registered_agent(
    address: Address,
    network: Network,
) -> Result<bool> {
    let rpc_url = get_rpc_url(network);
    let identity_addr = get_identity_address(network);

    let provider = ProviderBuilder::new()
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let call = IIdentityRegistry::balanceOfCall { owner: address };
    let input = Bytes::from(call.abi_encode());

    let tx = TransactionRequest::default()
        .to(identity_addr)
        .input(input.into());

    match provider.call(tx).await {
        Ok(result) => {
            let result_bytes: &[u8] = result.as_ref();
            let bytes32: [u8; 32] = result_bytes.try_into().unwrap_or([0u8; 32]);
            let balance = U256::from_be_bytes::<32>(bytes32);
            Ok(balance > U256::ZERO)
        }
        Err(_) => Ok(false),
    }
}
