#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
//! Types for bridge validation, which include flows for deposit and completed withdrawal verification.

pub mod evm;
pub mod solana;
pub mod stellar;
pub mod ton;

use crate::Base58;
use crate::Base58Array;
use crate::ChainId;
use crate::bridge::solana::SolanaInputData;
use anyhow::{Result, bail};
use borsh::BorshSerialize;
use derive_more::{From, TryFrom, TryInto};
use evm::EvmInputData;
use rlp::RlpStream;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;
use sha2::Digest;
use stellar::StellarInputData;
use ton::TonInputData;

#[derive(Serialize, Deserialize)]
pub struct HotVerifyBridge {
    pub chain_id: ChainId,
    pub action: Action,
}

#[derive(Serialize, Deserialize)]
pub enum Action {
    Deposit(DepositData),
    ClearCompletedWithdrawal(CompletedWithdrawal),
}

/// Note: order and types of fields should stay persistent, as it being deserialized to borsh for further
/// cryptograpich processing (e.g. in Solana logic)
#[serde_as]
#[derive(
    Debug, Serialize, Deserialize, BorshSerialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone,
)]
pub struct DepositData {
    #[serde_as(as = "Base58Array<32>")]
    #[schemars(with = "String")]
    pub sender: [u8; 32],
    #[serde_as(as = "Base58Array<32>")]
    #[schemars(with = "String")]
    pub receiver: [u8; 32],
    #[serde_as(as = "Base58")]
    #[schemars(with = "String")]
    pub token_id: Vec<u8>,
    #[serde(with = "crate::integer::u128_string")]
    #[schemars(with = "String")]
    pub amount: u128,
    #[serde(with = "crate::integer::u128_string")]
    #[schemars(with = "String")]
    pub nonce: u128,
}

impl DepositData {
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
pub struct CompletedWithdrawal {
    #[schemars(with = "String")]
    #[serde(with = "crate::integer::u128_string")]
    pub nonce: u128,
    pub receiver_address: String,
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
    TryFrom,
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
