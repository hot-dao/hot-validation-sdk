use crate::verifiers::Verifier;
use anyhow::anyhow;
use futures_util::{stream, StreamExt};
use hot_validation_primitives::bridge::InputData;
use hot_validation_primitives::ExtendedChainId;
use rand::prelude::{SliceRandom, StdRng};
use rand::SeedableRng;
use std::collections::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::sync::Arc;
use thiserror::Error;
use tracing::instrument;

type Id = String;

pub trait Identifiable {
    fn id(&self) -> String;
}

/// An interface, to call `hot_verify` concurrently on each `SingleVerifier`,
/// and checking whether there's at least `threshold` successes.
pub struct ThresholdVerifier<T: Identifiable> {
    pub(crate) threshold: usize,
    pub(crate) verifiers: Vec<Arc<T>>,
}

impl<T: Identifiable> ThresholdVerifier<T> {
    pub async fn threshold_call<F, Fut, R>(&self, functor: F) -> anyhow::Result<R>
    where
        R: Eq + Hash + Clone + Debug,
        F: Fn(Arc<T>) -> Fut + Clone,
        Fut: Future<Output = anyhow::Result<R>> + Send + 'static,
    {
        let threshold = self.threshold;

        let shuffled_verifiers = {
            let mut rng = StdRng::from_os_rng();
            let mut verifiers = self.verifiers.clone();
            verifiers.shuffle(&mut rng);
            verifiers
        };

        let mut responses = stream::iter(shuffled_verifiers)
            .map(|verifier| async { (verifier.id(), functor(verifier).await) })
            .buffer_unordered(threshold);

        let mut votes: HashMap<R, Vec<Id>> = HashMap::new();
        let mut errors: HashMap<Id, _> = HashMap::new();

        while let Some((id, result)) = responses.next().await {
            match result {
                Ok(vote) => {
                    let entry = votes.entry(vote.clone()).or_default();
                    entry.push(id);

                    // as soon as any variant reaches the threshold, return it
                    if entry.len() >= threshold {
                        return Ok(vote);
                    }
                }
                Err(err) => {
                    errors.insert(id, err);
                }
            }
        }

        // if we exit the loop, nobody hit the threshold
        Err(anyhow!(
            "No consensus for threshold call, success({}): {:#?}, errors({}): {:#?}",
            votes.len(),
            votes,
            errors.len(),
            errors,
        ))
    }
}

#[derive(Error, Debug)]
#[error(
    "Verification failed for {chain_id}, contract={auth_contract_id}, method={method_name}: {kind}"
)]
pub struct VerificationError {
    pub chain_id: ExtendedChainId,
    pub auth_contract_id: String,
    pub method_name: String,
    pub input_data: InputData,
    pub kind: anyhow::Error,
}

impl<T: Identifiable + Verifier + Sync + Send + 'static> ThresholdVerifier<T> {
    fn chain_id(&self) -> ExtendedChainId {
        self.verifiers
            .first()
            .expect("There should be at least one verifier")
            .chain_id()
    }

    pub async fn verify(
        &self,
        auth_contract_id: String,
        method_name: String,
        input_data: InputData,
    ) -> Result<bool, VerificationError> {
        let auth_contract_id_ = auth_contract_id.clone();
        let method_name_ = method_name.clone();
        let input_data_ = input_data.clone();
        self.threshold_call(move |verifier| {
            let auth_contract_id = auth_contract_id.clone();
            let method_name = method_name.clone();
            let input_data = input_data.clone();
            async move {
                verifier
                    .verify(auth_contract_id, method_name, input_data)
                    .await
            }
        })
        .await
        .map_err(|kind| VerificationError {
            chain_id: self.chain_id(),
            auth_contract_id: auth_contract_id_,
            method_name: method_name_,
            input_data: input_data_,
            kind,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use anyhow::Result;

    use futures_util::future::BoxFuture;
    use tokio::time::{sleep, timeout, Duration};

    #[derive(Clone)]
    struct DummyVerifier {
        delay: Duration,
        resp: Option<u8>,
    }

    impl Identifiable for DummyVerifier {
        fn id(&self) -> String {
            String::default()
        }
    }

    #[tokio::test]
    async fn threshold_reaches_consensus() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(10),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(50),
                resp: Some(2),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
            })
        };

        let result = tv.threshold_call(functor).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn threshold_no_consensus() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(10),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(2),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(30),
                resp: None,
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
            })
        };

        let err = tv.threshold_call(functor).await.unwrap_err();
        assert!(err.to_string().contains("No consensus for threshold call"));
    }

    #[tokio::test]
    async fn threshold_returns_early() {
        let verifiers = vec![
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(20),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(150),
                resp: Some(1),
            }),
            Arc::new(DummyVerifier {
                delay: Duration::from_millis(200),
                resp: Some(2),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = |v: Arc<DummyVerifier>| -> BoxFuture<'static, Result<u8>> {
            Box::pin(async move {
                sleep(v.delay).await;
                v.resp.ok_or(anyhow!("No response"))
            })
        };

        let result = timeout(Duration::from_millis(180), tv.threshold_call(functor))
            .await
            .expect("timed out")
            .unwrap();
        assert_eq!(result, 1);
    }

    #[derive(Clone)]
    struct BoolVerifier {
        delay: Duration,
        result: Result<bool, ()>,
    }

    impl Identifiable for BoolVerifier {
        fn id(&self) -> String {
            String::default()
        }
    }

    impl BoolVerifier {
        async fn verify(&self, _auth_contract_id: &str) -> anyhow::Result<bool> {
            sleep(self.delay).await;
            match self.result {
                Ok(b) => Ok(b),
                Err(()) => Err(anyhow!("boom")),
            }
        }
    }

    impl ThresholdVerifier<BoolVerifier> {
        pub async fn verify(&self, auth_contract_id: &str) -> anyhow::Result<bool> {
            let auth_contract_id = Arc::new(auth_contract_id.to_string());
            let functor = move |verifier: Arc<BoolVerifier>| -> BoxFuture<'static, Result<bool>> {
                let auth = auth_contract_id.clone();
                Box::pin(async move { verifier.verify(&auth).await })
            };

            let result = self.threshold_call(functor).await?;
            Ok(result)
        }
    }

    #[tokio::test]
    async fn verify_reaches_consensus_true() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(50),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let res = tv.verify("dummy").await.unwrap();
        assert!(res);
    }

    #[tokio::test]
    async fn verify_reaches_consensus_false() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(false),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(false),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(30),
                result: Ok(true),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let res = tv.verify("dummy").await.unwrap();
        assert!(!res);
    }

    #[tokio::test]
    async fn verify_no_consensus() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(10),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Err(()),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(30),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let err = tv.verify("dummy").await.unwrap_err();
        assert!(err.to_string().contains("No consensus for threshold call"));
    }

    #[tokio::test]
    async fn verify_returns_early() {
        let verifiers = vec![
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(20),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(150),
                result: Ok(true),
            }),
            Arc::new(BoolVerifier {
                delay: Duration::from_millis(200),
                result: Ok(false),
            }),
        ];
        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let result = timeout(Duration::from_millis(180), tv.verify("dummy"))
            .await
            .expect("timed out")
            .unwrap();
        assert!(result);
    }
    #[derive(Clone)]
    struct CountVerifier {
        counter: Arc<AtomicUsize>,
    }

    impl Identifiable for CountVerifier {
        fn id(&self) -> String {
            String::default()
        }
    }

    #[tokio::test]
    async fn stops_invoking_after_threshold_reached() {
        let counter = Arc::new(AtomicUsize::new(0));

        let verifiers = (0..5)
            .map(|_| {
                Arc::new(CountVerifier {
                    counter: counter.clone(),
                })
            })
            .collect::<Vec<_>>();

        let tv = ThresholdVerifier {
            threshold: 2,
            verifiers,
        };

        let functor = move |v: Arc<CountVerifier>| -> BoxFuture<'static, anyhow::Result<()>> {
            let counter = v.counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        };

        tv.threshold_call(functor).await.unwrap();

        let invoked = counter.load(Ordering::SeqCst);
        assert_eq!(
            invoked, 2,
            "expected only threshold verifiers to be invoked"
        );
    }
}
