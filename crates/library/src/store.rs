//! The library's SQLite file: synced records, local rows, a change cursor.

use std::path::Path;

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

use crate::records::{Changes, Kind, Row};

/// Each step makes one version; append only, never edit a shipped one.
const MIGRATIONS: &[(&str, &str)] = &[(
    "2026-10-15 records, local rows and meta",
    "CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
     CREATE TABLE records(
         kind TEXT NOT NULL,
         id TEXT NOT NULL,
         body TEXT NOT NULL,
         seq INTEGER NOT NULL,
         PRIMARY KEY(kind, id)
     ) WITHOUT ROWID;
     CREATE INDEX records_seq ON records(seq);
     CREATE TABLE local(
         kind TEXT NOT NULL,
         id TEXT NOT NULL,
         body TEXT NOT NULL,
         PRIMARY KEY(kind, id)
     ) WITHOUT ROWID;",
)];

/// Why the file couldn't be opened.
#[derive(Debug)]
pub enum OpenError {
    /// Not a database, or a damaged one: keep it aside and start again.
    Unreadable(String),
    Other(String),
}

impl OpenError {
    fn of(e: &rusqlite::Error) -> Self {
        match e.sqlite_error_code() {
            Some(ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt) => {
                Self::Unreadable(e.to_string())
            }
            _ => Self::Other(e.to_string()),
        }
    }
}

/// An open library file; one writer, the library's disk thread.
pub struct Store {
    conn: Connection,
    /// The file's version, past ours when a newer Mareader wrote it.
    version: usize,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("version", &self.version)
            .finish()
    }
}

/// A record and the save that last wrote it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub row: Row,
    pub seq: u64,
}

impl Store {
    /// Opens or makes the file and brings it up to date, unless newer.
    pub fn open(path: &Path) -> Result<Self, OpenError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| OpenError::Other(e.to_string()))?;
        }
        let conn = Connection::open(path).map_err(|e| OpenError::of(&e))?;
        let version = Self::prepare(&conn).map_err(|e| OpenError::of(&e))?;
        let mut store = Self { conn, version };
        if !store.newer() {
            store.migrate().map_err(|e| OpenError::of(&e))?;
        }
        Ok(store)
    }

    /// Settings for one writer that a crash can't corrupt.
    fn prepare(conn: &Connection) -> rusqlite::Result<usize> {
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        Ok(usize::try_from(version).unwrap_or(0))
    }

    fn migrate(&mut self) -> rusqlite::Result<()> {
        for (n, (_, sql)) in MIGRATIONS.iter().enumerate().skip(self.version) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i64::try_from(n + 1).unwrap_or(0))?;
            tx.commit()?;
            self.version = n + 1;
        }
        Ok(())
    }

    /// A newer Mareader wrote it: read what we can, write nothing.
    pub fn newer(&self) -> bool {
        self.version > MIGRATIONS.len()
    }

    /// Every row of a kind this build knows; others are left be.
    pub fn rows(&self) -> Result<Vec<Row>, String> {
        let read = || -> rusqlite::Result<Vec<Row>> {
            let sql =
                "SELECT kind, id, body FROM records UNION ALL SELECT kind, id, body FROM local";
            let mut statement = self.conn.prepare(sql)?;
            let rows =
                statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (kind, id, body) = row?;
                if let Some(kind) = Kind::parse(&kind) {
                    out.push(Row { kind, id, body });
                }
            }
            Ok(out)
        };
        read().map_err(|e| e.to_string())
    }

    /// A value kept beside the rows, such as the clock.
    pub fn meta(&self, key: &str) -> Result<Option<String>, String> {
        let sql = "SELECT value FROM meta WHERE key = ?1";
        let value = self.conn.query_row(sql, [key], |r| r.get(0)).optional();
        value.map_err(|e| e.to_string())
    }

    /// Writes one save whole, or none of it.
    pub fn apply(&mut self, changes: &Changes) -> Result<(), String> {
        if self.newer() {
            return Err("a newer Mareader saved this library".into());
        }
        self.write(changes).map_err(|e| e.to_string())
    }

    fn write(&mut self, changes: &Changes) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        let last: Option<String> = tx
            .query_row("SELECT value FROM meta WHERE key = 'seq'", [], |r| r.get(0))
            .optional()?;
        let seq = last.and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) + 1;
        let seq = i64::try_from(seq).unwrap_or(i64::MAX);
        {
            let mut record = tx.prepare(
                "INSERT INTO records(kind, id, body, seq) VALUES(?1, ?2, ?3, ?4)
                 ON CONFLICT(kind, id) DO UPDATE SET body = ?3, seq = ?4",
            )?;
            let mut local = tx.prepare(
                "INSERT INTO local(kind, id, body) VALUES(?1, ?2, ?3)
                 ON CONFLICT(kind, id) DO UPDATE SET body = ?3",
            )?;
            let mut gone = tx.prepare("DELETE FROM local WHERE kind = ?1 AND id = ?2")?;
            for row in &changes.put {
                let (kind, id, body) = (row.kind.name(), &row.id, &row.body);
                match row.kind.synced() {
                    true => record.execute(params![kind, id, body, seq])?,
                    false => local.execute(params![kind, id, body])?,
                };
            }
            // Records are never deleted: a removal is a tombstone in one.
            for (kind, id) in changes.gone.iter().filter(|(k, _)| !k.synced()) {
                gone.execute(params![kind.name(), id])?;
            }
            let mut meta = tx.prepare(
                "INSERT INTO meta(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?2",
            )?;
            if let Some(clock) = &changes.clock {
                meta.execute(params!["clock", clock])?;
            }
            if changes.put.iter().any(|r| r.kind.synced()) {
                meta.execute(params!["seq", seq.to_string()])?;
            }
        }
        tx.commit()
    }

    /// Records written after save `seq`: what a sync would send.
    pub fn changed_since(&self, seq: u64) -> Result<Vec<Change>, String> {
        let read = || -> rusqlite::Result<Vec<Change>> {
            let sql = "SELECT kind, id, body, seq FROM records WHERE seq > ?1 ORDER BY seq";
            let mut statement = self.conn.prepare(sql)?;
            let since = i64::try_from(seq).unwrap_or(i64::MAX);
            let rows = statement.query_map([since], |r| {
                let kind: String = r.get(0)?;
                let seq: i64 = r.get(3)?;
                Ok((kind, r.get(1)?, r.get(2)?, seq))
            })?;
            let mut out = Vec::new();
            for row in rows {
                let (kind, id, body, seq) = row?;
                if let Some(kind) = Kind::parse(&kind) {
                    let seq = u64::try_from(seq).unwrap_or(0);
                    out.push(Change {
                        row: Row { kind, id, body },
                        seq,
                    });
                }
            }
            Ok(out)
        };
        read().map_err(|e| e.to_string())
    }
}
