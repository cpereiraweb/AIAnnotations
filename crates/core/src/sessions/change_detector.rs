//! Decides whether a run changed files (Inv §3.4 `ChangeDetector`). Port of `ChangeDetector.php`.
//!
//! The transcript is the primary signal, limited to the files touched inside the run's interval
//! (started_at..ended_at, unbounded while open). The git snapshots only complement it, and only
//! when no other session of the same project was active in the interval (the change could be
//! theirs).

use rusqlite::{OptionalExtension, Transaction, params};

use crate::Result;
use crate::transcripts::TranscriptExtract;

/// The run fields the decision depends on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunGit {
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub head_start: Option<String>,
    pub head_end: Option<String>,
    pub status_hash_start: Option<String>,
    pub status_hash_end: Option<String>,
}

/// Files touched inside the run's interval.
pub fn files_for(x: &TranscriptExtract, run: &RunGit) -> Vec<String> {
    x.files_touched_between(run.started_at, run.ended_at)
}

/// `Some(true)` with files in the interval, or with differing snapshots and no parallel session;
/// `Some(false)` when there were turns; `None` without any evidence.
pub fn decide(x: Option<&TranscriptExtract>, run: &RunGit, parallel_active: bool) -> Option<bool> {
    if x.is_some_and(|x| !files_for(x, run).is_empty()) {
        return Some(true);
    }
    if !parallel_active && snapshots_differ(run) {
        return Some(true);
    }
    // v1 checks `userPrompts !== []`: any turn counts, even one with an empty prompt.
    if x.is_some_and(|x| !x.turns.is_empty()) {
        return Some(false);
    }
    None
}

fn snapshots_differ(run: &RunGit) -> bool {
    let has_start = run.head_start.is_some() || run.status_hash_start.is_some();
    let has_end = run.head_end.is_some() || run.status_hash_end.is_some();
    has_start
        && has_end
        && (run.head_start != run.head_end || run.status_hash_start != run.status_hash_end)
}

/// `changed_files` of run `run_id` (`None` also when the run does not exist), with "now" as the
/// end of an open run. `now_ms` is the caller's clock (v1 used `now()`).
pub fn detect(
    tx: &Transaction,
    run_id: i64,
    extract: Option<&TranscriptExtract>,
    now_ms: i64,
) -> Result<Option<bool>> {
    let row = tx
        .query_row(
            "SELECT r.ai_session_id, a.project_id, r.started_at, r.ended_at, r.head_start, \
             r.head_end, r.status_hash_start, r.status_hash_end \
             FROM session_runs r JOIN ai_sessions a ON a.id = r.ai_session_id WHERE r.id = ?1",
            [run_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    RunGit {
                        started_at: Some(r.get(2)?),
                        ended_at: r.get(3)?,
                        head_start: r.get(4)?,
                        head_end: r.get(5)?,
                        status_hash_start: r.get(6)?,
                        status_hash_end: r.get(7)?,
                    },
                ))
            },
        )
        .optional()?;
    let Some((session_id, project_id, run)) = row else {
        return Ok(None);
    };
    let parallel = parallel_active(tx, session_id, project_id, &run, now_ms)?;
    Ok(decide(extract, &run, parallel))
}

/// Another session of the same project had a run overlapping this one. Compared in whole seconds,
/// as v1 stores instants without milliseconds.
fn parallel_active(
    tx: &Transaction,
    session_id: i64,
    project_id: i64,
    run: &RunGit,
    now_ms: i64,
) -> Result<bool> {
    let start_s = run.started_at.unwrap_or(0).div_euclid(1000);
    let end_s = run.ended_at.unwrap_or(now_ms).div_euclid(1000);
    // started_s < end_s  <=>  started_at < end_s * 1000; ended_s > start_s  <=>  ended_at >= (start_s + 1) * 1000.
    Ok(tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM session_runs r JOIN ai_sessions a ON a.id = r.ai_session_id \
         WHERE r.ai_session_id != ?1 AND a.project_id = ?2 AND r.started_at < ?3 \
         AND (r.ended_at IS NULL OR r.ended_at >= ?4))",
        params![session_id, project_id, end_s * 1000, (start_s + 1) * 1000],
        |r| r.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::parse_ts;
    use crate::transcripts::{FileTouch, Turn};

    fn extract(prompts: &[&str], files: &[&str]) -> TranscriptExtract {
        TranscriptExtract {
            cwd: Some("/x".into()),
            turns: prompts
                .iter()
                .map(|p| Turn {
                    prompt: p.to_string(),
                    response: None,
                })
                .collect(),
            files_touched: files.iter().map(|f| f.to_string()).collect(),
            tool_use_count: files.len() as u32,
            ..Default::default()
        }
    }

    fn run() -> RunGit {
        RunGit {
            head_start: Some("aaa".into()),
            head_end: Some("aaa".into()),
            status_hash_start: Some("s1".into()),
            status_hash_end: Some("s1".into()),
            ..Default::default()
        }
    }

    /// arquivos tocados na transcrição indicam alteração
    #[test]
    fn files_touched_in_transcript_mean_change() {
        assert_eq!(
            decide(Some(&extract(&["oi"], &["/x/a.php"])), &run(), true),
            Some(true)
        );
    }

    /// snapshots diferentes sem sessão paralela indicam alteração
    #[test]
    fn different_snapshots_without_parallel_session_mean_change() {
        let status = RunGit {
            status_hash_end: Some("s2".into()),
            ..run()
        };
        let head = RunGit {
            head_end: Some("bbb".into()),
            ..run()
        };
        assert_eq!(
            decide(Some(&extract(&["oi"], &[])), &status, false),
            Some(true)
        );
        assert_eq!(decide(None, &head, false), Some(true));
    }

    /// snapshots diferentes com sessão paralela não contam, e prompts sem arquivos dão false
    #[test]
    fn different_snapshots_with_parallel_session_do_not_count() {
        let status = RunGit {
            status_hash_end: Some("s2".into()),
            ..run()
        };
        assert_eq!(
            decide(Some(&extract(&["oi"], &[])), &status, true),
            Some(false)
        );
        assert_eq!(
            decide(Some(&extract(&["oi"], &[])), &run(), false),
            Some(false)
        );
    }

    /// sem evidência nenhuma devolve null
    #[test]
    fn no_evidence_returns_none() {
        let status = RunGit {
            status_hash_end: Some("s2".into()),
            ..run()
        };
        let no_start = RunGit {
            head_start: None,
            status_hash_start: None,
            ..run()
        };
        assert_eq!(decide(None, &run(), false), None);
        assert_eq!(decide(Some(&extract(&[], &[])), &status, true), None);
        assert_eq!(decide(None, &no_start, false), None);
    }

    /// só contam os arquivos tocados dentro do intervalo da execução
    #[test]
    fn only_files_touched_inside_the_run_interval_count() {
        let t = |v: &str| parse_ts(v).unwrap();
        let x = TranscriptExtract {
            file_touches: vec![FileTouch {
                path: "/x/a.php".into(),
                at_ms: Some(t("2026-10-20T10:05:00Z")),
            }],
            ..extract(&["oi", "de novo"], &["/x/a.php"])
        };
        let inside = RunGit {
            started_at: Some(t("2026-10-20T10:00:00Z")),
            ended_at: Some(t("2026-10-20T10:30:00Z")),
            ..run()
        };
        let later = RunGit {
            started_at: Some(t("2026-10-20T12:00:00Z")),
            ended_at: None,
            ..run()
        };
        assert_eq!(decide(Some(&x), &inside, true), Some(true));
        assert_eq!(decide(Some(&x), &later, true), Some(false));
    }

    /// `detect` reads the run from the database and takes "now" from the caller: an open run
    /// overlaps a later run of another session of the same project only once `now_ms` passes it.
    #[test]
    fn detect_uses_callers_now_for_open_run_overlap() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::db::open(&dir.path().join("t.db")).unwrap();
        let tx = crate::db::write_tx(&mut conn).unwrap();
        let t = |v: &str| parse_ts(v).unwrap();
        tx.execute(
            "INSERT INTO projects (key, name, path) VALUES ('/p', 'p', '/p')",
            [],
        )
        .unwrap();
        for sid in ["a", "b"] {
            tx.execute(
                "INSERT INTO ai_sessions (project_id, tool, session_id, cwd, last_activity_at) \
                 VALUES (1, 'claude', ?1, '/p', 0)",
                [sid],
            )
            .unwrap();
        }
        // Run of a: open since 10:00, snapshots differ.
        tx.execute(
            "INSERT INTO session_runs (ai_session_id, started_at, head_start, head_end) \
             VALUES (1, ?1, 'aaa', 'bbb')",
            [t("2026-10-20T10:00:00Z")],
        )
        .unwrap();
        let run = tx.last_insert_rowid();
        // Run of b: open since 11:00.
        tx.execute(
            "INSERT INTO session_runs (ai_session_id, started_at) VALUES (2, ?1)",
            [t("2026-10-20T11:00:00Z")],
        )
        .unwrap();

        assert_eq!(
            detect(&tx, run, None, t("2026-10-20T10:30:00Z")).unwrap(),
            Some(true)
        );
        assert_eq!(
            detect(&tx, run, None, t("2026-10-20T12:00:00Z")).unwrap(),
            None
        );
        assert_eq!(
            detect(&tx, 999, None, t("2026-10-20T12:00:00Z")).unwrap(),
            None
        );
    }
}
