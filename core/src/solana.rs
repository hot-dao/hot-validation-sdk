use crate::internals::{SingleVerifier, ThresholdVerifier, TIMEOUT};
use anyhow::{anyhow, ensure, Context, Result};
use async_trait::async_trait;
use borsh::BorshDeserialize;
use futures_util::future::BoxFuture;
use hot_validation_primitives::bridge::solana::{
    anchor, CompletedWithdrawalData, DepositData, SolanaInputData, UserAccount,
};
use hot_validation_primitives::ChainValidationConfig;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_sdk::message::Address;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::transaction::Transaction;
use std::str::FromStr;
use std::sync::Arc;

pub(crate) struct SolanaVerifier {
    client: RpcClient,
    server: String,
}

impl SolanaVerifier {
    pub fn new(server: String) -> Self {
        Self {
            client: RpcClient::new_with_timeout(server.clone(), TIMEOUT),
            server,
        }
    }

    fn get_simulation_config() -> RpcSimulateTransactionConfig {
        RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: true,
            commitment: Some(CommitmentConfig::confirmed()),
            ..RpcSimulateTransactionConfig::default()
        }
    }

    async fn handle_deposit(
        &self,
        program_id: &Address,
        method_name: &str,
        deposit_data: DepositData,
    ) -> Result<()> {
        let simulation_config = Self::get_simulation_config();
        let message = deposit_data.get_message(program_id, method_name)?;
        let tx = Transaction::new_unsigned(message);
        self.client
            .simulate_transaction_with_config(&tx, simulation_config)
            .await?;
        Ok(())
    }

    async fn handle_completed_withdrawal(
        &self,
        program_id: &Address,
        completed_withdrawal_data: CompletedWithdrawalData,
    ) -> Result<()> {
        let user_pk = completed_withdrawal_data.get_user_address(program_id);

        let data = self
            .client
            .get_account_data(&user_pk)
            .await
            .with_context(|| format!("failed to fetch account data for {user_pk}"))?;

        let disc = anchor::account_discriminator("User");
        if data.len() < 8 || data[..8] != disc {
            return Err(anyhow!(
                "account {} is not an Anchor `User` (bad discriminator)",
                user_pk
            ));
        }

        // Deserialize the struct after the 8-byte discriminator
        let user = UserAccount::try_from_slice(&data[8..])
            .context("failed to Borsh-deserialize `User`")?;
        ensure!(
            completed_withdrawal_data.nonce <= user.last_withdraw_nonce,
            "Nonce is not used: got {}, last used: {}",
            completed_withdrawal_data.nonce,
            user.last_withdraw_nonce
        );
        Ok(())
    }

    async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: SolanaInputData,
    ) -> Result<bool> {
        let program_id = Pubkey::from_str(auth_contract_id)?;
        match input {
            SolanaInputData::Deposit(deposit_data) => {
                self.handle_deposit(&program_id, method_name, deposit_data)
                    .await?;
            }
            SolanaInputData::CheckCompletedWithdrawal(completed_withdrawal_data) => {
                self.handle_completed_withdrawal(&program_id, completed_withdrawal_data)
                    .await?;
            }
        }
        Ok(true)
    }
}

#[async_trait]
impl SingleVerifier for SolanaVerifier {
    fn get_endpoint(&self) -> String {
        self.server.clone()
    }
}

impl ThresholdVerifier<SolanaVerifier> {
    pub fn new_solana(config: &ChainValidationConfig) -> Self {
        let verifiers = config
            .servers
            .iter()
            .map(|server| Arc::new(SolanaVerifier::new(server.clone())))
            .collect::<Vec<_>>();
        Self {
            threshold: config.threshold,
            verifiers,
        }
    }

    pub async fn verify(
        &self,
        auth_contract_id: &str,
        method_name: &str,
        input: SolanaInputData,
    ) -> Result<bool> {
        let auth_contract_id = Arc::new(auth_contract_id.to_string());
        let functor = move |verifier: Arc<SolanaVerifier>| -> BoxFuture<'static, Result<bool>> {
            let auth = auth_contract_id.clone();
            let method_name = method_name.to_string();
            Box::pin(async move {
                verifier
                    .verify(&auth, &method_name, input)
                    .await
                    .context(format!(
                        "Error calling solana `verify` with {}",
                        verifier.sanitized_endpoint()
                    ))
            })
        };

        let result = self.threshold_call(functor).await?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::SolanaVerifier;

    use hot_validation_primitives::bridge::solana::{
        CompletedWithdrawalData, DepositData, SolanaInputData,
    };

    use serde_json::json;

    fn get_deposit_data() -> DepositData {
        let json = json!({
            "proof": "47b8b751a0d90d113e4e16678ebda646a01a02d376f49f666ddd17ee9f383c2f",
            "sender": "5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho",
            "receiver": "BJu6S7gT4gnx7AXPnghM7aYiS5dPfSUixqAZJq1Uqf4V",
            "mint": "BYPsjxa3YuZESQz1dKuBw1QSFCSpecsm8nCQhY5xbU1Z",
            "amount": 10_000_000,
            "nonce": "1757984522000007228"
        });
        serde_json::from_value(json).unwrap()
    }

    fn get_completed_withdrawal_data(nonce: &str) -> CompletedWithdrawalData {
        let json = json!({
            "nonce": nonce,
            "receiver": "5eMysQ7ywu4D8pmN5RtDoPxbu5YbiEThQy8gaBcmMoho",
        });
        serde_json::from_value(json).unwrap()
    }

    #[tokio::test]
    async fn deposit_verification() -> anyhow::Result<()> {
        let verifier = SolanaVerifier::new("https://api.mainnet-beta.solana.com".to_string());
        let auth_contract = "8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC";
        let method_name = "hot_verify_deposit";
        let input = SolanaInputData::Deposit(get_deposit_data());

        verifier.verify(auth_contract, method_name, input).await?;
        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_verification_low() -> anyhow::Result<()> {
        let verifier = SolanaVerifier::new("https://api.mainnet-beta.solana.com".to_string());
        let auth_contract = "8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC";
        let method_name = "";
        let input = SolanaInputData::CheckCompletedWithdrawal(get_completed_withdrawal_data(
            "1749390032000000032243",
        ));

        verifier.verify(auth_contract, method_name, input).await?;
        Ok(())
    }

    #[tokio::test]
    async fn completed_withdrawal_verification_high() -> anyhow::Result<()> {
        let verifier = SolanaVerifier::new("https://api.mainnet-beta.solana.com".to_string());
        let auth_contract = "8sXzdKW2jFj7V5heRwPMcygzNH3JZnmie5ZRuNoTuKQC";
        let method_name = "";
        let input = SolanaInputData::CheckCompletedWithdrawal(get_completed_withdrawal_data(
            "2749390032000000032243",
        ));

        let result = verifier.verify(auth_contract, method_name, input).await;
        result.expect_err("expected error");
        Ok(())
    }
}
