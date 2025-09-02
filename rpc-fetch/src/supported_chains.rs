use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, EnumIter)]
#[serde(try_from = "u64", into = "u64")]
pub enum ChainId {
    Near = 0,
    Eth = 1,
    Optimism = 10,
    Bsc = 56,
    Polygon = 137,
    MonadTestnet = 143,
    ZkSync = 324,
    Stellar = 1100,
    Kava = 2222,
    Base = 8453,
    Arbitrum = 42161,
    Avax = 43114,
    BeraChain = 80094,
    Scroll = 534352,
    Aurora = 1313161554,
}

pub trait SlugFromChainId {
    fn slug(chain_id: ChainId) -> Option<String>;
}

impl From<ChainId> for u64 {
    fn from(c: ChainId) -> Self {
        c as u64
    }
}

impl TryFrom<u64> for ChainId {
    type Error = &'static str;
    fn try_from(v: u64) -> Result<Self, Self::Error> {
        use ChainId::*;
        Ok(match v {
            0 => Near,
            1 => Eth,
            10 => Optimism,
            56 => Bsc,
            137 => Polygon,
            143 => MonadTestnet,
            324 => ZkSync,
            1100 => Stellar,
            2222 => Kava,
            8453 => Base,
            42161 => Arbitrum,
            43114 => Avax,
            80094 => BeraChain,
            534352 => Scroll,
            1313161554 => Aurora,
            _ => return Err("unknown chain id"),
        })
    }
}
