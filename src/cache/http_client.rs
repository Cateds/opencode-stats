use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

pub async fn fetch_json(url: &str) -> Result<Value> {
    let client = reqwest::Client::builder()
        .user_agent("oc-stats/0.1")
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                anyhow!("Fetch {url} timed out")
            } else {
                anyhow!("Failed to fetch {url}: {e}")
            }
        })?
        .error_for_status()
        .map_err(|e| anyhow!("request to {url} failed: {e}"))?;

    response
        .json::<Value>()
        .await
        .context("failed to decode JSON response")
}
