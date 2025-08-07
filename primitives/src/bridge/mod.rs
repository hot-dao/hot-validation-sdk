//! Types for bridge validation, which include flows for deposit and withdrawal.

pub mod evm;
pub mod stellar;

use crate::ChainId;
use anyhow::{Result, bail};
use derive_more::{TryFrom, TryInto};
use serde::{Deserialize, Serialize};

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
)]
#[try_into(owned, ref, ref_mut)]
#[serde(untagged)]
pub enum InputData {
    Evm(evm::EvmInputData),
    Stellar(stellar::StellarInputData),
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct HotVerifyAuthCall {
    pub contract_id: String,
    pub method: String,
    pub chain_id: ChainId,
    pub input: InputData,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(untagged)]
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
