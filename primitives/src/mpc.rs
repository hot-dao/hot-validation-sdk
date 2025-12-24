use crate::Base58Array;
use crate::ProofModel;
use crate::uid::Uid;
use cait_sith::ecdsa::sign::FullSignature;
use cait_sith::{frost_ed25519, frost_secp256k1};
use k256::Secp256k1;
use serde::{Deserialize, Serialize};
use serde_with::hex::Hex;
use serde_with::serde_as;

pub use k256;
pub use cait_sith;

#[serde_as]
#[derive(Serialize, Deserialize)]
pub struct PublicKeyRequest {
    #[serde_as(as = "Base58Array<32>")]
    pub uid: Uid,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PublicKeyResponse {
    pub eddsa: frost_ed25519::VerifyingKey,
    pub ecdsa: k256::AffinePoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantsInfo {
    pub participants: Vec<String>,
    pub me: String,
    pub threshold: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OffchainSignatureResponse {
    Ecdsa {
        big_r: k256::AffinePoint,
        signature: k256::Scalar,
        public_key: k256::AffinePoint,
        participants: Vec<String>,
    },
    Eddsa {
        signature: frost_ed25519::Signature,
        public_key: frost_ed25519::VerifyingKey,
        participants: Vec<String>,
    },
}

#[repr(u8)]
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(into = "u8", from = "u8")]
pub enum KeyType {
    Ecdsa = 0,
    Eddsa = 1,
}

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct OffchainSignatureRequest {
    #[serde_as(as = "Hex")]
    pub uid: Uid,
    #[serde_as(as = "Hex")]
    pub message: Vec<u8>,
    pub proof: ProofModel,
    pub key_type: KeyType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub participants: Option<Vec<String>>,
}

impl From<Uid> for PublicKeyRequest {
    fn from(uid: Uid) -> Self {
        Self { uid }
    }
}

impl From<KeyType> for u8 {
    fn from(value: KeyType) -> Self {
        match value {
            KeyType::Ecdsa => 0,
            KeyType::Eddsa => 1,
        }
    }
}

impl From<u8> for KeyType {
    fn from(value: u8) -> Self {
        match value {
            0 => KeyType::Ecdsa,
            1 => KeyType::Eddsa,
            _ => panic!("Invalid curve type. Expected 0 (Ecdsa) or 1 (Ed25519)."),
        }
    }
}

impl OffchainSignatureResponse {
    #[must_use]
    pub fn from_ecdsa(
        signature: &FullSignature<Secp256k1>,
        public_key: frost_secp256k1::VerifyingKey,
        participants: Vec<String>,
    ) -> Self {
        Self::Ecdsa {
            big_r: signature.big_r,
            signature: signature.s,
            public_key: public_key.to_element().into(),
            participants,
        }
    }

    #[must_use]
    pub fn from_eddsa(
        signature: frost_ed25519::Signature,
        public_key: frost_ed25519::VerifyingKey,
        participants: Vec<String>,
    ) -> Self {
        Self::Eddsa {
            signature,
            public_key,
            participants,
        }
    }
}
