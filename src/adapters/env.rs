//! Process environment variable lookup.

use crate::ports::Env;

pub struct StdEnv;

impl Env for StdEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}
