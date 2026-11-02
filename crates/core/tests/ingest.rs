//! Port of Laravel `tests/Feature/Sessions/IngestTest.php` (cases 11, 29, 30, 31, 32): spool
//! ingestion. Cases 25 and 26 live in `crates/cli/tests/ingest.rs` (they need the CLI binary).

mod support;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aiannotations_core::Error;
use aiannotations_core::model::Tool;
use aiannotations_core::sessions::ingest::{self, IngestReport};
use serde_json::{Value, json};
use support::*;

/// Queues an envelope as the hook would (names in creation order); returns its path.
fn queue(env: &Env, seq: &mut u32, tool: &str, event: &str, payload: Value, at: &str) -> PathBuf {
    *seq += 1;
    let name = format!("{:019}-{tool}-{event}-1.json", *seq);
    let path = env.paths.spool_dir.join(name);
    std::fs::create_dir_all(&env.paths.spool_dir).unwrap();
    let envelope = json!({"tool": tool, "event": event, "received_at": at, "payload": payload});
    std::fs::write(&path, envelope.to_string()).unwrap();
    path
}

fn set_mtime(path: &Path, ms: i64) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_millis(ms as u64);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn json_names(dir: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<String> = rd
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    v.sort();
    v
}

fn ingest_all(env: &mut Env) -> IngestReport {
    ingest::run(&mut env.conn, &env.paths, &env.settings, &env.git, now_ms()).unwrap()
}

fn busy() -> Error {
    Error::Db(rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
        None,
    ))
}

/// JSON inválido vai para failed e o próximo arquivo é processado
#[test]
fn invalid_json_goes_to_failed_and_next_file_is_processed() {
    let mut env = Env::new();
    let spool = env.paths.spool_dir.clone();
    std::fs::create_dir_all(&spool).unwrap();
    std::fs::write(
        spool.join("0000000000000000001-claude-end-1.json"),
        r#"{"tool":"claude","event":"end","payl"#,
    )
    .unwrap();
    std::fs::write(spool.join("0000000000000000002-claude-end-1.json"), "").unwrap();
    std::fs::write(
        spool.join("0000000000000000003-claude-end-1.json"),
        json!({"tool":"claude","event":"end","received_at":"2026-10-20T10:00:00Z","payload":{"cwd":"/tmp"}}).to_string(),
    )
    .unwrap();
    let cwd = env.base.to_string_lossy().into_owned();
    let mut seq = 3;
    queue(
        &env,
        &mut seq,
        "claude",
        "start",
        json!({"session_id":"ok","transcript_path":null,"cwd":cwd,"source":"startup"}),
        "2026-10-20T10:00:00Z",
    );

    let report = ingest_all(&mut env);

    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (1, 3, 0)
    );
    assert_eq!(report.warnings.len(), 3, "{:?}", report.warnings);
    assert_eq!(json_names(&spool.join("failed")).len(), 3);
    assert_eq!(json_names(&spool.join("done")).len(), 1);
    assert!(json_names(&spool).is_empty());
    assert_eq!(sole_session(&env.conn).session_id, "ok");
    assert_eq!(ingest_all(&mut env), IngestReport::default());
}

/// banco ocupado deixa o evento na fila; depois de 1 h vai para failed
#[test]
fn busy_database_keeps_event_queued_and_after_one_hour_fails_it() {
    let env = Env::new();
    let (mut seq, p) = (
        0,
        json!({"transcript_path":null,"cwd":"/tmp","source":"startup"}),
    );
    let mut with = |sid: &str| {
        let mut payload = p.clone();
        payload["session_id"] = json!(sid);
        queue(
            &env,
            &mut seq,
            "claude",
            "start",
            payload,
            "2026-10-20T10:00:00Z",
        )
    };
    let recent = with("novo");
    let old = with("velho");
    set_mtime(&recent, now_ms() - 60_000);
    set_mtime(&old, now_ms() - 2 * 3600 * 1000);

    let report =
        ingest::run_with(&env.paths.spool_dir, 3600, now_ms(), |_, _| Err(busy())).unwrap();

    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (0, 1, 1)
    );
    let spool = &env.paths.spool_dir;
    assert_eq!(
        json_names(spool),
        [recent.file_name().unwrap().to_str().unwrap()]
    );
    assert_eq!(
        json_names(&spool.join("failed")),
        [old.file_name().unwrap().to_str().unwrap()]
    );
    assert!(json_names(&spool.join("done")).is_empty());
}

/// banco ocupado retém também os eventos seguintes da mesma sessão, na ordem
#[test]
fn busy_database_holds_following_events_of_same_session_in_order() {
    let env = Env::new();
    let mut seq = 0;
    let base = json!({"transcript_path":null,"cwd":"/tmp"});
    let mk = |sid: &str, extra: Value| {
        let mut p = base.clone();
        p["session_id"] = json!(sid);
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        p
    };
    let a_start = queue(
        &env,
        &mut seq,
        "claude",
        "start",
        mk("a", json!({"source":"startup"})),
        "2026-10-20T10:00:00Z",
    );
    let b_start = queue(
        &env,
        &mut seq,
        "claude",
        "start",
        mk("b", json!({"source":"startup"})),
        "2026-10-20T10:00:01Z",
    );
    let a_end = queue(
        &env,
        &mut seq,
        "claude",
        "end",
        mk("a", json!({"reason":"prompt_input_exit"})),
        "2026-10-20T10:00:02Z",
    );
    let codex_end = queue(
        &env,
        &mut seq,
        "codex",
        "end",
        mk("a", json!({"reason":"other"})),
        "2026-10-20T10:00:03Z",
    );
    // The held end is old, but it is not even tried: it must not fail by age.
    set_mtime(&a_end, now_ms() - 2 * 3600 * 1000);

    let seen = RefCell::new(Vec::new());
    let report = ingest::run_with(&env.paths.spool_dir, 3600, now_ms(), |ev, _| {
        seen.borrow_mut().push((ev.tool, ev.session_id.clone()));
        if ev.tool == Tool::Claude && ev.session_id == "a" {
            Err(busy())
        } else {
            Ok(())
        }
    })
    .unwrap();

    let name = |p: &PathBuf| p.file_name().unwrap().to_str().unwrap().to_string();
    let spool = &env.paths.spool_dir;
    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (2, 0, 2)
    );
    assert_eq!(json_names(spool), [name(&a_start), name(&a_end)]);
    assert_eq!(
        json_names(&spool.join("done")),
        [name(&b_start), name(&codex_end)]
    );
    assert!(json_names(&spool.join("failed")).is_empty());
    assert_eq!(
        *seen.borrow(),
        [
            (Tool::Claude, "a".to_string()),
            (Tool::Claude, "b".to_string()),
            (Tool::Codex, "a".to_string())
        ]
    );
}

/// erro que não é de banco ocupado continua indo para failed
#[test]
fn non_busy_error_still_goes_to_failed() {
    let mut env = Env::new();
    let mut seq = 0;
    let cwd = env.base.to_string_lossy().into_owned();
    queue(
        &env,
        &mut seq,
        "claude",
        "start",
        json!({"session_id":"x","transcript_path":null,"cwd":cwd,"source":"startup"}),
        "2026-10-20T10:00:00Z",
    );
    // A real database error (not busy): the schema is broken.
    env.conn.execute_batch("DROP TABLE session_runs").unwrap();

    let report = ingest_all(&mut env);

    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (0, 1, 0)
    );
    assert!(json_names(&env.paths.spool_dir).is_empty());
    assert_eq!(json_names(&env.paths.spool_dir.join("failed")).len(), 1);
    assert_eq!(report.warnings.len(), 1);
}

/// transações do SQLite começam IMMEDIATE (o busy_timeout vale para a escrita)
///
/// Not a proof of IMMEDIATE (see `write_tx_is_immediate` in tests/db.rs): end to end, another
/// connection holding the write lock defers the event, which stays in the queue.
#[test]
fn write_lock_held_elsewhere_defers_the_event() {
    let mut env = Env::new();
    let mut seq = 0;
    let cwd = env.base.to_string_lossy().into_owned();
    let file = queue(
        &env,
        &mut seq,
        "claude",
        "start",
        json!({"session_id":"x","transcript_path":null,"cwd":cwd,"source":"startup"}),
        "2026-10-20T10:00:00Z",
    );
    env.conn.busy_timeout(Duration::ZERO).unwrap();
    let other = aiannotations_core::db::open(&env.paths.db_path).unwrap();
    other.execute_batch("BEGIN IMMEDIATE").unwrap();

    let report = ingest_all(&mut env);
    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (0, 0, 1)
    );
    assert!(file.exists());

    other.execute_batch("ROLLBACK").unwrap();
    let report = ingest_all(&mut env);
    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (1, 0, 0)
    );
    assert_eq!(count(&env.conn, "ai_sessions"), 1);
}

/// A recorded event whose move to done/ fails is not re-recorded by the same pass, does not abort
/// it, and is reported as a warning.
#[test]
fn unwritable_done_dir_warns_and_pass_continues() {
    let mut env = Env::new();
    let mut seq = 0;
    let cwd = env.base.to_string_lossy().into_owned();
    for sid in ["a", "b"] {
        queue(
            &env,
            &mut seq,
            "claude",
            "start",
            json!({"session_id":sid,"transcript_path":null,"cwd":cwd,"source":"startup"}),
            "2026-10-20T10:00:00Z",
        );
    }
    // done/ is a regular file: create_dir_all/rename into it fails.
    std::fs::write(env.paths.spool_dir.join("done"), "x").unwrap();

    let report = ingest_all(&mut env);

    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (2, 0, 0)
    );
    assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
    assert!(report.warnings.iter().all(|w| w.contains("done/")));
    assert_eq!(count(&env.conn, "ai_sessions"), 2);
    assert_eq!(count(&env.conn, "session_runs"), 2);
}

/// A file taken by another ingestor between listing and processing does not abort the pass.
#[test]
fn file_vanishing_between_listing_and_reading_is_skipped() {
    let env = Env::new();
    let mut seq = 0;
    let p = json!({"session_id":"x","transcript_path":null,"cwd":"/tmp","source":"startup"});
    let first = queue(
        &env,
        &mut seq,
        "claude",
        "start",
        p.clone(),
        "2026-10-20T10:00:00Z",
    );
    let second = queue(&env, &mut seq, "codex", "start", p, "2026-10-20T10:00:01Z");
    // The closure runs for `first`; it removes `second` (not yet read) as a racing ingestor would.
    let hidden = env.base.join("taken.json");
    let seen = RefCell::new(Vec::new());
    let report = ingest::run_with(&env.paths.spool_dir, 3600, now_ms(), |ev, _| {
        seen.borrow_mut().push(ev.tool);
        std::fs::rename(&second, &hidden).unwrap();
        Ok(())
    })
    .unwrap();

    assert_eq!(*seen.borrow(), [Tool::Claude]);
    assert_eq!(
        (report.processed, report.failed, report.deferred),
        (1, 0, 0)
    );
    assert!(report.warnings.is_empty());
    assert_eq!(json_names(&env.paths.spool_dir.join("done")).len(), 1);
    assert!(!first.exists());
}
