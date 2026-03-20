use std::time::Duration;

use serde_json::Value;

use crate::cache::errors::{Error, Result};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn fetch_json(url: &str) -> Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("oc-stats/0.1")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(Error::HttpClientBuild)?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                Error::HttpTimeout {
                    url: url.to_string(),
                }
            } else {
                Error::HttpFetch {
                    url: url.to_string(),
                    source: e,
                }
            }
        })?
        .error_for_status()
        .map_err(|e| {
            let status = e.status().map(|s| s.as_u16()).unwrap_or(0);
            Error::HttpStatus {
                url: url.to_string(),
                status,
            }
        })?;

    response
        .json::<Value>()
        .await
        .map_err(|e| Error::JsonDecode {
            url: url.to_string(),
            source: e,
        })
}
