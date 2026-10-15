//! No library file in the browser: the library lives in memory.

use std::path::Path;

use crate::records::{Changes, Row};

#[derive(Debug)]
pub enum OpenError {
    Unreadable(String),
    Other(String),
}

#[derive(Debug)]
pub struct Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub row: Row,
    pub seq: u64,
}

const NONE: &str = "no library file in the browser";

impl Store {
    pub fn open(_path: &Path) -> Result<Self, OpenError> {
        Err(OpenError::Other(NONE.into()))
    }

    pub fn newer(&self) -> bool {
        false
    }

    pub fn rows(&self) -> Result<Vec<Row>, String> {
        Err(NONE.into())
    }

    pub fn meta(&self, _key: &str) -> Result<Option<String>, String> {
        Err(NONE.into())
    }

    pub fn apply(&mut self, _changes: &Changes) -> Result<(), String> {
        Err(NONE.into())
    }

    pub fn changed_since(&self, _seq: u64) -> Result<Vec<Change>, String> {
        Err(NONE.into())
    }
}
