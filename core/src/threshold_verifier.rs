use crate::verifiers::VerifierTag;
use anyhow::anyhow;
use futures_util::future::BoxFuture;
use futures_util::{stream, StreamExt};
use rand::prelude::{SliceRandom, StdRng};
use rand::SeedableRng;
use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

/// An interface, to call `hot_verify` concurrently on each `SingleVerifier`,
/// and checking whether there's at least `threshold` successes.
pub(crate) struct ThresholdVerifier<T: VerifierTag> {
    pub(crate) threshold: usize,
    pub(crate) verifiers: Vec<Arc<T>>,
}

impl<T: VerifierTag> ThresholdVerifier<T> {
    /// We can request data from a `SingleVerifier`. Each verifier casts a vote on the data it has returned.
    /// We collect all the votes and return a data with at least `threshold` votes.
    /// This logic was abstracted because we might call `verify`, `get_wallet_auth` or something else in the future.
    ///
    /// `functor` should return an `Option<R>`,
    /// with `None` being a vote for no data (when a server is unavailable), and `Some(R)` being a vote for `R`.
    pub async fn threshold_call<F, R>(&self, functor: F) -> anyhow::Result<R>
    where
        R: Eq + Hash + Clone + Debug,
        F: Clone + FnOnce(Arc<T>) -> BoxFuture<'static, anyhow::Result<R>>,
    {
        let threshold = self.threshold;

        let mut counts: HashMap<R, usize> = HashMap::new();
        let mut rng = StdRng::from_os_rng();

        let mut verifiers = self.verifiers.clone();
        verifiers.shuffle(&mut rng);
        let mut responses = stream::iter(self.verifiers.iter().cloned())
            .map(|caller| functor.clone()(caller))
            .buffer_unordered(threshold);

        let mut errors = vec![];
        while let Some(result_response) = responses.next().await {
            match result_response {
                Ok(response) => {
                    let entry = counts.entry(response.clone()).or_insert(0);
                    *entry += 1;

                    // as soon as any variant reaches the threshold, return it
                    if *entry >= threshold {
                        return Ok(response);
                    }
                }
                Err(err) => errors.push(err),
            }
        }

        if !errors.is_empty() {
            tracing::warn!("threshold call encountered errors: {:?}", errors);
        }

        // if we exit the loop, nobody hit the threshold
        Err(anyhow!(
            "No consensus for threshold call, got: {counts:?}, errors: {errors:?}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use anyhow::Result;

    use futures_util::future::BoxFuture;
    use tokio::time::{sleep, timeout, Duration};

    #[derive(Clone)]
    struct DummyVerifier {
        delay: Duration,
        resp: Option<u8>,
    }

    impl VerifierTag for DummyVerifier {
        fn get_endpoint(&self) -> &'static str {
            "dummy"
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

    impl BoolVerifier {
        async fn verify(&self, _auth_contract_id: &str) -> anyhow::Result<bool> {
            sleep(self.delay).await;
            match self.result {
                Ok(b) => Ok(b),
                Err(()) => Err(anyhow!("boom")),
            }
        }
    }

    impl VerifierTag for BoolVerifier {
        fn get_endpoint(&self) -> &'static str {
            "bool"
        }
    }

    impl ThresholdVerifier<BoolVerifier> {
        pub async fn verify(
            &self,
            auth_contract_id: &str,
        ) -> anyhow::Result<bool> {
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

        let res = tv
            .verify("dummy")
            .await
            .unwrap();
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

        let res = tv
            .verify("dummy")
            .await
            .unwrap();
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

        let err = tv
            .verify("dummy")
            .await
            .unwrap_err();
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

        let result = timeout(
            Duration::from_millis(180),
            tv.verify("dummy"),
        )
        .await
        .expect("timed out")
        .unwrap();
        assert!(result);
    }
}
