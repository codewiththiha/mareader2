//! What the app holds: a runtime off the UI thread, clients, jobs.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use mareader_settings::{AiSettings, Provider};
use mareader_study::{Explainer, Failure, Why, WordInfo};
use serde_json::Value;
use tokio::runtime::{Builder, Runtime};

use crate::backend::{Backend, BoxFuture, Nobody, Partial, run};
use crate::board::{AppleState, Board, Job, Reply, Tried};
use crate::compat::{Compat, failure_with_keys};
use crate::keys::{Clients, pasted, row};
use crate::prompt::Prompt;
use crate::task::Explain;

/// Wakes the UI when a job finishes.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// The sample a trial explains.
const SAMPLE: Explain<'static> = Explain {
    word: "quick",
    context: "The quick brown fox jumps over the lazy dog.",
};

/// The runtime, shut down without waiting, from wherever it drops.
struct Engine(Option<Runtime>);

impl Engine {
    fn rt(&self) -> Option<&Runtime> {
        self.0.as_ref()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(rt) = self.0.take() {
            rt.shutdown_background();
        }
    }
}

/// AI for the app: answerers for the study tools, jobs for the settings.
pub struct Assist {
    root: PathBuf,
    engine: Arc<Engine>,
    clients: Arc<Clients>,
    apple: Option<Arc<crate::Apple>>,
    ai: AiSettings,
    tx: Sender<Reply>,
    rx: Receiver<Reply>,
    waker: Waker,
    board: Board,
}

impl Assist {
    /// Keys are kept under `root`; `waker` runs as each job ends.
    pub fn new(root: PathBuf, waker: Waker) -> std::io::Result<Self> {
        let rt = Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mareader-assist")
            .enable_all()
            .build()?;
        let (tx, rx) = channel();
        Ok(Self {
            engine: Arc::new(Engine(Some(rt))),
            clients: Arc::new(Clients::new(root.clone())),
            root,
            apple: locate_apple(),
            ai: AiSettings::default(),
            tx,
            rx,
            waker,
            board: Board::default(),
        })
    }

    /// Keeps the master key in a file under the root, never the keychain.
    pub fn without_keychain(mut self) -> Self {
        let root = self.root.clone();
        self.clients = Arc::new(Clients::new(root).without_keychain());
        self
    }

    /// Uses `apple` for Apple Intelligence, as tests do with a mock.
    pub fn with_apple(mut self, apple: crate::Apple) -> Self {
        self.apple = Some(Arc::new(apple));
        self
    }

    /// The answerer for `ai`; later jobs use these settings too.
    pub fn configure(&mut self, ai: &AiSettings) -> Arc<dyn Explainer> {
        self.ai = ai.clone();
        Arc::new(AiExplainer {
            engine: Arc::clone(&self.engine),
            backend: self.backend(ai.provider),
        })
    }

    /// Who answers for `p`, as configured.
    pub fn backend(&self, p: Provider) -> Arc<dyn Backend> {
        match (p, &self.apple) {
            (Provider::Auto | Provider::Apple, Some(apple)) => {
                Arc::clone(apple) as Arc<dyn Backend>
            }
            (Provider::Auto, None) => Arc::new(Nobody(Why::NotHere.into())),
            (Provider::Apple, None) => Arc::new(Nobody(no_apple())),
            (server, _) => Arc::new(Server {
                clients: Arc::clone(&self.clients),
                provider: server,
                url: self.ai.url(server),
                model: self.ai.model(server),
            }),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Takes in finished jobs; true if any changed the board.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(reply) = self.rx.try_recv() {
            self.board.finish(reply);
            changed = true;
        }
        changed
    }

    /// Starts `job` in the background; [`Self::poll`] takes its reply.
    pub fn send(&mut self, job: Job) {
        let Some(rt) = self.engine.rt() else {
            return;
        };
        self.board.start(&job);
        let work = Work {
            clients: Arc::clone(&self.clients),
            apple: self.apple.clone(),
            ai: self.ai.clone(),
            trial: match &job {
                Job::Try(p) => Some(self.backend(*p)),
                _ => None,
            },
        };
        let (tx, waker) = (self.tx.clone(), Arc::clone(&self.waker));
        rt.spawn(async move {
            let _ = tx.send(work.run(job).await);
            waker();
        });
    }
}

#[cfg(target_os = "macos")]
fn locate_apple() -> Option<Arc<crate::Apple>> {
    crate::Apple::locate().ok().map(Arc::new)
}

#[cfg(not(target_os = "macos"))]
fn locate_apple() -> Option<Arc<crate::Apple>> {
    None
}

fn no_apple() -> Failure {
    let why = match cfg!(target_os = "macos") {
        true => "this build has no Apple Intelligence helper",
        false => "Apple Intelligence runs only on a Mac",
    };
    Failure::detailed(Why::NotHere, why)
}

/// One provider's server, its client built on first use.
struct Server {
    clients: Arc<Clients>,
    provider: Provider,
    url: String,
    model: String,
}

impl Backend for Server {
    fn ask<'a>(
        &'a self,
        prompt: &'a Prompt,
        partial: Partial<'a>,
    ) -> BoxFuture<'a, Result<Value, Failure>> {
        Box::pin(async move {
            let client = self.clients.asking(self.provider, &self.url).await?;
            let compat = Compat::new(client, self.model.as_str());
            compat.ask(prompt, partial).await
        })
    }
}

/// The study tools' answerer: blocks its worker thread, not the UI.
struct AiExplainer {
    engine: Arc<Engine>,
    backend: Arc<dyn Backend>,
}

impl Explainer for AiExplainer {
    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        let rt = self.engine.rt().ok_or(Why::NotHere)?;
        let task = Explain { word, context };
        rt.block_on(run(self.backend.as_ref(), &task, partial))
    }
}

/// What a job needs, moved onto the runtime.
struct Work {
    clients: Arc<Clients>,
    apple: Option<Arc<crate::Apple>>,
    ai: AiSettings,
    trial: Option<Arc<dyn Backend>>,
}

impl Work {
    async fn run(self, job: Job) -> Reply {
        match job {
            Job::Probe => Reply::Apple(match &self.apple {
                Some(apple) => apple.probe().await,
                None => AppleState::Missing(no_apple().detail.unwrap_or_default()),
            }),
            Job::Keys(p) => self.keys(p, None).await,
            Job::AddKey(p, key) => {
                let problem = match pasted(&key) {
                    Ok(key) => self.change(p, Change::Add(key)).await,
                    Err(f) => Some(f),
                };
                self.keys(p, problem).await
            }
            Job::RemoveKey(p, id) => self.changed(p, Change::Remove(&id)).await,
            Job::BanKey(p, id) => self.changed(p, Change::Ban(&id)).await,
            Job::RecoverKey(p, id) => self.changed(p, Change::Recover(&id)).await,
            Job::Models(p) => Reply::Models(p, self.models(p).await),
            Job::Try(p) => Reply::Tried(p, self.trial().await),
        }
    }

    async fn changed(&self, p: Provider, change: Change<'_>) -> Reply {
        let problem = self.change(p, change).await;
        self.keys(p, problem).await
    }

    /// Makes `change`; the failure, if any.
    async fn change(&self, p: Provider, change: Change<'_>) -> Option<Failure> {
        let client = match self.clients.client(p, &self.ai.url(p), true).await {
            Ok(c) => c,
            Err(f) => return Some(f),
        };
        let keys = client.manager();
        let done = match change {
            Change::Add(key) => keys.add_key(key).await.map(drop),
            Change::Remove(id) => keys.remove_key(id).await,
            Change::Ban(id) => keys.ban_key(id).await,
            Change::Recover(id) => keys.recover_key(id).await,
        };
        done.err().map(|e| crate::compat::failure(&e))
    }

    async fn keys(&self, p: Provider, problem: Option<Failure>) -> Reply {
        let keys = match self.clients.client(p, &self.ai.url(p), true).await {
            Ok(client) => client
                .manager()
                .list_keys()
                .await
                .map_err(|e| crate::compat::failure(&e)),
            Err(f) => Err(f),
        };
        Reply::Keys {
            provider: p,
            keys: keys.map(|k| k.into_iter().map(row).collect()),
            problem,
        }
    }

    async fn models(&self, p: Provider) -> Result<Vec<String>, Failure> {
        let client = self.clients.asking(p, &self.ai.url(p)).await?;
        match client.models().await {
            Ok(models) => Ok(models.into_iter().map(|m| m.id).collect()),
            Err(e) => Err(failure_with_keys(&client, e).await),
        }
    }

    async fn trial(&self) -> Result<Tried, Failure> {
        let backend = self.trial.as_ref().ok_or(Why::NotHere)?;
        let start = Instant::now();
        let info = run(backend.as_ref(), &SAMPLE, &|_| {}).await?;
        Ok(Tried {
            millis: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            meaning: info.meaning,
        })
    }
}

enum Change<'a> {
    Add(&'a str),
    Remove(&'a str),
    Ban(&'a str),
    Recover(&'a str),
}
