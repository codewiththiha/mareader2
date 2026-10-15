//! Fields a newer Mareader wrote: kept and merged, though never read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock::Stamp;

/// A record's fields this build doesn't know, by name.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Extra(BTreeMap<String, Value>);

impl Extra {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }

    pub fn insert(&mut self, name: &str, value: Value) {
        self.0.insert(name.to_owned(), value);
    }

    /// Per field, the later stamp wins, as a known field's would.
    pub fn merge(&mut self, other: Self) {
        for (name, theirs) in other.0 {
            match self.0.get_mut(&name) {
                Some(mine) if rank(&theirs) > rank(mine) => *mine = theirs,
                Some(_) => {}
                None => {
                    self.0.insert(name, theirs);
                }
            }
        }
    }

    /// The latest stamp among the fields, for the clock to pass.
    pub fn latest(&self) -> Option<Stamp> {
        self.0.values().filter_map(stamp).max()
    }
}

/// A stamped field is `{v, t}`, as ours are.
fn stamp(value: &Value) -> Option<Stamp> {
    let text = value.get("t")?.as_str()?;
    Stamp::try_from(text.to_owned()).ok()
}

/// By stamp, then by text: any two values order the same everywhere.
fn rank(value: &Value) -> (Option<Stamp>, String) {
    (stamp(value), value.to_string())
}
