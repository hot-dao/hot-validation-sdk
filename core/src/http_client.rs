use crate::metrics;
use anyhow::anyhow;
use hot_validation_primitives::ChainId;
use reqwest::{header::ACCEPT, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tracing::instrument;

pub const TIMEOUT: Duration = Duration::from_millis(1500);
const LOG_SNIP_MAX: usize = 600;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("request failed for {url} (status={status:?} body_snip={body_snip:?}): {source}")]
    RequestFailed {
        url: String,
        status: Option<StatusCode>, // None ⇒ transport error
        body_snip: String,
        #[source]
        source: anyhow::Error,
    },

    #[error("JSON decode failed for {url} (status={status} body_snip={body_snip}): {source}")]
    JsonDecode {
        url: String,
        status: StatusCode,
        body_snip: String,
        #[source]
        source: serde_json::Error,
    },
}

impl HttpError {
    fn request_failed_error(
        chain_id: ChainId,
        url: String,
        status: Option<StatusCode>,
        body: Option<Vec<u8>>,
        source: anyhow::Error,
    ) -> Self {
        metrics::bump_metrics_rpc_call_fail(chain_id, &url);
        let body_snip = body.map_or_else(|| String::from("no body"), |s| snip_bytes(&s));
        Self::RequestFailed {
            url,
            status,
            body_snip,
            source,
        }
    }
}

fn snip_bytes(b: &[u8]) -> String {
    let s = String::from_utf8_lossy(b);
    if s.len() <= LOG_SNIP_MAX {
        s.into_owned()
    } else {
        format!(
            "{}…{}",
            &s[..LOG_SNIP_MAX / 2],
            &s[s.len() - LOG_SNIP_MAX / 2..]
        )
    }
}

/// Generic POST JSON → JSON with unified error handling.
///
/// - Combines transport & non-success HTTP into `RequestFailed`
/// - Keeps `JsonDecode` separate for clarity
#[instrument(skip(client, body))]
pub async fn post_json_receive_json<T, U>(
    client: &Arc<Client>,
    url: &str,
    body: &T,
    chain_id: ChainId, // for metrics
) -> std::result::Result<U, HttpError>
where
    T: Serialize + ?Sized,
    U: DeserializeOwned,
{
    metrics::bump_metrics_rpc_call_total(chain_id, url);

    let req = client
        .post(url)
        .json(body)
        .header(ACCEPT, "application/json")
        .timeout(TIMEOUT);

    let resp = req.send().await.map_err(|e| {
        HttpError::request_failed_error(chain_id, url.to_string(), None, None, anyhow!(e))
    })?;

    let status = resp.status();
    let url_final = resp.url().to_string();
    let bytes = resp.bytes().await.map_err(|e| {
        HttpError::request_failed_error(chain_id, url_final.clone(), Some(status), None, anyhow!(e))
    })?;

    if !status.is_success() {
        return Err(HttpError::request_failed_error(
            chain_id,
            url_final,
            Some(status),
            Some(bytes.to_vec()),
            anyhow!("HTTP non-success status code"),
        ));
    }

    serde_json::from_slice::<U>(&bytes).map_err(|e| HttpError::JsonDecode {
        url: url_final,
        status,
        body_snip: snip_bytes(&bytes),
        source: e,
    })
}

/// Generic GET → JSON with unified error handling.
///
/// - Combines transport & non-success HTTP into `RequestFailed`
/// - Keeps `JsonDecode` separate for clarity
pub async fn get_json<U>(
    client: &Arc<Client>,
    url: &str,
    chain_id: ChainId, // for metrics
) -> std::result::Result<U, HttpError>
where
    U: DeserializeOwned,
{
    metrics::bump_metrics_rpc_call_total(chain_id, url);

    let req = client
        .get(url)
        .header(ACCEPT, "application/json")
        .timeout(TIMEOUT);

    let resp = req.send().await.map_err(|e| {
        HttpError::request_failed_error(chain_id, url.to_string(), None, None, anyhow!(e))
    })?;

    let status = resp.status();
    let url_final = resp.url().to_string();
    let bytes = resp.bytes().await.map_err(|e| {
        HttpError::request_failed_error(chain_id, url_final.clone(), Some(status), None, anyhow!(e))
    })?;

    if !status.is_success() {
        return Err(HttpError::request_failed_error(
            chain_id,
            url_final,
            Some(status),
            Some(bytes.to_vec()),
            anyhow!("HTTP non-success status code"),
        ));
    }

    serde_json::from_slice::<U>(&bytes).map_err(|e| HttpError::JsonDecode {
        url: url_final,
        status,
        body_snip: snip_bytes(&bytes),
        source: e,
    })
}
