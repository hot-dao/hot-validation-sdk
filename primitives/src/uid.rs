#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

use std::fmt;
use anyhow::{Context, Result};
use derive_more::{Deref, From, Into};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

#[derive(Serialize, Deserialize, Clone, Debug, From, Into, Deref, Eq, PartialEq)]
pub struct Uid(pub [u8; 32]);

#[derive(Serialize, Deserialize, Clone, Debug, From, Into, Deref)]
pub struct WalletId(pub String);

impl fmt::Display for WalletId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Uid {
    pub fn to_wallet_id(&self) -> Result<WalletId> {
        let uid_bytes = hex::decode(&self.0).context("Failed to decode UID from hex")?;
        let sha256_bytes = Sha256::new_with_prefix(uid_bytes).finalize();
        let bs58_string = bs58::encode(sha256_bytes).into_string();
        Ok(WalletId(bs58_string))
    }

    /// Differs from `wallet_id` because of legacy decisions
    #[must_use]
    pub fn to_tweak(&self) -> [u8; 32] {
        // (!) no hex::decode
        let mut hasher = Sha256::new();
        hasher.update(&self.0);
        let mut bytes = hasher.finalize().to_vec();
        bytes.reverse(); // (!)

        bytes
            .try_into()
            .expect("sha256 hash should be 32 bytes long")
    }
}

pub mod uid_de {
    use serde::{Deserialize, Deserializer};
    use serde_with::DeserializeAs;
    use crate::uid::Uid;

    /// serde_with adapter that accepts:
    /// - 32-byte hex
    /// - 32-byte base58
    pub struct HexOrBase58;

    impl<'de> DeserializeAs<'de, Uid> for HexOrBase58 {
        fn deserialize_as<D>(d: D) -> Result<Uid, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(d)?;

            // hex
            if let Ok(b) = hex::decode(&s) {
                if b.len() == 32 {
                    return Ok(Uid(b.try_into().unwrap()));
                }
            }

            // base58
            if let Ok(b) = bs58::decode(&s).into_vec() {
                if b.len() == 32 {
                    return Ok(Uid(b.try_into().unwrap()));
                }
            }

            Err(serde::de::Error::custom("invalid uid: neither hex nor bs58 of 32 bytes"))
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::uid::uid_de::HexOrBase58;
    use serde::Deserialize;
    use serde_with::serde_as;
    use crate::uid::Uid;

    #[serde_as]
    #[derive(Deserialize)]
    struct UidWrapper(
        #[serde_as(as = "HexOrBase58")]
        Uid
    );

    #[test]
    fn uid_deserialization() {
        let uid: UidWrapper = serde_json::from_str("\"0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f\"").unwrap();
        // let str = serde_json::to_string(&uid).unwrap();
        // println!("{str}");
    }

    // #[test]
    // fn test_uid_to_tweak() {
    //     let uid =
    //         Uid("0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f".to_string());
    //     let hexed_tweak = hex::encode(uid.to_tweak());
    //     assert_eq!(
    //         hexed_tweak,
    //         "6fad344c80c6e813ecbe2ca6309c9bda422ffae0b6b6857ed30b25a7534dddba"
    //     );
    // }
    //
    // #[test]
    // fn test_uid_to_wallet_id() {
    //     let uid =
    //         Uid("0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f".to_string());
    //     let expected = "A8NpkSkn1HZPYjxJRCpD4iPhDHzP81bbduZTqPpHmEgn".to_string();
    //     let actual = uid.to_wallet_id().unwrap().0;
    //     assert_eq!(actual, expected);
    // }
}
