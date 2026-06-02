//! HTTP-backed remote queue API adapter.

use crate::error::{Error, Result};
use crate::ports::RemoteQueue;

pub struct HttpRemoteQueue {
    base_url: String,
}

impl HttpRemoteQueue {
    pub fn new(base_url: String) -> Self {
        Self { base_url }
    }
}

impl RemoteQueue for HttpRemoteQueue {
    fn fetch(&self) -> Result<Vec<String>> {
        let response: serde_json::Value = ureq::get(&self.base_url)
            .call()
            .map_err(|e| Error::Http(e.to_string()))?
            .body_mut()
            .read_json::<serde_json::Value>()
            .map_err(|e| Error::Http(e.to_string()))?;

        let items = response["items"]
            .as_array()
            .ok_or_else(|| Error::Parse("API response missing 'items' array".to_string()))?;

        Ok(items
            .iter()
            .filter_map(|v| v["item"].as_str().map(str::to_string))
            .collect())
    }

    fn remove(&self, urls: &[String]) -> Result<()> {
        let body = serde_json::json!({ "items": urls });
        ureq::delete(&self.base_url)
            .force_send_body()
            .header("Content-Type", "application/json")
            .send_json(&body)
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(())
    }
}
