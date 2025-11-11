use std::sync::Arc;
use std::time::Duration;
use anyhow::{Result, anyhow};
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde::de::DeserializeOwned;
use serde::Serialize;

pub const TIMEOUT: Duration = Duration::from_millis(1500);
const LOG_SNIP_MAX: usize = 600;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("request failed for {url} (status={status:?} body_snip={body_snip:?}): {source}")]
    RequestFailed {
        url: String,
        status: Option<StatusCode>, // None ⇒ transport error
        body_snip: String,
        #[source] source: anyhow::Error,
    },

    #[error("JSON decode failed for {url} (status={status} body_snip={body_snip}): {source}")]
    JsonDecode {
        url: String,
        status: StatusCode,
        body_snip: String,
        #[source] source: serde_json::Error,
    },
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
pub async fn post_json_receive_json<T, U>(
    client: &Arc<Client>,
    url: &str,
    body: &T,
) -> std::result::Result<U, HttpError>
where
    T: Serialize + ?Sized,
    U: DeserializeOwned,
{
    let req = client
        .post(url)
        .json(body)
        .header(ACCEPT, "application/json")
        .timeout(TIMEOUT);

    let resp = req.send().await.map_err(|e| HttpError::RequestFailed {
        url: url.to_string(),
        status: None,
        body_snip: String::new(),
        source: anyhow!(e),
    })?;

    let status = resp.status();
    let url_final = resp.url().to_string();
    let bytes = resp.bytes().await.map_err(|e| HttpError::RequestFailed {
        url: url_final.clone(),
        status: Some(status),
        body_snip: String::new(),
        source: anyhow!(e),
    })?;

    if !status.is_success() {
        return Err(HttpError::RequestFailed {
            url: url_final,
            status: Some(status),
            body_snip: snip_bytes(&bytes),
            source: anyhow!("non-success status"),
        });
    }

    serde_json::from_slice::<U>(&bytes).map_err(|e| HttpError::JsonDecode {
        url: url_final,
        status,
        body_snip: snip_bytes(&bytes),
        source: e,
    })
}
