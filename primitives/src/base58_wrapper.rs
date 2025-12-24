use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer, Serializer};
use serde_with::{DeserializeAs, SerializeAs};

// TODO: It can be generalized with `T: Impl AsRef<[u8]>` or something
pub struct Base58;

impl SerializeAs<Vec<u8>> for Base58 {
    fn serialize_as<S>(value: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&bs58::encode(value).into_string())
    }
}

impl<'de> DeserializeAs<'de, Vec<u8>> for Base58 {
    fn deserialize_as<D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        bs58::decode(&s)
            .into_vec()
            .map_err(serde::de::Error::custom)
    }
}

// Fixed-size arrays
pub struct Base58Array<const N: usize>;

impl<const N: usize> SerializeAs<[u8; N]> for Base58Array<N> {
    fn serialize_as<S>(value: &[u8; N], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&bs58::encode(value).into_string())
    }
}

impl<'de, I, const N: usize> DeserializeAs<'de, I> for Base58Array<N>
where
    I: From<[u8; N]>
{
    fn deserialize_as<D>(deserializer: D) -> Result<I, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = bs58::decode(&s)
            .into_vec()
            .context("failed to decode from base58")
            .map_err(serde::de::Error::custom)?;
        let arr: [u8; N] = bytes
            .try_into()
            .map_err(|v: Vec<u8>| {
                serde::de::Error::invalid_length(
                    v.len(),
                    &format!("expected {N} bytes after base58 decoding").as_str(),
                )
            })?;

        Ok(arr.into())
    }
}
