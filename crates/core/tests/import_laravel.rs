//! `aiannotations import-laravel` (spec §10.6): the Laravel v1 database is read-only, every
//! entity comes across with remapped ids, naive `America/Sao_Paulo` datetimes become ms UTC,
//! the summary state survives a collector pass and a second import changes nothing.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aiannotations_core::import_laravel::{self, ImportOptions, ImportReport, LARAVEL_TZ};
use aiannotations_core::model::Tool;
use aiannotations_core::sessions::collector::{self, CollectMode};
use aiannotations_core::transcripts::factory;
use rusqlite::{Connection, OptionalExtension, params};
use support::*;

const SCHEMA: &str = include_str!("fixtures/laravel_schema.sql");
const FIXTURE_CWD: &str = "/home/fulano/Code/Projeto Ação/Exemplo";

/// A Laravel database file built from `fixtures/laravel_schema.sql`.
struct Laravel {
    _dir: tempfile::TempDir,
    path: PathBuf,
    conn: Connection,
}

fn laravel() -> Laravel {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Laravel Ação").join("database.sqlite");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    Laravel {
        _dir: dir,
        path,
        conn,
    }
}

impl Laravel {
    fn sql(&self, sql: &str) {
        self.conn.execute_batch(sql).unwrap();
    }
}

/// 2 projects (ids with gaps), 3 sessions (one in a `/clear` chain, one Codex subagent),
/// 4 runs, 2 dismissals and 1 briefing; datetimes as Laravel writes them (naive São Paulo).
const ENTITIES: &str = r#"
INSERT INTO users (id, name, email, password) VALUES (1, 'x', 'x@x', 'x');
INSERT INTO cache (key, value, expiration) VALUES ('aiannotations:paused', 'b:1;', 0);
INSERT INTO projects (id, key, name, path, remote_url, last_activity_at, created_at, updated_at) VALUES
 (7, '/home/u/Code/Alpha', 'Alpha', '/home/u/Code/Alpha', 'git@github.com:u/alpha.git',
  '2026-10-29 15:14:50', '2026-10-20 10:00:00', '2026-10-29 15:14:51'),
 (12, '/home/u/Code/Beta', 'Beta', '/home/u/Code/Beta', NULL, NULL, NULL, NULL);
INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, worktree_path, transcript_path,
  model, title, origin, previous_session_id, parent_session_id, is_empty, last_activity_at,
  created_at, updated_at, start_source, transcript_mtime) VALUES
 (21, 7, 'claude', 'sid-b', '/home/u/Code/Alpha/sub', '/home/u/Code/Alpha-wt', '/t/sid-b.jsonl',
  'claude-opus', 'Emissão de NFS-e', 'interactive', 20, NULL, 0, '2026-10-29 15:14:50',
  '2026-10-29 14:00:00', '2026-10-29 15:14:50', 'clear', 1792505690000),
 (20, 7, 'claude', 'sid-a', '/home/u/Code/Alpha', NULL, '/t/sid-a.jsonl', NULL, 'Primeira',
  'interactive', NULL, NULL, 0, '2026-10-29 13:00:00', NULL, NULL, 'startup', 1792500000000),
 (30, 12, 'codex', 'thr-c', '/home/u/Code/Beta', NULL, NULL, 'gpt-5', NULL, 'subagent', NULL,
  'thr-parent', 1, '2026-10-28 09:00:00', NULL, NULL, NULL, 1792501500123);
INSERT INTO session_runs (id, ai_session_id, started_at, ended_at, end_reason, ended_cleanly,
  branch, head_start, head_end, status_hash_start, status_hash_end, changed_files, files_touched,
  summary_status, summary, next_step, unfinished, kind, summary_attempts, summary_available_at,
  is_backfill, created_at, updated_at, summarized_fingerprint, discovered) VALUES
 (100, 20, '2026-10-29 12:00:00', '2026-10-29 13:00:00', 'clear', 1, 'main', 'h1', 'h2', 's1',
  's2', 1, '["\/home\/u\/Code\/Alpha\/a.php"]', 'done', 'Fez a parte A', 'Fazer a B', 0,
  'codigo', 1, NULL, 0, '2026-10-29 12:00:00', '2026-10-29 13:00:05', '1792501500:2:2', 0),
 (101, 21, '2026-10-29 14:00:00', '2026-10-29 15:14:50', 'idle', 0, 'feat/nfs-e', NULL, NULL,
  NULL, NULL, NULL, '[]', 'failed', NULL, NULL, NULL, NULL, 3, '2026-10-29 15:30:00', 0, NULL,
  NULL, NULL, 1),
 (102, 21, '2026-10-29 15:20:00', NULL, NULL, NULL, 'feat/nfs-e', NULL, NULL, NULL, NULL, NULL,
  NULL, 'pending', NULL, NULL, NULL, NULL, 0, NULL, 0, NULL, NULL, NULL, 0),
 (103, 30, '2026-10-28 08:00:00', '2026-10-28 09:00:00', 'other', 1, NULL, NULL, NULL, NULL,
  NULL, 0, NULL, 'skipped', NULL, NULL, NULL, 'conversa', 0, NULL, 1, NULL, NULL, NULL, 0);
INSERT INTO inbox_dismissals (id, item_key, state_hash, created_at, updated_at) VALUES
 (1, 'interrupted:7:feat/nfs-e', '101:1792505690', '2026-10-29 16:00:00', '2026-10-29 16:00:00'),
 (2, 'dirty:12', 'da39a3ee5e6b4b0d3255bfef95601890afd80709', NULL, NULL);
INSERT INTO briefings (id, date, status, body, next_steps, covers_from, covers_until,
  generated_at, created_at, updated_at) VALUES
 (3, '2026-10-29', 'done', 'Ontem: Alpha.',
  '[{"project_id":7,"projeto":"Alpha","passo":"Fazer a B"},{"project_id":12,"projeto":"Beta","passo":"Rever"}]',
  '2026-10-28 06:00:08', '2026-10-29 06:00:08', '2026-10-29 06:00:18', '2026-10-29 06:00:08',
  '2026-10-29 06:00:18');
"#;

/// Import with `merge`: most tests start from a database that already holds sessions.
fn import(l: &Laravel, env: &mut Env) -> ImportReport {
    import_laravel::run(&l.path, &mut env.conn, &MERGE).unwrap()
}

const MERGE: ImportOptions = ImportOptions {
    tz: LARAVEL_TZ,
    merge: true,
};
const FRESH: ImportOptions = ImportOptions {
    tz: LARAVEL_TZ,
    merge: false,
};

fn id_of(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn project_id(conn: &Connection, key: &str) -> i64 {
    conn.query_row("SELECT id FROM projects WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .unwrap()
}

fn session_id(conn: &Connection, sid: &str) -> i64 {
    session_by_sid(conn, sid).id
}

fn run_id(conn: &Connection, session: i64, started_at: i64) -> i64 {
    conn.query_row(
        "SELECT id FROM session_runs WHERE ai_session_id = ?1 AND started_at = ?2",
        params![session, started_at],
        |r| r.get(0),
    )
    .unwrap()
}

fn dismissals(conn: &Connection) -> Vec<(String, String)> {
    conn.prepare("SELECT item_key, state_hash FROM inbox_dismissals ORDER BY item_key")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

/// New database with an unrelated project and session already in it, so the new ids differ
/// from the Laravel ones.
fn env_with_offset() -> Env {
    let env = Env::new();
    insert_session(
        &env.conn,
        Tool::Claude,
        "pre-existing",
        Path::new("/home/u/Code/Other"),
        None,
    );
    env
}

#[test]
fn imports_all_entities_with_remapped_ids() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = env_with_offset();

    let report = import(&l, &mut env);

    assert_eq!(
        report,
        ImportReport {
            projects: 2,
            sessions: 3,
            runs: 4,
            dismissals: 2,
            briefings: 1,
            skipped: vec![],
        }
    );
    assert_eq!(count(&env.conn, "projects"), 3);
    assert_eq!(count(&env.conn, "ai_sessions"), 4);
    assert_eq!(count(&env.conn, "session_runs"), 4);
    assert_eq!(count(&env.conn, "inbox_dismissals"), 2);
    assert_eq!(count(&env.conn, "briefings"), 1);
    // Laravel-only tables stay out.
    let users: Option<String> = env
        .conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE name IN ('users', 'cache', 'jobs', 'sessions')",
            [],
            |r| r.get(0),
        )
        .optional()
        .unwrap();
    assert_eq!(users, None);

    let alpha = project_id(&env.conn, "/home/u/Code/Alpha");
    let beta = project_id(&env.conn, "/home/u/Code/Beta");
    assert_ne!(alpha, 7);
    assert_ne!(beta, 12);
    assert_eq!(
        project(&env.conn, alpha),
        (
            "/home/u/Code/Alpha".to_string(),
            "Alpha".to_string(),
            Some("git@github.com:u/alpha.git".to_string()),
            Some(ts("2026-10-29T18:14:50Z")),
        )
    );
    let (created, updated): (Option<i64>, Option<i64>) = env
        .conn
        .query_row(
            "SELECT created_at, updated_at FROM projects WHERE id = ?1",
            [alpha],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(created, Some(ts("2026-10-20T13:00:00Z")));
    assert_eq!(updated, Some(ts("2026-10-29T18:14:51Z")));
    assert_eq!(project(&env.conn, beta).3, None);

    // Sessions: every column, project and `/clear` chain remapped.
    let a = session_by_sid(&env.conn, "sid-a");
    let b = session_by_sid(&env.conn, "sid-b");
    let c = session_by_sid(&env.conn, "thr-c");
    assert_eq!(a.project_id, alpha);
    assert_eq!(c.project_id, beta);
    assert_eq!(
        b,
        SessionView {
            id: b.id,
            project_id: alpha,
            tool: "claude".into(),
            session_id: "sid-b".into(),
            cwd: "/home/u/Code/Alpha/sub".into(),
            worktree_path: Some("/home/u/Code/Alpha-wt".into()),
            transcript_path: Some("/t/sid-b.jsonl".into()),
            model: Some("claude-opus".into()),
            title: Some("Emissão de NFS-e".into()),
            origin: "interactive".into(),
            previous_session_id: Some(a.id),
            start_source: Some("clear".into()),
            is_empty: false,
            last_activity_at: ts("2026-10-29T18:14:50Z"),
        }
    );
    assert_eq!(c.origin, "subagent");
    assert!(c.is_empty);
    assert_eq!(a.previous_session_id, None);
    let (parent, mtime): (Option<String>, Option<i64>) = env
        .conn
        .query_row(
            "SELECT parent_session_id, transcript_mtime FROM ai_sessions WHERE id = ?1",
            [c.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(parent.as_deref(), Some("thr-parent"));
    // Already ms: copied as is.
    assert_eq!(mtime, Some(1792501500123));

    // Runs: every column of the `done` one.
    let r100 = runs(&env.conn, &format!("ai_session_id = {}", a.id));
    assert_eq!(
        r100,
        vec![RunView {
            id: r100[0].id,
            ai_session_id: a.id,
            started_at: ts("2026-10-29T15:00:00Z"),
            ended_at: Some(ts("2026-10-29T16:00:00Z")),
            end_reason: Some("clear".into()),
            ended_cleanly: Some(true),
            branch: Some("main".into()),
            head_start: Some("h1".into()),
            head_end: Some("h2".into()),
            status_hash_start: Some("s1".into()),
            status_hash_end: Some("s2".into()),
            changed_files: Some(true),
            files_touched: Some(vec!["/home/u/Code/Alpha/a.php".into()]),
            summary_status: "done".into(),
            summary_attempts: 1,
            summary_available_at: None,
            is_backfill: false,
            discovered: false,
            created_at: Some(ts("2026-10-29T15:00:00Z")),
            updated_at: Some(ts("2026-10-29T16:00:05Z")),
        }]
    );
    let extra: (String, String, bool, String, String) = env
        .conn
        .query_row(
            "SELECT summary, next_step, unfinished, kind, summarized_fingerprint \
             FROM session_runs WHERE id = ?1",
            [r100[0].id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        extra,
        (
            "Fez a parte A".into(),
            "Fazer a B".into(),
            false,
            "codigo".into(),
            "1792501500:2:2".into()
        )
    );
    let rb = runs(&env.conn, &format!("ai_session_id = {}", b.id));
    assert_eq!(rb.len(), 2);
    assert_eq!(rb[0].summary_status, "failed");
    assert_eq!(rb[0].summary_attempts, 3);
    assert_eq!(rb[0].summary_available_at, Some(ts("2026-10-29T18:30:00Z")));
    assert_eq!(rb[0].changed_files, None);
    assert!(rb[0].discovered);
    assert_eq!(rb[1].ended_at, None);
    let rc = runs(&env.conn, &format!("ai_session_id = {}", c.id));
    assert_eq!(rc.len(), 1);
    assert!(rc[0].is_backfill);
    assert_eq!(rc[0].summary_status, "skipped");
    assert_eq!(rc[0].changed_files, Some(false));

    // Briefing: dates converted, `date` kept, next-step project ids remapped.
    let briefing: (String, String, String, String, i64, i64, Option<i64>) = env
        .conn
        .query_row(
            "SELECT date, status, body, next_steps, covers_from, covers_until, generated_at \
             FROM briefings",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(briefing.0, "2026-10-29");
    assert_eq!(briefing.1, "done");
    assert_eq!(briefing.2, "Ontem: Alpha.");
    let steps: serde_json::Value = serde_json::from_str(&briefing.3).unwrap();
    assert_eq!(
        steps,
        serde_json::json!([
            {"project_id": alpha, "projeto": "Alpha", "passo": "Fazer a B"},
            {"project_id": beta, "projeto": "Beta", "passo": "Rever"},
        ])
    );
    assert_eq!(briefing.4, ts("2026-10-28T09:00:08Z"));
    assert_eq!(briefing.5, ts("2026-10-29T09:00:08Z"));
    assert_eq!(briefing.6, Some(ts("2026-10-29T09:00:18Z")));
}

/// Review Focus 4: Laravel keeps naive `America/Sao_Paulo` text.
#[test]
fn converts_sao_paulo_naive_datetimes_to_utc() {
    let l = laravel();
    l.sql(
        "INSERT INTO projects (id, key, name, path) VALUES (1, '/p', 'p', '/p');
         INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, last_activity_at)
           VALUES (1, 1, 'claude', 's', '/p', '2026-10-29 15:14:50');
         INSERT INTO session_runs (id, ai_session_id, started_at, ended_at) VALUES
           (1, 1, '2026-10-29 15:14:50', NULL),
           -- End of the last São Paulo DST (2019-02-17 00:00 -02 -> 2019-02-16 23:00 -03):
           -- 23:30 happened twice; the earliest instant wins.
           (2, 1, '2019-02-16 23:30:00', '2019-02-16 22:59:59'),
           -- Start of the 2018 DST (2018-11-04 00:00 -03 -> 01:00 -02): 00:30 never
           -- happened; read with the offset in force before the gap, as PHP does.
           (3, 1, '2018-11-04 00:30:00', '2018-11-04 01:00:00');",
    );
    let mut env = Env::new();
    import(&l, &mut env);

    let got: Vec<(i64, Option<i64>)> = runs(&env.conn, "1")
        .into_iter()
        .map(|r| (r.started_at, r.ended_at))
        .collect();
    assert_eq!(
        got,
        vec![
            (ts("2018-11-04T03:30:00Z"), Some(ts("2018-11-04T03:00:00Z"))),
            (ts("2019-02-17T01:30:00Z"), Some(ts("2019-02-17T00:59:59Z"))),
            (ts("2026-10-29T18:14:50Z"), None),
        ]
    );
    assert_eq!(
        sole_session(&env.conn).last_activity_at,
        ts("2026-10-29T18:14:50Z")
    );
}

/// A datetime that is not Laravel's format aborts the import and writes nothing.
#[test]
fn invalid_datetime_aborts_without_writing() {
    let l = laravel();
    l.sql(
        "INSERT INTO projects (id, key, name, path) VALUES (1, '/p', 'p', '/p');
         INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, last_activity_at)
           VALUES (1, 1, 'claude', 's', '/p', 'ontem à tarde');",
    );
    let mut env = Env::new();

    let err = import_laravel::run(&l.path, &mut env.conn, &FRESH).unwrap_err();

    let msg = err.to_string();
    assert!(
        msg.contains("ai_sessions") && msg.contains("last_activity_at"),
        "{msg}"
    );
    assert!(msg.contains("ontem à tarde"), "{msg}");
    assert_eq!(count(&env.conn, "projects"), 0);
}

#[test]
fn missing_database_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = Env::new();
    let path = dir.path().join("database.sqlite");

    let err = import_laravel::run(&path, &mut env.conn, &FRESH).unwrap_err();

    assert!(err.to_string().contains("not found"), "{err}");
    assert!(
        !path.exists(),
        "the importer must not create the Laravel database"
    );
}

/// `item_key` is `<kind>:<project_id>[:rest]` (`app/Inbox/Rules/*.php`); the `interrupted:`
/// `state_hash` is `<run_id>:<ended_at seconds>`. Contract for Plano 2's inbox rules.
#[test]
fn remaps_dismissal_keys_and_interrupted_state_hash() {
    let l = laravel();
    l.sql(ENTITIES);
    l.sql(
        "INSERT INTO inbox_dismissals (item_key, state_hash) VALUES
          ('stale-branch:7:feat/old', '1792000000'),
          ('ongoing:12:docs/ongoing-sessions/x.md', '1792013756'),
          ('reminder:7:n17920204943329', 'bab56c4b8efdd78dff7133316aa631d333dc1d76'),
          ('worktree:12:/home/u/Code/Beta-wt:odd', 'abc123'),
          ('dirty:99', 'h'),
          ('interrupted:7:main', '555:1792505690'),
          ('mystery:7', 'h');",
    );
    let mut env = env_with_offset();

    let report = import(&l, &mut env);

    let alpha = project_id(&env.conn, "/home/u/Code/Alpha");
    let beta = project_id(&env.conn, "/home/u/Code/Beta");
    let b = session_id(&env.conn, "sid-b");
    let r101 = run_id(&env.conn, b, ts("2026-10-29T17:00:00Z"));
    assert_ne!(r101, 101);
    let mut expected = vec![
        (
            format!("dirty:{beta}"),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709".to_string(),
        ),
        (
            format!("interrupted:{alpha}:feat/nfs-e"),
            format!("{r101}:1792505690"),
        ),
        (
            format!("ongoing:{beta}:docs/ongoing-sessions/x.md"),
            "1792013756".to_string(),
        ),
        (
            format!("reminder:{alpha}:n17920204943329"),
            "bab56c4b8efdd78dff7133316aa631d333dc1d76".to_string(),
        ),
        (
            format!("stale-branch:{alpha}:feat/old"),
            "1792000000".to_string(),
        ),
        (
            format!("worktree:{beta}:/home/u/Code/Beta-wt:odd"),
            "abc123".to_string(),
        ),
    ];
    expected.sort();
    assert_eq!(dismissals(&env.conn), expected);
    assert_eq!(report.dismissals, 6);
    assert_eq!(report.skipped.len(), 3, "{:?}", report.skipped);
    for key in ["dirty:99", "interrupted:7:main", "mystery:7"] {
        assert!(
            report.skipped.iter().any(|s| s.contains(key)),
            "{key} not reported: {:?}",
            report.skipped
        );
    }
    let created: Option<i64> = env
        .conn
        .query_row(
            "SELECT created_at FROM inbox_dismissals WHERE item_key LIKE 'interrupted:%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(created, Some(ts("2026-10-29T19:00:00Z")));
}

#[test]
fn is_idempotent() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = env_with_offset();
    let first = import(&l, &mut env);
    assert_eq!(first.runs, 4);
    let tables = [
        "projects",
        "ai_sessions",
        "session_runs",
        "inbox_dismissals",
        "briefings",
        "search_index",
    ];
    let before: Vec<i64> = tables.iter().map(|t| count(&env.conn, t)).collect();
    // A record changed in the new database since the first import is kept as it is.
    env.conn
        .execute(
            "UPDATE session_runs SET summary = 'Editado no app novo' WHERE summary = 'Fez a parte A'",
            [],
        )
        .unwrap();

    let second = import(&l, &mut env);

    assert_eq!(second, ImportReport::default());
    let after: Vec<i64> = tables.iter().map(|t| count(&env.conn, t)).collect();
    assert_eq!(before, after);
    let edited = id_of(
        &env.conn,
        "SELECT COUNT(*) FROM session_runs WHERE summary = 'Editado no app novo'",
    );
    assert_eq!(edited, 1);
}

fn set_mtime(path: &Path, ms: i64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_millis(ms as u64))
        .unwrap();
}

/// The summarizer must not see again what Laravel already summarized: fingerprint, status,
/// attempts and `transcript_mtime` survive. The run is still open in Laravel, so the collector
/// closes it as Idle and recomputes its summary status from the transcript (`refresh`): with
/// the same fingerprint it stays `done`.
#[test]
fn preserves_summary_state_and_collector_does_not_resummarize() {
    let mut env = Env::new();
    let transcript = env
        .paths
        .claude_dir
        .join("projects/-home-fulano-Exemplo/abc.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, CLAUDE_FIXTURE).unwrap();
    // Laravel stored whole seconds × 1000; the file has sub-second precision, so the collector
    // sees a "changed" mtime and rereads the transcript.
    set_mtime(&transcript, ts("2026-10-20T12:05:30.456Z"));
    let fingerprint = factory::read(Tool::Claude, "abc", Some(&transcript), &env.paths)
        .unwrap()
        .unwrap()
        .fingerprint();

    let l = laravel();
    l.conn
        .execute_batch(&format!(
            "INSERT INTO projects (id, key, name, path) VALUES (5, '{FIXTURE_CWD}', 'Exemplo',
               '{FIXTURE_CWD}');
             INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, transcript_path,
               title, start_source, last_activity_at, transcript_mtime) VALUES
               (9, 5, 'claude', 'abc', '{FIXTURE_CWD}', '{}', 'Página de contato com validação',
                NULL, '2026-10-20 09:05:30', {});",
            transcript.display(),
            ts("2026-10-20T12:05:30Z"),
        ))
        .unwrap();
    l.conn
        .execute(
            "INSERT INTO session_runs (ai_session_id, started_at, ended_at, end_reason,
               ended_cleanly, changed_files, files_touched, summary_status, summary, next_step,
               kind, summary_attempts, summarized_fingerprint, discovered)
             VALUES (9, '2026-10-20 09:00:00', NULL, NULL, NULL, 1, ?1, 'done',
               'Página de contato.', 'Validar.', 'codigo', 1, ?2, 1)",
            params![format!("[\"{}\"]", CONTATO), fingerprint],
        )
        .unwrap();

    import(&l, &mut env);
    let mtime: Option<i64> = env
        .conn
        .query_row("SELECT transcript_mtime FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mtime, Some(ts("2026-10-20T12:05:30Z")));
    for now in ["2026-10-20T13:00:00Z", "2026-10-20T14:00:00Z"] {
        let report = collector::run(
            &mut env.conn,
            &env.paths,
            &env.settings,
            &env.git,
            CollectMode::Window,
            ts(now),
            &mut |_, _| {},
        )
        .unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.seen, 1);

        let run = sole_run(&env.conn);
        assert_eq!(run.summary_status, "done", "at {now}");
        assert_eq!(run.summary_attempts, 1);
        assert_eq!(run.end_reason.as_deref(), Some("idle"));
        assert_eq!(run.ended_at, Some(ts("2026-10-20T12:05:30Z")));
        let (summary, fp): (String, String) = env
            .conn
            .query_row(
                "SELECT summary, summarized_fingerprint FROM session_runs",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(summary, "Página de contato.");
        assert_eq!(fp, fingerprint);
    }
    assert_eq!(count(&env.conn, "ai_sessions"), 1);
}

/// Review Focus 5: Laravel running during the import. Everything is still in the `-wal` (no
/// checkpoint, writer connection open); the import sees it and never writes the main file.
#[test]
fn reads_wal_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("database.sqlite");
    let writer = Connection::open(&path).unwrap();
    writer
        .execute_batch("PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;")
        .unwrap();
    let meta = |p: &Path| {
        let m = std::fs::metadata(p).unwrap();
        (m.len(), m.modified().unwrap())
    };
    let main_before = meta(&path);
    std::thread::sleep(Duration::from_millis(20));
    writer.execute_batch(SCHEMA).unwrap();
    writer.execute_batch(ENTITIES).unwrap();
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let wal_before = meta(&wal);
    assert!(wal_before.0 > 0, "data must be in the WAL");
    assert_eq!(meta(&path), main_before, "data must be only in the WAL");

    let mut env = Env::new();
    let report = import_laravel::run(&path, &mut env.conn, &FRESH).unwrap();

    assert_eq!((report.projects, report.sessions, report.runs), (2, 3, 4));
    assert_eq!(meta(&path), main_before);
    assert_eq!(meta(&wal), wal_before);
    // The Laravel app keeps writing meanwhile.
    writer
        .execute("UPDATE projects SET name = 'Alpha2' WHERE id = 7", [])
        .unwrap();
    drop(writer);
}

#[test]
fn search_index_rebuilt_for_imported_runs() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = env_with_offset();
    import(&l, &mut env);

    assert_eq!(count(&env.conn, "search_index"), 4);
    let b = session_id(&env.conn, "sid-b");
    let r101 = run_id(&env.conn, b, ts("2026-10-29T17:00:00Z"));
    let row: (String, String, String, String, String, String) = env
        .conn
        .query_row(
            "SELECT project, branch, title, summary, next_step, compact FROM search_index \
             WHERE run_id = ?1",
            [r101],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "Alpha".into(),
            "feat/nfs-e".into(),
            "Emissão de NFS-e".into(),
            "".into(),
            "".into(),
            "Alpha feat/nfse Emissão de NFSe  ".into(),
        )
    );
    let hits: Vec<i64> = env
        .conn
        .prepare(
            "SELECT run_id FROM search_index WHERE search_index MATCH 'compact : \"nfse\"*' \
             ORDER BY run_id",
        )
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let r102 = run_id(&env.conn, b, ts("2026-10-29T18:20:00Z"));
    assert_eq!(hits, vec![r101, r102]);
    let a = session_id(&env.conn, "sid-a");
    let r100 = run_id(&env.conn, a, ts("2026-10-29T15:00:00Z"));
    let by_summary = id_of(
        &env.conn,
        "SELECT run_id FROM search_index WHERE search_index MATCH '\"parte\"*'",
    );
    assert_eq!(by_summary, r100);
}

/// The target already has sessions (a collect or hook ran first): without `merge` the import is
/// refused and nothing is written.
#[test]
fn refuses_a_non_empty_target_without_merge() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = env_with_offset();
    let before = (
        count(&env.conn, "projects"),
        count(&env.conn, "ai_sessions"),
    );

    let err = import_laravel::run(&l.path, &mut env.conn, &FRESH).unwrap_err();

    let msg = err.to_string();
    assert!(msg.contains("before the first collect/hook run"), "{msg}");
    assert!(msg.contains("--merge"), "{msg}");
    assert_eq!(
        (
            count(&env.conn, "projects"),
            count(&env.conn, "ai_sessions")
        ),
        before
    );
    assert_eq!(count(&env.conn, "session_runs"), 0);
    assert_eq!(count(&env.conn, "briefings"), 0);
}

/// With `merge` the same non-empty target is imported into.
#[test]
fn merges_into_a_non_empty_target_with_merge() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = env_with_offset();

    let report = import_laravel::run(&l.path, &mut env.conn, &MERGE).unwrap();

    assert_eq!((report.projects, report.sessions, report.runs), (2, 3, 4));
    assert_eq!(count(&env.conn, "ai_sessions"), 4);
}

/// An empty target is imported into without `merge`.
#[test]
fn imports_into_an_empty_target_without_merge() {
    let l = laravel();
    l.sql(ENTITIES);
    let mut env = Env::new();

    let report = import_laravel::run(&l.path, &mut env.conn, &FRESH).unwrap();

    assert_eq!((report.projects, report.sessions, report.runs), (2, 3, 4));
}

/// A value outside the model's enum leaves the record out, with the reason; its runs (and a
/// dismissal pointing at one of them) are left out too and listed.
#[test]
fn unknown_enum_value_skips_the_record_and_its_dependents() {
    let l = laravel();
    l.sql(
        "INSERT INTO projects (id, key, name, path) VALUES (1, '/p', 'p', '/p');
         INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, origin, last_activity_at)
           VALUES (1, 1, 'claude', 'ok', '/p', 'interactive', '2026-10-29 10:00:00'),
                  (2, 1, 'claude', 'bad', '/p', 'telepathy', '2026-10-29 10:00:00');
         INSERT INTO session_runs (id, ai_session_id, started_at, end_reason, summary_status)
           VALUES (10, 1, '2026-10-29 09:00:00', NULL, 'pending'),
                  (11, 2, '2026-10-29 09:00:00', NULL, 'pending'),
                  (12, 1, '2026-10-29 09:30:00', 'vanished', 'pending'),
                  (13, 1, '2026-10-29 09:40:00', NULL, 'maybe');
         INSERT INTO inbox_dismissals (id, item_key, state_hash)
           VALUES (5, 'interrupted:1:main', '11:1792501500');",
    );
    let mut env = Env::new();

    let report = import_laravel::run(&l.path, &mut env.conn, &FRESH).unwrap();

    let sids: Vec<String> = env
        .conn
        .prepare("SELECT session_id FROM ai_sessions")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(sids, ["ok"]);
    assert_eq!((report.sessions, report.runs, report.dismissals), (1, 1, 0));
    assert_eq!(
        report.skipped,
        [
            "ai_sessions #2: origin 'telepathy' is not a known value",
            "session_runs #12: end_reason 'vanished' is not a known value",
            "session_runs #13: summary_status 'maybe' is not a known value",
            "session_runs #11: session #2 not imported",
            "inbox_dismissals #5 'interrupted:1:main': run #11 not imported",
        ],
    );
}

/// The `/clear` chain is linked even when the previous session has the higher Laravel id.
#[test]
fn links_previous_session_with_a_higher_id() {
    let l = laravel();
    l.sql(
        "INSERT INTO projects (id, key, name, path) VALUES (1, '/p', 'p', '/p');
         INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, previous_session_id,
           last_activity_at)
           VALUES (5, 1, 'claude', 'later', '/p', 9, '2026-10-29 10:00:00'),
                  (9, 1, 'claude', 'earlier', '/p', NULL, '2026-10-29 09:00:00');",
    );
    let mut env = env_with_offset();

    let report = import(&l, &mut env);

    assert!(report.skipped.is_empty(), "{:?}", report.skipped);
    let later = session_by_sid(&env.conn, "later");
    assert_eq!(
        later.previous_session_id,
        Some(session_id(&env.conn, "earlier"))
    );
}
