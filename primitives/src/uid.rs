use crate::Base58Array;
use anyhow::{Context, Result};
use derive_more::{AsRef, Deref, DerefMut, From, Into};
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{DeserializeAs, serde_as};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

#[derive(Clone, From, Into, Deref, DerefMut, Eq, PartialEq, AsRef)]
pub struct Uid(pub [u8; 32]);

impl AsRef<[u8]> for Uid {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Uid").field(&"[REDACTED]").finish()
    }
}

pub struct HexOrBase58;

impl<'de> DeserializeAs<'de, Uid> for HexOrBase58 {
    fn deserialize_as<D>(deserializer: D) -> Result<Uid, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = <&str>::deserialize(deserializer)?;
        let mut errors = Vec::new();

        // Try hex first
        match Uid::from_hex(s) {
            Ok(uid) => return Ok(uid),
            Err(e) => errors.push(format!("hex: {e:#}")),
        }

        // Try base58
        match Uid::from_bs58(s) {
            Ok(uid) => return Ok(uid),
            Err(e) => errors.push(format!("base58: {e:#}")),
        }

        Err(serde::de::Error::custom(format!(
            "Failed to parse UID from any format:\n  {}",
            errors.join("\n  ")
        )))
    }
}

impl Uid {
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let array = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Expected 32 bytes, got {}", bytes.len()))?;
        Ok(Uid(array))
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes_vec = hex::decode(s).context("Failed to decode UID from hex")?;
        let uid = Self::from_bytes(&bytes_vec)?;
        Ok(uid)
    }

    pub fn from_bs58(s: &str) -> Result<Self> {
        let bytes_vec = bs58::decode(s)
            .into_vec()
            .context("Failed to decode UID from bs58")?;
        let uid = Self::from_bytes(&bytes_vec)?;
        Ok(uid)
    }
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug, From, Into, Deref, DerefMut)]
pub struct WalletId(#[serde_as(as = "Base58Array<32>")] pub [u8; 32]);

impl fmt::Display for WalletId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", bs58::encode(**self).into_string())
    }
}

impl FromStr for WalletId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let bytes_vec = bs58::decode(s)
            .into_vec()
            .context("Failed to decode WalletId from bs58")?;
        let array: [u8; 32] = bytes_vec
            .as_slice()
            .try_into()
            .map_err(|_| anyhow::anyhow!("Expected 32 bytes, got {} bytes", bytes_vec.len()))?;
        Ok(WalletId(array))
    }
}
impl From<Uid> for WalletId {
    fn from(value: Uid) -> Self {
        let hashed = Sha256::digest(value.0).into();
        Self(hashed)
    }
}

impl Uid {
    /// Differs from `wallet_id` because of legacy decisions
    #[must_use]
    pub fn to_tweak(&self) -> [u8; 32] {
        let hexed = hex::encode(self.0);
        let mut hashed: [u8; 32] = Sha256::digest(hexed.as_bytes()).into();
        hashed.reverse();
        hashed
    }
}

#[cfg(test)]
mod tests {
    use crate::uid::Uid;
    use crate::uid::{HexOrBase58, WalletId};
    use anyhow::Result;
    use derive_more::{Deref, DerefMut, From, Into};
    use serde::{Deserialize, Serialize};
    use serde_with::hex::Hex;
    use serde_with::serde_as;

    const UID_HEX: &str = "0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f";
    const UID_BS58: &str = "2rgKUfdGTErcyrYHso4ipyN6LRAqKTkqzP4LoNBQ3xsX";
    const TWEAK_HEX: &str = "6fad344c80c6e813ecbe2ca6309c9bda422ffae0b6b6857ed30b25a7534dddba";
    const WALLET_ID_HEX: &str = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn";

    #[serde_as]
    #[derive(Serialize, Deserialize, Into, From, Deref, DerefMut)]
    struct UidWrapper(#[serde_as(deserialize_as = "HexOrBase58", serialize_as = "Hex")] Uid);

    #[test]
    fn test_se_uid_into_hex() -> Result<()> {
        let uid = Uid::from_hex(UID_HEX)?;
        let wrapper: UidWrapper = uid.into();
        let json = serde_json::to_string(&wrapper)?;
        println!("{json}");
        Ok(())
    }

    #[test]
    fn test_de_uid_from_hex() -> Result<()> {
        let _: UidWrapper = serde_json::from_str(&format!("\"{UID_HEX}\""))?;
        Ok(())
    }

    #[test]
    fn test_de_uid_from_bs58() -> Result<()> {
        let _: UidWrapper = serde_json::from_str(&format!("\"{UID_BS58}\""))?;
        Ok(())
    }

    #[test]
    fn test_uid_debug_redacted() {
        let uid = Uid([0; 32]);
        let debug_output = format!("{uid:?}");
        assert!(debug_output.contains("REDACTED"));
    }

    #[test]
    fn test_tweak_consistency() -> Result<()> {
        let uid = Uid::from_hex(UID_HEX)?;
        let tweak = uid.to_tweak();

        assert_eq!(hex::encode(tweak), TWEAK_HEX);

        Ok(())
    }

    #[test]
    fn test_wallet_id_consistency() -> Result<()> {
        let uid = Uid::from_hex(UID_HEX)?;
        let wallet_ud = WalletId::from(uid);
        dbg!(wallet_ud.to_string());

        assert_eq!(bs58::encode(*wallet_ud).into_string(), WALLET_ID_HEX,);

        Ok(())
    }
}
