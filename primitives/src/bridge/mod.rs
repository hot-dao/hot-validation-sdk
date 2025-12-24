#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
//! Types for bridge validation, which include flows for deposit and completed withdrawal verification.

pub mod cosmos;
pub mod evm;
pub mod solana;
pub mod stellar;
pub mod ton;

use crate::Base58;
use crate::Base58Array;
use crate::ChainId;
use crate::bridge::cosmos::CosmosInputData;
use crate::bridge::solana::SolanaInputData;
use anyhow::{Result, bail};
use borsh::BorshSerialize;
use derive_more::{From, TryFrom, TryInto};
use evm::EvmInputData;
use rlp::RlpStream;
use serde::{Deserialize, Serialize};
use serde_with::DisplayFromStr;
use serde_with::serde_as;
use sha2::Digest;
use stellar::StellarInputData;
use ton::TonInputData;

#[derive(Serialize, Deserialize)]
pub enum HotVerifyBridge {
    Deposit(DepositAction),
    ClearCompletedWithdrawal(CompletedWithdrawalAction),
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct DepositAction {
    pub chain_id: ChainId,
    #[serde(flatten)]
    pub data: DepositData,
}

impl DepositAction {
    pub fn build_challenge_for_deposit(&self) -> Result<[u8; 32]> {
        let challenge = DepositData::build_challenge_for_deposit(
            self.data.get_receiver()?,
            self.chain_id,
            self.data.get_token_id()?,
            self.data.get_amount()?,
            self.data.nonce,
        );
        Ok(challenge)
    }
}

/// Many of the fields are optional, because there are different use cases for this structure.
/// You want to be those fields `Some(...)` only when building a challenge. In other cases, it's enough to have `nonce` only.
/// Note: order and types of fields should stay persistent, as it being deserialized to borsh for further
/// cryptographic processing (e.g. in Solana logic)
///
/// Most of the time optional, because it's needed for Solana only
#[serde_as]
#[derive(
    Debug, Serialize, Deserialize, BorshSerialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone,
)]
pub struct DepositData {
    #[serde_as(as = "Option<Base58Array<32>>")]
    #[schemars(with = "Option<String>")]
    #[serde(alias = "sender_id")]
    pub sender: Option<[u8; 32]>,
    #[serde_as(as = "Option<Base58Array<32>>")]
    #[schemars(with = "Option<String>")]
    #[serde(alias = "receiver_id")]
    pub receiver: Option<[u8; 32]>,
    #[serde_as(as = "Option<Base58>")]
    #[schemars(with = "Option<String>")]
    #[serde(alias = "contract_id")]
    pub token_id: Option<Vec<u8>>,
    #[serde_as(as = "Option<DisplayFromStr>")]
    #[schemars(with = "Option<String>")]
    pub amount: Option<u128>,
    #[serde_as(as = "DisplayFromStr")]
    #[schemars(with = "String")]
    pub nonce: u128,
}

impl DepositData {
    #[must_use]
    pub fn from_nonce(nonce: u128) -> Self {
        Self {
            sender: None,
            receiver: None,
            token_id: None,
            amount: None,
            nonce,
        }
    }

    fn get_sender(&self) -> Result<&[u8; 32]> {
        let sender = self
            .sender
            .as_ref()
            .ok_or(anyhow::anyhow!("Sender not set"))?;
        Ok(sender)
    }

    fn get_amount(&self) -> Result<u128> {
        let amount = self.amount.ok_or(anyhow::anyhow!("Amount not set"))?;
        Ok(amount)
    }

    fn get_receiver(&self) -> Result<&[u8; 32]> {
        let receiver = self
            .receiver
            .as_ref()
            .ok_or(anyhow::anyhow!("Receiver not set"))?;
        Ok(receiver)
    }

    fn get_token_id(&self) -> Result<&[u8]> {
        let token_id = self
            .token_id
            .as_ref()
            .ok_or(anyhow::anyhow!("Token ID not set"))?;
        Ok(token_id)
    }

    #[must_use]
    pub fn build_challenge_for_deposit(
        receiver_id: &[u8],
        chain_id: ChainId,
        contract_id: &[u8],
        amount: u128,
        nonce: u128,
    ) -> [u8; 32] {
        let mut stream = RlpStream::new_list(5);

        match chain_id {
            ChainId::Stellar | ChainId::Ton | ChainId::TON_V2 | ChainId::Evm(_) => {
                let chain_id: u64 = chain_id.into();
                stream.append(&nonce.to_be_bytes().as_ref());
                stream.append(&chain_id.to_be_bytes().as_ref());
                stream.append(&contract_id);
                stream.append(&receiver_id);
                stream.append(&amount.to_be_bytes().as_ref());
            }

            ChainId::Solana => {
                let chain_id: u64 = chain_id.into();

                // * Amounts stored as u64
                // * ChainId expected as u16

                let amount = <u64>::try_from(amount)
                    .expect("Unsuccessful downcast for amount to u64 from u128");
                let chain_id = <u16>::try_from(chain_id)
                    .expect("Unsuccessful downcast for chain_id to u16 from u64");
                stream.append(&nonce.to_be_bytes().as_ref());
                stream.append(&chain_id.to_be_bytes().as_ref());
                stream.append(&contract_id);
                stream.append(&receiver_id);
                stream.append(&amount.to_be_bytes().as_ref());
            }
            ChainId::Near => {
                unreachable!("Withdrawal serialization should not happen for Near")
            }
        }
        let data = stream.out().to_vec();
        sha2::Sha256::digest(&data).into()
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct CompletedWithdrawalAction {
    pub chain_id: ChainId,
    #[serde(flatten)]
    pub data: CompletedWithdrawal,
}

#[serde_as]
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct CompletedWithdrawal {
    #[schemars(with = "String")]
    #[serde_as(as = "DisplayFromStr")]
    pub nonce: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver_address: Option<String>,
}

impl CompletedWithdrawal {
    /// Note: This challenge binds the omni bridge nonce, thus we don't have to bind any other data
    /// to avoid collisions.
    #[must_use]
    pub fn build_challenge_for_removal(nonce: u128) -> [u8; 32] {
        let mut stream = RlpStream::new_list(2);
        stream.append(&b"CLEAR_HOT_BRIDGE_NONCE".to_vec());
        stream.append(&nonce.to_be_bytes().as_ref());
        let data = stream.out();
        sha2::Sha256::digest(&data).into()
    }

    #[must_use]
    pub fn build_challenge_for_removal_owned(&self) -> [u8; 32] {
        Self::build_challenge_for_removal(self.nonce)
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(untagged)] // for back compatability reasons, because there's at first there was a `bool` option only
pub enum HotVerifyResult {
    AuthCall(HotVerifyAuthCall),
    Result(bool),
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct HotVerifyAuthCall {
    pub contract_id: String,
    pub method: String,
    pub chain_id: ChainId,
    pub input: InputData,
}

#[derive(
    Debug,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
    Eq,
    PartialEq,
    Hash,
    Clone,
    TryInto,
    From,
)]
#[try_into(owned, ref, ref_mut)]
#[serde(untagged)]
pub enum InputData {
    Evm(EvmInputData),
    Stellar(StellarInputData),
    Ton(TonInputData),
    Solana(SolanaInputData),
    Cosmos(CosmosInputData),
}

impl HotVerifyResult {
    pub fn as_result(&self) -> Result<bool> {
        match self {
            HotVerifyResult::Result(result) => Ok(*result),
            HotVerifyResult::AuthCall(_) => {
                bail!("Expected result, got auth call")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bridge::CompletedWithdrawal;
    use rlp::RlpStream;
    use sha2::Digest;

    #[test]
    fn completed_withdrawal_challenge_consistency() {
        let nonce = 42u128;
        let mut stream = RlpStream::new_list(2);
        stream.append(&b"CLEAR_HOT_BRIDGE_NONCE".to_vec());
        stream.append(&nonce.to_be_bytes().as_ref());
        let expected = sha2::Sha256::digest(stream.out());

        let actual = CompletedWithdrawal::build_challenge_for_removal(nonce);
        assert_eq!(expected.as_slice(), actual.as_slice());
    }
}
