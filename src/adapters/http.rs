//! `ureq`-backed [`Http`] adapter — the only place the HTTP crate appears.

use crate::error::{Error, Result};
use crate::ports::Http;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .build()
            .into();
        Self { agent }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn get_text(&self, url: &str) -> Result<String> {
        self.agent
            .get(url)
            .call()
            .map_err(|e| Error::Http(e.to_string()))?
            .body_mut()
            .read_to_string()
            .map_err(|e| Error::Http(e.to_string()))
    }

    fn post_form(&self, url: &str, body: &str) -> Result<serde_json::Value> {
        self.agent
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .send(body)
            .map_err(|e| Error::Http(e.to_string()))?
            .body_mut()
            .read_json::<serde_json::Value>()
            .map_err(|e| Error::Http(e.to_string()))
    }
}
