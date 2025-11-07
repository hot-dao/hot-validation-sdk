use serde_hex::SerHex;
use serde_hex::SerHexSeq;
use serde_hex::{StrictPfx, CompactPfx};
use std::fmt::Display;
use alloy_contract::Interface;
use alloy_dyn_abi::DynSolValue;
use alloy_json_abi::JsonAbi;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// TODO: pub(crate) is not needed in most cases

pub(crate) const BLOCK_DELAY: u64 = 1;

pub(crate) enum BlockSpecifier {
    Latest,
    BlockNumber(u64),
}

impl Display for BlockSpecifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockSpecifier::Latest => write!(f, "latest"),
            BlockSpecifier::BlockNumber(n) => write!(f, "0x{n:x}"),
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct RpcResponse {
    result: String,
}

impl RpcResponse {
    pub fn as_u64(&self) -> anyhow::Result<u64> {
        u64::from_str_radix(self.result.trim_start_matches("0x"), 16)
            .map_err(|_| anyhow::anyhow!("Invalid u64: {}", self.result))
    }

    pub fn as_bytes(&self) -> anyhow::Result<Vec<u8>> {
        hex::decode(self.result.trim_start_matches("0x"))
            .map_err(|_| anyhow::anyhow!("Couldn't decode from hex: {}", self.result))
    }
}

#[derive(Serialize)]
pub(crate) struct RpcRequest {
    jsonrpc: &'static str,
    id: &'static str,
    method: &'static str,
    params: Value,
}

impl RpcRequest {
    pub fn build_block_number() -> Self {
        RpcRequest {
            jsonrpc: "2.0",
            id: "dontcare",
            method: "eth_blockNumber",
            params: json!([]),
        }
    }

    pub fn build_eth_call(
        auth_contract_id: &str,
        method_name: &str,
        args: &[DynSolValue],
        block_specifier: &BlockSpecifier,
    ) -> anyhow::Result<Self> {
        #[derive(Serialize)]
        struct CallObject<'a> {
            to: &'a str,
            #[serde(with = "SerHexSeq::<StrictPfx>")]
            data: Vec<u8>,
        }
        let data = INTERFACE.encode_input(method_name, &args)?;

        Ok(RpcRequest {
            jsonrpc: "2.0",
            id: "dontcare",
            method: "eth_call",
            params: json!([
                CallObject {
                    to: auth_contract_id,
                    data
                },
                block_specifier.to_string()
            ]),
        })
    }
}

pub(crate) static INTERFACE: std::sync::LazyLock<Interface> = std::sync::LazyLock::new(|| {
    let abi: JsonAbi =
        serde_json::from_str(HOT_VERIFY_EVM_ABI).expect("Invalid JSON ABI for hot_verify");
    Interface::new(abi)
});

// JSON ABI for `hot_verify` method
pub const HOT_VERIFY_EVM_ABI: &str = r#"
[
  {
    "inputs": [
      { "internalType": "bytes32", "name": "msg_hash",    "type": "bytes32" },
      { "internalType": "bytes",   "name": "walletId",    "type": "bytes"   },
      { "internalType": "bytes",   "name": "userPayload", "type": "bytes"   },
      { "internalType": "bytes",   "name": "metadata",    "type": "bytes"   }
    ],
    "name": "hot_verify",
    "outputs": [
      { "internalType": "bool", "name": "", "type": "bool" }
    ],
    "stateMutability": "view",
    "type": "function"
  },
  {
    "inputs": [
      { "internalType": "uint128", "name": "", "type": "uint128" }
    ],
    "name": "usedNonces",
    "outputs": [
      { "internalType": "bool", "name": "", "type": "bool" }
    ],
    "stateMutability": "view",
    "type": "function"
  }
]
"#;
