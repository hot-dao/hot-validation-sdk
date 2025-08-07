use serde::{Deserialize, Serialize};
use serde_hex::SerHexSeq;
use serde_hex::StrictPfx;
use stellar_xdr::curr::{ScBytes, ScString, ScVal};

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(tag = "type", content = "value")]
pub enum StellarInputArg {
    #[serde(rename = "string")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    String(Vec<u8>),
    #[serde(rename = "bytes")]
    #[schemars(with = "[u8; 32]")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    Bytes(Vec<u8>),
}

impl TryFrom<StellarInputArg> for ScVal {
    type Error = anyhow::Error;

    fn try_from(value: StellarInputArg) -> std::result::Result<Self, anyhow::Error> {
        match value {
            StellarInputArg::String(data) => Ok(ScVal::String(ScString(data.try_into()?))),
            StellarInputArg::Bytes(data) => Ok(ScVal::Bytes(ScBytes(data.try_into()?))),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct StellarInputData(pub Vec<StellarInputArg>);

impl StellarInputData {
    pub fn from_parts(msg_hash: String, user_payload: String) -> anyhow::Result<Self> {
        Ok(Self(vec![
            StellarInputArg::String(hex::decode(msg_hash)?),
            StellarInputArg::Bytes(hex::decode(user_payload)?),
        ]))
    }
}

impl TryFrom<StellarInputData> for Vec<ScVal> {
    type Error = anyhow::Error;

    fn try_from(value: StellarInputData) -> std::result::Result<Self, anyhow::Error> {
        value.0.into_iter().map(TryFrom::try_from).collect()
    }
}
