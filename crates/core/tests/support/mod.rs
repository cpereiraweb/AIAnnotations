//! Shared test builders: database + paths environment, hook events, Claude/Codex transcripts,
//! git repositories and direct row inserts (Tasks 9–13).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

use aiannotations_core::db;
use aiannotations_core::git::Git;
use aiannotations_core::model::Tool;
use aiannotations_core::sessions::{EventKind, HookEvent, Recorder};
use aiannotations_core::time::parse_ts;
use aiannotations_core::{Paths, Settings};
use rusqlite::{Connection, params};
use serde_json::{Value, json};

pub const CLAUDE_FIXTURE: &str = include_str!("../fixtures/claude/session.jsonl");
pub const CONTATO: &str = "/home/fulano/Code/Projeto Ação/Exemplo/contato.html";

/// Milliseconds of an RFC 3339 instant.
pub fn ts(s: &str) -> i64 {
    parse_ts(s).unwrap_or_else(|| panic!("bad instant {s}"))
}

/// Fixed "now" of the tests (after every event instant they use): the core never reads the clock.
pub const NOW: &str = "2026-10-29T12:00:00Z";

pub fn now_ms() -> i64 {
    ts(NOW)
}

/// Temp dir (`base`, canonical, outside any git repo) with its own data dir, Claude dir and
/// Codex home, and an open database.
pub struct Env {
    _dir: tempfile::TempDir,
    pub base: PathBuf,
    pub paths: Paths,
    pub settings: Settings,
    pub git: Git,
    pub conn: Connection,
}

impl Env {
    pub fn new() -> Env {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap();
        let (data, claude, codex) = (
            base.join("data"),
            base.join("claude"),
            base.join("codex-home"),
        );
        let paths = Paths::from_env(
            |k| match k {
                "AIANNOTATIONS_DATA_DIR" => Some(data.clone().into()),
                "CLAUDE_CONFIG_DIR" => Some(claude.clone().into()),
                "CODEX_HOME" => Some(codex.clone().into()),
                _ => None,
            },
            &base,
            &base,
        );
        std::fs::create_dir_all(paths.claude_dir.join("projects")).unwrap();
        std::fs::create_dir_all(&paths.codex_home).unwrap();
        let conn = db::open(&paths.db_path).unwrap();
        Env {
            _dir: dir,
            base,
            paths,
            settings: Settings::default(),
            git: Git::default(),
            conn,
        }
    }

    pub fn recorder(&mut self) -> Recorder<'_> {
        Recorder::new(&mut self.conn, &self.paths, &self.settings, &self.git)
    }

    /// Records `ev`; a warning from the recorder fails the test (see [`Env::record_warnings`]).
    pub fn record(&mut self, ev: &HookEvent) {
        let warnings = self.record_warnings(ev);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    /// Records `ev` and returns the recorder's warnings.
    pub fn record_warnings(&mut self, ev: &HookEvent) -> Vec<String> {
        let mut recorder = self.recorder();
        recorder.record(ev, now_ms()).unwrap();
        recorder.take_warnings()
    }

    /// Copy of the Claude fixture (`tests/fixtures/claude/session.jsonl`) under
    /// `base/transcripts/<name>.jsonl`, with literal replacements.
    pub fn transcript(&self, name: &str, replace: &[(&str, &str)]) -> PathBuf {
        let mut body = CLAUDE_FIXTURE.to_string();
        for (from, to) in replace {
            body = body.replace(from, to);
        }
        let path = self.base.join("transcripts").join(format!("{name}.jsonl"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Git repository at `base/<rel>` (see [`git_repo`]).
    pub fn repo(&self, rel: &str) -> PathBuf {
        git_repo(&self.base.join(rel))
    }
}

/// A hook event without `source`/`reason`/`model`.
pub fn hook(
    tool: Tool,
    kind: EventKind,
    session_id: &str,
    cwd: &Path,
    transcript: Option<&Path>,
    at: &str,
) -> HookEvent {
    HookEvent {
        tool,
        kind,
        session_id: session_id.to_string(),
        cwd: cwd.to_path_buf(),
        transcript_path: transcript.map(Path::to_path_buf),
        model: None,
        source: None,
        reason: None,
        received_at_ms: ts(at),
    }
}

pub fn start(
    tool: Tool,
    session_id: &str,
    cwd: &Path,
    transcript: Option<&Path>,
    source: &str,
    at: &str,
) -> HookEvent {
    HookEvent {
        source: Some(source.to_string()),
        ..hook(tool, EventKind::Start, session_id, cwd, transcript, at)
    }
}

pub fn end(
    tool: Tool,
    session_id: &str,
    cwd: &Path,
    transcript: Option<&Path>,
    reason: &str,
    at: &str,
) -> HookEvent {
    HookEvent {
        reason: Some(reason.to_string()),
        ..hook(tool, EventKind::End, session_id, cwd, transcript, at)
    }
}

/// Writes one JSON value per line.
pub fn write_jsonl(path: &Path, lines: &[Value]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let body: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(path, body).unwrap();
}

/// PHP `File::append($path, "\n".json_encode($line)."\n")`.
pub fn append_line(path: &Path, line: &Value) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    write!(f, "\n{line}\n").unwrap();
}

/// A Claude user prompt line.
pub fn claude_user(session_id: &str, text: &str, at: &str) -> Value {
    json!({"type": "user", "sessionId": session_id, "entrypoint": "cli",
           "message": {"role": "user", "content": text}, "timestamp": at})
}

/// A Claude assistant text line.
pub fn claude_assistant(session_id: &str, model: &str, text: &str, at: &str) -> Value {
    json!({"type": "assistant", "sessionId": session_id, "entrypoint": "cli",
           "message": {"role": "assistant", "model": model,
                       "content": [{"type": "text", "text": text}]},
           "timestamp": at})
}

/// Minimal Codex rollout: `session_meta`, then one user prompt and one answer per pair, one
/// second apart starting at `at`.
pub fn codex_rollout(
    path: &Path,
    id: &str,
    cwd: &str,
    source: &str,
    at: &str,
    pairs: &[(&str, &str)],
) {
    let t0 = ts(at);
    let iso = |ms: i64| {
        chrono::DateTime::from_timestamp_millis(ms)
            .unwrap()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string()
    };
    let mut lines = vec![json!({"timestamp": iso(t0), "type": "session_meta",
        "payload": {"id": id, "timestamp": iso(t0), "cwd": cwd, "source": source}})];
    for (i, (prompt, answer)) in pairs.iter().enumerate() {
        let t = t0 + 1000 * (2 * i as i64 + 1);
        lines.push(json!({"timestamp": iso(t), "type": "event_msg",
            "payload": {"type": "user_message", "message": prompt, "images": []}}));
        lines.push(json!({"timestamp": iso(t + 1000), "type": "event_msg",
            "payload": {"type": "agent_message", "message": answer}}));
    }
    write_jsonl(path, &lines);
}

/// Runs git for test setup, independent of the developer's git config and environment.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        cmd.env_remove(var);
    }
    let out = cmd
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// `git init` on `main` plus an empty commit; returns the canonical path.
pub fn git_repo(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    git(path, &["init", "-q", "-b", "main"]);
    git(path, &["commit", "-q", "--allow-empty", "-m", "init"]);
    std::fs::canonicalize(path).unwrap()
}

pub fn insert_project(conn: &Connection, key: &str) -> i64 {
    let name = key.rsplit('/').next().unwrap_or(key);
    conn.execute(
        "INSERT INTO projects (key, name, path) VALUES (?1, ?2, ?1) \
         ON CONFLICT (key) DO NOTHING",
        params![key, name],
    )
    .unwrap();
    conn.query_row("SELECT id FROM projects WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .unwrap()
}

/// Session row as `AiSession::factory()` would leave it (project keyed by `cwd`).
pub fn insert_session(
    conn: &Connection,
    tool: Tool,
    session_id: &str,
    cwd: &Path,
    transcript: Option<&Path>,
) -> i64 {
    let project = insert_project(conn, &cwd.to_string_lossy());
    conn.execute(
        "INSERT INTO ai_sessions (project_id, tool, session_id, cwd, transcript_path, \
         last_activity_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            project,
            tool.as_str(),
            session_id,
            cwd.to_string_lossy(),
            transcript.map(|p| p.to_string_lossy().into_owned()),
            now_ms(),
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

/// Run columns for a direct insert; `None` leaves the schema default.
#[derive(Debug, Clone, Default)]
pub struct RunSeed {
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<&'static str>,
    pub ended_cleanly: Option<bool>,
    pub summary_status: Option<&'static str>,
    pub summary_attempts: i64,
    pub summary_available_at: Option<i64>,
    pub summarized_fingerprint: Option<String>,
    pub is_backfill: bool,
    pub discovered: bool,
}

pub fn insert_run(conn: &Connection, ai_session_id: i64, seed: RunSeed) -> i64 {
    conn.execute(
        "INSERT INTO session_runs (ai_session_id, started_at, ended_at, end_reason, ended_cleanly, \
         summary_status, summary_attempts, summary_available_at, summarized_fingerprint, \
         is_backfill, discovered) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            ai_session_id,
            seed.started_at,
            seed.ended_at,
            seed.end_reason,
            seed.ended_cleanly,
            seed.summary_status.unwrap_or("pending"),
            seed.summary_attempts,
            seed.summary_available_at,
            seed.summarized_fingerprint,
            seed.is_backfill,
            seed.discovered,
        ],
    )
    .unwrap();
    conn.last_insert_rowid()
}

/// A run as the collector creates it (Task 11): open, `discovered = 1`, from the transcript's
/// first event, on a session without `start_source`.
pub fn insert_discovered(
    conn: &Connection,
    session_id: &str,
    cwd: &Path,
    transcript: &Path,
    started_at: i64,
    is_backfill: bool,
) -> i64 {
    let session = insert_session(conn, Tool::Claude, session_id, cwd, Some(transcript));
    insert_run(
        conn,
        session,
        RunSeed {
            started_at,
            is_backfill,
            discovered: true,
            ..Default::default()
        },
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionView {
    pub id: i64,
    pub project_id: i64,
    pub tool: String,
    pub session_id: String,
    pub cwd: String,
    pub worktree_path: Option<String>,
    pub transcript_path: Option<String>,
    pub model: Option<String>,
    pub title: Option<String>,
    pub origin: String,
    pub previous_session_id: Option<i64>,
    pub start_source: Option<String>,
    pub is_empty: bool,
    pub last_activity_at: i64,
}

pub fn sessions(conn: &Connection, filter: &str) -> Vec<SessionView> {
    let sql = format!(
        "SELECT id, project_id, tool, session_id, cwd, worktree_path, transcript_path, model, \
         title, origin, previous_session_id, start_source, is_empty, last_activity_at \
         FROM ai_sessions WHERE {filter} ORDER BY id"
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    stmt.query_map([], |r| {
        Ok(SessionView {
            id: r.get(0)?,
            project_id: r.get(1)?,
            tool: r.get(2)?,
            session_id: r.get(3)?,
            cwd: r.get(4)?,
            worktree_path: r.get(5)?,
            transcript_path: r.get(6)?,
            model: r.get(7)?,
            title: r.get(8)?,
            origin: r.get(9)?,
            previous_session_id: r.get(10)?,
            start_source: r.get(11)?,
            is_empty: r.get(12)?,
            last_activity_at: r.get(13)?,
        })
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

/// The only session (`AiSession::sole()`).
pub fn sole_session(conn: &Connection) -> SessionView {
    let mut all = sessions(conn, "1");
    assert_eq!(all.len(), 1, "expected one session: {all:?}");
    all.remove(0)
}

pub fn session_by_sid(conn: &Connection, session_id: &str) -> SessionView {
    let mut all = sessions(conn, &format!("session_id = '{session_id}'"));
    assert_eq!(all.len(), 1, "expected one session {session_id}: {all:?}");
    all.remove(0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunView {
    pub id: i64,
    pub ai_session_id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<String>,
    pub ended_cleanly: Option<bool>,
    pub branch: Option<String>,
    pub head_start: Option<String>,
    pub head_end: Option<String>,
    pub status_hash_start: Option<String>,
    pub status_hash_end: Option<String>,
    pub changed_files: Option<bool>,
    pub files_touched: Option<Vec<String>>,
    pub summary_status: String,
    pub summary_attempts: i64,
    pub summary_available_at: Option<i64>,
    pub is_backfill: bool,
    pub discovered: bool,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

/// Runs matching `filter`, ordered by `started_at` then id.
pub fn runs(conn: &Connection, filter: &str) -> Vec<RunView> {
    let sql = format!(
        "SELECT id, ai_session_id, started_at, ended_at, end_reason, ended_cleanly, branch, \
         head_start, head_end, status_hash_start, status_hash_end, changed_files, files_touched, \
         summary_status, summary_attempts, summary_available_at, is_backfill, discovered, \
         created_at, updated_at FROM session_runs WHERE {filter} ORDER BY started_at, id"
    );
    let mut stmt = conn.prepare(&sql).unwrap();
    stmt.query_map([], |r| {
        let files: Option<String> = r.get(12)?;
        Ok(RunView {
            id: r.get(0)?,
            ai_session_id: r.get(1)?,
            started_at: r.get(2)?,
            ended_at: r.get(3)?,
            end_reason: r.get(4)?,
            ended_cleanly: r.get(5)?,
            branch: r.get(6)?,
            head_start: r.get(7)?,
            head_end: r.get(8)?,
            status_hash_start: r.get(9)?,
            status_hash_end: r.get(10)?,
            changed_files: r.get(11)?,
            files_touched: files.map(|f| serde_json::from_str(&f).unwrap()),
            summary_status: r.get(13)?,
            summary_attempts: r.get(14)?,
            summary_available_at: r.get(15)?,
            is_backfill: r.get(16)?,
            discovered: r.get(17)?,
            created_at: r.get(18)?,
            updated_at: r.get(19)?,
        })
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

/// The only run (`SessionRun::sole()`).
pub fn sole_run(conn: &Connection) -> RunView {
    let mut all = runs(conn, "1");
    assert_eq!(all.len(), 1, "expected one run: {all:?}");
    all.remove(0)
}

pub fn count(conn: &Connection, table: &str) -> i64 {
    conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

/// (key, name, remote_url, last_activity_at) of project `id`.
pub fn project(conn: &Connection, id: i64) -> (String, String, Option<String>, Option<i64>) {
    conn.query_row(
        "SELECT key, name, remote_url, last_activity_at FROM projects WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}
