//! Finds on disk the sessions the hooks did not record and closes the idle ones (Inv §3.5).
//! Port of `app/Sessions/Discovery/SessionCollector.php` and the `collect`/`backfill` commands.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{Connection, params};

use super::discovery::{self, DiscoveredSession};
use super::recorder::Recorder;
use crate::git::Git;
use crate::model::Tool;
use crate::{Paths, Result, Settings};

/// `progress` is called every this many sessions (and once at the end).
const PROGRESS_EVERY: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectMode {
    /// The last `collect_window_days` (the periodic pass).
    Window,
    /// The whole history, runs marked `is_backfill`.
    Backfill,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollectReport {
    /// Sessions found on disk and processed (outside `ai_workdir`), failed ones included.
    pub seen: usize,
    /// Real failures: one message per failed source, session or idle-closing pass.
    pub errors: Vec<String>,
    /// The recorder's warnings (unknown origin, unreadable transcript); the session was recorded.
    pub warnings: Vec<String>,
    /// Open runs closed as Idle in this pass (sessions born closed are not counted).
    pub closed_idle: usize,
}

/// One collector pass. An error in a source, a session or the idle-closing pass goes to `errors`
/// and the report of what was already done is still returned.
/// `progress(done, total)` is called every 100 sessions and once at the end.
pub fn run(
    conn: &mut Connection,
    paths: &Paths,
    settings: &Settings,
    git: &Git,
    mode: CollectMode,
    now_ms: i64,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<CollectReport> {
    let (since, backfill) = match mode {
        CollectMode::Window => (
            Some(now_ms - i64::from(settings.collect_window_days) * 86_400_000),
            false,
        ),
        CollectMode::Backfill => (None, true),
    };
    let mut report = CollectReport::default();

    let mut found = Vec::new();
    for (name, result) in [
        ("Claude", discovery::claude(&paths.claude_dir, since)),
        ("Codex", discovery::codex(&paths.codex_home, since)),
    ] {
        match result {
            Ok(mut sessions) => found.append(&mut sessions),
            Err(e) => report.errors.push(format!("{name} discovery failed: {e}")),
        }
    }
    let workdir = normalize(&paths.ai_workdir.to_string_lossy());
    found.retain(|d| !d.cwd.as_deref().is_some_and(|c| is_inside(c, &workdir)));

    let mut recorder = Recorder::new(conn, paths, settings, git);
    let mut seen: HashSet<(Tool, String)> = HashSet::new();
    let total = found.len();
    for (i, d) in found.iter().enumerate() {
        seen.insert((d.tool, d.session_id.clone()));
        match recorder.upsert_discovered_from(d, backfill, true, now_ms) {
            Ok(closed) => report.closed_idle += usize::from(closed),
            // One problematic transcript cannot stop the whole pass.
            Err(e) => report.errors.push(session_error(d, &e)),
        }
        if (i + 1) % PROGRESS_EVERY == 0 {
            progress(i + 1, total);
        }
    }
    if total % PROGRESS_EVERY != 0 || total == 0 {
        progress(total, total);
    }
    report.seen = total;

    if let Err(e) = close_outside_window(&mut recorder, settings, &seen, now_ms, &mut report) {
        report
            .errors
            .push(format!("closing idle sessions failed: {e}"));
    }
    report.warnings = recorder.take_warnings();
    Ok(report)
}

fn session_error(d: &DiscoveredSession, e: &crate::Error) -> String {
    format!("{} session {}: {e}", d.tool.as_str(), d.session_id)
}

/// Open runs of sessions the scan did not see (transcript outside the window, or deleted) are
/// also closed for inactivity.
fn close_outside_window(
    recorder: &mut Recorder,
    settings: &Settings,
    seen: &HashSet<(Tool, String)>,
    now_ms: i64,
    report: &mut CollectReport,
) -> Result<()> {
    let cutoff = now_ms - i64::from(settings.idle_minutes) * 60_000;
    let chunk = i64::from(settings.chunk_size.max(1));
    let mut after_id = 0;
    loop {
        // By id, not by offset: processing takes the rows out of the filter.
        let batch = {
            let mut stmt = recorder.conn.prepare(
                "SELECT a.id, a.tool, a.session_id, a.transcript_path, a.cwd, a.last_activity_at \
                 FROM ai_sessions a WHERE a.id > ?1 AND a.last_activity_at < ?2 \
                 AND EXISTS (SELECT 1 FROM session_runs r WHERE r.ai_session_id = a.id \
                             AND r.ended_at IS NULL) \
                 ORDER BY a.id LIMIT ?3",
            )?;
            stmt.query_map(params![after_id, cutoff, chunk], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let Some(last) = batch.last() else {
            return Ok(());
        };
        after_id = last.0;

        for (_, tool, session_id, transcript_path, cwd, last_activity_ms) in batch {
            let Ok(tool) = tool.parse::<Tool>() else {
                report
                    .errors
                    .push(format!("session {session_id}: unknown tool {tool:?}"));
                continue;
            };
            if seen.contains(&(tool, session_id.clone())) {
                continue;
            }
            let d = DiscoveredSession {
                tool,
                session_id,
                transcript_path: transcript_path.map(Into::into),
                cwd: Some(cwd),
                last_activity_ms,
            };
            // Not the transcript's mtime: the stored transcript_mtime stays as it is.
            match recorder.upsert_discovered_from(&d, false, false, now_ms) {
                Ok(closed) => report.closed_idle += usize::from(closed),
                Err(e) => report.errors.push(session_error(&d, &e)),
            }
        }
    }
}

/// Path separator: `/` everywhere, plus `\` on Windows (cwds arrive in either form there).
fn is_sep(c: char) -> bool {
    c == '/' || (cfg!(windows) && c == '\\')
}

fn is_inside(cwd: &str, dir: &str) -> bool {
    if dir.is_empty() {
        return false;
    }
    let raw = cwd.trim_end_matches(is_sep);
    [normalize(cwd).as_str(), raw]
        .iter()
        .any(|p| *p == dir || p.strip_prefix(dir).is_some_and(|r| r.starts_with(is_sep)))
}

/// The real path when it exists, without its trailing slash.
fn normalize(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let real = crate::paths::real_path(Path::new(path))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string());
    real.trim_end_matches(is_sep).to_string()
}
