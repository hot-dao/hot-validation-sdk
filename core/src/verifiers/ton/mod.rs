mod types;

use crate::http_client::post_json_receive_json;
use crate::threshold_verifier::{Identifiable, ThresholdVerifier};
use crate::verifiers::ton::types::{RpcRequest, RpcResponse};
use crate::verifiers::Verifier;
use anyhow::ensure;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use hot_validation_primitives::bridge::ton::{Action, StackItem, TonInputData};
use hot_validation_primitives::bridge::InputData;
use hot_validation_primitives::{ChainId, ChainValidationConfig, ExtendedChainId};
use primitive_types::U128;
use std::str::FromStr;
use std::sync::Arc;
use tonlib_core::TonAddress;

pub struct TonVerifier {
    client: Arc<reqwest::Client>,
    server: String,
}

impl Identifiable for TonVerifier {
    fn id(&self) -> String {
        self.server.clone()
    }
}

#[derive(thiserror::Error, Debug)]
pub enum TonError {
    #[error("TON Verification failed during Treasury call")]
    TreasuryCall(anyhow::Error),
    #[error("TON Verification failed during Child call")]
    ChildCall(anyhow::Error),
    #[error("TON Verification failed during Verification stage")]
    VerificationStage(anyhow::Error),
}

impl TonVerifier {
    fn new(client: Arc<reqwest::Client>, server: String) -> Self {
        Self { client, server }
    }

    async fn treasury_call(
        &self,
        treasury_address: TonAddress,
        method_name: String,
        input: TonInputData,
    ) -> Result<TonAddress> {
        let request = RpcRequest::build(&treasury_address, &method_name, input.treasury_call_args);
        let item: RpcResponse =
            post_json_receive_json(&self.client, &self.server, &request, ChainId::TON_V2).await?;
        let address = item.unpack()?.as_cell()?.parser().load_address()?;
        Ok(address)
    }

    async fn child_call(&self, child_address: TonAddress, input: TonInputData) -> Result<String> {
        let request = RpcRequest::build(
            &child_address,
            &input.child_call_method,
            input.child_call_args,
        );
        let item: RpcResponse =
            post_json_receive_json(&self.client, &self.server, &request, ChainId::TON_V2).await?;
        let item = item.unpack()?.as_num()?;
        Ok(item)
    }

    fn verification_stage(num: String, action: Action) -> Result<()> {
        match action {
            Action::Deposit => {
                ensure!(num == StackItem::SUCCESS_NUM, "Expected success, got {num}");
            }
            Action::CheckCompletedWithdrawal { nonce } => {
                let last_used_nonce = {
                    U128::from_str(&num)
                        .map_err(|e| anyhow!("Can't parse nonce ({num}) into u128: {e}"))?
                        .as_u128()
                };

                ensure!(
                    nonce <= last_used_nonce,
                    "Expected {nonce} <= {last_used_nonce}, last used: {last_used_nonce}"
                );
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Verifier for TonVerifier {
    fn chain_id(&self) -> ExtendedChainId {
        ExtendedChainId::Ton
    }

    async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        input_data: InputData,
    ) -> Result<bool> {
        let input: TonInputData = input_data.try_into()?;
        let treasury_address = TonAddress::from_base64_url(&auth_contract_id)?;
        let child_address = self
            .treasury_call(treasury_address, method_name, input.clone())
            .await
            .map_err(TonError::TreasuryCall)?;
        let num = self
            .child_call(child_address, input.clone())
            .await
            .map_err(TonError::ChildCall)?;
        Self::verification_stage(num, input.action).map_err(TonError::VerificationStage)?;
        Ok(true)
    }
}

impl ThresholdVerifier<TonVerifier> {
    pub fn new_ton(config: ChainValidationConfig, client: &Arc<reqwest::Client>) -> Self {
        let threshold = config.threshold;
        let servers = config.servers;
        let verifiers = servers
            .into_iter()
            .map(|url| Arc::new(TonVerifier::new(client.clone(), url)))
            .collect();
        Self {
            threshold,
            verifiers,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use anyhow::Result;

    use hot_validation_primitives::bridge::ton::{Action, StackItem, TonInputData};

    use std::sync::Arc;

    use crate::http_client::post_json_receive_json;
    use crate::verifiers::ton::types::{RpcRequest, RpcResponse};
    use crate::verifiers::ton::TonVerifier;
    use crate::verifiers::Verifier;
    use hot_validation_primitives::ChainId;
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
        let request = RpcRequest::build(&address, "get_deposit_jetton_address", vec![item]);
        let item: RpcResponse = post_json_receive_json(
            &verifier.client,
            &verifier.server,
            &request,
            ChainId::TON_V2,
        )
        .await?;

        let actual_address = item.unpack()?.as_cell()?.parser().load_address()?;
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

        let request = RpcRequest::build(&addr, "verify_withdraw", vec![item]);
        let item: RpcResponse = post_json_receive_json(
            &verifier.client,
            &verifier.server,
            &request,
            ChainId::TON_V2,
        )
        .await?;

        let actual = item.unpack()?.as_num()?;
        assert_eq!(actual, "-0x1");
        Ok(())
    }

    #[tokio::test]
    async fn deposit_fist_and_second_call_combined() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let input = TonInputData {
            treasury_call_args: vec![StackItem::from_nonce("1753218716000000003679".to_string())],
            child_call_method: "verify_withdraw".to_string(),
            child_call_args: vec![StackItem::from_proof(
                "bcb143828f64d7e4bf0b6a8e66a2a2d03c916c16e9e9034419ae778b9f699d3c".to_string(),
            )?],
            action: Action::Deposit,
        };

        verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ".to_string(),
                "get_deposit_jetton_address".to_string(),
                input.into(),
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

        let request = RpcRequest::build(&treasury_address, "get_user_jetton_address", vec![item]);
        let item: RpcResponse = post_json_receive_json(
            &verifier.client,
            &verifier.server,
            &request,
            ChainId::TON_V2,
        )
        .await?;

        let actual_address = item.unpack()?.as_cell()?.parser().load_address()?;
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
        let request = RpcRequest::build(&addr, "get_last_withdrawn_nonce", vec![]);
        let item: RpcResponse = post_json_receive_json(
            &verifier.client,
            &verifier.server,
            &request,
            ChainId::TON_V2,
        )
        .await?;

        let _actual = item.unpack()?.as_num()?;
        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_fist_and_second_call_combined_low() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let input = TonInputData {
            treasury_call_args: vec![StackItem::from_address(
                "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I",
            )?],
            child_call_method: "get_last_withdrawn_nonce".to_string(),
            child_call_args: vec![],
            action: Action::CheckCompletedWithdrawal {
                nonce: 1_753_218_716_000_000_003_679_u128,
            },
        };

        verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ".to_string(),
                "get_user_jetton_address".to_string(),
                input.into(),
            )
            .await?;

        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_fist_and_second_call_combined_high() -> Result<()> {
        let verifier = TonVerifier::new(Arc::new(reqwest::Client::new()), ton_rpc());

        let input = TonInputData {
            treasury_call_args: vec![StackItem::from_address(
                "UQA3zc65LQyIR9SoDniLaZA0UDPudeiNs6P06skYcCuCtw8I",
            )?],
            child_call_method: "get_last_withdrawn_nonce".to_string(),
            child_call_args: vec![],
            action: Action::CheckCompletedWithdrawal {
                nonce: 2_753_218_716_000_000_003_679_u128,
            },
        };

        let result = verifier
            .verify(
                "EQANEViM3AKQzi6Aj3sEeyqFu8pXqhy9Q9xGoId_0qp3CNVJ".to_string(),
                "get_user_jetton_address".to_string(),
                input.into(),
            )
            .await;
        assert!(result.is_err());

        Ok(())
    }
}
