//! Per-run AI summaries (Inv §4.2). Port of `SessionSummarizer.php` and `SummarizeCommand.php`.
//!
//! The AI call is slow, so nothing is held across it: the run is read with plain statements, the
//! AI is called, and the result is written in one short `write_tx` that only applies if the run
//! is still as it was read (`pending`, same `updated_at` and `ended_at`). Failures count an
//! attempt and back off `2^attempts` minutes; usage limits and a missing CLI count nothing and
//! pause the whole batch. The core never reads the clock: `clock` is called again after the AI
//! call and that later instant stamps the write.

pub mod prompt;

use std::path::Path;
use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::ai::{AiClient, AiError, ModelRole};
use crate::db::{self, app_state};
use crate::model::Tool;
use crate::sessions::change_detector::{RunGit, files_for};
use crate::transcripts::factory;
use crate::{Paths, Result, Settings};

/// `app_state` key of the usage-limit pause (value and `expires_at`: the instant it ends, ms).
pub const PAUSE_KEY: &str = "ai:paused-until";
/// `app_state` lock that spaces the backfill summaries.
pub const BACKFILL_LOCK_KEY: &str = "ai:last-backfill-summary";

const TEXT_LIMIT: usize = 2000;
/// Longest AI message kept in `SummarizeReport::pause_reason`.
const PAUSE_REASON_CHARS: usize = 200;
const MINUTE_MS: i64 = 60_000;

/// Optimistic-write guard shared by every update of a run being summarized.
const UNCHANGED: &str =
    "WHERE id = ?1 AND summary_status = 'pending' AND updated_at IS ?2 AND ended_at IS ?3";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryOutcome {
    Summarized,
    Rescheduled,
    Failed,
    Skipped,
}

#[derive(Debug, Default, Serialize)]
pub struct SummarizeReport {
    pub retried: usize,
    pub summarized: usize,
    pub rescheduled: usize,
    pub skipped: usize,
    pub failed: usize,
    pub paused_until_ms: Option<i64>,
    pub pause_reason: Option<String>,
    pub was_paused: bool,
}

/// What was read of the run before the AI call; the write only applies if it still holds.
struct Read {
    id: i64,
    updated_at: Option<i64>,
    ended_at: Option<i64>,
    attempts: i64,
}

struct Loaded {
    read: Read,
    status: String,
    started_at: i64,
    branch: Option<String>,
    tool: String,
    session_id: String,
    cwd: String,
    transcript_path: Option<String>,
    title: Option<String>,
    project_name: String,
}

fn load(conn: &Connection, run_id: i64) -> Result<Option<Loaded>> {
    Ok(conn
        .query_row(
            "SELECT r.summary_status, r.summary_attempts, r.updated_at, r.ended_at, r.started_at, \
             r.branch, a.tool, a.session_id, a.cwd, a.transcript_path, a.title, p.name \
             FROM session_runs r JOIN ai_sessions a ON a.id = r.ai_session_id \
             JOIN projects p ON p.id = a.project_id WHERE r.id = ?1",
            [run_id],
            |r| {
                Ok(Loaded {
                    status: r.get(0)?,
                    read: Read {
                        id: run_id,
                        attempts: r.get(1)?,
                        updated_at: r.get(2)?,
                        ended_at: r.get(3)?,
                    },
                    started_at: r.get(4)?,
                    branch: r.get(5)?,
                    tool: r.get(6)?,
                    session_id: r.get(7)?,
                    cwd: r.get(8)?,
                    transcript_path: r.get(9)?,
                    title: r.get(10)?,
                    project_name: r.get(11)?,
                })
            },
        )
        .optional()?)
}

struct Validated {
    summary: String,
    next_step: Option<String>,
    unfinished: bool,
    /// The value the database stores (`codigo` / `conversa`).
    kind: &'static str,
}

fn clip(s: &str) -> String {
    s.chars().take(TEXT_LIMIT).collect()
}

fn validate(data: &Map<String, Value>) -> std::result::Result<Validated, String> {
    let summary = data
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    let kind = match data.get("kind").and_then(Value::as_str) {
        Some("code") => "codigo",
        Some("conversation") => "conversa",
        _ => "",
    };
    let unfinished = data.get("unfinished").and_then(Value::as_bool);
    let next_step = match data.get("next_step") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.trim()).filter(|s| !s.is_empty()),
        Some(_) => return Err(invalid(data)),
    };
    match unfinished {
        Some(unfinished) if !summary.is_empty() && !kind.is_empty() => Ok(Validated {
            summary: clip(summary),
            next_step: next_step.map(clip),
            unfinished,
            kind,
        }),
        _ => Err(invalid(data)),
    }
}

fn invalid(data: &Map<String, Value>) -> String {
    let json: String = Value::Object(data.clone())
        .to_string()
        .chars()
        .take(300)
        .collect();
    format!("AI reply outside the expected format: {json}")
}

/// Summarizes one run. The outer `Err` is a database or I/O error. The inner `Err` is only
/// `RateLimited`/`Unavailable` (the batch must pause); any other AI failure is an attempt
/// (`Rescheduled`/`Failed`), never `AiError::Failed`.
pub fn summarize_run(
    conn: &mut Connection,
    ai: &dyn AiClient,
    paths: &Paths,
    settings: &Settings,
    run_id: i64,
    clock: &dyn Fn() -> i64,
) -> Result<std::result::Result<SummaryOutcome, AiError>> {
    let Some(run) = load(conn, run_id)? else {
        return Ok(Ok(SummaryOutcome::Skipped));
    };
    if run.status != "pending" {
        return Ok(Ok(SummaryOutcome::Skipped));
    }

    // Outside any transaction: reading and calling the AI hold no lock.
    let Ok(tool) = Tool::from_str(&run.tool) else {
        return reschedule(conn, settings, &run.read, clock).map(Ok);
    };
    let transcript = run.transcript_path.as_deref().map(Path::new);
    let extract = match factory::read(tool, &run.session_id, transcript, paths) {
        Ok(Some(x)) => x,
        Ok(None) => return mark_skipped(conn, &run.read, clock).map(Ok),
        Err(_) => return reschedule(conn, settings, &run.read, clock).map(Ok),
    };

    let git = RunGit {
        started_at: Some(run.started_at),
        ended_at: run.read.ended_at,
        ..Default::default()
    };
    let text = extract.to_prompt_text(
        settings.extract_max_chars as usize,
        Some(&files_for(&extract, &git)),
    );
    let project = if run.project_name.trim().is_empty() {
        Path::new(&run.cwd)
            .file_name()
            .map_or_else(|| run.cwd.clone(), |n| n.to_string_lossy().into_owned())
    } else {
        run.project_name.clone()
    };
    let prompt = prompt::for_run(
        settings.lang,
        &project,
        run.branch.as_deref().or(extract.branch.as_deref()),
        tool,
        run.title.as_deref().or(extract.title.as_deref()),
        &run.cwd,
        &text,
    );

    let reply = match ai.complete_json(&prompt, ModelRole::Summary) {
        Ok(data) => data,
        Err(e @ (AiError::RateLimited(_) | AiError::Unavailable(_))) => return Ok(Err(e)),
        Err(AiError::Failed(_)) => return reschedule(conn, settings, &run.read, clock).map(Ok),
    };
    let Ok(data) = validate(&reply) else {
        return reschedule(conn, settings, &run.read, clock).map(Ok);
    };

    let now = clock();
    let tx = db::write_tx(conn)?;
    let changed = tx.execute(
        &format!(
            "UPDATE session_runs SET summary = ?4, next_step = ?5, unfinished = ?6, kind = ?7, \
             summary_status = 'done', summary_available_at = NULL, summarized_fingerprint = ?8, \
             updated_at = ?9 {UNCHANGED}"
        ),
        params![
            run.read.id,
            run.read.updated_at,
            run.read.ended_at,
            data.summary,
            data.next_step,
            data.unfinished,
            data.kind,
            extract.fingerprint(),
            now,
        ],
    )?;
    tx.commit()?;
    Ok(Ok(if changed == 0 {
        SummaryOutcome::Skipped
    } else {
        SummaryOutcome::Summarized
    }))
}

/// No transcript, nothing to summarize: `skipped`, no attempt counted.
fn mark_skipped(
    conn: &mut Connection,
    read: &Read,
    clock: &dyn Fn() -> i64,
) -> Result<SummaryOutcome> {
    let now = clock();
    let tx = db::write_tx(conn)?;
    tx.execute(
        &format!(
            "UPDATE session_runs SET summary_status = 'skipped', summary_available_at = NULL, \
             updated_at = ?4 {UNCHANGED}"
        ),
        params![read.id, read.updated_at, read.ended_at, now],
    )?;
    tx.commit()?;
    Ok(SummaryOutcome::Skipped)
}

/// Counts an attempt: `failed` at `summary_max_attempts`, otherwise back to `pending` for
/// `2^attempts` minutes later. Discarded (`Skipped`) if the run changed meanwhile.
fn reschedule(
    conn: &mut Connection,
    settings: &Settings,
    read: &Read,
    clock: &dyn Fn() -> i64,
) -> Result<SummaryOutcome> {
    let attempts = read.attempts + 1;
    let failed = attempts >= i64::from(settings.summary_max_attempts);
    let now = clock();
    let available_at = (!failed).then(|| now + (1i64 << attempts.min(30)) * MINUTE_MS);
    let tx = db::write_tx(conn)?;
    let changed = tx.execute(
        &format!(
            "UPDATE session_runs SET summary_attempts = ?4, summary_status = ?5, \
             summary_available_at = ?6, updated_at = ?7 {UNCHANGED}"
        ),
        params![
            read.id,
            read.updated_at,
            read.ended_at,
            attempts,
            if failed { "failed" } else { "pending" },
            available_at,
            now,
        ],
    )?;
    tx.commit()?;
    Ok(match (changed, failed) {
        (0, _) => SummaryOutcome::Skipped,
        (_, true) => SummaryOutcome::Failed,
        _ => SummaryOutcome::Rescheduled,
    })
}

/// Summarizes up to `limit` due runs (new before backfill, most recent first). A usage limit
/// pauses the batch (and the next ones until the pause ends); backfill runs are spaced by
/// `backfill_interval_seconds`, failures included.
pub fn run_batch(
    conn: &mut Connection,
    ai: &dyn AiClient,
    paths: &Paths,
    settings: &Settings,
    limit: usize,
    retry_failed: bool,
    clock: &dyn Fn() -> i64,
) -> Result<SummarizeReport> {
    let mut report = SummarizeReport::default();
    let now = clock();

    if retry_failed {
        let tx = db::write_tx(conn)?;
        report.retried = tx.execute(
            "UPDATE session_runs SET summary_status = 'pending', summary_attempts = 0, \
             summary_available_at = NULL WHERE summary_status = 'failed'",
            [],
        )?;
        tx.commit()?;
    }

    if let Some(until) = app_state::get(conn, PAUSE_KEY, now)?.and_then(|v| v.parse::<i64>().ok()) {
        report.was_paused = true;
        report.paused_until_ms = Some(until);
        return Ok(report);
    }

    let candidates: Vec<(i64, bool)> = {
        let mut stmt = conn.prepare(
            "SELECT id, is_backfill FROM session_runs WHERE summary_status = 'pending' \
             AND ended_at IS NOT NULL AND (summary_available_at IS NULL OR summary_available_at <= ?1) \
             ORDER BY is_backfill, ended_at DESC LIMIT ?2",
        )?;
        stmt.query_map(params![now, limit.max(1) as i64], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<rusqlite::Result<_>>()?
    };

    for (run_id, is_backfill) in candidates {
        if is_backfill {
            let tx = db::write_tx(conn)?;
            let ttl = i64::from(settings.backfill_interval_seconds) * 1000;
            let taken = app_state::try_lock(&tx, BACKFILL_LOCK_KEY, ttl, clock())?;
            tx.commit()?;
            if !taken {
                break;
            }
        }
        match summarize_run(conn, ai, paths, settings, run_id, clock)? {
            Ok(SummaryOutcome::Summarized) => report.summarized += 1,
            Ok(SummaryOutcome::Rescheduled) => report.rescheduled += 1,
            Ok(SummaryOutcome::Failed) => report.failed += 1,
            Ok(SummaryOutcome::Skipped) => report.skipped += 1,
            Err(e) => {
                let until = clock() + i64::from(settings.ai_pause_minutes) * MINUTE_MS;
                let tx = db::write_tx(conn)?;
                app_state::set(&tx, PAUSE_KEY, &until.to_string(), Some(until))?;
                tx.commit()?;
                report.paused_until_ms = Some(until);
                // `summarize_run` never returns `AiError::Failed`.
                report.pause_reason = Some(match e {
                    AiError::Unavailable(msg) => format!("unavailable: {}", tail(&msg)),
                    _ => "rate_limited".to_string(),
                });
                break;
            }
        }
    }
    Ok(report)
}

/// The last `PAUSE_REASON_CHARS` characters of `msg`: CLI errors end with the cause.
fn tail(msg: &str) -> &str {
    let skip = msg.chars().count().saturating_sub(PAUSE_REASON_CHARS);
    msg.char_indices().nth(skip).map_or("", |(i, _)| &msg[i..])
}

/// `Some((done, total))` while a backfill run is still `pending`; `total` leaves out `skipped`.
pub fn backfill_progress(conn: &Connection) -> Result<Option<(u64, u64)>> {
    let (done, total, pending): (i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(summary_status = 'done'), 0), COUNT(*), \
         COALESCE(SUM(summary_status = 'pending'), 0) \
         FROM session_runs WHERE is_backfill = 1 AND summary_status != 'skipped'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((pending > 0).then_some((done as u64, total as u64)))
}
