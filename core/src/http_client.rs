use std::sync::Arc;
use std::time::Duration;
use anyhow::{Context, Result, anyhow};
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde::de::DeserializeOwned;
use serde::Serialize;

// pub const TIMEOUT: Duration = Duration::from_millis(750);
pub const TIMEOUT: Duration = Duration::from_millis(1500);
const LOG_SNIP_MAX: usize = 600;

#[derive(thiserror::Error, Debug)]
pub enum HttpError {
    #[error("transport error for {url}: {source}")]
    Transport {
        url: String,
        #[source] source: reqwest::Error,
    },
    #[error("non-success status {status} for {url} (body_snip={body_snip})")]
    Status {
        url: String,
        status: StatusCode,
        /// Truncated body (UTF-8 lossy), safe for logs
        body_snip: String,
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

/// Generic POST JSON → JSON with great error propagation.
///
/// - Preserves response body on errors (and includes a safe snippet)
/// - Returns typed JSON (`U`) instead of `serde_json::Value` (but you can set `U=Value`)
/// - Adds `Accept: application/json` and a per-request timeout
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

    let resp = req.send().await.map_err(|e| HttpError::Transport {
        url: url.to_string(),
        source: e,
    })?;

    let status = resp.status();
    let url_final = resp.url().to_string();
    let bytes = resp.bytes().await.map_err(|e| HttpError::Transport {
        url: url_final.clone(),
        source: e,
    })?;

    if !status.is_success() {
        return Err(HttpError::Status {
            url: url_final,
            status,
            body_snip: snip_bytes(&bytes),
        });
    }

    serde_json::from_slice::<U>(&bytes).map_err(|e| HttpError::JsonDecode {
        url: url_final,
        status,
        body_snip: snip_bytes(&bytes),
        source: e,
    })
}
