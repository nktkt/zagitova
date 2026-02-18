//! Automaton Wallet Management
//!
//! Creates and manages an EVM wallet for the automaton's identity and payments.
//! The private key is the automaton's sovereign identity.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use alloy::signers::local::PrivateKeySigner;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};

/// Directory name under the user's home for all automaton data.
const AUTOMATON_DIR_NAME: &str = ".automaton";

/// Wallet file name within the automaton directory.
const WALLET_FILENAME: &str = "wallet.json";

/// On-disk wallet representation.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletData {
    /// Hex-encoded private key with "0x" prefix.
    pub private_key: String,
    /// ISO-8601 timestamp of when this wallet was created.
    pub created_at: String,
}

/// Returns the automaton base directory: `~/.automaton`.
pub fn get_automaton_dir() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/root"));
    home.join(AUTOMATON_DIR_NAME)
}

/// Returns the full path to the wallet file: `~/.automaton/wallet.json`.
pub fn get_wallet_path() -> PathBuf {
    get_automaton_dir().join(WALLET_FILENAME)
}

/// Get or create the automaton's wallet.
///
/// If a wallet file already exists, loads the private key from it.
/// Otherwise, generates a new random secp256k1 private key and persists it.
///
/// Returns the signer and a boolean indicating whether a new wallet was created.
pub fn get_wallet() -> Result<(PrivateKeySigner, bool)> {
    let dir = get_automaton_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).context("Failed to create automaton directory")?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
            .context("Failed to set directory permissions")?;
    }

    let wallet_path = get_wallet_path();

    if wallet_path.exists() {
        // Load existing wallet
        let contents =
            fs::read_to_string(&wallet_path).context("Failed to read wallet file")?;
        let wallet_data: WalletData =
            serde_json::from_str(&contents).context("Failed to parse wallet JSON")?;

        let signer: PrivateKeySigner = wallet_data
            .private_key
            .parse()
            .context("Failed to parse private key from wallet file")?;

        Ok((signer, false))
    } else {
        // Generate new wallet
        let signer = PrivateKeySigner::random();

        let private_key_bytes = signer.credential().to_bytes();
        let private_key_hex = format!("0x{}", hex::encode(private_key_bytes));

        let wallet_data = WalletData {
            private_key: private_key_hex,
            created_at: Utc::now().to_rfc3339(),
        };

        let json =
            serde_json::to_string_pretty(&wallet_data).context("Failed to serialize wallet")?;

        fs::write(&wallet_path, &json).context("Failed to write wallet file")?;
        fs::set_permissions(&wallet_path, fs::Permissions::from_mode(0o600))
            .context("Failed to set wallet file permissions")?;

        Ok((signer, true))
    }
}

/// Get the wallet's checksummed Ethereum address without loading the full signer.
///
/// Returns `None` if the wallet file does not exist or cannot be read.
pub fn get_wallet_address() -> Option<String> {
    let wallet_path = get_wallet_path();
    if !wallet_path.exists() {
        return None;
    }

    let contents = fs::read_to_string(&wallet_path).ok()?;
    let wallet_data: WalletData = serde_json::from_str(&contents).ok()?;

    let signer: PrivateKeySigner = wallet_data.private_key.parse().ok()?;
    Some(signer.address().to_checksum(None))
}

/// Check whether a wallet file exists on disk.
pub fn wallet_exists() -> bool {
    get_wallet_path().exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_automaton_dir_is_under_home() {
        let dir = get_automaton_dir();
        assert!(dir.ends_with(".automaton"));
    }

    #[test]
    fn test_get_wallet_path_is_under_automaton_dir() {
        let path = get_wallet_path();
        assert!(path.ends_with("wallet.json"));
        assert!(path.starts_with(get_automaton_dir()));
    }
}
