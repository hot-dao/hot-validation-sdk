#![allow(clippy::unreadable_literal)]
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use strum_macros::EnumIter;

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

/// Richer description of the chains. This is used in logs/metrics.
/// We can not interchange it with the existing `ChainId`, because of the legacy: `ChainId` is being stored
/// as the contract state, and there's no easy way to migrate
#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter)]
#[serde(try_from = "u64", into = "u64")]
pub enum ExtendedChainId {
    Near = 0,
    Eth = 1,
    Optimism = 10,
    Flare = 14,
    Bsc = 56,
    Polygon = 137,
    XLayer = 196,
    ZkSync = 324,
    HyperEVM = 999,
    Solana = 1001,
    Stellar = 1100,
    Ton = 1117,
    Kava = 2222,
    Abstract = 2741,
    Mantle = 5000,
    Kaia = 8217,
    Base = 8453,
    MonadTestnet = 10143,
    Arbitrum = 42161,
    Avax = 43114,
    Ink = 57073,
    Linea = 59144,
    BeraChain = 80094,
    Scroll = 534352,
    Aurora = 1313161554,
}

impl Display for ExtendedChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}") // uses the Debug name ("Near", "Eth", etc.)
    }
}

impl From<ExtendedChainId> for ChainId {
    fn from(value: ExtendedChainId) -> Self {
        match value {
            ExtendedChainId::Near => ChainId::Near,
            ExtendedChainId::Stellar => ChainId::Stellar,
            ExtendedChainId::Solana => ChainId::Solana,
            ExtendedChainId::Ton => ChainId::TON_V2,
            _ => ChainId::Evm(value.into()),
        }
    }
}

impl TryFrom<ChainId> for ExtendedChainId {
    type Error = String;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        let id = <u64>::from(value);
        ExtendedChainId::try_from(id).map_err(|_| format!("unknown chain id: {id}"))
    }
}

impl From<ExtendedChainId> for u64 {
    fn from(c: ExtendedChainId) -> Self {
        c as u64
    }
}

impl TryFrom<u64> for ExtendedChainId {
    type Error = &'static str;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        use ExtendedChainId::*;
        Ok(match v {
            0 => Near,
            1 => Eth,
            10 => Optimism,
            14 => Flare,
            56 => Bsc,
            137 => Polygon,
            196 => XLayer,
            324 => ZkSync,
            999 => HyperEVM,
            1001 => Solana,
            1100 => Stellar,
            1117 => Ton,
            2222 => Kava,
            2741 => Abstract,
            5000 => Mantle,
            8217 => Kaia,
            8453 => Base,
            57073 => Ink,
            59144 => Linea,
            10143 => MonadTestnet,
            42161 => Arbitrum,
            43114 => Avax,
            80094 => BeraChain,
            534352 => Scroll,
            1313161554 => Aurora,
            _ => return Err("unknown chain id "),
        })
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
