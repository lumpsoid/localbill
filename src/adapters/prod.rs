//! The production [`Platform`]: one real adapter per capability, wired from
//! [`Config`]. `GitCli` backs both the `Vcs` and `RemoteReachable` ports.

use crate::adapters::clock::DateClock;
use crate::adapters::http::UreqHttp;
use crate::adapters::network::StdNetwork;
use crate::adapters::progress::SpinnerProgress;
use crate::adapters::prompt::StdinPrompt;
use crate::adapters::remote_queue::HttpRemoteQueue;
use crate::adapters::reporter::StdReporter;
use crate::adapters::store::{FileFailedLog, FileSchemaSource, FsQueueStore, FsTransactionStore};
use crate::adapters::vcs::GitCli;
use crate::config::Config;
use crate::ports::Platform;

pub struct ProdPlatform {
    http: UreqHttp,
    git: GitCli,
    network: StdNetwork,
    clock: DateClock,
    prompt: StdinPrompt,
    reporter: StdReporter,
    progress: SpinnerProgress,
    transactions: FsTransactionStore,
    queue: FsQueueStore,
    remote_queue: HttpRemoteQueue,
    failed: FileFailedLog,
    schema: FileSchemaSource,
}

impl ProdPlatform {
    pub fn new(cfg: &Config) -> Self {
        Self {
            http: UreqHttp::new(),
            git: GitCli,
            network: StdNetwork,
            clock: DateClock,
            prompt: StdinPrompt,
            reporter: StdReporter,
            progress: SpinnerProgress,
            transactions: FsTransactionStore::new(cfg.transaction_dir.clone()),
            queue: FsQueueStore::new(cfg.queue_file.clone()),
            remote_queue: HttpRemoteQueue::new(cfg.api_base_url()),
            failed: FileFailedLog::new(cfg.failed_links_file.clone()),
            schema: FileSchemaSource::new(cfg.schema_file.clone()),
        }
    }
}

impl Platform for ProdPlatform {
    type Http = UreqHttp;
    type Vcs = GitCli;
    type RemoteReachable = GitCli;
    type Network = StdNetwork;
    type Clock = DateClock;
    type Prompt = StdinPrompt;
    type Reporter = StdReporter;
    type Progress = SpinnerProgress;
    type Transactions = FsTransactionStore;
    type Queue = FsQueueStore;
    type RemoteQueue = HttpRemoteQueue;
    type Failed = FileFailedLog;
    type Schema = FileSchemaSource;

    fn http(&self) -> &Self::Http {
        &self.http
    }
    fn vcs(&self) -> &Self::Vcs {
        &self.git
    }
    fn remote(&self) -> &Self::RemoteReachable {
        &self.git
    }
    fn network(&self) -> &Self::Network {
        &self.network
    }
    fn clock(&self) -> &Self::Clock {
        &self.clock
    }
    fn prompt(&self) -> &Self::Prompt {
        &self.prompt
    }
    fn reporter(&self) -> &Self::Reporter {
        &self.reporter
    }
    fn progress(&self) -> &Self::Progress {
        &self.progress
    }
    fn transactions(&self) -> &Self::Transactions {
        &self.transactions
    }
    fn queue(&self) -> &Self::Queue {
        &self.queue
    }
    fn remote_queue(&self) -> &Self::RemoteQueue {
        &self.remote_queue
    }
    fn failed(&self) -> &Self::Failed {
        &self.failed
    }
    fn schema(&self) -> &Self::Schema {
        &self.schema
    }
}
