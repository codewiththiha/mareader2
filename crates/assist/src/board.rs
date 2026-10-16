//! What the AI settings show, and the jobs they start. Plain data.

use std::collections::HashMap;

use mareader_settings::Provider;
use mareader_study::Failure;

/// Something fetched in the background.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Load<T> {
    /// Not asked for yet.
    #[default]
    Idle,
    Busy,
    Ready(T),
    Failed(Failure),
}

impl<T> Load<T> {
    fn of(r: Result<T, Failure>) -> Self {
        r.map_or_else(Self::Failed, Self::Ready)
    }

    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(t) => Some(t),
            _ => None,
        }
    }
}

/// Whether Apple's model can answer on this Mac.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppleState {
    Ready,
    /// Still downloading or preparing.
    NotReady(String),
    /// Switched off in System Settings.
    Off(String),
    /// Not on this Mac, or the helper is missing.
    Missing(String),
}

/// A stored key's health.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyState {
    Ready,
    /// Rate limited; back by itself.
    Resting {
        secs: u64,
    },
    /// Turned away, set aside by hand, or no longer decryptable.
    Banned {
        reason: String,
    },
}

/// One key, as it may be shown: never the key itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRow {
    pub id: String,
    /// The start and end, like `sk-proj…8f92`.
    pub shown: String,
    pub state: KeyState,
}

/// A trial answer from the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tried {
    pub millis: u64,
    pub meaning: String,
}

/// Background work the settings ask for.
#[derive(Clone, PartialEq, Eq)]
pub enum Job {
    /// Whether Apple Intelligence is ready.
    Probe,
    Keys(Provider),
    AddKey(Provider, String),
    RemoveKey(Provider, String),
    BanKey(Provider, String),
    RecoverKey(Provider, String),
    Models(Provider),
    /// Explains a sample word with the provider as set.
    Try(Provider),
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // A key must not reach a log.
            Self::AddKey(p, _) => write!(f, "AddKey({p:?}, <secret>)"),
            Self::Probe => write!(f, "Probe"),
            Self::Keys(p) => write!(f, "Keys({p:?})"),
            Self::RemoveKey(p, id) => write!(f, "RemoveKey({p:?}, {id})"),
            Self::BanKey(p, id) => write!(f, "BanKey({p:?}, {id})"),
            Self::RecoverKey(p, id) => write!(f, "RecoverKey({p:?}, {id})"),
            Self::Models(p) => write!(f, "Models({p:?})"),
            Self::Try(p) => write!(f, "Try({p:?})"),
        }
    }
}

/// A finished job.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Apple(AppleState),
    /// The keys after a change; `problem` if the change itself failed.
    Keys {
        provider: Provider,
        keys: Result<Vec<KeyRow>, Failure>,
        problem: Option<Failure>,
    },
    Models(Provider, Result<Vec<String>, Failure>),
    Tried(Provider, Result<Tried, Failure>),
}

/// Everything the AI settings show that isn't a setting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub apple: Load<AppleState>,
    pub keys: HashMap<Provider, Load<Vec<KeyRow>>>,
    /// The last key change that failed, per provider.
    pub problems: HashMap<Provider, Failure>,
    pub models: HashMap<Provider, Load<Vec<String>>>,
    pub tried: HashMap<Provider, Load<Tried>>,
}

impl Board {
    pub fn keys(&self, p: Provider) -> &Load<Vec<KeyRow>> {
        self.keys.get(&p).unwrap_or(&Load::Idle)
    }

    pub fn models(&self, p: Provider) -> &Load<Vec<String>> {
        self.models.get(&p).unwrap_or(&Load::Idle)
    }

    pub fn tried(&self, p: Provider) -> &Load<Tried> {
        self.tried.get(&p).unwrap_or(&Load::Idle)
    }

    /// Marks what `job` will refresh as on its way.
    pub fn start(&mut self, job: &Job) {
        match job {
            Job::Probe => self.apple = Load::Busy,
            Job::Keys(p)
            | Job::AddKey(p, _)
            | Job::RemoveKey(p, _)
            | Job::BanKey(p, _)
            | Job::RecoverKey(p, _) => {
                // Rows stay on screen while a change is made.
                self.keys.entry(*p).or_insert(Load::Busy);
                self.problems.remove(p);
            }
            Job::Models(p) => {
                self.models.insert(*p, Load::Busy);
            }
            Job::Try(p) => {
                self.tried.insert(*p, Load::Busy);
            }
        }
    }

    pub fn finish(&mut self, reply: Reply) {
        match reply {
            Reply::Apple(state) => self.apple = Load::Ready(state),
            Reply::Keys {
                provider,
                keys,
                problem,
            } => {
                self.keys.insert(provider, Load::of(keys));
                if let Some(p) = problem {
                    self.problems.insert(provider, p);
                }
            }
            Reply::Models(p, models) => {
                self.models.insert(p, Load::of(models));
            }
            Reply::Tried(p, tried) => {
                self.tried.insert(p, Load::of(tried));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mareader_study::Why;

    use super::*;

    #[test]
    fn jobs_mark_what_they_refresh_and_replies_land() {
        let mut b = Board::default();
        let p = Provider::OpenAi;
        b.start(&Job::Keys(p));
        assert_eq!(b.keys(p), &Load::Busy);
        let row = KeyRow {
            id: "k1".into(),
            shown: "sk-…1234".into(),
            state: KeyState::Ready,
        };
        let problem = Some(Failure::from(Why::Setup));
        let keys = Ok(vec![row.clone()]);
        b.finish(Reply::Keys {
            provider: p,
            keys,
            problem,
        });
        assert_eq!(b.keys(p).ready().map(Vec::len), Some(1));
        assert!(b.problems.contains_key(&p));
        b.start(&Job::RemoveKey(p, "k1".into()));
        assert_eq!(b.keys(p).ready(), Some(&vec![row]), "rows stay meanwhile");
        assert!(!b.problems.contains_key(&p));
        assert_eq!(b.models(Provider::Gemini), &Load::Idle);
    }

    #[test]
    fn a_job_never_shows_its_key() {
        let job = Job::AddKey(Provider::Gemini, "sk-secret".into());
        assert!(!format!("{job:?}").contains("sk-secret"));
    }
}
