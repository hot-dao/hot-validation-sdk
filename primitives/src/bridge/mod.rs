#![allow(clippy::missing_errors_doc)]
//! Types for bridge validation, which include flows for deposit and withdrawal.

pub mod evm;
pub mod stellar;
pub mod ton;
pub mod solana;

use crate::ChainId;
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

#[cfg(test)]
mod tests {
    use crate::bridge::{HotVerifyAuthCall, InputData};
    use serde_json::json;

    #[test]
    fn foo() -> anyhow::Result<()> {
        let input = json!({
            "treasury_call_args":[
                ["num","1753218716000000003679"]
            ],
            "child_call_method":"verify_withdraw",
            "child_call_args": [
                ["slice","{\"data\":{\"b64\":\"vLFDgo9k1+S/C2qOZqKi0DyRbBbp6QNEGa53i59pnTw=\",\"len\":256},\"refs\":[],\"special\":false}"]
            ],
            "action":"Deposit",
        });
        serde_json::from_value::<InputData>(input)?;

        let json = json!({
            "chain_id": 1117,
            "contract_id":"EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ",
            "input": {
                "action":"Deposit",
                "child_call_args": [
                    ["slice","{\"data\":{\"b64\":\"vLFDgo9k1+S/C2qOZqKi0DyRbBbp6QNEGa53i59pnTw=\",\"len\":256},\"refs\":[],\"special\":false}"]
                ],
                "child_call_method":"verify_withdraw",
                "treasury_call_args":[["num","1753218716000000003679"]]
            }
        });

        serde_json::from_value::<HotVerifyAuthCall>(json)?;

        Ok(())
    }
}
