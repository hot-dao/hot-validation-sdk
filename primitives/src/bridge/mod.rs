#![allow(clippy::missing_errors_doc)]
//! Types for bridge validation, which include flows for deposit and completed withdrawal verification.

pub mod evm;
pub mod solana;
pub mod stellar;
pub mod ton;

use crate::ChainId;
use crate::bridge::solana::SolanaInputData;
use anyhow::{Result, bail};
use derive_more::{From, TryFrom, TryInto};
use evm::EvmInputData;
use serde::{Deserialize, Serialize};
use stellar::StellarInputData;
use ton::TonInputData;

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

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct HotVerifyAuthCall {
    pub contract_id: String,
    pub method: String,
    pub chain_id: ChainId,
    pub input: InputData,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(untagged)] // for back compatability reasons
pub enum HotVerifyResult {
    AuthCall(HotVerifyAuthCall),
    Result(bool),
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
