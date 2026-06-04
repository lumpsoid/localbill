//! In-memory test platform: a fake adapter per port plus a [`TestPlatform`]
//! that bundles them, enabling unit tests of the network/git/filesystem paths
//! that were previously untestable. Compiled only under `cfg(test)`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::error::{Error, Result};
use crate::ports::{
    Clock, Env, EnvVar, FailedLog, Http, Network, Platform, Progress, ProgressTask, Prompt,
    QueueStore, RemoteQueue, RemoteReachable, Reporter, SchemaSource, StoredDoc, Style, Styler,
    TransactionStore, Vcs,
};

// ── Fakes ────────────────────────────────────────────────────────────────────

/// HTTP fake: pages are returned in order by `get_text`, JSON item payloads by
/// `post_form`. Empty queues yield errors (mimicking exhausted responses).
#[derive(Default)]
pub struct FakeHttp {
    pub pages: RefCell<VecDeque<String>>,
    pub items: RefCell<VecDeque<serde_json::Value>>,
}

impl FakeHttp {
    pub fn with_pages(pages: Vec<String>, items: Vec<serde_json::Value>) -> Self {
        Self {
            pages: RefCell::new(pages.into()),
            items: RefCell::new(items.into()),
        }
    }
}

impl Http for FakeHttp {
    fn get_text(&self, _url: &str) -> Result<String> {
        self.pages
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| Error::Http("no more pages".into()))
    }

    fn post_form(&self, _url: &str, _body: &str) -> Result<serde_json::Value> {
        self.items
            .borrow_mut()
            .pop_front()
            .ok_or_else(|| Error::Parse("no more items".into()))
    }
}

/// VCS fake recording each mutating call.
pub struct FakeVcs {
    pub is_repo: bool,
    pub dirty: bool,
    pub calls: RefCell<Vec<String>>,
}

impl Default for FakeVcs {
    fn default() -> Self {
        Self {
            is_repo: true,
            dirty: true,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl Vcs for FakeVcs {
    fn is_repo(&self, _dir: &Path) -> bool {
        self.is_repo
    }
    fn pull(&self, _dir: &Path) -> Result<()> {
        self.calls.borrow_mut().push("pull".into());
        Ok(())
    }
    fn is_dirty(&self, _dir: &Path) -> Result<bool> {
        Ok(self.dirty)
    }
    fn commit_all(&self, _dir: &Path, msg: &str) -> Result<()> {
        self.calls.borrow_mut().push(format!("commit:{msg}"));
        Ok(())
    }
    fn current_branch(&self, _dir: &Path) -> Result<String> {
        Ok("main".into())
    }
    fn push(&self, _dir: &Path, branch: &str) -> Result<()> {
        self.calls.borrow_mut().push(format!("push:{branch}"));
        Ok(())
    }
}

pub struct ToggleRemote(pub bool);
impl RemoteReachable for ToggleRemote {
    fn reachable(&self, _dir: &Path) -> bool {
        self.0
    }
}

pub struct ToggleNetwork(pub bool);
impl Network for ToggleNetwork {
    fn has_internet(&self) -> bool {
        self.0
    }
}

pub struct FixedClock;
impl Clock for FixedClock {
    fn timestamp(&self) -> String {
        "2024-01-01 00:00:00".into()
    }
}

/// Returns scripted answers in order; an exhausted script behaves like pressing
/// Enter (empty input).
#[derive(Default)]
pub struct ScriptedPrompt {
    pub answers: RefCell<VecDeque<String>>,
}
impl ScriptedPrompt {
    /// Build a prompt that replays `answers` in order.
    pub fn with(answers: Vec<&str>) -> Self {
        Self {
            answers: RefCell::new(answers.into_iter().map(String::from).collect()),
        }
    }
}
impl Prompt for ScriptedPrompt {
    fn read_line(&self, _prompt: &str) -> Result<String> {
        Ok(self.answers.borrow_mut().pop_front().unwrap_or_default())
    }
}

/// No-op styler: returns text verbatim, so tests assert on plain strings.
pub struct FakeStyler;
impl Styler for FakeStyler {
    fn paint(&self, _style: Style, text: &str) -> String {
        text.to_string()
    }
}

#[derive(Default)]
pub struct RecordingReporter {
    pub out: RefCell<Vec<String>>,
    pub status: RefCell<Vec<String>>,
}
impl RecordingReporter {
    pub fn out_contains(&self, needle: &str) -> bool {
        self.out.borrow().iter().any(|l| l.contains(needle))
    }
    pub fn status_contains(&self, needle: &str) -> bool {
        self.status.borrow().iter().any(|l| l.contains(needle))
    }
}
impl Reporter for RecordingReporter {
    fn out(&self, line: &str) {
        self.out.borrow_mut().push(line.to_string());
    }
    fn status(&self, msg: &str) {
        self.status.borrow_mut().push(msg.to_string());
    }
}

/// Records the phase lists handed to `start`; the task itself is a no-op (no
/// thread, no terminal writes).
#[derive(Default)]
pub struct RecordingProgress {
    pub started: RefCell<Vec<Vec<String>>>,
}
impl RecordingProgress {
    /// The phases of the most recent `start` (empty if none).
    pub fn last_phases(&self) -> Vec<String> {
        self.started.borrow().last().cloned().unwrap_or_default()
    }
}
impl Progress for RecordingProgress {
    type Task = NoopTask;
    fn start(&self, phases: &[&str]) -> NoopTask {
        self.started
            .borrow_mut()
            .push(phases.iter().map(|s| s.to_string()).collect());
        NoopTask
    }
}
pub struct NoopTask;
impl ProgressTask for NoopTask {
    fn complete(&self) {}
    fn finish(self) {}
}

#[derive(Default)]
pub struct MapEnv(pub HashMap<String, String>);
impl MapEnv {
    pub fn set(mut self, key: EnvVar, val: &str) -> Self {
        self.0.insert(key.as_str().into(), val.into());
        self
    }
}
impl Env for MapEnv {
    fn var(&self, key: EnvVar) -> Option<String> {
        self.0.get(key.as_str()).cloned()
    }
}

/// In-memory transaction store rooted at a virtual `/mem` directory.
pub struct MemTransactions {
    pub dir: PathBuf,
    pub docs: RefCell<Vec<StoredDoc>>,
}
impl Default for MemTransactions {
    fn default() -> Self {
        Self {
            dir: PathBuf::from("/mem"),
            docs: RefCell::new(Vec::new()),
        }
    }
}
impl MemTransactions {
    pub fn with_docs(docs: Vec<StoredDoc>) -> Self {
        Self {
            dir: PathBuf::from("/mem"),
            docs: RefCell::new(docs),
        }
    }
}
impl TransactionStore for MemTransactions {
    fn list(&self) -> Result<Vec<StoredDoc>> {
        Ok(self.docs.borrow().clone())
    }
    fn list_at(&self, _dir: &Path) -> Result<Vec<StoredDoc>> {
        Ok(self.docs.borrow().clone())
    }
    fn read(&self, path: &Path) -> Result<StoredDoc> {
        self.docs
            .borrow()
            .iter()
            .find(|d| d.path == path)
            .cloned()
            .ok_or_else(|| Error::Io(std::io::Error::other("not found")))
    }
    fn exists(&self, filename: &str) -> bool {
        let target = self.dir.join(filename);
        self.docs.borrow().iter().any(|d| d.path == target)
    }
    fn write_new(&self, filename: &str, content: &str) -> Result<PathBuf> {
        let path = self.dir.join(filename);
        self.docs.borrow_mut().push(StoredDoc {
            path: path.clone(),
            content: content.to_string(),
        });
        Ok(path)
    }
}

#[derive(Default)]
pub struct MemQueue {
    pub urls: RefCell<Vec<String>>,
}
impl MemQueue {
    pub fn with(urls: Vec<String>) -> Self {
        Self {
            urls: RefCell::new(urls),
        }
    }
}
impl QueueStore for MemQueue {
    fn list(&self) -> Result<Vec<String>> {
        Ok(self.urls.borrow().clone())
    }
    fn enqueue(&self, url: &str) -> Result<()> {
        self.urls.borrow_mut().push(url.to_string());
        Ok(())
    }
    fn replace(&self, urls: &[String]) -> Result<()> {
        *self.urls.borrow_mut() = urls.to_vec();
        Ok(())
    }
}

#[derive(Default)]
pub struct FakeRemoteQueue {
    pub available: RefCell<Vec<String>>,
    pub removed: RefCell<Vec<String>>,
}
impl RemoteQueue for FakeRemoteQueue {
    fn fetch(&self) -> Result<Vec<String>> {
        Ok(self.available.borrow().clone())
    }
    fn remove(&self, urls: &[String]) -> Result<()> {
        self.removed.borrow_mut().extend_from_slice(urls);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemFailedLog {
    pub urls: RefCell<Vec<String>>,
}
impl FailedLog for MemFailedLog {
    fn record(&self, url: &str) -> Result<()> {
        self.urls.borrow_mut().push(url.to_string());
        Ok(())
    }
}

pub struct StrSchema(pub String);
impl Default for StrSchema {
    fn default() -> Self {
        // A minimal schema sufficient for `add`/`validate` exercises.
        StrSchema("type: object\nproperties:\n  name:\n    type: string\nrequired: [name]\n".into())
    }
}
impl SchemaSource for StrSchema {
    fn load(&self) -> Result<String> {
        Ok(self.0.clone())
    }
}

// ── Composition ────────────────────────────────────────────────────────────────

pub struct TestPlatform {
    pub http: FakeHttp,
    pub vcs: FakeVcs,
    pub remote: ToggleRemote,
    pub network: ToggleNetwork,
    pub clock: FixedClock,
    pub prompt: ScriptedPrompt,
    pub reporter: RecordingReporter,
    pub styler: FakeStyler,
    pub progress: RecordingProgress,
    pub transactions: MemTransactions,
    pub queue: MemQueue,
    pub remote_queue: FakeRemoteQueue,
    pub failed: MemFailedLog,
    pub schema: StrSchema,
}

impl Default for TestPlatform {
    fn default() -> Self {
        Self {
            http: FakeHttp::default(),
            vcs: FakeVcs::default(),
            remote: ToggleRemote(false),
            network: ToggleNetwork(true),
            clock: FixedClock,
            prompt: ScriptedPrompt::default(),
            reporter: RecordingReporter::default(),
            styler: FakeStyler,
            progress: RecordingProgress::default(),
            transactions: MemTransactions::default(),
            queue: MemQueue::default(),
            remote_queue: FakeRemoteQueue::default(),
            failed: MemFailedLog::default(),
            schema: StrSchema::default(),
        }
    }
}

impl Platform for TestPlatform {
    type Http = FakeHttp;
    type Vcs = FakeVcs;
    type RemoteReachable = ToggleRemote;
    type Network = ToggleNetwork;
    type Clock = FixedClock;
    type Prompt = ScriptedPrompt;
    type Reporter = RecordingReporter;
    type Styler = FakeStyler;
    type Progress = RecordingProgress;
    type Transactions = MemTransactions;
    type Queue = MemQueue;
    type RemoteQueue = FakeRemoteQueue;
    type Failed = MemFailedLog;
    type Schema = StrSchema;

    fn http(&self) -> &Self::Http {
        &self.http
    }
    fn vcs(&self) -> &Self::Vcs {
        &self.vcs
    }
    fn remote(&self) -> &Self::RemoteReachable {
        &self.remote
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
    fn styler(&self) -> &Self::Styler {
        &self.styler
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

// ── Fixtures ───────────────────────────────────────────────────────────────────

/// A valid invoice page; pass `with_token = false` to omit the view-model token
/// (forces the parser's token-failure retry path).
pub fn invoice_page(with_token: bool) -> String {
    let token_script = if with_token {
        "<script>viewModel.Token('tok123');</script>"
    } else {
        ""
    };
    format!(
        "<html><body>\
         <span id=\"invoiceNumberLabel\">INV-1</span>\
         <span id=\"shopFullNameLabel\">Maxi</span>\
         <span id=\"sdcDateTimeLabel\">15.03.2024. 14:30:00</span>\
         <span id=\"totalAmountLabel\">179,98</span>\
         <div id=\"collapse3\"><div><pre>receipt</pre></div></div>\
         {token_script}\
         </body></html>"
    )
}

/// A page missing the invoice-number element — parsing fails fast (no retry).
pub fn invalid_page() -> String {
    "<html><body><span id=\"shopFullNameLabel\">Maxi</span></body></html>".into()
}

pub fn items_json() -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "items": [{
            "name": "Mleko", "quantity": 2.0, "unitPrice": 89.99, "total": 179.98,
            "gtin": "", "label": "", "labelRate": 0.0, "taxBaseAmount": 0.0, "vatAmount": 0.0
        }]
    })
}

pub fn test_config() -> Config {
    Config {
        transaction_dir: PathBuf::from("/mem"),
        data_dir: PathBuf::from("/mem"),
        queue_file: PathBuf::from("/mem/queue.txt"),
        failed_links_file: PathBuf::from("/mem/failed.txt"),
        api_host: "localhost".into(),
        api_port: 8087,
        api_endpoint: "/queue".into(),
        schema_file: None,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::InsertArgs;
    use crate::commands::{insert, sync};
    use crate::config::{self};
    use crate::invoice::parser;

    fn insert_args() -> InsertArgs {
        InsertArgs {
            url: None,
            file: None,
            dry_run: false,
            no_sync: true,
            force: false,
        }
    }

    #[test]
    fn parser_happy_path() {
        let http = FakeHttp::with_pages(vec![invoice_page(true)], vec![items_json()]);
        let inv = parser::parse("http://x", &http).unwrap();
        assert_eq!(inv.retailer, "Maxi");
        assert_eq!(inv.date, "2024-03-15T14:30:00");
        assert_eq!(inv.items.len(), 1);
        assert_eq!(inv.items[0].name, "Mleko");
    }

    #[test]
    fn parser_retries_on_token_failure_then_succeeds() {
        // First page lacks the token (retryable); second page has it.
        let http = FakeHttp::with_pages(
            vec![invoice_page(false), invoice_page(true)],
            vec![items_json()],
        );
        // Zero retry delay keeps the test instant (prod uses 1s).
        let inv =
            parser::parse_with_retries("http://x", &http, 3, std::time::Duration::ZERO).unwrap();
        assert_eq!(inv.items.len(), 1);
    }

    #[test]
    fn parser_fails_fast_on_non_token_error() {
        // Missing invoice number → "element not found" → no retry, pages left.
        let http = FakeHttp::with_pages(vec![invalid_page(), invoice_page(true)], vec![]);
        assert!(parser::parse("http://x", &http).is_err());
        // The second page must remain unconsumed (proves no retry happened).
        assert_eq!(http.pages.borrow().len(), 1);
    }

    #[test]
    fn insert_queues_when_offline() {
        let tp = TestPlatform {
            network: ToggleNetwork(false),
            ..Default::default()
        };

        let outcome = insert::run_one("http://x/1", &insert_args(), &tp).unwrap();

        assert!(matches!(outcome, insert::Outcome::Queued { .. }));
        assert_eq!(tp.queue.urls.borrow().as_slice(), ["http://x/1"]);
        assert!(tp.transactions.docs.borrow().is_empty());
    }

    #[test]
    fn insert_skips_duplicate() {
        let tp = TestPlatform {
            transactions: MemTransactions::with_docs(vec![StoredDoc {
                path: PathBuf::from("/mem/old.md"),
                content: "link: \"http://x/1\"".into(),
            }]),
            ..Default::default()
        };

        let outcome = insert::run_one("http://x/1", &insert_args(), &tp).unwrap();

        // No new doc written.
        assert!(matches!(outcome, insert::Outcome::Skipped { .. }));
        assert_eq!(tp.transactions.docs.borrow().len(), 1);
    }

    #[test]
    fn insert_happy_path_writes_file() {
        let tp = TestPlatform {
            http: FakeHttp::with_pages(vec![invoice_page(true)], vec![items_json()]),
            ..Default::default()
        };

        let outcome = insert::run_one("http://x/1", &insert_args(), &tp).unwrap();

        assert!(matches!(outcome, insert::Outcome::Saved { files: 1, .. }));
        let docs = tp.transactions.docs.borrow();
        assert_eq!(docs.len(), 1);
        assert!(docs[0].content.contains("name: \"Mleko\""));
        // Sync is a batch-level step, so the per-URL checklist is parse + save only.
        assert_eq!(
            tp.progress.last_phases(),
            ["Parse invoice", "Save line items"]
        );
    }

    #[test]
    fn sync_offline_commits_without_push() {
        let tp = TestPlatform {
            network: ToggleNetwork(false), // forces offline
            ..Default::default()
        };
        let cfg = test_config();

        sync::run(
            crate::cli::SyncArgs {
                message: None,
                no_push: false,
            },
            &cfg,
            &tp,
        )
        .unwrap();

        let calls = tp.vcs.calls.borrow();
        assert!(calls.iter().any(|c| c.starts_with("commit:")));
        assert!(!calls.iter().any(|c| c.starts_with("push:")));
        assert!(tp.reporter.status_contains("Offline"));
    }

    #[test]
    fn sync_nothing_to_commit_when_clean() {
        let tp = TestPlatform {
            vcs: FakeVcs {
                is_repo: true,
                dirty: false,
                ..FakeVcs::default()
            },
            ..Default::default()
        };
        let cfg = test_config();

        sync::run(
            crate::cli::SyncArgs {
                message: None,
                no_push: false,
            },
            &cfg,
            &tp,
        )
        .unwrap();

        assert!(tp.vcs.calls.borrow().is_empty());
        assert!(tp.reporter.out_contains("Nothing to commit"));
    }

    #[test]
    fn queue_process_local_drains_successes_keeps_failures() {
        use crate::cli::{QueueArgs, QueueCommand};
        use crate::commands::queue;

        // url1 parses; url2 hits the invalid page and fails.
        let tp = TestPlatform {
            queue: MemQueue::with(vec!["http://x/1".into(), "http://x/2".into()]),
            http: FakeHttp::with_pages(
                vec![invoice_page(true), invalid_page()],
                vec![items_json()],
            ),
            ..Default::default()
        };
        let cfg = test_config();

        let res = queue::run(
            QueueArgs {
                command: QueueCommand::Process {
                    remote: false,
                    no_sync: true,
                },
            },
            &cfg,
            &tp,
        );

        assert!(res.is_err()); // one URL failed
        assert_eq!(tp.queue.urls.borrow().as_slice(), ["http://x/2"]);
    }

    #[test]
    fn config_env_overrides_file() {
        let env = MapEnv::default().set(EnvVar::TransactionDir, "/from/env");
        let file = "transaction_dir: /from/file\n";
        let cfg = config::parse(Some(file), &env).unwrap();
        assert_eq!(cfg.transaction_dir, PathBuf::from("/from/env"));
    }

    #[test]
    fn config_falls_back_to_file_then_default() {
        let env = MapEnv::default().set(EnvVar::Home, "/home/u");
        let file = "transaction_dir: /from/file\n";
        let cfg = config::parse(Some(file), &env).unwrap();
        assert_eq!(cfg.transaction_dir, PathBuf::from("/from/file"));
        // data_dir defaults to transaction_dir.
        assert_eq!(cfg.data_dir, PathBuf::from("/from/file"));
        // api_port default.
        assert_eq!(cfg.api_port, 8087);
    }
}
