//! Maintenance of the FTS5 `search_index` (Inv §9, port of `SessionSearch::index`/`reindex`).
//!
//! One row per run: project name, branch, session title, summary, next step and `compact`, the
//! same five fields joined by spaces with the hyphens removed ("nfse" finds "NFS-e"). Missing
//! values are indexed as empty strings (PHP `(string) null`).
//!
//! Migration 0002 keeps the index in step with the tables through triggers, so writers never
//! call [`index_run`]; it and [`rebuild`] remain for repair.

use std::str::FromStr;

use rusqlite::{Connection, Transaction};
use serde::Serialize;

use crate::model::Tool;
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

/// A run found by [`query`], with what the result list shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    pub run_id: i64,
    pub ai_session_id: i64,
    pub project_id: i64,
    pub project_name: String,
    pub tool: Tool,
    pub branch: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub next_step: Option<String>,
    pub started_at_ms: i64,
}

/// FTS5 expression for a free-text query (`SessionSearch::toMatchExpression`): every
/// whitespace-separated term becomes a quoted prefix phrase, also tried without hyphens on
/// `compact` ("nfse" finds "NFS-e"); terms without a letter or digit are dropped. `None` when
/// no term is left.
pub fn match_expression(text: &str) -> Option<String> {
    let has_word_char = |s: &str| s.chars().any(char::is_alphanumeric);
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| has_word_char(t))
        .map(|term| {
            let compact = term.replace('-', "");
            let alternative = if has_word_char(&compact) {
                format!(" OR compact : \"{compact}\"*")
            } else {
                String::new()
            };
            format!("(\"{term}\"*{alternative})")
        })
        .collect();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

/// Runs matching `text`, best bm25 first, at most `limit` (`SessionSearch::search`).
pub fn query(conn: &Connection, text: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let Some(expression) = match_expression(text) else {
        return Ok(Vec::new());
    };
    let ids: Vec<i64> = conn
        .prepare(
            "SELECT run_id FROM search_index WHERE search_index MATCH ?1 \
             ORDER BY bm25(search_index) LIMIT ?2",
        )?
        .query_map((&expression, limit as i64), |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut stmt = conn.prepare(
        "SELECT r.id, r.ai_session_id, p.id, p.name, s.tool, r.branch, s.title, r.summary,
                r.next_step, r.started_at
         FROM session_runs r
         JOIN ai_sessions s ON s.id = r.ai_session_id
         JOIN projects p ON p.id = s.project_id
         WHERE r.id = ?1",
    )?;
    let mut hits = Vec::with_capacity(ids.len());
    for id in ids {
        let row = stmt.query_row([id], |r| {
            Ok((
                SearchHit {
                    run_id: r.get(0)?,
                    ai_session_id: r.get(1)?,
                    project_id: r.get(2)?,
                    project_name: r.get(3)?,
                    tool: Tool::Claude,
                    branch: r.get(5)?,
                    title: r.get(6)?,
                    summary: r.get(7)?,
                    next_step: r.get(8)?,
                    started_at_ms: r.get(9)?,
                },
                r.get::<_, String>(4)?,
            ))
        });
        match row {
            Ok((mut hit, tool)) => {
                hit.tool = Tool::from_str(&tool)?;
                hits.push(hit);
            }
            // A stale index row of a run deleted meanwhile.
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(hits)
}
