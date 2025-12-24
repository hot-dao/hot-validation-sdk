//! A helper to serialize u128 with enclosed quoutes (i.e. serialize it as a string).
//! It's necessary when dealing with NEAR RPC API, because json can't handle number outside u64 range.
//! This is the same as `near_sdk::U128`, but we can't import the latter because it brings `get_rand` dependency.
//! todo: use `#[serde_as(as = "DisplayFromStr")]`
use serde::{Deserialize, Deserializer, Serializer};
use serde_with::{DeserializeAs, SerializeAs};

pub struct U128String;

impl SerializeAs<u128> for U128String {
    fn serialize_as<S>(source: &u128, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&source.to_string())
    }
}

impl<'de> DeserializeAs<'de, u128> for U128String {
    fn deserialize_as<D>(deserializer: D) -> Result<u128, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u128>().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_with::serde_as;

    #[serde_as]
    #[derive(Serialize, Deserialize)]
    struct Foo {
        #[serde_as(as = "crate::integer::U128String")]
        a: u128,
    }

    #[test]
    fn test_number_enclosed_with_quotes() {
        let foo = Foo { a: 123 };
        let x = serde_json::to_string(&foo).unwrap();
        assert_eq!(x, r#"{"a":"123"}"#);
    }
}
