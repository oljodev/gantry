//! The blob sweep (06 §3, §8): delete the bytes nothing references any more.
//!
//! Blobs are the one thing in the data model that deletion does not reach through a foreign key.
//! A chat's rows cascade away when the chat goes; the megabytes its PDFs and its artifact
//! versions put on disk do not, because a blob is content-addressed and two chats may be holding
//! the same one. Something has to ask, of each file, whether anybody still wants it.
//!
//! **What "referenced" means.** Not a stored count — [`repos::blobs::reachable`] reads the
//! referencing tables, so a reference exists exactly when a row says it does. A path that forgets
//! to release therefore cannot leak, and a path that forgets to take cannot over-delete; the one
//! thing that can go wrong is a *new* table whose column nobody added to
//! [`repos::blobs::HASH_COLUMNS`], which is why a test asks SQLite for that list.
//!
//! **The grace period.** Bytes are written before the row that references them — the ingest that
//! stores an attachment runs before the message carrying it is inserted, and a retried turn's
//! rows are deleted before the new turn's are written. A sweep landing inside one of those gaps
//! would delete a file its reference is milliseconds away from claiming. So a blob is only ever
//! collected once its file has sat untouched for [`GRACE`], and [`BlobStore::put`] moves that
//! time to now every time the bytes are handed out. The gaps are milliseconds; the grace is
//! minutes.

use std::{fs, path::Path, time::Duration};

use rusqlite::Connection;

use crate::{BlobStore, db::Result, repos::blobs};

/// How long a blob's bytes are left alone after they were last written or handed out.
///
/// The gap it covers is a `put` and the write that references what was put: milliseconds
/// everywhere except an attachment, where a large PDF is read and its text extracted in between,
/// and that is seconds. A minute is generous against the worst of those and short enough that
/// deleting a chat gives its disk space back rather than promising to.
pub const GRACE: Duration = Duration::from_secs(60);

/// A sweep runs at startup when the last one was longer ago than this, and whenever the user asks
/// in Settings → Data & privacy (06 §8).
pub const EVERY: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The `settings` key holding the last sweep's time, so a machine that is opened every day does
/// not sweep every day.
pub const SWEPT_AT_KEY: &str = "blobs.swept_at";

/// What a sweep removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepReport {
    /// Files deleted from `blobs/`.
    pub files: usize,
    /// What those files came to.
    pub bytes: u64,
    /// Catalogue rows deleted, which includes rows whose file was already missing.
    pub rows: usize,
}

impl SweepReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files == 0 && self.rows == 0
    }
}

/// Deletes every blob the database no longer references. Walks the whole `blobs/` directory, so
/// it also collects files that were written and never referenced at all — an attachment ingested
/// for a message that was never sent — and the `.part` files a crash left behind mid-write.
pub fn sweep(conn: &Connection, store: &BlobStore, grace: Duration) -> Result<SweepReport> {
    let keep = blobs::reachable(conn)?;
    let mut report = SweepReport::default();
    for (path, name) in files(store.root()) {
        if name.ends_with(".part") {
            // A half-written blob from a run that did not finish. Whether the hash it was
            // going to have is wanted says nothing about this file: it was never readable.
            if aged(&path, grace) {
                remove(&path, &mut report);
            }
            continue;
        }
        if keep.contains(&name) || !aged(&path, grace) {
            continue;
        }
        remove(&path, &mut report);
    }
    for hash in blobs::catalogued(conn)? {
        if keep.contains(&hash) || !collectable(&store.path_for(&hash), grace) {
            continue;
        }
        blobs::delete(conn, &hash)?;
        report.rows += 1;
    }
    Ok(report)
}

/// The same question asked about the blobs a deleted chat was holding, so its disk space comes
/// back when the user deletes it rather than at the end of the week. Called after the rows are
/// gone, in the same write: anything in `hashes` that nothing else references is collected.
pub fn collect(
    conn: &Connection,
    store: &BlobStore,
    hashes: &[String],
    grace: Duration,
) -> Result<SweepReport> {
    let mut report = SweepReport::default();
    for hash in hashes {
        let path = store.path_for(hash);
        if blobs::referenced(conn, hash)? || !collectable(&path, grace) {
            continue;
        }
        remove(&path, &mut report);
        blobs::delete(conn, hash)?;
        report.rows += 1;
    }
    Ok(report)
}

/// Every file two levels down from the blob root, as `(path, file name)`.
fn files(root: &Path) -> Vec<(std::path::PathBuf, String)> {
    let mut out = Vec::new();
    let Ok(prefixes) = fs::read_dir(root) else {
        return out;
    };
    for prefix in prefixes.flatten() {
        if !prefix.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(entries) = fs::read_dir(prefix.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            out.push((entry.path(), name));
        }
    }
    out
}

/// Whether the file has sat untouched for longer than the grace period. A file whose time cannot
/// be read is left alone: not knowing how old something is is not a reason to delete it.
fn aged(path: &Path, grace: Duration) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    modified.elapsed().is_ok_and(|since| since >= grace)
}

/// The same question for a catalogue row: a row whose file is not there has nothing left to
/// protect, and the grace period is about bytes on disk.
fn collectable(path: &Path, grace: Duration) -> bool {
    !path.exists() || aged(path, grace)
}

fn remove(path: &Path, report: &mut SweepReport) {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    match fs::remove_file(path) {
        Ok(()) => {
            report.files += 1;
            report.bytes += size;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("could not delete blob {}: {err}", path.display()),
    }
}
