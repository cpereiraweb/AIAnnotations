//! In-memory `ai_sessions` / `session_runs` rows with Eloquent-like saves: a save writes only
//! when a field changed, and then bumps `updated_at` (ms).

use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Result;
use crate::model::{self, EndReason, SessionOrigin, SummaryStatus, Tool};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionFields {
    pub project_id: i64,
    pub tool: Tool,
    pub session_id: String,
    pub cwd: String,
    pub worktree_path: Option<String>,
    pub transcript_path: Option<String>,
    pub transcript_mtime: Option<i64>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub origin: SessionOrigin,
    pub previous_session_id: Option<i64>,
    pub start_source: Option<String>,
    pub parent_session_id: Option<String>,
    pub is_empty: bool,
    pub last_activity_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct SessionRow {
    pub id: i64,
    pub f: SessionFields,
    saved: SessionFields,
}

const SESSION_COLUMNS: &str = "id, project_id, tool, session_id, cwd, worktree_path, \
    transcript_path, transcript_mtime, model, title, origin, previous_session_id, start_source, \
    parent_session_id, is_empty, last_activity_at";

fn session_from_row(r: &Row) -> rusqlite::Result<(i64, SessionFields, String, String)> {
    Ok((
        r.get(0)?,
        SessionFields {
            project_id: r.get(1)?,
            tool: Tool::Claude,
            session_id: r.get(3)?,
            cwd: r.get(4)?,
            worktree_path: r.get(5)?,
            transcript_path: r.get(6)?,
            transcript_mtime: r.get(7)?,
            model: r.get(8)?,
            title: r.get(9)?,
            origin: SessionOrigin::Interactive,
            previous_session_id: r.get(11)?,
            start_source: r.get(12)?,
            parent_session_id: r.get(13)?,
            is_empty: r.get(14)?,
            last_activity_at: r.get(15)?,
        },
        r.get(2)?,
        r.get(10)?,
    ))
}

impl SessionRow {
    fn query(conn: &Connection, filter: &str, args: impl rusqlite::Params) -> Result<Option<Self>> {
        let sql = format!("SELECT {SESSION_COLUMNS} FROM ai_sessions WHERE {filter}");
        let Some((id, mut f, tool, origin)) =
            conn.query_row(&sql, args, session_from_row).optional()?
        else {
            return Ok(None);
        };
        f.tool = Tool::from_str(&tool)?;
        f.origin = SessionOrigin::from_str(&origin)?;
        Ok(Some(SessionRow {
            id,
            saved: f.clone(),
            f,
        }))
    }

    pub fn find(conn: &Connection, id: i64) -> Result<Option<Self>> {
        Self::query(conn, "id = ?1", [id])
    }

    pub fn find_by_key(conn: &Connection, tool: Tool, session_id: &str) -> Result<Option<Self>> {
        Self::query(
            conn,
            "tool = ?1 AND session_id = ?2",
            params![tool.as_str(), session_id],
        )
    }

    pub fn insert(conn: &Connection, f: SessionFields, now_ms: i64) -> Result<Self> {
        conn.execute(
            "INSERT INTO ai_sessions (project_id, tool, session_id, cwd, worktree_path, \
             transcript_path, transcript_mtime, model, title, origin, previous_session_id, \
             start_source, parent_session_id, is_empty, last_activity_at, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
            params![
                f.project_id,
                f.tool.as_str(),
                f.session_id,
                f.cwd,
                f.worktree_path,
                f.transcript_path,
                f.transcript_mtime,
                f.model,
                f.title,
                f.origin.as_str(),
                f.previous_session_id,
                f.start_source,
                f.parent_session_id,
                f.is_empty,
                f.last_activity_at,
                now_ms,
            ],
        )?;
        Ok(SessionRow {
            id: conn.last_insert_rowid(),
            saved: f.clone(),
            f,
        })
    }

    /// Writes the changed row (all fields) and bumps `updated_at`; no-op when nothing changed.
    pub fn save(&mut self, conn: &Connection, now_ms: i64) -> Result<()> {
        if self.f == self.saved {
            return Ok(());
        }
        let f = &self.f;
        conn.execute(
            "UPDATE ai_sessions SET project_id = ?1, tool = ?2, session_id = ?3, cwd = ?4, \
             worktree_path = ?5, transcript_path = ?6, transcript_mtime = ?7, model = ?8, \
             title = ?9, origin = ?10, previous_session_id = ?11, start_source = ?12, \
             parent_session_id = ?13, is_empty = ?14, last_activity_at = ?15, updated_at = ?16 \
             WHERE id = ?17",
            params![
                f.project_id,
                f.tool.as_str(),
                f.session_id,
                f.cwd,
                f.worktree_path,
                f.transcript_path,
                f.transcript_mtime,
                f.model,
                f.title,
                f.origin.as_str(),
                f.previous_session_id,
                f.start_source,
                f.parent_session_id,
                f.is_empty,
                f.last_activity_at,
                now_ms,
                self.id,
            ],
        )?;
        self.saved = self.f.clone();
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunFields {
    pub ai_session_id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<EndReason>,
    pub ended_cleanly: Option<bool>,
    pub branch: Option<String>,
    pub head_start: Option<String>,
    pub head_end: Option<String>,
    pub status_hash_start: Option<String>,
    pub status_hash_end: Option<String>,
    pub changed_files: Option<bool>,
    /// Decoded `files_touched` JSON (compared decoded, as Eloquent's array cast does).
    pub files_touched: Option<Vec<String>>,
    pub summary_status: SummaryStatus,
    pub summary_attempts: i64,
    pub summarized_fingerprint: Option<String>,
    pub summary_available_at: Option<i64>,
    pub is_backfill: bool,
    pub discovered: bool,
}

impl RunFields {
    pub fn new(ai_session_id: i64, started_at: i64) -> Self {
        RunFields {
            ai_session_id,
            started_at,
            ended_at: None,
            end_reason: None,
            ended_cleanly: None,
            branch: None,
            head_start: None,
            head_end: None,
            status_hash_start: None,
            status_hash_end: None,
            changed_files: None,
            files_touched: None,
            summary_status: SummaryStatus::Pending,
            summary_attempts: 0,
            summarized_fingerprint: None,
            summary_available_at: None,
            is_backfill: false,
            discovered: false,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RunRow {
    pub id: i64,
    pub f: RunFields,
    saved: RunFields,
}

const RUN_COLUMNS: &str = "id, ai_session_id, started_at, ended_at, end_reason, ended_cleanly, \
    branch, head_start, head_end, status_hash_start, status_hash_end, changed_files, \
    files_touched, summary_status, summary_attempts, summarized_fingerprint, \
    summary_available_at, is_backfill, discovered";

struct RawRun {
    id: i64,
    f: RunFields,
    end_reason: Option<String>,
    files_touched: Option<String>,
    summary_status: String,
}

fn run_from_row(r: &Row) -> rusqlite::Result<RawRun> {
    Ok(RawRun {
        id: r.get(0)?,
        f: RunFields {
            ai_session_id: r.get(1)?,
            started_at: r.get(2)?,
            ended_at: r.get(3)?,
            end_reason: None,
            ended_cleanly: r.get(5)?,
            branch: r.get(6)?,
            head_start: r.get(7)?,
            head_end: r.get(8)?,
            status_hash_start: r.get(9)?,
            status_hash_end: r.get(10)?,
            changed_files: r.get(11)?,
            files_touched: None,
            summary_status: SummaryStatus::Pending,
            summary_attempts: r.get(14)?,
            summarized_fingerprint: r.get(15)?,
            summary_available_at: r.get(16)?,
            is_backfill: r.get(17)?,
            discovered: r.get(18)?,
        },
        end_reason: r.get(4)?,
        files_touched: r.get(12)?,
        summary_status: r.get(13)?,
    })
}

fn encode_files(files: &Option<Vec<String>>) -> Result<Option<String>> {
    Ok(match files {
        Some(list) => Some(serde_json::to_string(list)?),
        None => None,
    })
}

impl RunRow {
    fn from_raw(raw: RawRun) -> Result<Self> {
        let mut f = raw.f;
        f.end_reason = raw
            .end_reason
            .as_deref()
            .map(EndReason::from_str)
            .transpose()?;
        // Not a list of strings (corrupt or foreign value): treated as absent.
        f.files_touched = raw
            .files_touched
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());
        f.summary_status = SummaryStatus::from_str(&raw.summary_status)?;
        Ok(RunRow {
            id: raw.id,
            saved: f.clone(),
            f,
        })
    }

    fn query_all(conn: &Connection, tail: &str, args: impl rusqlite::Params) -> Result<Vec<Self>> {
        let sql = format!("SELECT {RUN_COLUMNS} FROM session_runs WHERE {tail}");
        let mut stmt = conn.prepare(&sql)?;
        let raws = stmt
            .query_map(args, run_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        raws.into_iter().map(Self::from_raw).collect()
    }

    pub fn find(conn: &Connection, id: i64) -> Result<Option<Self>> {
        Ok(Self::query_all(conn, "id = ?1", [id])?.into_iter().next())
    }

    /// `latestRun`: greatest `started_at`.
    pub fn latest(conn: &Connection, ai_session_id: i64) -> Result<Option<Self>> {
        match model::latest_run_id(conn, ai_session_id)? {
            Some(id) => Self::find(conn, id),
            None => Ok(None),
        }
    }

    /// Most recent run still open.
    pub fn latest_open(conn: &Connection, ai_session_id: i64) -> Result<Option<Self>> {
        Ok(Self::query_all(
            conn,
            "ai_session_id = ?1 AND ended_at IS NULL ORDER BY started_at DESC, id DESC LIMIT 1",
            [ai_session_id],
        )?
        .into_iter()
        .next())
    }

    /// Open runs created by the collector, most recent first.
    pub fn open_discovered(conn: &Connection, ai_session_id: i64) -> Result<Vec<Self>> {
        Self::query_all(
            conn,
            "ai_session_id = ?1 AND ended_at IS NULL AND discovered = 1 \
             ORDER BY started_at DESC, id DESC",
            [ai_session_id],
        )
    }

    pub fn insert(conn: &Connection, f: RunFields, now_ms: i64) -> Result<Self> {
        conn.execute(
            "INSERT INTO session_runs (ai_session_id, started_at, ended_at, end_reason, \
             ended_cleanly, branch, head_start, head_end, status_hash_start, status_hash_end, \
             changed_files, files_touched, summary_status, summary_attempts, \
             summarized_fingerprint, summary_available_at, is_backfill, discovered, created_at, \
             updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, \
             ?15, ?16, ?17, ?18, ?19, ?19)",
            params![
                f.ai_session_id,
                f.started_at,
                f.ended_at,
                f.end_reason.map(|r| r.as_str()),
                f.ended_cleanly,
                f.branch,
                f.head_start,
                f.head_end,
                f.status_hash_start,
                f.status_hash_end,
                f.changed_files,
                encode_files(&f.files_touched)?,
                f.summary_status.as_str(),
                f.summary_attempts,
                f.summarized_fingerprint,
                f.summary_available_at,
                f.is_backfill,
                f.discovered,
                now_ms,
            ],
        )?;
        Ok(RunRow {
            id: conn.last_insert_rowid(),
            saved: f.clone(),
            f,
        })
    }

    /// Writes the changed row and bumps `updated_at`; no-op when nothing changed. Columns owned by
    /// the summarizer (`summary`, `next_step`, `unfinished`, `kind`) are left untouched.
    pub fn save(&mut self, conn: &Connection, now_ms: i64) -> Result<()> {
        if self.f == self.saved {
            return Ok(());
        }
        let f = &self.f;
        conn.execute(
            "UPDATE session_runs SET ai_session_id = ?1, started_at = ?2, ended_at = ?3, \
             end_reason = ?4, ended_cleanly = ?5, branch = ?6, head_start = ?7, head_end = ?8, \
             status_hash_start = ?9, status_hash_end = ?10, changed_files = ?11, \
             files_touched = ?12, summary_status = ?13, summary_attempts = ?14, \
             summarized_fingerprint = ?15, summary_available_at = ?16, is_backfill = ?17, \
             discovered = ?18, updated_at = ?19 WHERE id = ?20",
            params![
                f.ai_session_id,
                f.started_at,
                f.ended_at,
                f.end_reason.map(|r| r.as_str()),
                f.ended_cleanly,
                f.branch,
                f.head_start,
                f.head_end,
                f.status_hash_start,
                f.status_hash_end,
                f.changed_files,
                encode_files(&f.files_touched)?,
                f.summary_status.as_str(),
                f.summary_attempts,
                f.summarized_fingerprint,
                f.summary_available_at,
                f.is_backfill,
                f.discovered,
                now_ms,
                self.id,
            ],
        )?;
        self.saved = self.f.clone();
        Ok(())
    }
}
