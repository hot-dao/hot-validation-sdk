use std::fmt::Display;

#[derive(
    Clone,
    Copy,
    Debug,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    Ord,
    PartialEq,
    PartialOrd,
    Hash,
)]
#[cfg_attr(
    feature = "abi",
    derive(borsh::BorshSerialize, borsh::BorshDeserialize, borsh::BorshSchema)
)]
#[serde(into = "u64", from = "u64")]
pub enum ChainId {
    Near,
    Solana,
    Ton,
    Stellar,
    Evm(u64),
}

impl ChainId {
    /// Note: it should always go before EVM branch when pattern matching
    pub const TON_V2: Self = Self::Evm(1117);
}

impl Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", <u64>::from(*self))
    }
}

impl From<ChainId> for u64 {
    fn from(value: ChainId) -> Self {
        match value {
            ChainId::Near => 0,
            ChainId::Solana => 1001,
            ChainId::Ton => 1111,
            ChainId::Stellar => 1100,
            ChainId::Evm(value) => value,
        }
    }
}

impl From<u64> for ChainId {
    fn from(value: u64) -> Self {
        match value {
            0 => ChainId::Near,
            1001 => ChainId::Solana,
            1100 => ChainId::Stellar,
            1111 => ChainId::Ton,
            _ => ChainId::Evm(value),
        }
    }
}

#[test]
fn chain_id_roundtrip() {
    assert_eq!(ChainId::from(0u64), ChainId::Near);
    assert_eq!(ChainId::from(1100u64), ChainId::Stellar);
    assert_eq!(ChainId::from(42u64), ChainId::Evm(42));

    assert_eq!(u64::from(ChainId::Near), 0u64);
    assert_eq!(u64::from(ChainId::Stellar), 1100u64);
    assert_eq!(u64::from(ChainId::Evm(7)), 7u64);
}

#[test]
fn chain_id_display() {
    assert_eq!(ChainId::Near.to_string(), "0");
    assert_eq!(ChainId::Stellar.to_string(), "1100");
    assert_eq!(ChainId::Evm(5).to_string(), "5");
}
