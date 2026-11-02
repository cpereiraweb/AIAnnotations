//! Manual maintenance: spool prune, reclassification of empty sessions and statistics
//! (Inv §3.3, §13.4). The library never prints: warnings come back in the reports.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::model::{SessionOrigin, SummaryStatus, Tool};
use crate::transcripts::factory;
use crate::{Paths, Result, Settings, db};

const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PruneReport {
    pub done_removed: usize,
    pub failed_removed: usize,
    pub warnings: Vec<String>,
}

/// Deletes the app's own spool files: `done/*.json` older than `spool_done_days` and
/// `failed/*.json` older than `spool_failed_days`, by file mtime against `now_ms`. Limits of 0
/// count as 1 day (zero would delete what was just processed). A `done/` or `failed/` that is a
/// symlink is skipped with a warning: following it would reach files outside the spool.
pub fn prune_spool(spool_dir: &Path, settings: &Settings, now_ms: i64) -> Result<PruneReport> {
    let mut report = PruneReport::default();
    report.done_removed = prune_dir(
        &spool_dir.join("done"),
        settings.spool_done_days,
        now_ms,
        &mut report.warnings,
    )?;
    report.failed_removed = prune_dir(
        &spool_dir.join("failed"),
        settings.spool_failed_days,
        now_ms,
        &mut report.warnings,
    )?;
    Ok(report)
}

fn prune_dir(dir: &Path, days: u32, now_ms: i64, warnings: &mut Vec<String>) -> Result<usize> {
    match std::fs::symlink_metadata(dir) {
        Ok(m) if m.file_type().is_symlink() => {
            warnings.push(format!("{} is a symlink; skipped.", dir.display()));
            return Ok(0);
        }
        Ok(m) if m.is_dir() => {}
        _ => return Ok(0),
    }
    let cutoff_ms = now_ms.saturating_sub(i64::from(days.max(1)) * DAY_MS);
    let mut removed = 0;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        // symlink_metadata: a link named *.json is not a file of the spool.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let Some(mtime_ms) = meta.modified().ok().and_then(epoch_ms) else {
            continue;
        };
        if mtime_ms < cutoff_ms {
            match std::fs::remove_file(&path) {
                Ok(()) => removed += 1,
                Err(e) => warnings.push(format!("could not remove {}: {e}", path.display())),
            }
        }
    }
    Ok(removed)
}

fn epoch_ms(t: SystemTime) -> Option<i64> {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|m| -m),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReclassifyReport {
    pub reclassified: usize,
    pub warnings: Vec<String>,
}

/// Re-evaluates interactive sessions marked empty with the current rule (empty = no user
/// prompt). Those that now have one go back to `is_empty = 0` and their closed `skipped` runs go
/// back to `pending` with the attempts reset; `is_backfill` is untouched (Inv §13.4).
/// Sessions whose transcript is missing or unreadable stay as they are.
pub fn reclassify(
    conn: &mut Connection,
    paths: &Paths,
    settings: &Settings,
    now_ms: i64,
) -> Result<ReclassifyReport> {
    let mut report = ReclassifyReport::default();
    let chunk = i64::from(settings.chunk_size.max(1));
    // Keyset by id: reclassifying takes rows out of the filter, an offset would skip some.
    let mut last_id = 0;
    loop {
        let batch: Vec<(i64, String, String, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT id, tool, session_id, transcript_path FROM ai_sessions \
                 WHERE origin = ?1 AND is_empty = 1 AND id > ?2 ORDER BY id LIMIT ?3",
            )?;
            stmt.query_map(
                params![SessionOrigin::Interactive.as_str(), last_id, chunk],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?
            .collect::<rusqlite::Result<_>>()?
        };
        let Some(&(last, ..)) = batch.last() else {
            break;
        };
        last_id = last;
        for (id, tool, session_id, transcript) in batch {
            let Ok(tool) = tool.parse::<Tool>() else {
                report.warnings.push(format!(
                    "session {session_id}: unknown tool {tool:?}; skipped"
                ));
                continue;
            };
            let extract = match factory::read(
                tool,
                &session_id,
                transcript.as_deref().map(Path::new),
                paths,
            ) {
                Ok(x) => x,
                Err(e) => {
                    report.warnings.push(format!(
                        "{} session {session_id}: transcript unreadable: {e}",
                        tool.as_str()
                    ));
                    continue;
                }
            };
            if !extract.is_some_and(|x| x.has_user_prompt()) {
                continue;
            }
            let tx = db::write_tx(conn)?;
            let changed = tx.execute(
                "UPDATE ai_sessions SET is_empty = 0, updated_at = ?2 \
                 WHERE id = ?1 AND is_empty = 1",
                params![id, now_ms],
            )?;
            if changed == 0 {
                continue;
            }
            tx.execute(
                "UPDATE session_runs SET summary_status = ?3, summary_attempts = 0, \
                 summary_available_at = NULL, updated_at = ?4 \
                 WHERE ai_session_id = ?1 AND ended_at IS NOT NULL AND summary_status = ?2",
                params![
                    id,
                    SummaryStatus::Skipped.as_str(),
                    SummaryStatus::Pending.as_str(),
                    now_ms
                ],
            )?;
            tx.commit()?;
            report.reclassified += 1;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub projects: u64,
    pub sessions_by_tool: BTreeMap<String, u64>,
    pub runs: u64,
    pub runs_by_summary_status: BTreeMap<String, u64>,
    /// Interactive and not empty (Inv §2, `scopeVisible`).
    pub visible_sessions: u64,
}

pub fn stats(conn: &Connection) -> Result<Stats> {
    let count = |sql: &str| -> rusqlite::Result<u64> {
        conn.query_row(sql, [], |r| r.get::<_, i64>(0).map(|n| n as u64))
    };
    let grouped = |sql: &str| -> rusqlite::Result<BTreeMap<String, u64>> {
        conn.prepare(sql)?
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })?
            .collect()
    };
    Ok(Stats {
        projects: count("SELECT COUNT(*) FROM projects")?,
        sessions_by_tool: grouped("SELECT tool, COUNT(*) FROM ai_sessions GROUP BY tool")?,
        runs: count("SELECT COUNT(*) FROM session_runs")?,
        runs_by_summary_status: grouped(
            "SELECT summary_status, COUNT(*) FROM session_runs GROUP BY summary_status",
        )?,
        visible_sessions: count(&format!(
            "SELECT COUNT(*) FROM ai_sessions WHERE origin = '{}' AND is_empty = 0",
            SessionOrigin::Interactive.as_str()
        ))?,
    })
}
