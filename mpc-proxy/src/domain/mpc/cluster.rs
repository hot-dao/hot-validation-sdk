use crate::domain::errors::AppError;
use crate::domain::mpc::api::Server;
use anyhow::anyhow;
use futures_util::{StreamExt, stream};
use hot_validation_primitives::ProofModel;
use hot_validation_primitives::mpc::{KeyType, OffchainSignatureResponse, ParticipantsInfo};
use hot_validation_primitives::uid::Uid;
use itertools::Itertools;
use rand::SeedableRng;
use rand::rngs::StdRng;
use rand::seq::{IteratorRandom, SliceRandom};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

const HEALTHCHECK_INTERVAL: Duration = Duration::from_secs(15);
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(2);
const HEALTHCHECK_MAX_CONCURRENCY: usize = 5;

#[derive(Clone, Debug)]
pub(crate) struct ClusterManager {
    client: reqwest::Client,
    clusters: Vec<Arc<Cluster>>,
}

impl ClusterManager {
    pub async fn new(clusters: Vec<Vec<Server>>) -> Result<Arc<Self>, AppError> {
        let client = reqwest::Client::new();
        let buffer_size = clusters.len();
        let clusters = stream::iter(clusters.into_iter())
            .map(|cluster| {
                let client = client.clone();
                async move {
                    let result = Cluster::new_initialized(cluster, client.clone()).await;
                    match result {
                        Ok(cluster) => Some(cluster),
                        Err(err) => {
                            error!(?err, "Cluster initialization failed");
                            None
                        }
                    }
                }
            })
            .buffer_unordered(buffer_size)
            .filter_map(|x| async move { x })
            .collect::<Vec<_>>()
            .await;

        if clusters.is_empty() {
            return Err(AppError::InitializationError(anyhow!(
                "All clusters failed to initialize"
            )));
        }

        let clusters = Self { client, clusters };

        Ok(Arc::new(clusters))
    }

    pub async fn sign(
        self: &Arc<Self>,
        uid: Uid,
        message: String,
        proof: ProofModel,
        key_type: KeyType,
    ) -> Result<OffchainSignatureResponse, AppError> {
        let mut combinations_from_clusters = stream::iter(self.clusters.iter().cloned())
            .map(|cluster| async move { cluster.alive_snapshot().await.combinations })
            .buffer_unordered(self.clusters.len())
            .filter_map(|x| async move { x })
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut rng = StdRng::from_os_rng();
        combinations_from_clusters.shuffle(&mut rng);
        // TODO: Metrics on how many tries we made
        for combination in combinations_from_clusters {
            let Some(leader) = combination.iter().choose(&mut rng) else {
                error!("iter::choose returned None");
                continue;
            };
            let accounts = combination
                .iter()
                .map(|info| info.participants_info.me.clone())
                .collect::<Vec<_>>();
            let result = leader
                .server
                .sign(
                    &self.client,
                    uid.clone(),
                    message.clone(),
                    proof.clone(),
                    key_type,
                    Some(accounts),
                )
                .await;
            match result {
                Ok(response) => {
                    return Ok(response);
                }
                Err(e) => {
                    error!(
                        "sign failed for combination {:?}, leader: {}, error: {}",
                        combination, leader.participants_info.me, e
                    );
                }
            }
        }
        Err(AppError::MpcSignError(anyhow!(
            "sign failed for all combinations"
        )))
    }
}

#[derive(Debug, Clone)]
struct ServerWithParticipantsInfo {
    server: Server,
    /// View of other participants from the perspective of this server.
    participants_info: ParticipantsInfo,
}

#[derive(Debug, Clone)]
struct LiveServers {
    servers: Vec<ServerWithParticipantsInfo>,
    /// combinations of `threshold` live servers, error if there are not enough servers
    combinations: Option<Vec<Vec<ServerWithParticipantsInfo>>>,
}

impl LiveServers {
    pub fn get_combinations(
        alive: &[ServerWithParticipantsInfo],
        threshold: usize,
    ) -> Option<Vec<Vec<ServerWithParticipantsInfo>>> {
        if alive.len() < threshold {
            error!(
                "Not enough alive servers need >= {}, got {}",
                alive.len(),
                threshold
            );
            return None;
        }
        let combinations = alive
            .iter()
            .cloned()
            .combinations(threshold)
            .collect::<Vec<_>>();
        Some(combinations)
    }

    pub(crate) fn new(servers: Vec<ServerWithParticipantsInfo>, threshold: usize) -> Self {
        let combinations = Self::get_combinations(&servers, threshold);
        Self {
            servers,
            combinations,
        }
    }
}

#[derive(Debug)]
struct Cluster {
    servers: Vec<Server>,
    threshold: usize,
    client: reqwest::Client,
    live_servers: RwLock<LiveServers>,
    cancel: CancellationToken,
    job: Mutex<Option<JoinHandle<()>>>,
}

impl Cluster {
    pub async fn new_initialized(
        servers: Vec<Server>,
        client: reqwest::Client,
    ) -> Result<Arc<Self>, AppError> {
        let live_servers = Self::compute_alive_once(servers.clone(), &client).await;

        let threshold: usize = live_servers
            .first()
            .ok_or(AppError::InitializationError(anyhow!(
                "No alive servers found for this cluster"
            )))?
            .participants_info
            .threshold
            .try_into()
            .map_err(anyhow::Error::from)
            .map_err(AppError::DataConversionError)?;

        let initial_alive = LiveServers::new(live_servers, threshold);

        let cluster = Arc::new(Self {
            servers,
            threshold,
            client,
            live_servers: RwLock::new(initial_alive),
            cancel: CancellationToken::new(),
            job: Mutex::new(None),
        });

        cluster.start_alive_job().await;

        Ok(cluster)
    }

    async fn start_alive_job(self: &Arc<Self>) {
        self.stop_alive_job().await;

        let me = Arc::clone(self);
        let token = me.cancel.child_token();

        let handle = tokio::spawn(async move {
            let mut tick = interval(HEALTHCHECK_INTERVAL);
            loop {
                tokio::select! {
                    () = token.cancelled() => break,
                    _ = tick.tick() => {
                        let alive = me.compute_alive().await;
                        *me.live_servers.write().await = LiveServers::new(alive, me.threshold);
                    }
                }
            }
        });

        *self.job.lock().await = Some(handle);
    }

    pub async fn stop_alive_job(&self) {
        self.cancel.cancel();
        if let Some(h) = self.job.lock().await.take() {
            let _ = h.await;
        }
    }

    pub async fn alive_snapshot(&self) -> LiveServers {
        self.live_servers.read().await.clone()
    }

    async fn compute_alive(&self) -> Vec<ServerWithParticipantsInfo> {
        let servers = self.servers.clone();
        Self::compute_alive_once(servers, &self.client).await
    }

    async fn compute_alive_once(
        servers: Vec<Server>,
        client: &reqwest::Client,
    ) -> Vec<ServerWithParticipantsInfo> {
        stream::iter(servers.into_iter().map(move |server| {
            let client = client.clone();
            async move {
                match timeout(HEALTHCHECK_TIMEOUT, server.get_participants(&client)).await {
                    Ok(Ok(participants_info)) => Some(ServerWithParticipantsInfo {
                        server,
                        participants_info,
                    }),
                    Ok(Err(e)) => {
                        warn!(?e, "participants fetch failed");
                        None
                    }
                    Err(_) => {
                        debug!("participants fetch timed out");
                        None
                    }
                }
            }
        }))
        .buffer_unordered(HEALTHCHECK_MAX_CONCURRENCY)
        .filter_map(|x| async move { x })
        .collect()
        .await
    }
}

#[cfg(test)]
mod tests {
    use crate::domain::mpc::api::Server;
    use crate::domain::mpc::cluster::{Cluster, ClusterManager};
    use crate::domain::mpc::tests::load_cluster_from_config;
    use anyhow::Result;
    use hot_validation_primitives::ProofModel;
    use hot_validation_primitives::mpc::KeyType;
    use hot_validation_primitives::uid::Uid;

    #[tokio::test]
    async fn check_valid_servers() -> Result<()> {
        let servers = load_cluster_from_config()?[0].clone();
        dbg!(&servers);
        let client = reqwest::Client::new();
        let cluster = Cluster::new_initialized(servers, client).await?;
        let alive = cluster.alive_snapshot().await;
        assert!(!alive.servers.is_empty());
        let alive = alive
            .servers
            .iter()
            .map(|p| p.participants_info.me.clone())
            .collect::<Vec<_>>();
        assert!(alive.contains(&"hot".to_string()));
        assert!(alive.contains(&"aurora".to_string()));
        assert!(alive.contains(&"everstake".to_string()));
        assert!(alive.contains(&"hapi".to_string()));
        Ok(())
    }

    #[tokio::test]
    async fn check_invalid_servers() -> Result<()> {
        let servers = vec![Server("http://kek.com".to_string())];
        dbg!(&servers);
        let client = reqwest::Client::new();
        let cluster = Cluster::new_initialized(servers, client).await;
        assert!(
            cluster
                .unwrap_err()
                .to_string()
                .contains("No alive servers found for this cluster")
        );
        Ok(())
    }

    #[tokio::test]
    async fn cluster_aggregation() -> Result<()> {
        let cluster = load_cluster_from_config()?;
        let cluster_manager = ClusterManager::new(cluster).await?;

        let uid: Uid = "0887d14fbe253e8b6a7b8193f3891e04f88a9ed744b91f4990d567ffc8b18e5f"
            .to_string()
            .into();
        let message =
            "57f42da8350f6a7c6ad567d678355a3bbd17a681117e7a892db30656d5caee32".to_string();
        let proof = ProofModel {
            message_body: "S8safEk4JWgnJsVKxans4TqBL796cEuV5GcrqnFHPdNW91AupymrQ6zgwEXoeRb6P3nyaSskoFtMJzaskXTDAnQUTKs5dGMWQHsz7irQJJ2UA2aDHSQ4qxgsU3h1U83nkq4rBstK8PL1xm6WygSYihvBTmuaMjuKCK6JT1tB4Uw71kGV262kU914YDwJa53BiNLuVi3s2rj5tboEwsSEpyJo9x5diq4Ckmzf51ZjZEDYCH8TdrP1dcY4FqkTCBA7JhjfCTToJR5r74ApfnNJLnDhTxkvJb4ReR9T9Ga7hPNazCFGE8Xq1deu44kcPjXNvb1GJGWLAZ5k1wxq9nnARb3bvkqBTmeYiDcPDamauhrwYWZkMNUsHtoMwF6286gcmY3ZgE3jja1NGuYKYQHnvscUqcutuT9qH".to_string(),
            user_payloads: vec![r#"{"auth_method":0,"signatures":["HZUhhJamfp8GJLL8gEa2F2qZ6TXPu4PYzzWkDqsTQsMcW9rQsG2Hof4eD2Vex6he2fVVy3UNhgi631CY8E9StAH"]}"#.to_string()],
        };
        let key_type = KeyType::Ecdsa;

        let result = cluster_manager.sign(uid, message, proof, key_type).await?;

        dbg!(&result);

        Ok(())
    }
}
