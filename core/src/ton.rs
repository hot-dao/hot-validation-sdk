use crate::internals::{ThresholdVerifier, Verifier};
use crate::metrics::{tick_metrics_verify_success_attempts, tick_metrics_verify_total_attempts};
use anyhow::{anyhow, Result};
use anyhow::{ensure, Context};
use async_trait::async_trait;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::ton::{Action, ResponseStackItem, StackItem, TonInputData};
use hot_validation_primitives::{ChainId, ChainValidationConfig};
use primitive_types::U128;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;
use tonlib_core::TonAddress;

pub struct TonVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl TonVerifier {
    fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn make_call(
        &self,
        address: &TonAddress,
        method: &str,
        stack: Vec<StackItem>,
    ) -> Result<StackItem> {
        let json = json!({
            "method": "runGetMethod",
            "params": {
                "address": address.to_base64_url(),
                "method": method,
                "stack": stack,
            },
            "id": "dontcare",
            "jsonrpc": "2.0",
        });

        let response = self
            .client
            .post(self.server.clone())
            .json(&json)
            .send()
            .await?;

        response
            .error_for_status_ref()
            .context("Failed to call ton server")?;

        let json: serde_json::Value = response.json().await?;

        let stack =
            serde_json::from_value::<Vec<ResponseStackItem>>(json["result"]["stack"].clone())
                .context(format!("Failed to parse stack from response {json}"))?;
        let stack = stack
            .into_iter()
            .map(|item| item.0)
            .collect::<Vec<StackItem>>();

        ensure!(
            stack.len() == 1,
            "expected 1 item in stack, got {}",
            stack.len()
        );

        Ok(stack[0].clone())
    }

    pub async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: TonInputData,
    ) -> Result<bool> {
        tick_metrics_verify_total_attempts(ChainId::TON_V2);
        let treasury_address = TonAddress::from_base64_url(auth_contract_id)?;
        let child_address = {
            let item = self
                .make_call(&treasury_address, method_name, input.treasury_call_args)
                .await?;
            item.as_cell()?.parser().load_address()?
        };

        let action = input.action;
        match action {
            Action::Deposit => {
                let item = self
                    .make_call(
                        &child_address,
                        &input.child_call_method,
                        input.child_call_args,
                    )
                    .await?;
                let num = item.as_num()?;
                ensure!(
                    num == StackItem::SUCCESS_NUM,
                    "Expected success, got {}",
                    num
                );
            }
            Action::CheckCompletedWithdrawal { nonce } => {
                let item = self
                    .make_call(
                        &child_address,
                        &input.child_call_method,
                        input.child_call_args,
                    )
                    .await?;

                let last_used_nonce = {
                    let num = item.as_num()?;
                    U128::from_str(&num)
                        .map_err(|e| anyhow!("Can't parse nonce ({}) into u128: {}", num, e))?
                        .as_u128()
                };

                ensure!(
                    nonce <= last_used_nonce,
                    "Expected {} <= {}, last used: {}",
                    nonce,
                    last_used_nonce,
                    last_used_nonce
                );
            }
        }

        tick_metrics_verify_success_attempts(ChainId::TON_V2);
        Ok(true)
    }
}

#[async_trait]
impl Verifier for TonVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

impl ThresholdVerifier<TonVerifier> {
    pub fn new_ton(config: ChainValidationConfig, client: &Arc<reqwest::Client>) -> Self {
        let threshold = config.threshold; // TODO: Check invariand, DRY
        let servers = config.servers;
        assert!(
            // TODO: Remove this check, because it's not needed anymore
            (threshold <= servers.len()),
            "Threshold {} > servers {}",
            threshold,
            servers.len()
        );
        let verifiers = servers
            .into_iter()
            .map(|url| Arc::new(TonVerifier::new(client.clone(), url)))
            .collect();
        Self {
            threshold,
            verifiers,
        }
    }

    pub async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: TonInputData,
    ) -> Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor = move |verifier: Arc<TonVerifier>| -> BoxFuture<'static, Result<bool>> {
            let auth = auth_contract_id.clone();
            let method_name = method_name.to_string();
            Box::pin(async move {
                verifier
                    .verify(&auth, &method_name, input)
                    .await
                    .context(format!(
                        "Error calling ton `verify` with {}",
                        verifier.sanitized_endpoint()
                    ))
            })
        };

        let result = self.threshold_call(functor).await?;
        Ok(result)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::ton::TonVerifier;
    use anyhow::Result;

    use hot_validation_primitives::bridge::ton::{Action, StackItem, TonInputData};

    use std::sync::Arc;

    use tonlib_core::TonAddress;

    pub(crate) fn ton_rpc() -> String {
        dotenv::var("TON_RPC")
            .unwrap_or_else(|_| "https://toncenter.com/api/v2/jsonRPC".to_string())
    }

    #[tokio::test]
    async fn deposit_first_call() -> Result<()> {
        let expected_addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let expected_addr = TonAddress::from_base64_url(expected_addr_raw)?;

        let item = StackItem::from_nonce("1753218716000000003679".to_string());

        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let address =
            TonAddress::from_base64_url("EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ")?;
        let stack_item = verifier
            .make_call(&address, "get_deposit_jetton_address", vec![item])
            .await?;

        let actual_address = stack_item.as_cell()?.parser().load_address()?;
        assert_eq!(actual_address, expected_addr);

        Ok(())
    }

    #[tokio::test]
    async fn deposit_second_call() -> Result<()> {
        let addr_raw = "EQAgwUhaRZwU77BXUVEbtnEN8tplzDWMqUr0TbXWfez58tTL";
        let addr = TonAddress::from_base64_url(addr_raw)?;
        let item = StackItem::from_proof(
            "bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c".to_string(),
        )?;

        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let stack_item = verifier
            .make_call(&addr, "verify_withdraw", vec![item])
            .await?;

        let actual = stack_item.as_num()?;
        assert_eq!(actual, "-0x1");
        Ok(())
    }

    #[tokio::test]
    async fn deposit_fist_and_second_call_combined() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ",
                "get_deposit_jetton_address",
                TonInputData {
                    treasury_call_args: vec![StackItem::from_nonce(
                        "1753218716000000003679".to_string(),
                    )],
                    child_call_method: "verify_withdraw".to_string(),
                    child_call_args: vec![StackItem::from_proof(
                        "bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c"
                            .to_string(),
                    )?],
                    action: Action::Deposit,
                },
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_first_call() -> Result<()> {
        let expected_addr = {
            let raw = "EQCJWrtdMceshv4LiGZOtJlkP6OdQJZjpsBbgmMksobq10c0";
            TonAddress::from_base64_url(raw)?
        };

        let item = StackItem::from_address("UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I")?;

        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let treasury_address =
            TonAddress::from_base64_url("EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ")?;
        let stack_item = verifier
            .make_call(&treasury_address, "get_user_jetton_address", vec![item])
            .await?;

        let actual_address = stack_item.as_cell()?.parser().load_address()?;
        assert_eq!(actual_address, expected_addr);

        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_second_call() -> Result<()> {
        let addr = {
            let raw = "EQCJWrtdMceshv4LiGZOtJlkP6OdQJZjpsBbgmMksobq10c0";
            TonAddress::from_base64_url(raw)?
        };

        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let stack_item = verifier
            .make_call(&addr, "get_last_withdrawn_nonce", vec![])
            .await?;

        let _actual = stack_item.as_num()?;
        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_fist_and_second_call_combined_low() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ",
                "get_user_jetton_address",
                TonInputData {
                    treasury_call_args: vec![StackItem::from_address(
                        "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I",
                    )?],
                    child_call_method: "get_last_withdrawn_nonce".to_string(),
                    child_call_args: vec![],
                    action: Action::CheckCompletedWithdrawal {
                        nonce: 1_753_218_716_000_000_003_679_u128,
                    },
                },
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_fist_and_second_call_combined_high() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let result = verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ",
                "get_user_jetton_address",
                TonInputData {
                    treasury_call_args: vec![StackItem::from_address(
                        "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I",
                    )?],
                    child_call_method: "get_last_withdrawn_nonce".to_string(),
                    child_call_args: vec![],
                    action: Action::CheckCompletedWithdrawal {
                        nonce: 2_753_218_716_000_000_003_679_u128,
                    },
                },
            )
            .await;
        assert!(result.is_err());

        Ok(())
    }
}
