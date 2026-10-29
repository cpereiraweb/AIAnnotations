//! Maintenance of the FTS5 `search_index` (Inv §9, port of `SessionSearch::index`/`reindex`).
//!
//! One row per run: project name, branch, session title, summary, next step and `compact`, the
//! same five fields joined by spaces with the hyphens removed ("nfse" finds "NFS-e"). Missing
//! values are indexed as empty strings (PHP `(string) null`).

use rusqlite::{Connection, Transaction};

use crate::{Result, db};

/// `SELECT` of the indexed columns of every run, joined to its session and project.
const ROWS: &str = "\
    SELECT r.id, COALESCE(p.name, ''), COALESCE(r.branch, ''), COALESCE(s.title, ''),
           COALESCE(r.summary, ''), COALESCE(r.next_step, ''),
           replace(COALESCE(p.name, '') || ' ' || COALESCE(r.branch, '') || ' '
                   || COALESCE(s.title, '') || ' ' || COALESCE(r.summary, '') || ' '
                   || COALESCE(r.next_step, ''), '-', '')
    FROM session_runs r
    LEFT JOIN ai_sessions s ON s.id = r.ai_session_id
    LEFT JOIN projects p ON p.id = s.project_id";

const INSERT: &str =
    "INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact) ";

/// Replaces the row of run `run_id` (`SessionRunObserver::saved`); a run that no longer exists
/// only loses its row (`SessionSearch::forget`).
pub fn index_run(tx: &Transaction, run_id: i64) -> Result<()> {
    tx.execute("DELETE FROM search_index WHERE run_id = ?1", [run_id])?;
    tx.execute(&format!("{INSERT}{ROWS} WHERE r.id = ?1"), [run_id])?;
    Ok(())
}

/// Rebuilds the whole index from the tables (`aiannotations:reindex`), dropping the orphans left
/// by cascading deletes; returns the number of indexed runs.
pub fn rebuild(conn: &mut Connection) -> Result<usize> {
    let tx = db::write_tx(conn)?;
    tx.execute("DELETE FROM search_index", [])?;
    let n = tx.execute(&format!("{INSERT}{ROWS}"), [])?;
    tx.commit()?;
    Ok(n)
}
