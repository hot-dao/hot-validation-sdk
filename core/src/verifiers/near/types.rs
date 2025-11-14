use crate::verifiers::near::types::base64_json::Base64OfJson;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::serde_as;
use hot_validation_primitives::uid::WalletId;

/// Arguments for `get_wallet` method on Near `mpc.hot.tg` smart contract.
#[derive(Debug, Serialize)]
pub struct GetWalletArgs {
    pub(crate) wallet_id: WalletId,
}

/// An input to the `hot_verify` method. A proof that a message is correct and can be signed.
#[derive(Debug, Serialize, Clone)]
pub struct VerifyArgs {
    /// In some cases, we need to know the exact message that we trying to sign.
    pub msg_body: String,
    /// The hash of the message that we try to sign.
    pub msg_hash: String,
    /// The wallet id, that initates the signing
    pub wallet_id: Option<WalletId>,
    /// The actual data, that authorizes signing
    pub user_payload: String,
    /// Additional field for the future, in case we need to override something
    pub metadata: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct RpcRequest<'a, T>
where
    T: Serialize + ?Sized,
{
    jsonrpc: &'static str,
    id: &'static str,
    method: &'static str,
    params: RpcParams<'a, T>,
}

impl<'a, T> RpcRequest<'a, T>
where
    T: Serialize + ?Sized,
{
    pub fn build(account_id: &'a str, method_name: &'a str, args: &'a T) -> Self {
        Self {
            jsonrpc: "2.0",
            id: "dontcare",
            method: "query",
            params: RpcParams::build(account_id, method_name, args),
        }
    }
}

#[serde_as]
#[derive(Serialize)]
struct RpcParams<'a, T>
where
    T: Serialize + ?Sized,
{
    request_type: &'static str,
    finality: &'static str,
    account_id: &'a str,
    method_name: &'a str,
    #[serde_as(as = "Base64OfJson")]
    #[serde(rename = "args_base64")]
    args: &'a T,
}

impl<'a, T> RpcParams<'a, T>
where
    T: Serialize + ?Sized,
{
    pub fn build(account_id: &'a str, method_name: &'a str, args: &'a T) -> Self {
        Self {
            request_type: "call_function",
            finality: "final",
            account_id,
            method_name,
            args,
        }
    }
}

pub mod base64_json {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    use serde::{Serialize, Serializer};
    use serde_with::SerializeAs;

    /// Serializes a value as base64(json(value)).
    pub struct Base64OfJson;

    impl<T: Serialize> SerializeAs<T> for Base64OfJson {
        fn serialize_as<S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
            let json = serde_json::to_vec(value).map_err(serde::ser::Error::custom)?;
            let b64 = B64.encode(json);
            serializer.serialize_str(&b64)
        }
    }
}

#[derive(serde::Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub(crate) struct RpcResponse<T> {
    result: RpcResult<T>,
}

impl<T> RpcResponse<T> {
    pub fn unpack(self) -> T {
        self.result.result
    }
}

#[derive(Deserialize)]
#[serde(bound(deserialize = "T: DeserializeOwned"))]
pub(crate) struct RpcResult<T> {
    #[serde(deserialize_with = "from_json_bytes_owned")]
    result: T,
}

fn from_json_bytes_owned<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let bytes = Vec::<u8>::deserialize(d)?;
    serde_json::from_slice(&bytes).map_err(serde::de::Error::custom)
}
