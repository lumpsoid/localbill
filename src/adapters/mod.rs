//! Production adapters: the real implementations of every [`crate::ports`]
//! trait. Each module is the **only** place a given external crate or syscall
//! appears (`ureq`, `std::fs`, `git`/`date` subprocesses, `TcpStream`, stdio).

pub mod clock;
pub mod env;
pub mod http;
pub mod network;
pub mod prod;
pub mod progress;
pub mod prompt;
pub mod remote_queue;
pub mod reporter;
pub mod store;
pub mod styler;
pub mod vcs;
