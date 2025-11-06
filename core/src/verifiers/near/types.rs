use crate::verifiers::near::types::base64_json::Base64OfJson;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::{Serialize, Serializer};
use serde_with::{serde_as, SerializeAs};
use crate::{MPC_GET_WALLET_METHOD, MPC_HOT_WALLET_CONTRACT};

/// Arguments for `get_wallet` method on Near `mpc.hot.tg` smart contract.
#[derive(Debug, Serialize)]
pub struct GetWalletArgs {
    pub(crate) wallet_id: String,
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
    pub fn build(
        account_id: &'a str,
        method_name: &'a str,
        args: &'a T,
    ) -> Self {
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
    pub fn build(
        account_id: &'a str,
        method_name: &'a str,
        args: &'a T,
    ) -> Self {
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
