use derive_more::{From, Into};
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::mpc::{
    KeyType, OffchainSignatureRequest, OffchainSignatureResponse, ParticipantsInfo,
    PublicKeyRequest, PublicKeyResponse,
};
use hot_validation_primitives::uid::Uid;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tracing::instrument;

const TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Into, From, Deserialize)]
pub(crate) struct Server(pub String);

#[instrument(skip(rb, url), err(Debug))]
async fn send_json<T: DeserializeOwned>(rb: reqwest::RequestBuilder, url: &str) -> Result<T> {
    let rb = rb.timeout(TIMEOUT);
    let resp = rb.send().await.context("error sending json")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("{url} failed: {status} — {body}");
    }
    resp.json().await.context("error receiving json")
}

async fn send_ok(rb: reqwest::RequestBuilder, url: &str) -> Result<()> {
    let rb = rb.timeout(TIMEOUT);
    let resp = rb.send().await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("{url} failed: {status} — {body}");
    }
    Ok(())
}

impl Server {
    pub(crate) async fn health_check(&self, client: &reqwest::Client) -> Result<()> {
        let url = format!("{}/health", self.0);
        send_ok(client.get(&url), &url).await
    }

    pub(crate) async fn get_participants(
        &self,
        client: &reqwest::Client,
    ) -> Result<ParticipantsInfo> {
        let url = format!("{}/participants", self.0);
        send_json(client.get(&url), &url).await
    }

    pub(crate) async fn get_public_key(
        &self,
        client: &reqwest::Client,
        uid: Uid,
    ) -> Result<PublicKeyResponse> {
        let url = format!("{}/public_key", self.0);
        let req: PublicKeyRequest = uid.into();
        send_json(client.post(&url).json(&req), &url).await
    }

    #[instrument(skip(client, uid, message, proof, key_type), err(Debug))]
    pub(crate) async fn sign(
        &self,
        client: &reqwest::Client,
        uid: Uid,
        message: Vec<u8>,
        proof: ProofModel,
        key_type: KeyType,
        participants: Option<Vec<String>>,
    ) -> Result<OffchainSignatureResponse> {
        let url = format!("{}/sign", self.0);
        let req = OffchainSignatureRequest {
            uid,
            message,
            proof,
            key_type,
            participants,
        };
        send_json(client.post(&url).json(&req), &url).await
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::mpc::tests::load_cluster_from_config;
    use anyhow::{Result, ensure};
    use hot_validation_primitives::ProofModel;
    use hot_validation_primitives::mpc::KeyType;
    use hot_validation_primitives::uid::Uid;

    fn staging_uid() -> Uid {
        Uid::from_hex("f44a64989027d8fea9037e190efe7ad830b9646acac406402f8771bec83d5b36").unwrap()
    }

    #[tokio::test]
    async fn test_health_check() -> Result<()> {
        let servers = load_cluster_from_config()?[0].clone();
        let client = reqwest::Client::new();
        for server in &servers {
            let result = server.health_check(&client).await;
            ensure!(result.is_ok(), "Server {} is not healthy", server.0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_get_participants() -> Result<()> {
        let servers = load_cluster_from_config()?[0].clone();
        let client = reqwest::Client::new();
        for server in &servers {
            let result = server.get_participants(&client).await;
            ensure!(result.is_ok(), "Server {} is not healthy", server.0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_get_public_key() -> Result<()> {
        let servers = load_cluster_from_config()?[0].clone();
        let client = reqwest::Client::new();
        let uid = staging_uid();

        for server in &servers {
            let result = server.get_public_key(&client, uid.clone()).await;
            dbg!(&result);
            ensure!(result.is_ok(), "Server {} is not healthy", server.0);
        }
        Ok(())
    }

    #[tokio::test]
    async fn test_sign() -> Result<()> {
        let servers = load_cluster_from_config()?[0].clone();
        let client = reqwest::Client::new();

        let uid =
            Uid::from_hex("0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f")?;
        let message =
            hex::decode("57f42da8350f6a7c6ad567d678355a3bbd17a681117e7a892db30656d5caee32")?;
        let proof = ProofModel {
            message_body: "S8safEk4JWgnJsVKxans4TqBL796cEuV5GcrqnFHPdNW91AupymrQ6zgwEXoeRb6P3nyaSskoFtMJzaskXTDAnQUTKs5dGMWQHsz7irQJJ2UA2aDHSQ4qxgsU3h1U83nkq4rBstK8PL1xm6WygSYihvBTmuaMjuKCK6JT1tB4Uw71kGV262kU914YDwJa53BiNLuVi3s2rj5tboEwsSEpyJo9x5diq4Ckmzf51ZjZEDYCH8TdrP1dcY4FqkTCBA7JhjfCTToJR5r74ApfnNJLnDhTxkvJb4ReR9T9Ga7hPNazCFGE8Xq1deu44kcPjXNvb1GJGWLAZ5k1wxq9nnARb3bvkqBTmeYiDcPDamauhrwYWZkMNUsHtoMwF6286gcmY3ZgE3jja1NGuYKYQHnvscUqcutuT9qH".to_string(),
            user_payloads: vec![r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string()],
        };
        let key_type = KeyType::Ecdsa;
        let participants = None;

        for server in &servers {
            let result = server
                .sign(
                    &client,
                    uid.clone(),
                    message.clone(),
                    proof.clone(),
                    key_type,
                    participants.clone(),
                )
                .await;
            println!("{result:?}");
            ensure!(result.is_ok(), "Server {} is not healthy", server.0);
        }
        Ok(())
    }
}
