//! Background worker: runs the periodic jobs (ingest, collect, summarize, briefing, maintenance)
//! on its own connection. One worker per database, enforced by the `worker:running` lock in
//! `app_state`. The caller owns the thread and the sleeping between ticks and passes the clock.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;

use aiannotations_core::ai::AiClient;
use aiannotations_core::briefing::{self, BriefingOutcome};
use aiannotations_core::db::app_state;
use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, GitCache};
use aiannotations_core::model::BriefingStatus;
use aiannotations_core::sessions::collector::{self, CollectMode};
use aiannotations_core::sessions::{ingest, maintenance};
use aiannotations_core::{Paths, Result, Settings, db, search, summaries};
use chrono::{NaiveDate, TimeZone, Timelike};
use chrono_tz::Tz;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::views;

const LOCK_KEY: &str = "worker:running";
/// Slack added to `ai_timeout_seconds` so one AI call never outlives the lock.
const LOCK_MARGIN_MS: i64 = 60_000;
/// Minimum spacing of the lock renewals made from inside a job.
const RENEW_EVERY_MS: i64 = 30_000;
const SUMMARIZE_LIMIT: usize = 5;
const BRIEFING_HOUR: u32 = 6;
const BRIEFING_RETRY_MS: i64 = 30 * 60_000;
const MAX_ERRORS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Job {
    Ingest,
    Summarize,
    Collect,
    Briefing,
    Maintenance,
}

impl Job {
    fn name(self) -> &'static str {
        match self {
            Job::Ingest => "ingest",
            Job::Summarize => "summarize",
            Job::Collect => "collect",
            Job::Briefing => "briefing",
            Job::Maintenance => "maintenance",
        }
    }
}

/// How often each job runs (spec §6.2). The briefing is daily instead of periodic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub ingest_ms: i64,
    pub summarize_ms: i64,
    pub collect_ms: i64,
    pub maintenance_ms: i64,
}

impl Default for Schedule {
    fn default() -> Self {
        Schedule {
            ingest_ms: 60_000,
            summarize_ms: 60_000,
            collect_ms: 900_000,
            maintenance_ms: 86_400_000,
        }
    }
}

fn local(tz: Tz, ms: i64) -> Option<(NaiveDate, u32)> {
    let d = tz.timestamp_millis_opt(ms).single()?;
    Some((d.date_naive(), d.hour()))
}

impl Schedule {
    /// Jobs due at `now_ms`, in the order Ingest, Collect, Summarize, Briefing, Maintenance. A job
    /// with no `last_run` is due (first tick), except the briefing before 06:00 local. The
    /// briefing is due from 06:00 local once per local day.
    pub fn due(&self, last_run: &HashMap<Job, i64>, now_ms: i64, tz: Tz) -> Vec<Job> {
        let interval = |job: Job, every: i64| {
            last_run
                .get(&job)
                .is_none_or(|last| now_ms.saturating_sub(*last) >= every)
        };
        let briefing = match (local(tz, now_ms), last_run.get(&Job::Briefing)) {
            (Some((_, hour)), _) if hour < BRIEFING_HOUR => false,
            (Some((today, _)), Some(last)) => local(tz, *last).is_none_or(|(day, _)| day < today),
            (Some(_), None) => true,
            (None, _) => false,
        };
        let mut due = Vec::new();
        if interval(Job::Ingest, self.ingest_ms) {
            due.push(Job::Ingest);
        }
        if interval(Job::Collect, self.collect_ms) {
            due.push(Job::Collect);
        }
        if interval(Job::Summarize, self.summarize_ms) {
            due.push(Job::Summarize);
        }
        if briefing {
            due.push(Job::Briefing);
        }
        if interval(Job::Maintenance, self.maintenance_ms) {
            due.push(Job::Maintenance);
        }
        due
    }
}

/// State shown by the tray and the screens.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct WorkerStatus {
    pub last_tick_ms: Option<i64>,
    pub paused_until_ms: Option<i64>,
    pub pause_reason: Option<String>,
    pub summarizing_paused_by_user: bool,
    pub last_errors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickReport {
    pub ran: Vec<Job>,
    /// Something was written: the screens should reload.
    pub changed: bool,
    /// Another worker holds the lock; nothing ran.
    pub skipped_locked: bool,
}

pub struct Worker {
    conn: Connection,
    paths: Paths,
    settings: Settings,
    ai: Box<dyn AiClient>,
    git: Git,
    tz: Tz,
    schedule: Schedule,
    cache: GitCache,
    owner_id: String,
    lock_ttl_ms: i64,
    last_run: HashMap<Job, i64>,
    briefing_failed_at: Option<i64>,
    summaries_paused: bool,
    status: WorkerStatus,
}

impl Worker {
    pub fn new(
        conn: Connection,
        paths: Paths,
        settings: Settings,
        ai: Box<dyn AiClient>,
        git: Git,
        tz: Tz,
    ) -> Worker {
        // The worker is the app layer: reading the system clock once here, only to make the lock
        // owner unique across processes and restarts, is acceptable.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let lock_ttl_ms = lock_ttl_ms(&settings);
        Worker {
            conn,
            paths,
            settings,
            ai,
            git,
            tz,
            schedule: Schedule::default(),
            cache: GitCache::new(),
            lock_ttl_ms,
            owner_id: format!("{}-{nanos}", std::process::id()),
            last_run: HashMap::new(),
            briefing_failed_at: None,
            summaries_paused: false,
            status: WorkerStatus::default(),
        }
    }

    /// "Pause summaries" of the tray: skips the summaries and the briefing (both spend AI quota).
    pub fn set_summaries_paused(&mut self, paused: bool) {
        self.summaries_paused = paused;
    }

    /// New settings and AI client after the user's choices change (the wizard's "Concluir").
    pub fn set_settings(&mut self, settings: Settings, ai: Box<dyn AiClient>) {
        self.lock_ttl_ms = lock_ttl_ms(&settings);
        self.settings = settings;
        self.ai = ai;
    }

    pub fn status(&self) -> WorkerStatus {
        WorkerStatus {
            summarizing_paused_by_user: self.summaries_paused,
            ..self.status.clone()
        }
    }

    /// Takes the lock, or renews it if this worker already holds it. `false`: someone else does.
    fn hold_lock(&mut self, now: i64) -> Result<bool> {
        let tx = db::write_tx(&mut self.conn)?;
        let mine = app_state::try_lock(&tx, LOCK_KEY, self.lock_ttl_ms, now)?
            || app_state::get(&tx, LOCK_KEY, now)?.as_deref() == Some(self.owner_id.as_str());
        if mine {
            // `try_lock` stores a placeholder value: record the owner with the same expiry.
            app_state::set(
                &tx,
                LOCK_KEY,
                &self.owner_id,
                Some(now.saturating_add(self.lock_ttl_ms)),
            )?;
        }
        tx.commit()?;
        Ok(mine)
    }

    fn record_error(&mut self, message: String) {
        self.status.last_errors.push(message);
        let excess = self.status.last_errors.len().saturating_sub(MAX_ERRORS);
        self.status.last_errors.drain(..excess);
    }

    /// Runs the due jobs. A job's error goes to `last_errors` and does not stop the others.
    /// The tray counts on the worker's own connection (see [`views::today::tray_counts`]).
    pub fn tray_counts(&self, now_ms: i64) -> Result<(usize, usize)> {
        views::today::tray_counts(&self.conn, &self.git, &self.cache, &self.settings, now_ms)
    }

    pub fn tick(&mut self, clock: &dyn Fn() -> i64) -> Result<TickReport> {
        let mut report = TickReport::default();
        let now = clock();
        if !self.hold_lock(now)? {
            report.skipped_locked = true;
            return Ok(report);
        }
        self.status.last_tick_ms = Some(now);

        let mut first = true;
        for job in self.schedule.due(&self.last_run, now, self.tz) {
            if matches!(job, Job::Summarize | Job::Briefing) && self.summaries_paused {
                continue;
            }
            if job == Job::Briefing
                && (self
                    .briefing_failed_at
                    .is_some_and(|at| now - at < BRIEFING_RETRY_MS)
                    || self.status.paused_until_ms.is_some_and(|until| until > now))
            {
                continue;
            }
            // A long job (AI calls) may outlive the TTL: renew before each one.
            if !first && !self.hold_lock(clock())? {
                report.skipped_locked = true;
                break;
            }
            first = false;

            let started = clock();
            let result = self.run_job(job, clock);
            // Failed jobs wait for their interval too; the briefing has its own retry rule.
            if job != Job::Briefing {
                self.last_run.insert(job, started);
            }
            match result {
                Ok(Some(changed)) => {
                    report.ran.push(job);
                    report.changed |= changed;
                }
                Ok(None) => {}
                Err(e) => {
                    report.ran.push(job);
                    if job == Job::Briefing {
                        self.briefing_failed_at = Some(started);
                    }
                    self.record_error(format!("{}: {e}", job.name()));
                }
            }
        }
        Ok(report)
    }

    /// `Ok(None)`: nothing to do (not counted as run); `Ok(Some(changed))`: ran.
    fn run_job(&mut self, job: Job, clock: &dyn Fn() -> i64) -> Result<Option<bool>> {
        let now = clock();
        // The AI jobs hold `&mut self.conn` for minutes: they get a clock that also renews the
        // lock, on its own short-lived connection.
        let last_renew = Cell::new(now);
        let renewing = || {
            let t = clock();
            if t - last_renew.get() >= RENEW_EVERY_MS {
                last_renew.set(t);
                // Best effort: if it fails, the next call tries again.
                let _ = renew_lock(&self.paths.db_path, &self.owner_id, self.lock_ttl_ms, t);
            }
            t
        };
        match job {
            Job::Ingest => {
                let r = ingest::run(&mut self.conn, &self.paths, &self.settings, &self.git, now)?;
                for w in r.warnings {
                    self.record_error(format!("ingest: {w}"));
                }
                Ok(Some(r.processed > 0))
            }
            Job::Collect => {
                let before = fingerprint(&self.conn)?;
                let r = collector::run(
                    &mut self.conn,
                    &self.paths,
                    &self.settings,
                    &self.git,
                    CollectMode::Window,
                    now,
                    &mut |_, _| {},
                )?;
                for e in r.errors {
                    self.record_error(format!("collect: {e}"));
                }
                // The collector touches every session it sees, so "wrote" is read from the data:
                // sessions or runs added, or runs closed.
                Ok(Some(
                    r.closed_idle > 0 || fingerprint(&self.conn)? != before,
                ))
            }
            Job::Summarize => {
                let r = summaries::run_batch(
                    &mut self.conn,
                    &*self.ai,
                    &self.paths,
                    &self.settings,
                    SUMMARIZE_LIMIT,
                    false,
                    &renewing,
                )?;
                match r.paused_until_ms {
                    Some(until) => {
                        self.status.paused_until_ms = Some(until);
                        if r.pause_reason.is_some() {
                            self.status.pause_reason = r.pause_reason;
                        }
                    }
                    None => {
                        self.status.paused_until_ms = None;
                        self.status.pause_reason = None;
                    }
                }
                Ok(Some(r.summarized + r.rescheduled + r.failed > 0))
            }
            Job::Briefing => {
                let today = briefing::today(&self.conn, self.tz, now)?;
                if today.is_some_and(|b| b.status == BriefingStatus::Done) {
                    self.last_run.insert(job, now);
                    return Ok(None);
                }
                let titles: Vec<String> =
                    inbox::build(&self.conn, &self.git, &self.cache, &self.settings, now)?
                        .into_iter()
                        .map(|i| i.title)
                        .collect();
                let outcome = briefing::run(
                    &mut self.conn,
                    &*self.ai,
                    &titles,
                    &self.settings,
                    self.tz,
                    false,
                    &renewing,
                )?;
                if outcome == BriefingOutcome::Failed {
                    self.briefing_failed_at = Some(now);
                    self.record_error("briefing: generation failed".to_string());
                } else {
                    self.briefing_failed_at = None;
                    self.last_run.insert(job, now);
                }
                Ok(Some(matches!(
                    outcome,
                    BriefingOutcome::Generated | BriefingOutcome::Failed
                )))
            }
            Job::Maintenance => {
                let pruned = maintenance::prune_spool(&self.paths.spool_dir, &self.settings, now)?;
                for w in pruned.warnings {
                    self.record_error(format!("maintenance: {w}"));
                }
                Ok(Some(search::rebuild(&mut self.conn)? > 0))
            }
        }
    }
}

fn lock_ttl_ms(settings: &Settings) -> i64 {
    i64::from(settings.ai_timeout_seconds) * 1000 + LOCK_MARGIN_MS
}

/// What a collect pass can change that the screens show: sessions, runs, open runs, end times.
fn fingerprint(conn: &Connection) -> Result<(i64, i64, i64, i64)> {
    Ok(conn.query_row(
        "SELECT (SELECT COUNT(*) FROM ai_sessions), (SELECT COUNT(*) FROM session_runs), \
         (SELECT COUNT(*) FROM session_runs WHERE ended_at IS NULL), \
         (SELECT COALESCE(SUM(ended_at), 0) FROM session_runs)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?)
}

/// Extends the lock's expiry on a fresh connection, only if `owner` still holds it. One short
/// IMMEDIATE transaction, never held across an AI call.
fn renew_lock(db_path: &Path, owner: &str, ttl_ms: i64, now: i64) -> Result<()> {
    let mut conn = db::open(db_path)?;
    let tx = db::write_tx(&mut conn)?;
    if app_state::get(&tx, LOCK_KEY, now)?.as_deref() == Some(owner) {
        app_state::set(&tx, LOCK_KEY, owner, Some(now.saturating_add(ttl_ms)))?;
    }
    tx.commit()?;
    Ok(())
}

impl Drop for Worker {
    /// Releases the lock so the next worker does not wait for the TTL (best effort).
    fn drop(&mut self) {
        let owned = app_state::get(&self.conn, LOCK_KEY, i64::MIN)
            .ok()
            .flatten();
        if owned.as_deref() == Some(self.owner_id.as_str())
            && let Ok(tx) = db::write_tx(&mut self.conn)
        {
            let _ = app_state::unlock(&tx, LOCK_KEY);
            let _ = tx.commit();
        }
    }
}
