//! Validation on TON happens in 2 steps.
//! Suppose we do deposit verification (funds being put into the treasury on a target chain):
//!     1. We call the treasury with the specified nonce
//!     2. Treasury returns a child contract which stores the data regarding the nonce
//!     3. We call the child contract to verify the proof.
//! This is because there's no developer-friendly hash-map support on TON at the moment.
//!
//! This logic implements TOP API V2 data format for `runGetMethod`: <https://toncenter.com/api/v2>/

use anyhow::Context;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use derive_more::{Deref, From};
use serde::ser::{SerializeMap, SerializeTuple};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::json;
use std::str::FromStr;
use tonlib_core::TonAddress;
use tonlib_core::cell::{ArcCell, Cell, CellBuilder};
use tonlib_core::tlb_types::tlb::TLB;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct TonInputData {
    pub treasury_call_args: Vec<StackItem>,
    pub child_call_method: String,
    pub child_call_args: Vec<StackItem>,
    pub action: Action,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub enum Action {
    Deposit,
    CheckCompletedWithdrawal {
        nonce: String, // todo: Replace with u128 wrapper
    },
}

#[derive(Debug, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub enum StackItem {
    Cell(SerializableCell),
    Slice(SerializableCell),
    Num(String),
}

#[derive(Debug, schemars::JsonSchema, Eq, PartialEq, Hash, Clone)]
pub struct ResponseStackItem(pub StackItem);

#[derive(Debug, Clone, Deref, From, schemars::JsonSchema, Eq, PartialEq, Hash)]
pub struct SerializableCell(#[schemars(with = "String")] pub ArcCell);

/// The type still has to implement `Deserialize`, even though we supply our own deserializer.
impl<'a> Deserialize<'a> for SerializableCell {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        #[derive(Deserialize)]
        struct DataField {
            b64: String,
            len: usize,
        }

        #[derive(Deserialize)]
        struct Helper {
            data: DataField,
            special: bool,
        }

        let helper = Helper::deserialize(deserializer)?;

        let bytes = BASE64_STANDARD
            .decode(&helper.data.b64)
            .map_err(de::Error::custom)?;

        Ok(SerializableCell(ArcCell::new(
            Cell::new(bytes, helper.data.len, vec![], helper.special).map_err(de::Error::custom)?,
        )))
    }
}
impl Serialize for SerializableCell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry(
            "data",
            &json!({
                "b64": BASE64_STANDARD.encode(self.data()),
                "len": self.bit_len()
            }),
        )?;
        map.serialize_entry("refs", &json!([]))?;
        map.serialize_entry("special", &self.0.is_exotic())?;
        map.end()
    }
}

impl StackItem {
    #[must_use]
    pub fn from_nonce(nonce: String) -> Self {
        StackItem::Num(nonce)
    }

    pub fn from_proof(proof: String) -> anyhow::Result<Self> {
        let bytes = hex::decode(proof)?;
        let cell = CellBuilder::new().store_slice(&bytes)?.build()?;

        Ok(StackItem::Slice(SerializableCell(ArcCell::new(cell))))
    }

    pub fn from_address(address: &str) -> anyhow::Result<Self> {
        let address = TonAddress::from_str(address)?;
        let cell = CellBuilder::new().store_address(&address)?.build()?;
        Ok(StackItem::Slice(SerializableCell(ArcCell::new(cell))))
    }
}

impl Serialize for StackItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tup = serializer.serialize_tuple(2)?;
        match self {
            StackItem::Cell(cell) => {
                tup.serialize_element("cell")?;
                tup.serialize_element(&serde_json::to_string(&cell).unwrap())?;
            }
            StackItem::Slice(slice) => {
                tup.serialize_element("slice")?;
                tup.serialize_element(&serde_json::to_string(&slice).unwrap())?;
            }
            StackItem::Num(num) => {
                tup.serialize_element("num")?;
                tup.serialize_element(num)?;
            }
        }
        tup.end()
    }
}

impl<'de> Deserialize<'de> for ResponseStackItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (tag, val): (String, serde_json::Value) = Deserialize::deserialize(deserializer)?;

        match tag.as_str() {
            "cell" | "slice" => {
                let expected = {
                    let bytes = val["bytes"]
                        .as_str()
                        .ok_or(de::Error::custom("missing bytes field"))?;
                    Cell::from_boc_b64(bytes).map_err(de::Error::custom)?
                };
                let actual = {
                    SerializableCell::deserialize(val["object"].clone())
                        .context("Error deserializing object field")
                        .map_err(de::Error::custom)?
                };

                if expected.data() != actual.data() {
                    Err(de::Error::custom("cell data mismatch"))?;
                }
                match tag.as_str() {
                    "cell" => Ok(ResponseStackItem(StackItem::Cell(actual))),
                    "slice" => Ok(ResponseStackItem(StackItem::Slice(actual))),
                    &_ => unreachable!(),
                }
            }
            "num" => {
                let num: String = Deserialize::deserialize(val).map_err(de::Error::custom)?;
                Ok(ResponseStackItem(StackItem::Num(num)))
            }
            other => Err(de::Error::custom(format!("unexpected tag: {other}"))),
        }
    }
}

impl<'de> Deserialize<'de> for StackItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let (tag, val): (String, String) = Deserialize::deserialize(deserializer)?;

        match tag.as_str() {
            "cell" => {
                let cell: SerializableCell =
                    serde_json::from_str(&val).map_err(de::Error::custom)?;
                Ok(StackItem::Cell(cell))
            }
            "slice" => {
                let cell: SerializableCell =
                    serde_json::from_str(&val).map_err(de::Error::custom)?;
                Ok(StackItem::Slice(cell))
            }
            "num" => Ok(StackItem::Num(val)),
            other => Err(de::Error::custom(format!("unexpected tag: {other}"))),
        }
    }
}

impl StackItem {
    pub const SUCCESS_NUM: &'static str = "-0x1";

    pub fn as_num(&self) -> anyhow::Result<String> {
        match self {
            StackItem::Num(n) => Ok(n.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a number")),
        }
    }

    pub fn as_slice(&self) -> anyhow::Result<SerializableCell> {
        match self {
            StackItem::Slice(s) => Ok(s.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a slice")),
        }
    }

    pub fn as_cell(&self) -> anyhow::Result<SerializableCell> {
        match self {
            StackItem::Cell(cell) => Ok(cell.clone()),
            _ => Err(anyhow::anyhow!("stack item is not a cell")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::bridge::ton::{ResponseStackItem, SerializableCell, StackItem, TonInputData};
    use serde_json::json;

    #[test]
    fn foo() -> anyhow::Result<()> {
        let json = json!({"action":"Deposit","child_call_args":[["slice","{\"data\":{\"b64\":\"vLFDgo9k1+S/C2qOZqKi0DyRbBbp6QNEGa53i59pnTw=\",\"len\":256},\"refs\":[],\"special\":false}"]],"child_call_method":"verify_withdraw","treasury_call_args":[["num","1753218716000000003679"]]});
        serde_json::from_value::<TonInputData>(json)?;
        Ok(())
    }

    #[test]
    fn test_serializable_cell() -> anyhow::Result<()> {
        let expected = json!({
            "data": {
                "b64": "gAQYKQtIs4Kd9grqKiN2ziG+W0y5hrGVKV6JtrrPvZ8+QA==",
                "len": 267
            },
            "refs": [],
            "special": false,
        });
        let item: SerializableCell = serde_json::from_value(expected.clone())?;
        dbg!(&item);
        let actual = serde_json::to_value(&item)?;
        assert_eq!(&expected, &actual);
        Ok(())
    }

    #[test]
    fn test_response_stack_item_cell_deserialize() -> anyhow::Result<()> {
        let expected = json!([
            "cell",
            {
                "bytes": "te6cckEBAQEAJAAAQ4AEGCkLSLOCnfYK6iojds4hvltMuYaxlSleiba6z72fPlDajt4V",
                "object": {
                    "data": {
                        "b64": "gAQYKQtIs4Kd9grqKiN2ziG+W0y5hrGVKV6JtrrPvZ8+QA==",
                        "len": 267
                    },
                    "refs": [],
                    "special": false,
                }
            }
        ]);
        let item: ResponseStackItem = serde_json::from_value(expected.clone())?;
        dbg!(&item);
        Ok(())
    }

    #[test]
    fn test_response_stack_item_cell_serialize() -> anyhow::Result<()> {
        let object = json!({
            "data": {
                "b64": "gAQYKQtIs4Kd9grqKiN2ziG+W0y5hrGVKV6JtrrPvZ8+QA==",
                "len": 267
            },
            "refs": [],
            "special": false,
        });
        let json = json!([
            "cell",
            {
                "bytes": "te6cckEBAQEAJAAAQ4AEGCkLSLOCnfYK6iojds4hvltMuYaxlSleiba6z72fPlDajt4V",
                "object": object,
            }
        ]);
        serde_json::from_value::<ResponseStackItem>(json.clone())?;
        Ok(())
    }

    #[test]
    fn test_stack_item_num() -> anyhow::Result<()> {
        let expected = json!(["num", "-0x1"]);
        let item: StackItem = serde_json::from_value(expected.clone())?;
        dbg!(&item);
        let actual = serde_json::to_value(item.as_num().unwrap())?;
        assert_eq!(&expected.as_array().unwrap()[1], &actual);
        Ok(())
    }
}
