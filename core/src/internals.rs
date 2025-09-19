use crate::{metrics, AuthMethod, Validation, VerifyArgs, HOT_VERIFY_METHOD_NAME};
use anyhow::Context;
use anyhow::{ensure, Result};
use hot_validation_primitives::bridge::evm::EvmInputData;
use hot_validation_primitives::bridge::solana::SolanaInputData;
use hot_validation_primitives::bridge::stellar::StellarInputData;
use hot_validation_primitives::bridge::ton::TonInputData;
use hot_validation_primitives::bridge::HotVerifyResult;
use hot_validation_primitives::ChainId;
use serde::Deserialize;
use std::fmt::Debug;
use std::sync::Arc;

impl Validation {
    async fn handle_near(
        self: Arc<Self>,
        wallet_id: String,
        auth_method: &AuthMethod,
        message_hex: String,
        message_body: String,
        user_payload: String,
    ) -> Result<bool> {
        #[derive(Debug, Deserialize)]
        struct MethodName {
            method: String,
        }

        let message_bs58 = hex::decode(&message_hex)
            .map(|message_bytes| bs58::encode(message_bytes).into_string())?;

        // Mostly used with omni bridge workflows because there's another method name.
        let method_name = if let Some(metadata) = &auth_method.metadata {
            let method_name = serde_json::from_str::<MethodName>(metadata)?;
            method_name.method
        } else {
            HOT_VERIFY_METHOD_NAME.to_string()
        };

        let verify_args = VerifyArgs {
            wallet_id: Some(wallet_id.clone()),
            msg_hash: message_bs58,
            metadata: auth_method.metadata.clone(),
            user_payload: user_payload.clone(),
            msg_body: message_body.clone(),
        };

        let status = self
            .near
            .clone()
            .verify(auth_method.account_id.clone(), method_name, verify_args)
            .await
            .context("Could not get HotVerifyResult from NEAR")?;

        let status = match status {
            HotVerifyResult::AuthCall(auth_call) => match auth_call.chain_id {
                ChainId::Stellar => {
                    self.handle_stellar(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Ton | ChainId::TON_V2 => {
                    self.handle_ton(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Evm(_) => {
                    self.handle_evm(
                        auth_call.chain_id,
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Solana => {
                    self.handle_solana(
                        &auth_call.contract_id,
                        &auth_call.method,
                        auth_call.input.try_into()?,
                    )
                    .await?
                }
                ChainId::Near => {
                    unimplemented!("Auth call should not lead to NEAR")
                }
            },
            HotVerifyResult::Result(status) => status,
        };
        Ok(status)
    }

    async fn handle_stellar(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: StellarInputData,
    ) -> Result<bool> {
        let status = self
            .stellar
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Stellar failed")?;
        Ok(status)
    }

    async fn handle_solana(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: SolanaInputData,
    ) -> Result<bool> {
        let status = self
            .solana
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Stellar failed")?;
        Ok(status)
    }

    async fn handle_evm(
        self: Arc<Self>,
        chain_id: ChainId,
        auth_contract_id: &str,
        method_name: &str,
        input: EvmInputData,
    ) -> Result<bool> {
        let validation = self.evm.get(&chain_id).ok_or(anyhow::anyhow!(
            "EVM validation is not configured for chain {:?}",
            chain_id
        ))?;
        let status = validation
            .verify(auth_contract_id, method_name, input)
            .await?;
        Ok(status)
    }

    async fn handle_ton(
        self: Arc<Self>,
        auth_contract_id: &str,
        method_name: &str,
        input: TonInputData,
    ) -> Result<bool> {
        let status = self
            .ton
            .clone()
            .verify(auth_contract_id, method_name, input)
            .await
            .context("Validation on Ton failed")?;
        Ok(status)
    }

    pub(crate) async fn verify_auth_method(
        self: Arc<Self>,
        wallet_id: String,
        auth_method: AuthMethod,
        message_body: String,
        message_hex: String,
        user_payload: String,
    ) -> Result<()> {
        let _timer = metrics::RPC_SINGLE_VERIFY_DURATION.start_timer();

        // TODO: auth method is always a NEAR contract, expect for legacy workflows, so we need to get
        //  rid of non-Near branches, when we are dealt with legacy.
        let status = match auth_method.chain_id {
            ChainId::Near => {
                self.handle_near(
                    wallet_id,
                    &auth_method,
                    message_hex,
                    message_body,
                    user_payload,
                )
                .await?
            }
            ChainId::Stellar => {
                self.handle_stellar(
                    &auth_method.account_id,
                    HOT_VERIFY_METHOD_NAME,
                    StellarInputData::from_parts(message_hex, user_payload)?,
                )
                .await?
            }
            ChainId::Ton | ChainId::TON_V2 => {
                unimplemented!("It's not expected to call TON as the auth method")
            }
            ChainId::Evm(_) => {
                self.handle_evm(
                    auth_method.chain_id,
                    &auth_method.account_id,
                    HOT_VERIFY_METHOD_NAME,
                    EvmInputData::from_parts(message_hex, user_payload)?,
                )
                .await?
            }
            ChainId::Solana => {
                unimplemented!("It's not expected to call Solana as the auth method")
            }
        };

        ensure!(
            status,
            "Authentication method {:?} returned False",
            auth_method
        );
        Ok(())
    }
}
