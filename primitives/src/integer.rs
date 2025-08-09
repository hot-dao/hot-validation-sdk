use serde::{Deserialize, Deserializer, Serializer};
use std::str::FromStr;

pub mod u128_string {
    pub use super::{deserialize, serialize};
}

pub fn serialize<S>(x: &u128, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&x.to_string())
}

pub fn deserialize<'de, D>(d: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(d)?;
    u128::from_str(&s).map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Foo {
        #[serde(with = "crate::integer::u128_string")]
        a: u128,
    }

    #[test]
    fn bar() {
        let foo = Foo { a: 123 };
        let x = serde_json::to_string(&foo).unwrap();
        assert_eq!(x, r#"{"a":"123"}"#);
    }
}
