use alloy_dyn_abi::DynSolValue;
use alloy_primitives::U256;
use alloy_sol_types::Word;
use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_hex::{SerHexSeq, StrictPfx};

#[derive(Debug, Serialize, Deserialize, JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(tag = "type", content = "value")]
pub enum EvmInputArg {
    #[serde(rename = "bytes32")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    FixedBytes(Vec<u8>),
    #[serde(rename = "bytes")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8]")]
    Bytes(Vec<u8>),
    #[serde(rename = "uint128")]
    #[serde(with = "crate::integer::u128_string")]
    #[schemars(with = "String")]
    Uint(u128),
}

impl From<EvmInputArg> for DynSolValue {
    fn from(arg: EvmInputArg) -> Self {
        match arg {
            EvmInputArg::FixedBytes(bytes) => DynSolValue::FixedBytes(Word::from_slice(&bytes), 32),
            EvmInputArg::Bytes(bytes) => DynSolValue::Bytes(bytes),
            EvmInputArg::Uint(value) => DynSolValue::Uint(U256::from(value), 128),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct EvmInputData(pub Vec<EvmInputArg>);

impl EvmInputData {
    pub fn from_parts(message_hex: String, user_payload: String) -> Result<Self> {
        Ok(Self(vec![
            EvmInputArg::FixedBytes(hex::decode(message_hex)?),
            EvmInputArg::Bytes(Vec::new()),
            EvmInputArg::Bytes(hex::decode(user_payload)?),
            EvmInputArg::Bytes(Vec::new()),
        ]))
    }
}

impl From<EvmInputData> for Vec<DynSolValue> {
    fn from(data: EvmInputData) -> Self {
        data.0.into_iter().map(DynSolValue::from).collect()
    }
}
