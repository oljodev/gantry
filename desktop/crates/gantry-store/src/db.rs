//! Opening the database, migrations, the writer actor and the read pool (06 §1, §6, §8).

use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        mpsc::{self, Sender},
    },
    thread,
};

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations, SchemaVersion};

const MIGRATIONS: &[M<'static>] = &[
    M::up(include_str!("../migrations/0001_init.sql")),
    M::up(include_str!("../migrations/0002_chats.sql")),
    M::up(include_str!("../migrations/0003_tools.sql")),
    M::up(include_str!("../migrations/0004_artifacts.sql")),
    M::up(include_str!("../migrations/0005_grants.sql")),
    M::up(include_str!("../migrations/0006_connectors.sql")),
    M::up(include_str!("../migrations/0007_tool_schemas.sql")),
    M::up(include_str!("../migrations/0008_surfaces.sql")),
    M::up(include_str!("../migrations/0009_file_edits.sql")),
    M::up(include_str!("../migrations/0010_edit_origin.sql")),
    M::up(include_str!("../migrations/0011_model_release.sql")),
    M::up(include_str!("../migrations/0012_skills_memory.sql")),
    M::up(include_str!("../migrations/0013_incognito.sql")),
    M::up(include_str!("../migrations/0014_projects.sql")),
    M::up(include_str!("../migrations/0015_blob_sweep.sql")),
    M::up(include_str!("../migrations/0016_sub_agents.sql")),
];
const READERS: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("migration: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    #[error(
        "the database was written by a newer Gantry (schema {found}, this build knows {supported})"
    )]
    NewerSchema { found: usize, supported: usize },
    #[error("the database writer has stopped")]
    WriterGone,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl From<StoreError> for gantry_core::GantryError {
    fn from(err: StoreError) -> Self {
        gantry_core::GantryError::Store(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// The open database: a dedicated writer thread and a pool of read connections.
pub struct Store {
    path: PathBuf,
    writer: Sender<Job>,
    readers: Mutex<Vec<Connection>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").field("path", &self.path).finish()
    }
}

impl Store {
    /// Opens (or creates) the database at `path`, backs it up before applying pending
    /// migrations, refuses a database from a newer schema, then starts the writer.
    pub fn open(path: impl AsRef<Path>) -> Result<Store> {
        let path = path.as_ref().to_path_buf();
        let mut writer = Connection::open(&path)?;
        configure(&writer)?;
        writer.pragma_update(None, "journal_mode", "WAL")?;
        migrate(&mut writer, &path)?;

        let mut readers = Vec::with_capacity(READERS);
        for _ in 0..READERS {
            let conn = Connection::open(&path)?;
            configure(&conn)?;
            readers.push(conn);
        }

        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("gantry-store-writer".into())
            .spawn(move || {
                for job in rx {
                    job(&mut writer);
                }
            })?;

        Ok(Store {
            path,
            writer: tx,
            readers: Mutex::new(readers),
        })
    }

    /// The database file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `f` on the writer thread and awaits its result.
    pub async fn write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.writer
            .send(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            }))
            .map_err(|_| StoreError::WriterGone)?;
        rx.await.map_err(|_| StoreError::WriterGone)?
    }

    /// Queues `f` on the writer thread and returns at once. Jobs run in the order they were
    /// queued, so a detached write lands before any later write; failures are logged.
    pub fn write_detached<F>(&self, f: F)
    where
        F: FnOnce(&mut Connection) -> Result<()> + Send + 'static,
    {
        let sent = self.writer.send(Box::new(move |conn| {
            if let Err(err) = f(conn) {
                log::error!("detached write failed: {err}");
            }
        }));
        if sent.is_err() {
            log::error!("detached write dropped: the writer has stopped");
        }
    }

    /// Copies the database to `dest` with `VACUUM INTO`, consistent even while the app runs
    /// (docs/plan/11 §2, Data & privacy). Credentials are excluded by clearing them in the copy.
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        let dest = dest.to_path_buf();
        if dest.exists() {
            std::fs::remove_file(&dest)?;
        }
        self.write_blocking(move |conn| {
            conn.execute("VACUUM INTO ?1", [dest.to_string_lossy().as_ref()])?;
            let copy = Connection::open(&dest)?;
            copy.execute("UPDATE providers SET credential_id = NULL", [])?;
            copy.execute("DELETE FROM credentials", [])?;
            Ok(())
        })
    }

    /// `PRAGMA integrity_check`; `Ok(())` when SQLite answers `ok`.
    pub fn integrity_check(&self) -> Result<()> {
        let answer: String =
            self.read(|c| Ok(c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?))?;
        if answer == "ok" {
            Ok(())
        } else {
            Err(StoreError::Other(answer))
        }
    }

    /// Rebuilds the file; never automatic (06 §6).
    pub fn vacuum(&self) -> Result<()> {
        self.write_blocking(|conn| {
            conn.execute("VACUUM", [])?;
            Ok(())
        })
    }

    /// Runs `f` on the writer thread and blocks the caller until it is done. For startup and
    /// tests; never call it from an async task.
    pub fn write_blocking<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let (tx, rx) = mpsc::sync_channel(1);
        self.writer
            .send(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            }))
            .map_err(|_| StoreError::WriterGone)?;
        rx.recv().map_err(|_| StoreError::WriterGone)?
    }

    /// Runs `f` on a pooled read connection. Reads are short; the caller holds one connection
    /// for the duration and the pool blocks when all are busy.
    pub fn read<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = loop {
            let taken = self.readers.lock().unwrap_or_else(|e| e.into_inner()).pop();
            match taken {
                Some(conn) => break conn,
                None => thread::yield_now(),
            }
        };
        let result = f(&conn);
        self.readers
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(conn);
        result
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let migrations = Migrations::from_slice(MIGRATIONS);
    let supported = MIGRATIONS.len();
    match migrations.current_version(conn)? {
        SchemaVersion::Outside(found) => {
            return Err(StoreError::NewerSchema {
                found: found.get(),
                supported,
            });
        }
        SchemaVersion::Inside(current) if current.get() < supported => {
            // A real upgrade of an existing file: keep a copy first (06 §6).
            let backup = path.with_extension(format!("db.bak-{}", current.get()));
            std::fs::copy(path, &backup)?;
            log::info!("backed up the database to {}", backup.display());
        }
        _ => {}
    }
    migrations.to_latest(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_migrates_and_round_trips_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("t.db")).unwrap();
        store
            .write_blocking(|c| {
                c.execute(
                    "INSERT INTO settings (key, value_json, updated_at) VALUES ('a', '1', 0)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let n: i64 = store
            .read(|c| Ok(c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 1);
        let v: i64 = store
            .read(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(v, MIGRATIONS.len() as i64);
    }

    #[test]
    fn refuses_a_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        {
            let c = Connection::open(&path).unwrap();
            c.pragma_update(None, "user_version", 999).unwrap();
        }
        let err = Store::open(&path).err().unwrap();
        assert!(matches!(err, StoreError::NewerSchema { found: 999, .. }));
    }
}
