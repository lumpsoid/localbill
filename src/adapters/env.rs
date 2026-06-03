//! Process environment variable lookup.

use crate::ports::{Env, EnvVar};

pub struct StdEnv;

impl Env for StdEnv {
    fn var(&self, key: EnvVar) -> Option<String> {
        std::env::var(key.as_str()).ok()
    }
}
