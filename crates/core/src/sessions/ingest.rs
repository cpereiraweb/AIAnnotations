//! Drains the hook spool into the database (Inv §3.3). Port of `app/Sessions/SpoolIngestor.php`.

use std::collections::HashSet;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode};

use super::event::HookEvent;
use super::recorder::Recorder;
use crate::git::Git;
use crate::model::Tool;
use crate::{Error, Paths, Result, Settings, spool};

/// Outcome of one pass over the spool.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestReport {
    /// Events recorded (moved to `done/`).
    pub processed: usize,
    /// Events moved to `failed/`.
    pub failed: usize,
    /// Events left in the queue for the next pass (busy database, or behind a busy event).
    pub deferred: usize,
    /// Recorder warnings and the reason of each failed file; the caller decides how to show them.
    pub warnings: Vec<String>,
}

/// Ingests every pending envelope with a [`Recorder`].
pub fn run(
    conn: &mut Connection,
    paths: &Paths,
    settings: &Settings,
    git: &Git,
    now_ms: i64,
) -> Result<IngestReport> {
    let mut recorder = Recorder::new(conn, paths, settings, git);
    run_with(
        &paths.spool_dir,
        settings.busy_max_age_seconds,
        now_ms,
        |ev, warnings| {
            let result = recorder.record(ev, now_ms);
            warnings.extend(recorder.take_warnings());
            result
        },
    )
}

/// The ingestion loop with the recording step injected; the closure pushes the recorder's
/// warnings into the sink on success and on error alike.
///
/// Test seam (a real database lock blocks every event, so a selective busy needs a stand-in).
#[doc(hidden)]
pub fn run_with(
    spool_dir: &Path,
    busy_max_age_seconds: u32,
    now_ms: i64,
    mut record: impl FnMut(&HookEvent, &mut Vec<String>) -> Result<()>,
) -> Result<IngestReport> {
    let mut report = IngestReport::default();
    if !spool_dir.is_dir() {
        return Ok(report);
    }
    // Sessions (tool + session_id) with an event kept in the queue during this pass.
    let mut held: HashSet<(Tool, String)> = HashSet::new();

    for file in spool::pending(spool_dir)? {
        // A file whose mtime cannot be read counts as new (`now`).
        let mtime_ms = mtime_ms(&file).unwrap_or(now_ms);
        let event = match parse(&file, mtime_ms) {
            Ok(ev) => ev,
            // Taken by another ingestor since the listing: already handled.
            Err(Error::Io(e)) if e.kind() == ErrorKind::NotFound => continue,
            Err(e) => {
                fail(&file, spool_dir, &e, &mut report);
                continue;
            }
        };
        let key = (event.tool, event.session_id.clone());
        // An earlier event of the same session is waiting: this one waits too, keeping order.
        if held.contains(&key) {
            report.deferred += 1;
            continue;
        }
        let mut warnings = Vec::new();
        let result = record(&event, &mut warnings);
        report.warnings.append(&mut warnings);
        match result {
            Ok(()) => {
                // Recorded: a move failure must not abort the pass nor count as a failed event.
                match move_to(&file, &spool_dir.join("done")) {
                    Ok(()) => report.processed += 1,
                    Err(e) if e.kind() == ErrorKind::NotFound => {}
                    Err(e) => {
                        report.processed += 1;
                        report.warnings.push(format!(
                            "{}: recorded but could not be moved to done/: {e}",
                            file_name(&file)
                        ));
                    }
                }
            }
            Err(e) if is_busy(&e) => {
                // Whole seconds, as in v1 (Global Constraints).
                let age_s = now_ms.div_euclid(1000) - mtime_ms.div_euclid(1000);
                if age_s > i64::from(busy_max_age_seconds) {
                    fail(&file, spool_dir, &e, &mut report);
                } else {
                    held.insert(key);
                    report.deferred += 1;
                }
            }
            Err(e) => fail(&file, spool_dir, &e, &mut report),
        }
    }
    Ok(report)
}

fn parse(file: &Path, mtime_ms: i64) -> Result<HookEvent> {
    let envelope = serde_json::from_slice(&std::fs::read(file)?)?;
    HookEvent::from_envelope(&envelope, mtime_ms)
}

fn file_name(file: &Path) -> String {
    file.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// Moves an event that was not recorded to `failed/`; never aborts the pass.
fn fail(file: &Path, spool_dir: &Path, err: &Error, report: &mut IngestReport) {
    let name = file_name(file);
    match move_to(file, &spool_dir.join("failed")) {
        Ok(()) => {
            report.warnings.push(format!("{name}: {err}"));
            report.failed += 1;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => report.warnings.push(format!(
            "{name}: {err} (and could not be moved to failed/: {e})"
        )),
    }
}

/// SQLITE_BUSY / SQLITE_LOCKED: another connection holds the write lock.
fn is_busy(err: &Error) -> bool {
    matches!(
        err,
        Error::Db(rusqlite::Error::SqliteFailure(e, _))
            if matches!(e.code, ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

fn mtime_ms(file: &Path) -> Option<i64> {
    let since_epoch = std::fs::metadata(file)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

fn move_to(file: &Path, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let target: PathBuf = dir.join(file.file_name().unwrap_or_default());
    std::fs::rename(file, target)
}
