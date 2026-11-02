//! Turns hook events into sessions (tool + session_id) and runs (each start→end interval).
//! Port of `app/Sessions/SessionRecorder.php` (Inv §3.4).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::change_detector::{self, RunGit};
use super::discovery::DiscoveredSession;
use super::event::{EventKind, HookEvent};
use super::projects;
use super::rows::{RunFields, RunRow, SessionFields, SessionRow};
use crate::git::{Git, GitSnapshot};
use crate::model::{EndReason, SessionOrigin, SummaryStatus, Tool};
use crate::transcripts::php::PHP_WS;
use crate::transcripts::{TranscriptExtract, factory, origin};
use crate::{Error, Paths, Result, Settings, db};

pub const TITLE_LIMIT: usize = 200;

/// Claude end reasons that count as a clean exit.
const CLEAN_CLAUDE_REASONS: [EndReason; 3] = [
    EndReason::PromptInputExit,
    EndReason::Clear,
    EndReason::Logout,
];

/// Outcome of reading a session's transcript: absent (no file / unknown thread) is not an error,
/// unreadable (I/O, locked or unexpected Codex database...) is. Callers that must go on recording
/// treat both as "no transcript"; the collector (Task 11) must not memorize the mtime of an
/// unreadable read (v1 `$unreadable`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TranscriptRead {
    Absent,
    Read(Box<TranscriptExtract>),
    Unreadable(String),
}

impl TranscriptRead {
    pub(crate) fn read(
        tool: Tool,
        session_id: &str,
        transcript_path: Option<&Path>,
        paths: &Paths,
    ) -> Self {
        match factory::read(tool, session_id, transcript_path, paths) {
            Ok(Some(x)) => TranscriptRead::Read(Box::new(x)),
            Ok(None) => TranscriptRead::Absent,
            Err(e) => TranscriptRead::Unreadable(e.to_string()),
        }
    }

    pub(crate) fn into_extract(self) -> Option<TranscriptExtract> {
        match self {
            TranscriptRead::Read(x) => Some(*x),
            _ => None,
        }
    }
}

pub struct Recorder<'a> {
    pub conn: &'a mut Connection,
    pub paths: &'a Paths,
    pub settings: &'a Settings,
    pub git: &'a Git,
    warnings: Vec<String>,
    /// Git snapshots per cwd, only for the collector (v1 `$discoverySnapshots`).
    discovery_snapshots: HashMap<String, GitSnapshot>,
}

impl<'a> Recorder<'a> {
    pub fn new(
        conn: &'a mut Connection,
        paths: &'a Paths,
        settings: &'a Settings,
        git: &'a Git,
    ) -> Self {
        Recorder {
            conn,
            paths,
            settings,
            git,
            warnings: Vec::new(),
            discovery_snapshots: HashMap::new(),
        }
    }

    /// Warnings collected since the last call (unreadable transcripts, unknown origins); the
    /// library never prints them itself.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Records one event in its own `IMMEDIATE` transaction. The git snapshot is taken before the
    /// write lock. `now_ms` (the caller's clock) stamps `created_at`/`updated_at` and is the end of
    /// open runs for the parallel-session check.
    pub fn record(&mut self, ev: &HookEvent, now_ms: i64) -> Result<()> {
        if ev.session_id.is_empty() || ev.cwd.as_os_str().is_empty() {
            return Err(Error::InvalidEvent(
                "session_id and cwd are required".into(),
            ));
        }
        let snap = self.git.snapshot(&ev.cwd);
        let tx = db::write_tx(self.conn)?;
        let w = Writer {
            tx: &tx,
            paths: self.paths,
            settings: self.settings,
            now_ms,
            warnings: RefCell::new(Vec::new()),
            unreadable: Cell::new(false),
        };
        let result = match ev.kind {
            EventKind::Start => w.start(ev, &snap),
            EventKind::End => w.end(ev, &snap),
        };
        self.warnings.append(&mut w.warnings.take());
        result?;
        tx.commit()?;
        Ok(())
    }

    /// Updates the session and its latest run from the transcript (`summary_status`, files,
    /// `changed_files`...). Without `extract`, reads the transcript now. Unknown session: no-op.
    pub fn refresh_from_transcript(
        &mut self,
        ai_session_id: i64,
        extract: Option<&TranscriptExtract>,
        now_ms: i64,
    ) -> Result<()> {
        let tx = db::write_tx(self.conn)?;
        let w = Writer {
            tx: &tx,
            paths: self.paths,
            settings: self.settings,
            now_ms,
            warnings: RefCell::new(Vec::new()),
            unreadable: Cell::new(false),
        };
        let result = (|| {
            if let Some(mut s) = SessionRow::find(&tx, ai_session_id)? {
                let read;
                let extract = match extract {
                    Some(x) => Some(x),
                    None => {
                        read = w.extract(&s);
                        read.as_ref()
                    }
                };
                w.refresh(&mut s, extract)?;
            }
            Ok::<_, Error>(())
        })();
        self.warnings.append(&mut w.warnings.take());
        result?;
        tx.commit()?;
        Ok(())
    }

    /// Records a session found by the collector (no hook) or updates the known one, in its own
    /// `IMMEDIATE` transaction (Inv §3.5 `upsertDiscovered`):
    /// - new: creates the session and a `discovered` run from `first_at` to `last_at`, closed as
    ///   Idle when the inactivity exceeds `idle_minutes`;
    /// - latest run Idle with activity after its `ended_at`: reopens it;
    /// - latest run closed cleanly and activity after it (resumed without hook): a new run;
    /// - open run and inactivity above `idle_minutes`: closes it as Idle.
    ///
    /// The transcript is only reread when its mtime changed since the previous pass.
    pub fn upsert_discovered(
        &mut self,
        d: &DiscoveredSession,
        backfill: bool,
        now_ms: i64,
    ) -> Result<()> {
        self.upsert_discovered_from(d, backfill, true, now_ms)
            .map(|_| ())
    }

    /// `upsert_discovered`; `from_disk = false` is a session the scan did not see, revisited only
    /// to close it for inactivity: its `last_activity_ms` is the stored activity, not an mtime, so
    /// `transcript_mtime` is left as is. Returns whether an open run was closed as Idle.
    pub(crate) fn upsert_discovered_from(
        &mut self,
        d: &DiscoveredSession,
        backfill: bool,
        from_disk: bool,
        now_ms: i64,
    ) -> Result<bool> {
        // A new session needs a git snapshot, taken before the write lock.
        let existing = SessionRow::find_by_key(self.conn, d.tool, &d.session_id)?;
        let has_run = match &existing {
            Some(s) => crate::model::latest_run_id(self.conn, s.id)?.is_some(),
            None => false,
        };
        let snap = match (has_run, non_empty(&d.cwd)) {
            (false, Some(cwd)) => Some(
                self.discovery_snapshots
                    .entry(cwd.to_string())
                    .or_insert_with(|| self.git.snapshot(Path::new(cwd)))
                    .clone(),
            ),
            _ => None,
        };

        let tx = db::write_tx(self.conn)?;
        let w = Writer {
            tx: &tx,
            paths: self.paths,
            settings: self.settings,
            now_ms,
            warnings: RefCell::new(Vec::new()),
            unreadable: Cell::new(false),
        };
        let result = w.upsert_discovered(d, snap.as_ref(), self.git, backfill, from_disk);
        self.warnings.append(&mut w.warnings.take());
        let closed = result?;
        tx.commit()?;
        Ok(closed)
    }
}

/// Whole seconds of an instant: v1 stores instants without milliseconds (Inv §3.4).
fn sec(ms: i64) -> i64 {
    ms.div_euclid(1000)
}

fn non_empty(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

struct Writer<'a, 'c> {
    tx: &'a Transaction<'c>,
    paths: &'a Paths,
    settings: &'a Settings,
    /// Caller's clock for `created_at`/`updated_at` and the end of open runs.
    now_ms: i64,
    warnings: RefCell<Vec<String>>,
    /// Some transcript read failed transiently during this write (v1 `$unreadable`).
    unreadable: Cell<bool>,
}

impl Writer<'_, '_> {
    fn start(&self, ev: &HookEvent, snap: &GitSnapshot) -> Result<()> {
        let at = ev.received_at_ms;
        let mut s = self.upsert_session(ev, snap, at)?;
        let mut latest = RunRow::latest(self.tx, s.id)?;
        let source = non_empty(&ev.source);

        // The first start seen, even if the session already exists (found by the collector, or
        // created by an end without start): the inverted /clear chaining depends on it.
        if s.f.start_source.is_none()
            && let Some(src) = source
        {
            s.f.start_source = Some(src.to_string());
            s.save(self.tx, self.now_ms)?;
        }

        // Compaction creates no run: it only marks activity.
        if source == Some("compact")
            && let Some(run) = latest.as_mut()
        {
            if run.f.end_reason == Some(EndReason::Idle) {
                self.reopen(run)?;
            }
            return self.touch(&mut s, at);
        }

        if source == Some("clear")
            && s.f.previous_session_id.is_none()
            && let Some(previous) = self.cleared_predecessor(&s, at)?
        {
            s.f.previous_session_id = Some(previous);
            s.save(self.tx, self.now_ms)?;
        }

        if let Some(run) = latest.as_mut()
            && run.f.end_reason == Some(EndReason::Idle)
            && self.had_activity_after(&s, run)
        {
            self.reopen(run)?;
            return self.touch(&mut s, at);
        }

        // The collector found the transcript before the start was ingested: same run.
        if let Some(mut run) = self.recent_discovered_run(&s, at)? {
            let f = &mut run.f;
            f.started_at = f.started_at.min(at);
            f.head_start = f.head_start.take().or_else(|| snap.head.clone());
            f.status_hash_start = f
                .status_hash_start
                .take()
                .or_else(|| snap.status_hash.clone());
            f.discovered = false;
            // A live session now, not imported history.
            f.is_backfill = false;
            f.branch = f.branch.take().or_else(|| snap.branch.clone());
            run.save(self.tx, self.now_ms)?;
            return self.touch(&mut s, at);
        }

        let mut f = RunFields::new(s.id, at);
        f.branch = snap.branch.clone();
        f.head_start = snap.head.clone();
        f.status_hash_start = snap.status_hash.clone();
        RunRow::insert(self.tx, f, self.now_ms)?;
        self.touch(&mut s, at)
    }

    /// Open run created by the collector (no start) that began up to `idle_minutes` before or
    /// after `at`.
    fn recent_discovered_run(&self, s: &SessionRow, at: i64) -> Result<Option<RunRow>> {
        let window = i64::from(self.settings.idle_minutes) * 60;
        Ok(RunRow::open_discovered(self.tx, s.id)?
            .into_iter()
            .find(|r| (sec(r.f.started_at) - sec(at)).abs() <= window))
    }

    fn end(&self, ev: &HookEvent, snap: &GitSnapshot) -> Result<()> {
        let at = ev.received_at_ms;
        let mut s = self.upsert_session(ev, snap, at)?;
        let reason = ev
            .reason
            .as_deref()
            .and_then(|r| r.parse::<EndReason>().ok())
            .filter(|r| *r != EndReason::Idle)
            .unwrap_or(EndReason::Other);

        let latest = RunRow::latest(self.tx, s.id)?;
        // A real end replaces the collector's "closed without /exit" mark.
        let run = RunRow::latest_open(self.tx, s.id)?.or_else(|| {
            latest
                .clone()
                .filter(|l| l.f.end_reason == Some(EndReason::Idle))
        });

        // Repeated SessionEnd: the latest run already closed for the same reason moments ago.
        if run.is_none()
            && let Some(l) = &latest
            && let Some(ended) = l.f.ended_at
            && l.f.end_reason == Some(reason)
            && (0..=i64::from(self.settings.duplicate_end_seconds))
                .contains(&(sec(at) - sec(ended)))
        {
            return Ok(());
        }

        let extract = self.extract(&s);
        let close = |f: &mut RunFields| {
            f.ended_at = Some(at);
            f.end_reason = Some(reason);
            f.ended_cleanly =
                Some(s.f.tool == Tool::Codex || CLEAN_CLAUDE_REASONS.contains(&reason));
            f.head_end = snap.head.clone();
            f.status_hash_end = snap.status_hash.clone();
            f.branch = f.branch.take().or_else(|| snap.branch.clone());
        };

        match run {
            Some(mut run) => {
                close(&mut run.f);
                run.save(self.tx, self.now_ms)?;
            }
            None => {
                let started = synthesized_start(extract.as_ref(), latest.as_ref(), at);
                let mut f = RunFields::new(s.id, started.min(at));
                f.branch = snap.branch.clone();
                close(&mut f);
                RunRow::insert(self.tx, f, self.now_ms)?;
            }
        }

        self.touch(&mut s, at)?;
        self.refresh(&mut s, extract.as_ref())?;

        if reason == EndReason::Clear {
            self.chain_cleared_successor(&s, at)?;
        }
        Ok(())
    }

    fn upsert_discovered(
        &self,
        d: &DiscoveredSession,
        snap: Option<&GitSnapshot>,
        git: &Git,
        backfill: bool,
        from_disk: bool,
    ) -> Result<bool> {
        let last_activity = d.last_activity_ms;
        let idle = last_activity < self.now_ms - i64::from(self.settings.idle_minutes) * 60_000;

        let session = SessionRow::find_by_key(self.tx, d.tool, &d.session_id)?;
        let latest = match &session {
            Some(s) => RunRow::latest(self.tx, s.id)?,
            None => None,
        };
        let (Some(mut s), Some(mut run)) = (session, latest) else {
            self.create_discovered(d, snap, git, idle, backfill)?;
            return Ok(false);
        };

        if s.f.transcript_path.is_none()
            && let Some(p) = &d.transcript_path
        {
            s.f.transcript_path = Some(p.to_string_lossy().into_owned());
            s.save(self.tx, self.now_ms)?;
        }

        // Same mtime as the previous pass: the transcript did not change, nothing to reread.
        // Without the real mtime (session outside the scan), only the idle closing runs.
        let changed = from_disk && s.f.transcript_mtime != Some(last_activity);

        if changed
            && run.f.end_reason == Some(EndReason::Idle)
            && run.f.ended_at.is_some_and(|e| sec(last_activity) > sec(e))
            && self.had_activity_after(&s, &run)
        {
            self.reopen(&mut run)?;
        }

        if changed && self.resumed_without_hook(&run, last_activity) {
            let extract = self.extract(&s);
            let tolerance = i64::from(self.settings.resume_tolerance_seconds);
            if let Some(x) = &extract
                && let (Some(last), Some(ended)) = (x.last_at, run.f.ended_at)
                && sec(last) > sec(ended) + tolerance
            {
                self.start_discovered_run(&mut s, &run, x, last, idle, backfill)?;
                if !self.unreadable.get() {
                    s.f.transcript_mtime = Some(last_activity);
                }
                self.touch(&mut s, last_activity)?;
                return Ok(false);
            }
        }

        self.touch(&mut s, last_activity)?;

        let mut closed = false;
        if run.f.ended_at.is_none() && idle {
            let extract = self.extract(&s);
            let ended = extract
                .as_ref()
                .and_then(|x| x.last_at)
                .unwrap_or(last_activity);
            run.f.ended_at = Some(ended.max(run.f.started_at));
            run.f.end_reason = Some(EndReason::Idle);
            run.f.ended_cleanly = Some(false);
            run.save(self.tx, self.now_ms)?;
            self.refresh(&mut s, extract.as_ref())?;
            closed = true;
        }

        if changed && !self.unreadable.get() {
            s.f.transcript_mtime = Some(last_activity);
            s.save(self.tx, self.now_ms)?;
        }
        Ok(closed)
    }

    /// Latest run closed cleanly (by a hook, not for inactivity) and activity more than
    /// `resume_tolerance_seconds` later: the session was resumed without SessionStart (Codex
    /// desktop app, Claude resumed outside the terminal).
    fn resumed_without_hook(&self, run: &RunRow, last_activity: i64) -> bool {
        let tolerance = i64::from(self.settings.resume_tolerance_seconds);
        run.f.ended_cleanly == Some(true)
            && run.f.end_reason != Some(EndReason::Idle)
            && run
                .f
                .ended_at
                .is_some_and(|e| sec(last_activity) > sec(e) + tolerance)
    }

    /// New run from the first activity after the previous run's end plus the tolerance, open or
    /// already closed as Idle depending on the inactivity.
    fn start_discovered_run(
        &self,
        s: &mut SessionRow,
        previous: &RunRow,
        x: &TranscriptExtract,
        ended: i64,
        idle: bool,
        backfill: bool,
    ) -> Result<()> {
        let previous_end = previous.f.ended_at.unwrap_or(ended);
        // After the tolerance: final lines written seconds after the clean end open nothing.
        let tolerance_ms = i64::from(self.settings.resume_tolerance_seconds) * 1000;
        let started = x
            .first_at_after(previous_end + tolerance_ms)
            .unwrap_or(previous_end)
            .min(ended);
        let mut f = RunFields::new(s.id, started);
        if idle {
            f.ended_at = Some(ended);
            f.end_reason = Some(EndReason::Idle);
            f.ended_cleanly = Some(false);
        }
        f.branch = x.branch.clone().or_else(|| previous.f.branch.clone());
        f.is_backfill = backfill;
        f.discovered = true;
        RunRow::insert(self.tx, f, self.now_ms)?;
        self.refresh(s, Some(x))
    }

    fn create_discovered(
        &self,
        d: &DiscoveredSession,
        snap: Option<&GitSnapshot>,
        git: &Git,
        idle: bool,
        backfill: bool,
    ) -> Result<()> {
        let Some(cwd) = non_empty(&d.cwd) else {
            return Err(Error::InvalidEvent(format!(
                "discovered {} session {} has no cwd",
                d.tool.as_str(),
                d.session_id
            )));
        };
        // Normally taken before the lock; only a session created meanwhile lacks it.
        let taken;
        let snap = match snap {
            Some(snap) => snap,
            None => {
                taken = git.snapshot(Path::new(cwd));
                &taken
            }
        };
        let ev = HookEvent {
            tool: d.tool,
            kind: EventKind::Start,
            session_id: d.session_id.clone(),
            cwd: cwd.into(),
            transcript_path: d.transcript_path.clone(),
            model: None,
            source: None,
            reason: None,
            received_at_ms: d.last_activity_ms,
        };
        let mut s = self.upsert_session(&ev, snap, d.last_activity_ms)?;

        let extract = self.extract(&s);
        let ended = extract
            .as_ref()
            .and_then(|x| x.last_at)
            .unwrap_or(d.last_activity_ms);
        let started = extract
            .as_ref()
            .and_then(|x| x.first_at)
            .unwrap_or(ended)
            .min(ended);

        s.f.transcript_mtime = (!self.unreadable.get()).then_some(d.last_activity_ms);
        s.save(self.tx, self.now_ms)?;

        let mut f = RunFields::new(s.id, started);
        if idle {
            f.ended_at = Some(ended);
            f.end_reason = Some(EndReason::Idle);
            f.ended_cleanly = Some(false);
        }
        f.branch = extract
            .as_ref()
            .and_then(|x| x.branch.clone())
            .or_else(|| snap.branch.clone());
        f.is_backfill = backfill;
        f.discovered = true;
        RunRow::insert(self.tx, f, self.now_ms)?;

        self.refresh(&mut s, extract.as_ref())
    }

    fn refresh(&self, s: &mut SessionRow, extract: Option<&TranscriptExtract>) -> Result<()> {
        let run = RunRow::latest(self.tx, s.id)?;

        if let Some(x) = extract {
            if let Some(model) = &x.model {
                s.f.model = Some(model.clone());
            }
            if let Some(title) = &x.title {
                s.f.title = Some(
                    title
                        .trim_matches(PHP_WS)
                        .chars()
                        .take(TITLE_LIMIT)
                        .collect(),
                );
            }
            s.f.origin = x.origin;
            if let Some(parent) = &x.parent_session_id {
                s.f.parent_session_id = Some(parent.clone());
            }
            // Empty = no real user prompt (turns with an empty prompt do not count).
            s.f.is_empty = !x.has_user_prompt();
            if let Some(last) = x.last_at {
                bump_activity(s, last);
            }
            if let Some(value) = &x.unknown_origin
                && origin::warn_unknown_once(self.tx, s.f.tool, value, self.now_ms)?
            {
                self.warnings.borrow_mut().push(format!(
                    "unknown {} session origin {value:?} (session {}); treated as interactive",
                    s.f.tool.as_str(),
                    s.f.session_id
                ));
            }
        }
        s.save(self.tx, self.now_ms)?;

        if let Some(mut run) = run {
            let git = RunGit {
                started_at: Some(run.f.started_at),
                ended_at: run.f.ended_at,
                ..Default::default()
            };
            run.f.files_touched = extract.map(|x| change_detector::files_for(x, &git));
            run.f.changed_files = change_detector::detect(self.tx, run.id, extract, self.now_ms)?;

            let mut status = if s.f.origin != SessionOrigin::Interactive || s.f.is_empty {
                SummaryStatus::Skipped
            } else {
                SummaryStatus::Pending
            };
            // Summary already made over this same content: reopening/closing again keeps it.
            if status == SummaryStatus::Pending
                && run.f.summary_status == SummaryStatus::Done
                && extract.is_some_and(|x| {
                    run.f.summarized_fingerprint.as_deref() == Some(x.fingerprint().as_str())
                })
            {
                status = SummaryStatus::Done;
            }
            // Back to Pending (resumed or grown session): a fresh attempts budget.
            if status == SummaryStatus::Pending && run.f.summary_status != SummaryStatus::Pending {
                run.f.summary_attempts = 0;
                run.f.summary_available_at = None;
            }
            run.f.summary_status = status;
            run.save(self.tx, self.now_ms)?;
        }

        projects::bump_activity(self.tx, s.f.project_id, s.f.last_activity_at, self.now_ms)
    }

    fn upsert_session(&self, ev: &HookEvent, snap: &GitSnapshot, at: i64) -> Result<SessionRow> {
        let existing = SessionRow::find_by_key(self.tx, ev.tool, &ev.session_id)?;
        // Leaves the project's activity as v1 does: an existing session's own activity is
        // already reflected in its project.
        let activity = existing.as_ref().map_or(at, |s| s.f.last_activity_at);
        let project_id = projects::upsert(self.tx, snap, &ev.cwd, activity, self.now_ms)?;
        let cwd = ev.cwd.to_string_lossy().into_owned();
        // Set only in a linked worktree (the main checkout's top level is the repository root).
        let worktree_path = snap
            .toplevel
            .as_ref()
            .filter(|top| Some(*top) != snap.repo_root.as_ref())
            .map(|top| top.to_string_lossy().into_owned());
        let transcript_path = ev
            .transcript_path
            .as_deref()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned());
        let model = non_empty(&ev.model).map(str::to_string);

        match existing {
            Some(mut s) => {
                s.f.project_id = project_id;
                s.f.cwd = cwd;
                s.f.worktree_path = worktree_path;
                if transcript_path.is_some() {
                    s.f.transcript_path = transcript_path;
                }
                if model.is_some() {
                    s.f.model = model;
                }
                s.save(self.tx, self.now_ms)?;
                Ok(s)
            }
            None => SessionRow::insert(
                self.tx,
                SessionFields {
                    project_id,
                    tool: ev.tool,
                    session_id: ev.session_id.clone(),
                    cwd,
                    worktree_path,
                    transcript_path,
                    transcript_mtime: None,
                    model,
                    title: None,
                    origin: SessionOrigin::Interactive,
                    previous_session_id: None,
                    start_source: None,
                    parent_session_id: None,
                    is_empty: false,
                    last_activity_at: at,
                },
                self.now_ms,
            ),
        }
    }

    /// Session of the same tool + cwd whose latest run ended with /clear up to
    /// `chain_window_seconds` before this start.
    fn cleared_predecessor(&self, s: &SessionRow, at: i64) -> Result<Option<i64>> {
        let window = i64::from(self.settings.chain_window_seconds);
        let (lo, hi) = (sec(at) - window, sec(at));
        let mut stmt = self.tx.prepare(
            "SELECT r.id, r.ai_session_id FROM session_runs r \
             JOIN ai_sessions a ON a.id = r.ai_session_id \
             WHERE r.end_reason = 'clear' AND r.ended_at >= ?1 AND r.ended_at < ?2 \
             AND a.tool = ?3 AND a.cwd = ?4 AND a.id != ?5 \
             ORDER BY r.ended_at DESC, r.id DESC",
        )?;
        let candidates = stmt
            .query_map(
                params![lo * 1000, (hi + 1) * 1000, s.f.tool.as_str(), s.f.cwd, s.id],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (run_id, session_id) in candidates {
            if crate::model::latest_run_id(self.tx, session_id)? == Some(run_id) {
                return Ok(Some(session_id));
            }
        }
        Ok(None)
    }

    /// Inverted /clear order: the new session's start(source=clear) was recorded before this
    /// end(reason=clear). Chains the session of the same tool + cwd started with clear within
    /// `chain_window_seconds` of this end and still without a predecessor.
    fn chain_cleared_successor(&self, s: &SessionRow, at: i64) -> Result<()> {
        let has_successor: bool = self.tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM ai_sessions WHERE previous_session_id = ?1)",
            [s.id],
            |r| r.get(0),
        )?;
        if has_successor {
            return Ok(());
        }
        let window = i64::from(self.settings.chain_window_seconds);
        let mut stmt = self.tx.prepare(
            "SELECT a.id FROM ai_sessions a WHERE a.tool = ?1 AND a.cwd = ?2 \
             AND a.start_source = 'clear' AND a.previous_session_id IS NULL AND a.id != ?3 \
             AND EXISTS (SELECT 1 FROM session_runs r WHERE r.ai_session_id = a.id \
                         AND r.started_at >= ?4 AND r.started_at < ?5) \
             ORDER BY a.id",
        )?;
        let candidates = stmt
            .query_map(
                params![
                    s.f.tool.as_str(),
                    s.f.cwd,
                    s.id,
                    (sec(at) - window) * 1000,
                    (sec(at) + window + 1) * 1000
                ],
                |r| r.get::<_, i64>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        // The run found has to be the session's first one (the clear start itself).
        let mut best: Option<(i64, i64)> = None;
        for id in candidates {
            let first: Option<i64> = self
                .tx
                .query_row(
                    "SELECT started_at FROM session_runs WHERE ai_session_id = ?1 \
                     ORDER BY started_at, id LIMIT 1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(first) = first else { continue };
            let distance = (sec(first) - sec(at)).abs();
            if distance <= window && best.is_none_or(|(_, d)| distance < d) {
                best = Some((id, distance));
            }
        }

        if let Some((id, _)) = best
            && let Some(mut successor) = SessionRow::find(self.tx, id)?
        {
            successor.f.previous_session_id = Some(s.id);
            successor.save(self.tx, self.now_ms)?;
        }
        Ok(())
    }

    fn had_activity_after(&self, s: &SessionRow, run: &RunRow) -> bool {
        let last = self.extract(s).and_then(|x| x.last_at);
        matches!((last, run.f.ended_at), (Some(l), Some(e)) if sec(l) > sec(e))
    }

    fn reopen(&self, run: &mut RunRow) -> Result<()> {
        run.f.ended_at = None;
        run.f.end_reason = None;
        run.f.ended_cleanly = None;
        run.save(self.tx, self.now_ms)
    }

    /// The transcript, or `None` when it is absent or unreadable right now (locked Codex
    /// database): the event is recorded anyway, and an unreadable read leaves a warning.
    fn extract(&self, s: &SessionRow) -> Option<TranscriptExtract> {
        let read = TranscriptRead::read(
            s.f.tool,
            &s.f.session_id,
            s.f.transcript_path.as_deref().map(Path::new),
            self.paths,
        );
        if let TranscriptRead::Unreadable(e) = &read {
            self.unreadable.set(true);
            self.warnings.borrow_mut().push(format!(
                "{} session {}: transcript unreadable: {e}",
                s.f.tool.as_str(),
                s.f.session_id
            ));
        }
        read.into_extract()
    }

    fn touch(&self, s: &mut SessionRow, at: i64) -> Result<()> {
        bump_activity(s, at);
        s.save(self.tx, self.now_ms)?;
        projects::bump_activity(self.tx, s.f.project_id, s.f.last_activity_at, self.now_ms)
    }
}

fn bump_activity(s: &mut SessionRow, at: i64) {
    if at > s.f.last_activity_at {
        s.f.last_activity_at = at;
    }
}

/// Start of a synthesized run (end without start): the transcript's first activity after the
/// previous run's end; without a previous run, the transcript's `first_at`. A previous run
/// without later activity: starts at its end.
fn synthesized_start(
    extract: Option<&TranscriptExtract>,
    previous: Option<&RunRow>,
    at: i64,
) -> i64 {
    if let Some(ended) = previous.and_then(|p| p.f.ended_at) {
        return extract
            .and_then(|x| x.first_at_after(ended))
            .unwrap_or(ended);
    }
    extract.and_then(|x| x.first_at).unwrap_or(at)
}
