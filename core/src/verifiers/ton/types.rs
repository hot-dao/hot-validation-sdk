use anyhow::ensure;
use hot_validation_primitives::bridge::ton::{ResponseStackItem, StackItem};
use serde::{Deserialize, Serialize};
use tonlib_core::TonAddress;

#[derive(Debug, Serialize)]
pub(crate) struct RpcRequest {
    jsonrpc: &'static str,
    id: &'static str,
    method: &'static str,
    params: Params,
}

#[derive(Debug, Serialize)]
struct Params {
    address: String,
    method: String,
    stack: Vec<StackItem>,
}

#[derive(Deserialize)]
pub(crate) struct RpcResponse {
    result: ResultStack,
}

impl RpcResponse {
    pub(crate) fn unpack(self) -> anyhow::Result<StackItem> {
        let stack = self
            .result
            .stack
            .into_iter()
            .map(|item| item.0)
            .collect::<Vec<StackItem>>();
        ensure!(
            stack.len() == 1,
            "expected 1 item in stack, got {}: stack={:?}",
            stack.len(), stack
        );
        Ok(stack[0].clone())
    }
}

#[derive(Deserialize)]
struct ResultStack {
    stack: Vec<ResponseStackItem>,
}

impl RpcRequest {
    pub(crate) fn build(address: &TonAddress, method: &str, stack: Vec<StackItem>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: "dontcare",
            method: "runGetMethod",
            params: Params {
                address: address.to_base64_url(),
                method: method.to_string(),
                stack,
            },
        }
    }
}
