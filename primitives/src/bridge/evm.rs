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
#[serde(transparent)]
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

#[cfg(test)]
mod tests {
    use serde::de::DeserializeOwned;
    use serde_json::{json, Deserializer, Value};
    use crate::bridge::evm::{EvmInputArg, EvmInputData};
    use crate::bridge::{HotVerifyAuthCall, HotVerifyResult};

    #[test]
    fn bytes_and_bytes32_take_0x_hex() {
        // bytes
        let v: super::EvmInputArg = serde_json::from_str(r#"{ "type":"bytes", "value":"0x74657374" }"#).unwrap();
        match v { super::EvmInputArg::Bytes(b) => assert_eq!(b, b"test"), _ => panic!() }

        // bytes32 (short -> ok, will be padded later when converting to DynSolValue)
        let v: super::EvmInputArg = serde_json::from_str(r#"{ "type":"bytes32", "value":"0x74657374" }"#).unwrap();
        match v { super::EvmInputArg::FixedBytes(b) => assert_eq!(b, b"test"), _ => panic!() }
    }

    #[test]
    fn check_evm_bridge_validation_format() {
        let input = r#"[{"type":"bytes32","value":"0x00"}]"#;
        let input: Vec<EvmInputArg> = serde_json::from_str(input).unwrap();

        let input = json!([
         {
           "type": "bytes32",
           "value": "0x74657374"
         },
         {
           "type": "bytes",
           "value": "0x5075766b334752376276426d4a71673253647a73344432414647415733725871396977704a7261426b474a"
         },
         {
           "type": "bytes",
           "value": "0x000000000000000000000000000000000000000000000000000000000001d97c00"
         },
         {
           "type": "bytes",
           "value": "0x00"
         }
        ]);

        serde_json::from_str::<Vec<EvmInputArg>>(&input.to_string()).unwrap();
        serde_json::from_str::<EvmInputData>(&input.clone().to_string()).unwrap();

        let x = json!({
            "chain_id": 56,
            "contract_id": "0x233c5370CCfb3cD7409d9A3fb98ab94dE94Cb4Cd",
            "input": input,
            "method": "hot_verify"
        });
        serde_json::from_str::<HotVerifyAuthCall>(&x.to_string()).unwrap();
        serde_json::from_str::<HotVerifyResult>(&x.to_string()).unwrap();
    }
}
