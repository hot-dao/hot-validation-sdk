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

    #[must_use]
    pub fn is_cosmos(&self) -> bool {
        let chain_id: u64 = (*self).into();
        chain_id.to_string().starts_with("4444")
    }
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

/// Define a u64-discriminant enum plus reverse `TryFrom<u64>` without code duplication.
/// Works in `no_std` (no allocation). You can attach derives/serde via the header attrs.
#[macro_export]
macro_rules! define_u64_enum_with_reverse {
    (
        $(#[$meta:meta])*
        $vis:vis enum $Name:ident {
            $($Var:ident = $val:expr),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u64)]
        $vis enum $Name {
            $($Var = $val),+
        }

        impl ::core::convert::From<$Name> for u64 {
            #[inline]
            fn from(v: $Name) -> Self {
                v as u64
            }
        }

        impl ::core::convert::TryFrom<u64> for $Name {
            type Error = &'static str;

            #[inline]
            fn try_from(v: u64) -> ::core::result::Result<Self, Self::Error> {
                match v {
                    $($val => Ok($Name::$Var),)+
                    _ => Err("unknown chain id"),
                }
            }
        }
    }
}

define_u64_enum_with_reverse! {
    /// Richer description of the chains. This is used in logs/metrics.
    /// We can not interchange it with the existing `ChainId`, because of the legacy: `ChainId` is being stored
    /// as the contract state, and there's no easy way to migrate
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter)]
    #[serde(try_from = "u64", into = "u64")]
    pub enum ExtendedChainId {
        Near = 0,
        Eth = 1,
        Optimism = 10,
        Flare = 14,
        Bsc = 56,
        Polygon = 137,
        MonadMainnet = 143,
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
        ADI = 36900,
        Arbitrum = 42161,
        Avax = 43114,
        Ink = 57073,
        Linea = 59144,
        BeraChain = 80094,
        Scroll = 534352,
        Juno = 4444_118,
        Gonka = 4444_119,
        Aurora = 1313161554,
    }
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

impl ExtendedChainId {
    /// A list of chains that can roll-back blocks after they've been transmitted (mem pool -> latest),
    /// but they are not yet "safe" nor "finalized".
    #[must_use]
    pub const fn can_reorg(self) -> bool {
        match self {
            // ✅ Probabilistic-finality or L2 rollups
            Self::Eth
            | Self::Optimism
            | Self::Base
            | Self::Polygon
            | Self::Arbitrum
            | Self::ZkSync
            | Self::Linea
            | Self::Mantle
            | Self::MonadMainnet
            | Self::MonadTestnet
            | Self::Flare => true,

            // 🧱 BFT / PoA / deterministic
            Self::Near
            | Self::ADI
            | Self::Bsc
            | Self::Avax
            | Self::Kaia
            | Self::Kava
            | Self::Aurora
            | Self::XLayer
            | Self::BeraChain
            | Self::Abstract
            | Self::HyperEVM
            | Self::Stellar
            | Self::Ton
            | Self::Solana
            | Self::Scroll
            | Self::Juno
            | Self::Gonka
            | Self::Ink => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ChainId, ExtendedChainId};
    use strum::IntoEnumIterator;

    #[test]
    fn test_cosmos_conversion() {
        let chain_id: ChainId = ExtendedChainId::Gonka.into();
        assert!(chain_id.is_cosmos());
    }

    #[test]
    fn extended_chain_id_roundtrip() {
        for chain_id in ExtendedChainId::iter() {
            let serialized: u64 = chain_id.into();
            let deserialized: ExtendedChainId = serialized.try_into().unwrap();
            assert_eq!(chain_id, deserialized);
        }
    }

    #[test]
    fn chain_id_display() {
        assert_eq!(ChainId::Near.to_string(), "0");
        assert_eq!(ChainId::Stellar.to_string(), "1100");
        assert_eq!(ChainId::Evm(5).to_string(), "5");
    }
}
