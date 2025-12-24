use crate::integer::U128String;
use serde::{Deserialize, Serialize};
use serde_hex::SerHexSeq;
use serde_hex::StrictPfx;
use serde_with::serde_as;
use stellar_xdr::curr::{Limited, Limits, ReadXdr, ScBytes, ScString, ScVal, UInt128Parts};

#[serde_as]
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
#[serde(tag = "type", content = "value")]
pub enum StellarInputArg {
    #[serde(rename = "string")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    String(Vec<u8>),
    #[serde(rename = "bytes")]
    #[serde(with = "SerHexSeq::<StrictPfx>")]
    #[schemars(with = "[u8; 32]")]
    Bytes(Vec<u8>),
    #[serde(rename = "u128")]
    #[schemars(with = "String")]
    U128(#[serde_as(as = "U128String")] u128),
}

impl TryFrom<StellarInputArg> for ScVal {
    type Error = anyhow::Error;

    fn try_from(value: StellarInputArg) -> Result<Self, anyhow::Error> {
        match value {
            StellarInputArg::String(data) => Ok(ScVal::String(ScString(data.try_into()?))),
            StellarInputArg::Bytes(data) => Ok(ScVal::Bytes(ScBytes(data.try_into()?))),
            StellarInputArg::U128(data) => {
                let bytes = data.to_be_bytes();
                let mut limited = Limited::new(bytes.as_slice(), Limits::none());
                Ok(ScVal::U128(UInt128Parts::read_xdr(&mut limited)?))
            }
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

#[cfg(test)]
mod tests {
    use crate::bridge::stellar::{StellarInputArg, StellarInputData};
    use anyhow::Result;
    use stellar_xdr::curr::ScVal;

    #[test]
    fn check_u128() -> Result<()> {
        let x: ScVal = StellarInputArg::U128(((1 << 16) - 1) << 56).try_into()?;
        let ScVal::U128(parts) = x else { panic!() };
        assert_eq!(parts.hi, (1 << 8) - 1);
        assert_eq!(parts.lo, ((1 << 8) - 1) << 56);
        Ok(())
    }

    #[test]
    fn check_input_data() -> Result<()> {
        let x = r#"
        [{"type":"string","value":""},{"type":"bytes","value":"0x000000000000005f1d038ae3e890ca50c9a9f00772fcf664b4a8fefb93170d1a6f0e9843a2a816797bab71b6a99ca881"}]
        "#.to_string();
        serde_json::from_str::<StellarInputData>(&x)?;
        Ok(())
    }
}
