use serde_with::hex::Hex;
use hot_validation_core::uid::HexOrBase58;
use crate::domain::errors::AppError;
use aes::{Aes128, Aes192, Aes256};
use anyhow::{Context, anyhow};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use cbc::Decryptor;
use cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hot_validation_primitives::uid::Uid;
use near_workspaces::AccountId;
use near_workspaces::types::SecretKey;
use rpassword::prompt_password;
use serde::{Deserialize, Serialize};
use std::fs::read_to_string;
use std::path::Path;
use serde_with::serde_as;

/// A registry of helper-uids, that's used for protocol-specific actions
#[serde_as]
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct UidRegistry {
    #[serde_as(deserialize_as = "HexOrBase58", serialize_as = "Hex")]
    bridge_deposit: Uid,
    #[serde_as(deserialize_as = "HexOrBase58", serialize_as = "Hex")]
    bridge_withdrawal: Uid,
}

impl UidRegistry {
    pub fn get_bridge_deposit(&self) -> Uid {
        self.bridge_deposit.clone()
    }

    pub fn get_bridge_withdrawal(&self) -> Uid {
        self.bridge_withdrawal.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct NearRegistryAccount {
    account_id: AccountId,
    private_key: SecretKey,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct SecretsConfig {
    near_registry_account: NearRegistryAccount,
    mpc_auth_key: String,
    uid_registry: UidRegistry,
}

impl SecretsConfig {
    pub fn decrypt_with_prompt(encrypted_config_path: &Path) -> Result<SecretsConfig, AppError> {
        let encrypted_config_b64 = read_to_string(encrypted_config_path)
            .context("failed to read encrypted config")
            .map_err(AppError::OsError)?;
        let config_str = decrypt_config_prompt_key(encrypted_config_b64.as_str())?;
        let config = serde_yaml::from_str(&config_str)
            .map_err(anyhow::Error::from)
            .map_err(AppError::DataConversionError)?;
        Ok(config)
    }

    #[cfg(feature = "debug")]
    pub fn decrypt_with_key(
        encryption_key_path: &Path,
        encrypted_config_path: &Path,
    ) -> Result<SecretsConfig, AppError> {
        let key_b58 = read_to_string(encryption_key_path)
            .context("failed to read encryption key")
            .map_err(AppError::OsError)?;

        let key = bs58::decode(key_b58.trim())
            .into_vec()
            .context("failed to base58-decode key")
            .map_err(AppError::DataConversionError)?;

        let encrypted_config_b64 = read_to_string(encrypted_config_path)
            .context("failed to read encrypted config")
            .map_err(AppError::OsError)?;

        let config_str = decrypt_config(&key, &encrypted_config_b64)?;

        let config = serde_yaml::from_str(&config_str)
            .map_err(anyhow::Error::from)
            .map_err(AppError::DataConversionError)?;
        Ok(config)
    }
}

/// Prompts for the Base58 key (hidden input) and decrypts.
pub fn decrypt_config_prompt_key(encrypted_config_b64: &str) -> Result<String, AppError> {
    // 🔒 Hidden prompt
    let key_b58 = prompt_password("Config Key: ")
        .map_err(anyhow::Error::from)
        .map_err(AppError::OsError)?;
    let key = bs58::decode(key_b58.trim())
        .into_vec()
        .context("failed to base58-decode key")
        .map_err(AppError::DataConversionError)?;

    decrypt_config(&key, encrypted_config_b64)
}

/// Decrypt AES-CBC with PKCS#7 padding.
/// Input: base64 of (IV[16] || CIPHERTEXT)
fn decrypt_config(key: &[u8], encrypted_config_b64: &str) -> Result<String, AppError> {
    let buf = BASE64_STANDARD
        .decode(encrypted_config_b64)
        .context("failed to base64-decode encrypted payload")
        .map_err(AppError::DataConversionError)?;

    if buf.len() < 16 {
        return Err(AppError::DataConversionError(anyhow!(
            "ciphertext too short (missing 16-byte IV)"
        )));
    }

    let iv = &buf[..16];
    let mut ct = buf[16..].to_vec();

    let plaintext = match key.len() {
        16 => {
            let dec = Decryptor::<Aes128>::new_from_slices(key, iv)
                .map_err(|_| anyhow!("invalid key/iv for AES-128"))
                .map_err(AppError::DataConversionError)?;
            dec.decrypt_padded_mut::<Pkcs7>(&mut ct)
                .map_err(|_| anyhow!("AES-128 PKCS7 unpad failed"))
                .map_err(AppError::DataConversionError)?
                .to_vec()
        }
        24 => {
            let dec = Decryptor::<Aes192>::new_from_slices(key, iv)
                .map_err(|_| anyhow!("invalid key/iv for AES-192"))
                .map_err(AppError::DataConversionError)?;
            dec.decrypt_padded_mut::<Pkcs7>(&mut ct)
                .map_err(|_| anyhow!("AES-192 PKCS7 unpad failed"))
                .map_err(AppError::DataConversionError)?
                .to_vec()
        }
        32 => {
            let dec = Decryptor::<Aes256>::new_from_slices(key, iv)
                .map_err(|_| anyhow!("invalid key/iv for AES-256"))
                .map_err(AppError::DataConversionError)?;
            dec.decrypt_padded_mut::<Pkcs7>(&mut ct)
                .map_err(|_| anyhow!("AES-256 PKCS7 unpad failed"))
                .map_err(AppError::DataConversionError)?
                .to_vec()
        }
        n => {
            return Err(AppError::DataConversionError(anyhow!(
                "unsupported AES key length: {n} (expected 16/24/32)"
            )));
        }
    };

    String::from_utf8(plaintext)
        .context("decrypted bytes are not valid UTF-8")
        .map_err(AppError::DataConversionError)
}

#[cfg(test)]
mod tests {
    use aes::{Aes128, Aes192, Aes256};
    use anyhow::Result;
    use anyhow::{Context, anyhow, bail};
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;
    use cbc::Encryptor;
    use cipher::block_padding::Pkcs7;
    use cipher::{BlockEncryptMut, KeyIvInit};
    use rand::prelude::StdRng;
    use std::fs;
    use std::fs::read_to_string;

    use crate::secrets::{SecretsConfig, decrypt_config};
    use rand::{RngCore, SeedableRng};

    /// Encrypts `plaintext` with AES-CBC + PKCS#7.
    /// - Key length must be 16/24/32 bytes (AES-128/192/256).
    /// - Generates a random 16-byte IV.
    /// - Returns base64( IV || CIPHERTEXT ).
    pub fn encrypt_config(key: &[u8], plaintext: &[u8]) -> Result<String> {
        if !(key.len() == 16 || key.len() == 24 || key.len() == 32) {
            bail!(
                "unsupported AES key length: {} (expected 16/24/32)",
                key.len()
            );
        }

        // Random IV
        let mut iv = [0u8; 16];
        let mut rng = StdRng::from_os_rng();
        rng.fill_bytes(&mut iv);

        // Encrypt with PKCS#7 padding
        let ct = match key.len() {
            16 => {
                let enc = Encryptor::<Aes128>::new_from_slices(key, &iv)
                    .map_err(|_| anyhow!("invalid key/iv for AES-128"))?;
                enc.encrypt_padded_vec_mut::<Pkcs7>(plaintext)
            }
            24 => {
                let enc = Encryptor::<Aes192>::new_from_slices(key, &iv)
                    .map_err(|_| anyhow!("invalid key/iv for AES-192"))?;
                enc.encrypt_padded_vec_mut::<Pkcs7>(plaintext)
            }
            32 => {
                let enc = Encryptor::<Aes256>::new_from_slices(key, &iv)
                    .map_err(|_| anyhow!("invalid key/iv for AES-256"))?;
                enc.encrypt_padded_vec_mut::<Pkcs7>(plaintext)
            }
            _ => unreachable!(),
        };

        // Prepend IV and base64-encode
        let mut out = Vec::with_capacity(16 + ct.len());
        out.extend_from_slice(&iv);
        out.extend_from_slice(&ct);
        Ok(BASE64_STANDARD.encode(out))
    }

    const PLAIN_CONFIG_PATH: &str = "integration-tests/test-data/secrets-config.yml";
    const ENCRYPTED_CONFIG_PATH: &str = "integration-tests/test-data/secrets-config.yml.enc";
    const ENCRYPTION_KEY_PATH: &str = "integration-tests/test-data/enc.key";

    fn load_secret_key() -> Result<Vec<u8>> {
        let key_b58 = read_to_string(ENCRYPTION_KEY_PATH)?;
        let key = bs58::decode(key_b58.trim())
            .into_vec()
            .context("failed to base58-decode key")?;
        Ok(key)
    }

    fn load_encrypted_config() -> Result<String> {
        let config = read_to_string(ENCRYPTED_CONFIG_PATH)?;
        Ok(config)
    }

    fn load_config() -> Result<SecretsConfig> {
        let config_str = read_to_string(PLAIN_CONFIG_PATH)?;
        let config = serde_yaml::from_str(&config_str)?;
        Ok(config)
    }

    #[test]
    fn config_serialization() -> Result<()> {
        let config = load_config()?;
        dbg!(&config);
        Ok(())
    }

    #[test]
    fn config_encryption_round_trip() -> Result<()> {
        let config = load_config()?;
        let key = load_secret_key()?;
        let config_str = serde_yaml::to_string(&config)?;
        //
        let encrypted = encrypt_config(&key, config_str.as_bytes())?;
        fs::write(ENCRYPTED_CONFIG_PATH, encrypted)?;
        //
        let encrypted_config = load_encrypted_config()?;
        let decrypted = decrypt_config(&key, encrypted_config.as_str())?;
        let actual = serde_yaml::from_str::<SecretsConfig>(&decrypted)?;
        dbg!(&actual);
        assert_eq!(config, actual);
        Ok(())
    }
}
