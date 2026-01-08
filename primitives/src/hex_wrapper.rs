use serde::{Deserialize, Deserializer, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use std::marker::PhantomData;

pub struct PrefixedHex<T = Vec<u8>>(PhantomData<T>);

impl<'de> DeserializeAs<'de, Vec<u8>> for PrefixedHex<Vec<u8>> {
    fn deserialize_as<D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        prefix_hex::decode(&s).map_err(serde::de::Error::custom)
    }
}

impl SerializeAs<Vec<u8>> for PrefixedHex<Vec<u8>> {
    fn serialize_as<S>(source: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&prefix_hex::encode(source))
    }
}

#[cfg(test)]
mod tests {
    use crate::hex_wrapper::PrefixedHex;
    use serde::{Deserialize, Serialize};
    use serde_with::serde_as;

    #[serde_as]
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Msg {
        #[serde_as(as = "PrefixedHex")]
        payload: Vec<u8>,
    }

    #[test]
    fn roundtrip_prefixed_hex() {
        let msg = Msg {
            payload: vec![0x00, 0x01, 0xAB, 0xFF],
        };

        let json = serde_json::to_string(&msg).unwrap();
        assert_eq!(json, r#"{"payload":"0x0001abff"}"#);

        let decoded: Msg = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, msg);
    }

    #[test]
    fn rejects_missing_prefix() {
        let json = r#"{"payload":"deadbeef"}"#;
        let err = serde_json::from_str::<Msg>(json).unwrap_err();

        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("0x"));
    }
}
