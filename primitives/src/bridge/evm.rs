use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_hex::SerHexSeq;
use serde_hex::StrictPfx;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(tag = "type", content = "value")]
pub enum EvmInputArg {
    #[serde(rename = "bytes32")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    FixedBytes(Vec<u8>),
    #[serde(rename = "bytes")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    Bytes(Vec<u8>),
}

impl From<EvmInputArg> for ethabi::Token {
    fn from(value: EvmInputArg) -> Self {
        match value {
            EvmInputArg::FixedBytes(data) => ethabi::Token::FixedBytes(data),
            EvmInputArg::Bytes(data) => ethabi::Token::Bytes(data),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct EvmInputData(pub Vec<EvmInputArg>);

impl EvmInputData {
    pub fn from_parts(message_hex: String, user_payload: String) -> Result<Self> {
        let result = EvmInputData(vec![
            EvmInputArg::FixedBytes(hex::decode(message_hex)?),
            EvmInputArg::Bytes(vec![]),
            EvmInputArg::Bytes(hex::decode(user_payload)?),
            EvmInputArg::Bytes(vec![]),
        ]);
        Ok(result)
    }
}

impl From<EvmInputData> for Vec<ethabi::Token> {
    fn from(value: EvmInputData) -> Self {
        value.0.into_iter().map(|v| v.into()).collect()
    }
}
