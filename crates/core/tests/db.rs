use std::str::FromStr;
use std::time::Duration;

use aiannotations_core::Error;
use aiannotations_core::db::{self, app_state};
use aiannotations_core::model::{self, EndReason, SessionOrigin, SummaryStatus, Tool};
use rusqlite::{Connection, ErrorCode, params};

fn mem() -> Connection {
    let dir = tempfile::tempdir().unwrap();
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    // keep the directory alive for the connection's lifetime
    std::mem::forget(dir);
    conn
}

fn project(conn: &Connection, key: &str) -> i64 {
    conn.execute(
        "INSERT INTO projects (key, name, path) VALUES (?1, ?1, ?1)",
        [key],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn session(conn: &Connection, project_id: i64, tool: &str, sid: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO ai_sessions (project_id, tool, session_id, cwd, last_activity_at) \
         VALUES (?1, ?2, ?3, '/x', 0)",
        params![project_id, tool, sid],
    )?;
    Ok(conn.last_insert_rowid())
}

fn run(conn: &Connection, session_id: i64, started_at: i64) -> i64 {
    conn.execute(
        "INSERT INTO session_runs (ai_session_id, started_at) VALUES (?1, ?2)",
        params![session_id, started_at],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn is_unique_violation(e: &rusqlite::Error) -> bool {
    e.sqlite_error_code() == Some(ErrorCode::ConstraintViolation)
}

#[test]
fn unique_tool_session_id() {
    let conn = mem();
    let p = project(&conn, "a");
    session(&conn, p, "claude", "abc").unwrap();
    let err = session(&conn, p, "claude", "abc").unwrap_err();
    assert!(is_unique_violation(&err));
}

#[test]
fn same_session_id_other_tool_ok() {
    let conn = mem();
    let p = project(&conn, "a");
    session(&conn, p, "claude", "abc").unwrap();
    session(&conn, p, "codex", "abc").unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn latest_run_is_max_started_at() {
    let conn = mem();
    let p = project(&conn, "a");
    let s = session(&conn, p, "claude", "abc").unwrap();
    run(&conn, s, 1000);
    let latest = run(&conn, s, 3000);
    run(&conn, s, 2000);
    assert_eq!(model::latest_run_id(&conn, s).unwrap(), Some(latest));
    let s2 = session(&conn, p, "claude", "none").unwrap();
    assert_eq!(model::latest_run_id(&conn, s2).unwrap(), None);
}

#[test]
fn visible_excludes_automation_subagent_internal_empty() {
    let conn = mem();
    let p = project(&conn, "a");
    let ok = session(&conn, p, "claude", "ok").unwrap();
    for (sid, origin, empty) in [
        ("auto", "automation", 0),
        ("sub", "subagent", 0),
        ("int", "internal", 0),
        ("empty", "interactive", 1),
    ] {
        let id = session(&conn, p, "claude", sid).unwrap();
        conn.execute(
            "UPDATE ai_sessions SET origin = ?1, is_empty = ?2 WHERE id = ?3",
            params![origin, empty, id],
        )
        .unwrap();
    }
    let sql = format!(
        "SELECT id FROM ai_sessions WHERE {}",
        model::VISIBLE_SESSION_SQL
    );
    let ids: Vec<i64> = conn
        .prepare(&sql)
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(ids, vec![ok]);
}

#[test]
fn briefing_date_unique() {
    let conn = mem();
    let ins = "INSERT INTO briefings (date, status, covers_from, covers_until) \
               VALUES ('2026-10-20', 'done', 0, 1)";
    conn.execute(ins, []).unwrap();
    assert!(is_unique_violation(&conn.execute(ins, []).unwrap_err()));
}

#[test]
fn dismissal_item_key_unique() {
    let conn = mem();
    let ins = "INSERT INTO inbox_dismissals (item_key, state_hash) VALUES ('branch:/r:x', 'abc')";
    conn.execute(ins, []).unwrap();
    assert!(is_unique_violation(&conn.execute(ins, []).unwrap_err()));
}

#[test]
fn cascade_and_set_null() {
    let conn = mem();
    let p = project(&conn, "a");
    let first = session(&conn, p, "claude", "first").unwrap();
    let second = session(&conn, p, "claude", "second").unwrap();
    conn.execute(
        "UPDATE ai_sessions SET previous_session_id = ?1 WHERE id = ?2",
        [first, second],
    )
    .unwrap();
    run(&conn, second, 1);

    conn.execute("DELETE FROM ai_sessions WHERE id = ?1", [first])
        .unwrap();
    let prev: Option<i64> = conn
        .query_row(
            "SELECT previous_session_id FROM ai_sessions WHERE id = ?1",
            [second],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(prev, None);

    conn.execute("DELETE FROM projects WHERE id = ?1", [p])
        .unwrap();
    for table in ["ai_sessions", "session_runs"] {
        let n: i64 = conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[test]
fn pragmas_applied() {
    let conn = mem();
    let jm: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    let bt: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
        .unwrap();
    let sync: i64 = conn
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    assert_eq!(jm, "wal");
    assert_eq!(fk, 1);
    assert_eq!(bt, 5000);
    assert_eq!(sync, 1); // NORMAL
}

#[test]
fn open_creates_parent_dir_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/deeper/app.db");
    drop(db::open(&path).unwrap());
    let conn = db::open(&path).unwrap();
    let v: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, 2);
}

#[test]
fn fts5_available() {
    let conn = mem();
    conn.execute(
        "INSERT INTO search_index (run_id, project, branch, title, summary, next_step, compact) \
         VALUES (1, 'p', 'main', 'Emitir NFSé', 'resumo', 'passo', 'c')",
        [],
    )
    .unwrap();
    let hit: i64 = conn
        .query_row(
            "SELECT count(*) FROM search_index WHERE search_index MATCH '\"nfse\"*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(hit, 1);
}

#[test]
fn write_tx_is_immediate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    let mut first = db::open(&path).unwrap();
    let second = db::open(&path).unwrap();
    second.execute_batch("BEGIN IMMEDIATE").unwrap();

    first.busy_timeout(Duration::from_millis(100)).unwrap();
    let started = std::time::Instant::now();
    let err = db::write_tx(&mut first).unwrap_err();
    assert!(started.elapsed() >= Duration::from_millis(80));
    match err {
        Error::Db(e) => assert_eq!(e.sqlite_error_code(), Some(ErrorCode::DatabaseBusy)),
        other => panic!("unexpected {other:?}"),
    }

    second.execute_batch("COMMIT").unwrap();
    let tx = db::write_tx(&mut first).unwrap();
    tx.commit().unwrap();
}

#[test]
fn app_state_set_get_delete() {
    let mut conn = mem();
    let tx = db::write_tx(&mut conn).unwrap();
    assert_eq!(app_state::get(&tx, "k", 0).unwrap(), None);
    app_state::set(&tx, "k", "v1", None).unwrap();
    app_state::set(&tx, "k", "v2", None).unwrap();
    assert_eq!(app_state::get(&tx, "k", 0).unwrap().as_deref(), Some("v2"));
    app_state::delete(&tx, "k").unwrap();
    assert_eq!(app_state::get(&tx, "k", 0).unwrap(), None);
    tx.commit().unwrap();
}

#[test]
fn app_state_lock_and_expiry() {
    let mut conn = mem();
    let tx = db::write_tx(&mut conn).unwrap();
    assert!(app_state::try_lock(&tx, "lock", 1000, 10_000).unwrap());
    assert!(!app_state::try_lock(&tx, "lock", 1000, 10_500).unwrap());
    assert!(app_state::try_lock(&tx, "lock", 1000, 11_000).unwrap());
    assert!(!app_state::try_lock(&tx, "lock", 1000, 11_500).unwrap());
    app_state::unlock(&tx, "lock").unwrap();
    assert!(app_state::try_lock(&tx, "lock", 1000, 11_600).unwrap());
    tx.commit().unwrap();
}

#[test]
fn app_state_get_honors_expiry_at_given_now() {
    let mut conn = mem();
    let tx = db::write_tx(&mut conn).unwrap();
    app_state::set(&tx, "old", "v", Some(5_000)).unwrap();
    app_state::set(&tx, "live", "v", Some(20_000)).unwrap();
    tx.commit().unwrap();
    // expires_at is exclusive: a key is expired once now_ms reaches it
    assert_eq!(app_state::get(&conn, "old", 10_000).unwrap(), None);
    assert_eq!(app_state::get(&conn, "old", 5_000).unwrap(), None);
    assert_eq!(
        app_state::get(&conn, "old", 4_999).unwrap().as_deref(),
        Some("v")
    );
    assert_eq!(
        app_state::get(&conn, "live", 10_000).unwrap().as_deref(),
        Some("v")
    );
}

#[test]
fn enum_strings_round_trip() {
    for (v, s) in [(Tool::Claude, "claude"), (Tool::Codex, "codex")] {
        assert_eq!(v.as_str(), s);
        assert_eq!(Tool::from_str(s).unwrap(), v);
    }
    for (v, s) in [
        (SessionOrigin::Interactive, "interactive"),
        (SessionOrigin::Automation, "automation"),
        (SessionOrigin::Subagent, "subagent"),
        (SessionOrigin::Internal, "internal"),
    ] {
        assert_eq!(v.as_str(), s);
        assert_eq!(SessionOrigin::from_str(s).unwrap(), v);
    }
    for (v, s) in [
        (SummaryStatus::Pending, "pending"),
        (SummaryStatus::Done, "done"),
        (SummaryStatus::Failed, "failed"),
        (SummaryStatus::Skipped, "skipped"),
    ] {
        assert_eq!(v.as_str(), s);
        assert_eq!(SummaryStatus::from_str(s).unwrap(), v);
    }
    for (v, s) in [
        (EndReason::PromptInputExit, "prompt_input_exit"),
        (EndReason::Clear, "clear"),
        (EndReason::Logout, "logout"),
        (EndReason::Other, "other"),
        (EndReason::Idle, "idle"),
    ] {
        assert_eq!(v.as_str(), s);
        assert_eq!(EndReason::from_str(s).unwrap(), v);
    }
    assert!(Tool::from_str("gemini").is_err());
}
