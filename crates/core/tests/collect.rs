//! Port of Laravel `tests/Feature/Sessions/CollectTest.php` (20 cases, Inv §3.5), plus Codex
//! schema drift (Review Focus 3) and backfill progress. `aiannotations:collect` is
//! `collector::run(.., CollectMode::Window, ..)` and `aiannotations:backfill` is
//! `CollectMode::Backfill`; the summarizer (a later task) is stood in for by marking the run
//! `done` with the transcript's fingerprint, which is what it leaves behind.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aiannotations_core::model::Tool;
use aiannotations_core::sessions::collector::{self, CollectMode, CollectReport};
use aiannotations_core::transcripts::factory;
use rusqlite::{Connection, params};
use serde_json::json;
use support::*;

const FIXTURE_CWD: &str = "/home/fulano/Code/Projeto Ação/Exemplo";
const CODEX_MAIN: &str = "0199bbbb-0000-7000-8000-000000000001";
const CODEX_SUBAGENT: &str = "0199bbbb-0000-7000-8000-000000000002";
const CODEX_EXEC: &str = "0199bbbb-0000-7000-8000-000000000004";

fn set_mtime(path: &Path, at: &str) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_millis(ts(at) as u64);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

/// `$this->claude`: the Claude fixture at `{claude_dir}/projects/<rel>` with the given mtime.
fn claude(env: &Env, rel: &str, mtime: &str, replace: &[(&str, &str)]) -> PathBuf {
    let mut body = CLAUDE_FIXTURE.to_string();
    for (from, to) in replace {
        body = body.replace(from, to);
    }
    let path = env.paths.claude_dir.join("projects").join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).unwrap();
    set_mtime(&path, mtime);
    path
}

/// `$this->grow`: appends a user prompt at `at` and moves the mtime there.
fn grow(path: &Path, at: &str) {
    let sid = path.file_stem().unwrap().to_string_lossy().into_owned();
    append_line(
        path,
        &json!({"type": "user", "isSidechain": false, "cwd": FIXTURE_CWD, "sessionId": sid,
                "entrypoint": "cli",
                "message": {"role": "user", "content": "Continue de onde parou."},
                "uuid": format!("u-extra-{}", ts(at)), "timestamp": at}),
    );
    set_mtime(path, at);
}

/// `$this->codex`: the Codex databases of `tests/fixtures/codex/thread_history.sql`.
fn codex(env: &Env) {
    let sql = include_str!("fixtures/codex/thread_history.sql");
    for section in sql.split("-- @database ").skip(1) {
        let (file, body) = section.split_once('\n').unwrap();
        Connection::open(env.paths.codex_home.join(file.trim()))
            .unwrap()
            .execute_batch(body)
            .unwrap();
    }
}

fn run(env: &mut Env, mode: CollectMode, now: &str) -> CollectReport {
    collector::run(
        &mut env.conn,
        &env.paths,
        &env.settings,
        &env.git,
        mode,
        ts(now),
        &mut |_, _| {},
    )
    .unwrap()
}

/// `aiannotations:collect` at `now`; its report must be free of errors.
fn collect(env: &mut Env, now: &str) -> CollectReport {
    let report = run(env, CollectMode::Window, now);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
    report
}

fn backfill(env: &mut Env, now: &str) -> CollectReport {
    let report = run(env, CollectMode::Backfill, now);
    assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
    report
}

fn session_ids(conn: &Connection) -> Vec<String> {
    sessions(conn, "1")
        .into_iter()
        .map(|s| s.session_id)
        .collect()
}

/// What the summarizer leaves behind: `done`, one attempt, the transcript's fingerprint.
fn mark_summarized(env: &Env, session_id: &str, transcript: &Path) {
    let fingerprint = factory::read(Tool::Claude, session_id, Some(transcript), &env.paths)
        .unwrap()
        .unwrap()
        .fingerprint();
    env.conn
        .execute(
            "UPDATE session_runs SET summary_status = 'done', summary_attempts = 1, \
             summarized_fingerprint = ?1, summary = 'Página de contato.'",
            [fingerprint],
        )
        .unwrap();
}

/// Every column of every row of `table`, in id order (`toArray()` snapshots).
fn dump(conn: &Connection, table: &str) -> Vec<Vec<rusqlite::types::Value>> {
    let mut stmt = conn
        .prepare(&format!("SELECT * FROM {table} ORDER BY id"))
        .unwrap();
    let n = stmt.column_count();
    stmt.query_map([], |r| {
        (0..n)
            .map(|i| r.get::<_, rusqlite::types::Value>(i))
            .collect()
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

fn set_session(conn: &Connection, sql_set: &str, id: i64) {
    conn.execute(
        &format!("UPDATE ai_sessions SET {sql_set} WHERE id = ?1"),
        [id],
    )
    .unwrap();
}

/// descobre sessão Claude sem hook e fecha como Idle após 30 min
#[test]
fn discovers_claude_session_without_hook_and_closes_idle_after_30_min() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );

    collect(&mut env, "2026-10-20T12:40:00Z");

    let s = sole_session(&env.conn);
    let run = sole_run(&env.conn);
    assert_eq!(s.tool, "claude");
    assert_eq!(s.session_id, "abc");
    assert_eq!(s.transcript_path.as_deref(), Some(&*path.to_string_lossy()));
    assert_eq!(s.cwd, FIXTURE_CWD);
    assert_eq!(s.origin, "interactive");
    assert_eq!(s.title.as_deref(), Some("Página de contato com validação"));
    assert_eq!(s.last_activity_at, ts("2026-10-20T12:05:30Z"));
    assert_eq!(run.started_at, ts("2026-10-20T12:00:00Z"));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:05:30Z")));
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    assert_eq!(run.ended_cleanly, Some(false));
    assert!(!run.is_backfill);
    assert!(run.discovered);
    assert_eq!(run.summary_status, "pending");
    assert!(run.files_touched.unwrap().contains(&CONTATO.to_string()));

    // Running again creates nothing.
    collect(&mut env, "2026-10-20T12:40:00Z");
    assert_eq!(count(&env.conn, "ai_sessions"), 1);
    assert_eq!(count(&env.conn, "session_runs"), 1);
}

/// não fecha sessão com atividade há 10 min
#[test]
fn does_not_close_session_active_10_min_ago() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );

    collect(&mut env, "2026-10-20T12:15:30Z");

    let run = sole_run(&env.conn);
    assert_eq!(run.ended_at, None);
    assert_eq!(run.end_reason, None);
    assert_eq!(run.started_at, ts("2026-10-20T12:00:00Z"));
}

/// fecha como Idle a execução aberta pelo hook quando a inatividade passa de 30 min
#[test]
fn closes_open_run_as_idle_when_inactivity_exceeds_30_min() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    let first = collect(&mut env, "2026-10-20T12:15:30Z");
    assert_eq!(sole_run(&env.conn).ended_at, None);
    assert_eq!(first.closed_idle, 0);

    let report = collect(&mut env, "2026-10-20T12:36:00Z");

    let run = sole_run(&env.conn);
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    assert_eq!(run.ended_cleanly, Some(false));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:05:30Z")));
    assert_eq!(report.seen, 1);
    assert_eq!(report.closed_idle, 1);
}

/// fecha execução aberta de sessão que saiu da janela do collect
#[test]
fn closes_open_run_of_session_outside_collect_window() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    collect(&mut env, "2026-10-20T12:15:30Z");

    // Three days later the transcript is outside the 2-day window.
    let report = collect(&mut env, "2026-10-23T12:00:00Z");

    let run = sole_run(&env.conn);
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T12:05:30Z")));
    assert_eq!(report.seen, 0);
    assert_eq!(report.closed_idle, 1);
}

/// reabre sessão Idle quando a transcrição volta a crescer
#[test]
fn reopens_idle_session_when_transcript_grows_again() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    collect(&mut env, "2026-10-20T12:40:00Z");
    assert_eq!(sole_run(&env.conn).end_reason.as_deref(), Some("idle"));

    grow(&path, "2026-10-20T13:00:00Z");
    collect(&mut env, "2026-10-20T13:05:00Z");

    let run = sole_run(&env.conn);
    assert_eq!(run.ended_at, None);
    assert_eq!(run.end_reason, None);
    assert_eq!(run.ended_cleanly, None);
    assert_eq!(
        sole_session(&env.conn).last_activity_at,
        ts("2026-10-20T13:00:00Z")
    );

    // Without new activity it closes again, now at the end of the new activity.
    collect(&mut env, "2026-10-20T13:40:00Z");
    let run = sole_run(&env.conn);
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    assert_eq!(run.ended_at, Some(ts("2026-10-20T13:00:00Z")));

    // Stable: the next collect does not reopen.
    collect(&mut env, "2026-10-20T13:40:00Z");
    assert_eq!(sole_run(&env.conn).end_reason.as_deref(), Some("idle"));
}

/// ignora transcrições em subagents/
#[test]
fn ignores_transcripts_under_subagents() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    claude(
        &env,
        "-home-fulano-Exemplo/abc/subagents/agent-1.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    claude(
        &env,
        "-home-fulano-Exemplo/abc/agent-2.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );

    collect(&mut env, "2026-10-20T12:40:00Z");

    assert_eq!(session_ids(&env.conn), ["abc"]);
}

/// ignora cwd dentro de ai_workdir
#[test]
fn ignores_cwd_inside_ai_workdir() {
    let mut env = Env::new();
    let workdir = env.paths.ai_workdir.to_string_lossy().into_owned();
    std::fs::create_dir_all(&workdir).unwrap();
    let (sub, neighbour) = (format!("{workdir}/sub"), format!("{workdir}-vizinho"));
    for (rel, cwd) in [
        ("-ai-workdir/igual.jsonl", workdir.as_str()),
        ("-ai-workdir-sub/dentro.jsonl", sub.as_str()),
        ("-ai-workdir-vizinho/vizinho.jsonl", neighbour.as_str()),
    ] {
        claude(&env, rel, "2026-10-20T12:05:30Z", &[(FIXTURE_CWD, cwd)]);
    }

    let report = collect(&mut env, "2026-10-20T12:40:00Z");

    assert_eq!(session_ids(&env.conn), ["vizinho"]);
    assert_eq!(report.seen, 1);
}

/// Codex: thread exec vira automation e Skipped
#[test]
fn codex_exec_thread_becomes_automation_and_skipped() {
    let mut env = Env::new();
    codex(&env);

    let report = collect(&mut env, "2026-10-20T14:00:00Z");

    assert_eq!(report.seen, 4);
    assert_eq!(count(&env.conn, "ai_sessions"), 4);
    let exec = session_by_sid(&env.conn, CODEX_EXEC);
    let exec_run = sole(runs(&env.conn, &format!("ai_session_id = {}", exec.id)));
    assert_eq!(exec.tool, "codex");
    assert_eq!(exec.origin, "automation");
    assert_eq!(exec.transcript_path, None);
    assert_eq!(exec_run.summary_status, "skipped");
    assert_eq!(exec_run.end_reason.as_deref(), Some("idle"));

    let subagent = session_by_sid(&env.conn, CODEX_SUBAGENT);
    let sub_run = sole(runs(&env.conn, &format!("ai_session_id = {}", subagent.id)));
    assert_eq!(subagent.origin, "subagent");
    assert_eq!(sub_run.summary_status, "skipped");

    let interactive = session_by_sid(&env.conn, CODEX_MAIN);
    assert_eq!(interactive.origin, "interactive");
    assert_eq!(interactive.cwd, "/home/fulano/Code/Agenda Escolar");
    assert_eq!(interactive.title.as_deref(), Some("Agenda com lembretes"));
    let run = sole(runs(
        &env.conn,
        &format!("ai_session_id = {}", interactive.id),
    ));
    assert_eq!(run.summary_status, "pending");
    assert_eq!(run.started_at, 1792501200000);
    assert_eq!(run.ended_at, Some(1792501500000));
    assert_eq!(interactive.last_activity_at, 1792501500000);
}

fn sole<T: std::fmt::Debug>(mut v: Vec<T>) -> T {
    assert_eq!(v.len(), 1, "expected one row: {v:?}");
    v.remove(0)
}

/// Codex: usa rollout_path quando o arquivo existe
#[test]
fn codex_uses_rollout_path_when_file_exists() {
    let mut env = Env::new();
    codex(&env);
    let rollout = env.paths.codex_home.join("sessions/rollout-1.jsonl");
    std::fs::create_dir_all(rollout.parent().unwrap()).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/rollout.jsonl"),
        &rollout,
    )
    .unwrap();
    Connection::open(env.paths.codex_home.join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET rollout_path = ?1 WHERE id = ?2",
            params![rollout.to_string_lossy(), CODEX_MAIN],
        )
        .unwrap();

    collect(&mut env, "2026-10-20T14:00:00Z");

    assert_eq!(
        session_by_sid(&env.conn, CODEX_MAIN)
            .transcript_path
            .as_deref(),
        Some(&*rollout.to_string_lossy())
    );
}

/// collect ignora sessões com atividade há mais de 2 dias e o backfill as importa
#[test]
fn collect_skips_sessions_older_than_2_days_and_backfill_imports_them() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/antiga.jsonl",
        "2026-10-17T12:05:30Z",
        &[],
    );

    collect(&mut env, "2026-10-20T12:40:00Z");
    assert_eq!(count(&env.conn, "ai_sessions"), 0);

    backfill(&mut env, "2026-10-20T12:40:00Z");
    assert_eq!(sole_session(&env.conn).session_id, "antiga");
}

/// backfill marca is_backfill e é idempotente
#[test]
fn backfill_marks_is_backfill_and_is_idempotent() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    codex(&env);

    let report = backfill(&mut env, "2026-10-25T09:00:00Z");
    assert_eq!(report.seen, 5);

    let snapshot = |conn: &Connection| {
        runs(conn, "1")
            .into_iter()
            .map(|r| {
                (
                    r.id,
                    r.ai_session_id,
                    r.started_at,
                    r.ended_at,
                    r.end_reason,
                )
            })
            .collect::<Vec<_>>()
    };
    let before = snapshot(&env.conn);
    assert_eq!(count(&env.conn, "ai_sessions"), 5);
    assert_eq!(count(&env.conn, "session_runs"), 5);
    assert_eq!(runs(&env.conn, "is_backfill = 1").len(), 5);
    assert_eq!(runs(&env.conn, "ended_at IS NULL").len(), 0);

    let again = backfill(&mut env, "2026-10-25T09:00:00Z");
    assert_eq!(again.seen, 5);
    assert_eq!(again.closed_idle, 0);
    assert_eq!(count(&env.conn, "ai_sessions"), 5);
    assert_eq!(count(&env.conn, "session_runs"), 5);
    assert_eq!(snapshot(&env.conn), before);
}

/// sessão Idle com timestamps em milissegundos e mtime 1 s depois não volta para a IA
#[test]
fn idle_session_with_ms_timestamps_and_mtime_1s_later_does_not_go_back_to_ai() {
    let mut env = Env::new();
    // Real transcript: lastAt with milliseconds and the mtime 1 s after the last event.
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:31Z",
        &[("12:05:30.000Z", "12:05:30.456Z")],
    );

    collect(&mut env, "2026-10-20T12:40:00Z");
    assert_eq!(sole_run(&env.conn).summary_status, "pending");
    mark_summarized(&env, "abc", &path);

    for at in ["2026-10-20T12:55:00Z", "2026-10-20T13:10:00Z"] {
        collect(&mut env, at);
        let run = sole_run(&env.conn);
        // Still done: the summarizer would not pick it again.
        assert_eq!(run.summary_status, "done", "at {at}");
        assert_eq!(run.end_reason.as_deref(), Some("idle"));
        let summary: Option<String> = env
            .conn
            .query_row("SELECT summary FROM session_runs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(summary.as_deref(), Some("Página de contato."));
    }
}

/// reprocessar a transcrição sem conteúdo novo mantém o resumo Done
#[test]
fn reprocessing_transcript_without_new_content_keeps_summary_done() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    collect(&mut env, "2026-10-20T12:40:00Z");
    mark_summarized(&env, "abc", &path);

    let id = sole_session(&env.conn).id;
    env.recorder()
        .refresh_from_transcript(id, None, now_ms())
        .unwrap();
    assert_eq!(sole_run(&env.conn).summary_status, "done");

    // New content (a prompt after the summary) goes back to Pending.
    grow(&path, "2026-10-20T13:00:00Z");
    env.recorder()
        .refresh_from_transcript(id, None, now_ms())
        .unwrap();
    let run = sole_run(&env.conn);
    assert_eq!(run.summary_status, "pending");
    assert_eq!(run.summary_attempts, 0);
}

/// erro numa sessão não interrompe a coleta das demais
#[test]
fn error_in_one_session_does_not_stop_the_others() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Ruim/ruim.jsonl",
        "2026-10-20T12:05:30Z",
        &[(FIXTURE_CWD, "/home/fulano/Code/Ruim")],
    );
    claude(
        &env,
        "-home-fulano-Exemplo/boa.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    // Stand-in for v1's mock throwing on `ruim`: the database refuses that one session.
    env.conn
        .execute_batch(
            "CREATE TRIGGER boom BEFORE INSERT ON ai_sessions WHEN NEW.session_id = 'ruim' \
             BEGIN SELECT RAISE(ABORT, 'boom'); END;",
        )
        .unwrap();

    let report = run(&mut env, CollectMode::Backfill, "2026-10-20T12:40:00Z");

    assert_eq!(session_ids(&env.conn), ["boa"]);
    assert_eq!(report.seen, 2);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(report.errors[0].contains("ruim"), "{:?}", report.errors);
    assert!(report.errors[0].contains("boom"), "{:?}", report.errors);
    // The failed session left nothing behind (its own transaction rolled back): its project,
    // created before the refused insert, is gone too.
    assert_eq!(count(&env.conn, "projects"), 1);
    let ruim: i64 = env
        .conn
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE path LIKE '%Ruim%' OR name LIKE '%Ruim%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(ruim, 0);
}

/// coleta sem mudança no mtime não relê a transcrição nem altera nada
#[test]
fn collect_without_mtime_change_does_not_reread_or_change_anything() {
    let mut env = Env::new();
    // mtime 1 s after the last event: without the stored mtime every pass reread the transcript.
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:31Z",
        &[],
    );
    collect(&mut env, "2026-10-20T12:40:00Z");

    let mtime: Option<i64> = env
        .conn
        .query_row("SELECT transcript_mtime FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mtime, Some(ts("2026-10-20T12:05:31Z")));

    // Stand-in for v1's read counter: the transcript gains activity but keeps its mtime, so any
    // reread would reopen the run.
    grow(&path, "2026-10-20T12:50:00Z");
    set_mtime(&path, "2026-10-20T12:05:31Z");
    let before = (
        dump(&env.conn, "ai_sessions"),
        dump(&env.conn, "session_runs"),
    );

    collect(&mut env, "2026-10-20T12:55:00Z");
    assert_eq!(
        (
            dump(&env.conn, "ai_sessions"),
            dump(&env.conn, "session_runs")
        ),
        before
    );

    // With the file changed, it reads again.
    set_mtime(&path, "2026-10-20T12:50:00Z");
    collect(&mut env, "2026-10-20T12:55:00Z");
    let run = sole_run(&env.conn);
    assert_eq!(run.ended_at, None);
    assert_eq!(
        sole_session(&env.conn).last_activity_at,
        ts("2026-10-20T12:50:00Z")
    );
}

/// sessão fechada limpa que volta a ter atividade sem hook ganha execução nova
#[test]
fn cleanly_closed_session_with_activity_without_hook_gets_new_run() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    let session = insert_session(
        &env.conn,
        Tool::Claude,
        "abc",
        Path::new(FIXTURE_CWD),
        Some(&path),
    );
    set_session(
        &env.conn,
        &format!("last_activity_at = {}", ts("2026-10-20T12:05:40Z")),
        session,
    );
    let closed = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:05:40Z")),
            end_reason: Some("prompt_input_exit"),
            ended_cleanly: Some(true),
            summary_status: Some("done"),
            ..Default::default()
        },
    );

    // Activity 50 s after /exit (e.g. final lines written by claude itself): nothing changes.
    grow(&path, "2026-10-20T12:06:30Z");
    collect(&mut env, "2026-10-20T12:07:00Z");
    assert_eq!(count(&env.conn, "session_runs"), 1);

    // Resumed outside the terminal: new activity, no SessionStart.
    grow(&path, "2026-10-20T13:00:00Z");
    collect(&mut env, "2026-10-20T13:05:00Z");

    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, closed);
    assert_eq!(all[0].end_reason.as_deref(), Some("prompt_input_exit"));
    assert_eq!(all[0].summary_status, "done");
    // Starts at the first activity after ended_at + 60 s tolerance: the 12:06:30 one (final
    // /exit lines) is left out; the new run starts at 13:00:00.
    assert_eq!(all[1].started_at, ts("2026-10-20T13:00:00Z"));
    assert_eq!(all[1].ended_at, None);
    assert_eq!(all[1].summary_status, "pending");
    assert!(all[1].discovered);

    // Without activity, the new run closes as Idle.
    collect(&mut env, "2026-10-20T13:40:00Z");
    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].end_reason.as_deref(), Some("idle"));
    assert_eq!(all[1].ended_at, Some(ts("2026-10-20T13:00:00Z")));
}

/// atividade depois de fechamento limpo já inativa cria a execução fechada como Idle
#[test]
fn activity_after_clean_close_already_idle_creates_run_closed_as_idle() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    let session = insert_session(
        &env.conn,
        Tool::Claude,
        "abc",
        Path::new(FIXTURE_CWD),
        Some(&path),
    );
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:05:30Z")),
            end_reason: Some("clear"),
            ended_cleanly: Some(true),
            ..Default::default()
        },
    );

    grow(&path, "2026-10-20T13:00:00Z");
    collect(&mut env, "2026-10-20T14:00:00Z");

    let all = runs(&env.conn, "1");
    assert_eq!(all.len(), 2);
    let latest = &all[1];
    assert_eq!(latest.started_at, ts("2026-10-20T13:00:00Z"));
    assert_eq!(latest.ended_at, Some(ts("2026-10-20T13:00:00Z")));
    assert_eq!(latest.end_reason.as_deref(), Some("idle"));
    assert_eq!(latest.ended_cleanly, Some(false));
}

/// fecha todas as execuções abertas fora da janela mesmo com mais sessões que o tamanho do lote
#[test]
fn closes_every_open_run_outside_window_even_with_more_sessions_than_chunk_size() {
    let mut env = Env::new();
    env.settings.chunk_size = 2;
    for i in 1..=5 {
        let gone = env.base.join(format!("sumiu-{i}.jsonl"));
        let session = insert_session(
            &env.conn,
            Tool::Claude,
            &format!("fora-{i}"),
            Path::new(FIXTURE_CWD),
            Some(&gone),
        );
        set_session(
            &env.conn,
            &format!("last_activity_at = {}", ts("2026-10-20T12:00:00Z")),
            session,
        );
        insert_run(
            &env.conn,
            session,
            RunSeed {
                started_at: ts("2026-10-20T11:00:00Z"),
                ..Default::default()
            },
        );
    }

    let report = collect(&mut env, "2026-10-23T12:00:00Z");

    assert_eq!(runs(&env.conn, "ended_at IS NULL").len(), 0);
    assert_eq!(runs(&env.conn, "end_reason = 'idle'").len(), 5);
    assert_eq!(report.closed_idle, 5);
}

/// Codex com histórico ilegível registra a sessão sem memorizar o mtime e relê na passada seguinte
#[test]
fn codex_with_unreadable_history_records_session_without_mtime_and_rereads_next_pass() {
    let mut env = Env::new();
    codex(&env);
    let history = env.paths.codex_home.join("thread_history_1.sqlite");
    let good = env.base.join("thread_history_1.sqlite.bom");
    std::fs::rename(&history, &good).unwrap();
    std::fs::write(&history, "lixo".repeat(1024)).unwrap();

    let report = run(&mut env, CollectMode::Window, "2026-10-20T13:10:00Z");
    // The unreadable reads are reported as warnings, not swallowed nor counted as failures.
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!report.warnings.is_empty());
    assert!(
        report.warnings.iter().all(|e| e.contains("unreadable")),
        "{:?}",
        report.warnings
    );

    let mtime = |conn: &Connection| -> Option<i64> {
        conn.query_row(
            "SELECT transcript_mtime FROM ai_sessions WHERE session_id = ?1",
            [CODEX_MAIN],
            |r| r.get(0),
        )
        .unwrap()
    };
    let session = session_by_sid(&env.conn, CODEX_MAIN);
    assert_eq!(mtime(&env.conn), None);
    let run = sole(runs(&env.conn, &format!("ai_session_id = {}", session.id)));
    assert_ne!(run.summary_status, "skipped");

    std::fs::rename(&good, &history).unwrap();
    collect(&mut env, "2026-10-20T13:10:00Z");

    assert_eq!(mtime(&env.conn), Some(1792501500000));
}

/// fechar uma execução fora da janela não grava last_activity_at como transcript_mtime
#[test]
fn closing_run_outside_window_does_not_store_last_activity_as_transcript_mtime() {
    let mut env = Env::new();
    let gone = env.base.join("sumiu.jsonl");
    let session = insert_session(
        &env.conn,
        Tool::Claude,
        "fora",
        Path::new(FIXTURE_CWD),
        Some(&gone),
    );
    set_session(
        &env.conn,
        &format!(
            "last_activity_at = {}, transcript_mtime = 1792497000123",
            ts("2026-10-20T12:00:00Z")
        ),
        session,
    );
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T11:00:00Z"),
            ..Default::default()
        },
    );

    collect(&mut env, "2026-10-23T12:00:00Z");

    let run = sole_run(&env.conn);
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    let mtime: Option<i64> = env
        .conn
        .query_row("SELECT transcript_mtime FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mtime, Some(1792497000123));
}

/// Review Focus 3: a Codex `threads` table without `updated_at_ms` is an isolated error; the
/// Claude discovery goes on.
#[test]
fn codex_schema_drift_is_isolated() {
    let mut env = Env::new();
    Connection::open(env.paths.codex_home.join("state_5.sqlite"))
        .unwrap()
        .execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, \
             updated_at INTEGER); \
             INSERT INTO threads VALUES ('t1', '', '/tmp/x', 1792501500);",
        )
        .unwrap();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );

    let report = run(&mut env, CollectMode::Window, "2026-10-20T12:40:00Z");

    assert_eq!(session_ids(&env.conn), ["abc"]);
    assert_eq!(report.seen, 1);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0].to_lowercase().contains("codex"),
        "{:?}",
        report.errors
    );
}

/// A failure in the idle-closing pass goes to `errors`; the work already done is still reported.
#[test]
fn idle_pass_failure_is_reported_without_losing_the_report() {
    let mut env = Env::new();
    claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    // An open, idle session whose transcript_path is a BLOB makes the idle query fail to decode.
    env.conn
        .execute_batch(
            "INSERT INTO projects (key, name, path) VALUES ('quebrado', 'quebrado', '/q'); \
             INSERT INTO ai_sessions (project_id, tool, session_id, cwd, transcript_path, \
                                      last_activity_at) \
             VALUES (last_insert_rowid(), 'claude', 'quebrada', '/q', X'00', 0); \
             INSERT INTO session_runs (ai_session_id, started_at) VALUES (last_insert_rowid(), 0);",
        )
        .unwrap();

    let report = run(&mut env, CollectMode::Window, "2026-10-20T12:40:00Z");

    assert_eq!(report.seen, 1);
    let abc: i64 = env
        .conn
        .query_row(
            "SELECT COUNT(*) FROM ai_sessions WHERE session_id = 'abc'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(abc, 1);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0].starts_with("closing idle sessions failed: "),
        "{:?}",
        report.errors
    );
}

/// The onboarding progress: every 100 sessions and once at the end, with the total.
#[test]
fn backfill_progress_reports_totals() {
    let mut env = Env::new();
    for i in 0..250 {
        claude(
            &env,
            &format!("-home-fulano-Exemplo/s{i:03}.jsonl"),
            "2026-10-20T12:05:30Z",
            &[],
        );
    }
    let mut calls = Vec::new();

    let report = collector::run(
        &mut env.conn,
        &env.paths,
        &env.settings,
        &env.git,
        CollectMode::Backfill,
        ts("2026-10-25T09:00:00Z"),
        &mut |done, total| calls.push((done, total)),
    )
    .unwrap();

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.seen, 250);
    assert_eq!(calls, [(100, 250), (200, 250), (250, 250)]);
    assert_eq!(count(&env.conn, "ai_sessions"), 250);
}

/// An unreadable read while closing a known session for inactivity does not memorize the new
/// mtime either (v1 `$unreadable` on the update path), so the next pass rereads.
#[test]
fn unreadable_history_on_idle_close_keeps_previous_mtime() {
    let mut env = Env::new();
    codex(&env);
    collect(&mut env, "2026-10-20T13:10:00Z");
    let mtime = |conn: &Connection| -> Option<i64> {
        conn.query_row(
            "SELECT transcript_mtime FROM ai_sessions WHERE session_id = ?1",
            [CODEX_MAIN],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(mtime(&env.conn), Some(1792501500000));

    Connection::open(env.paths.codex_home.join("state_5.sqlite"))
        .unwrap()
        .execute(
            "UPDATE threads SET updated_at_ms = 1792501560000 WHERE id = ?1",
            [CODEX_MAIN],
        )
        .unwrap();
    let history = env.paths.codex_home.join("thread_history_1.sqlite");
    let good = env.base.join("thread_history_1.sqlite.bom");
    std::fs::rename(&history, &good).unwrap();
    std::fs::write(&history, "lixo".repeat(1024)).unwrap();

    let report = run(&mut env, CollectMode::Window, "2026-10-20T14:00:00Z");

    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert!(!report.warnings.is_empty());
    let session = session_by_sid(&env.conn, CODEX_MAIN);
    let run = sole(runs(&env.conn, &format!("ai_session_id = {}", session.id)));
    assert_eq!(run.end_reason.as_deref(), Some("idle"));
    assert_eq!(mtime(&env.conn), Some(1792501500000));

    std::fs::rename(&good, &history).unwrap();
    collect(&mut env, "2026-10-20T14:00:00Z");
    assert_eq!(mtime(&env.conn), Some(1792501560000));
}

/// A newer mtime without transcript activity past the tolerance does not open a run after a clean
/// close (the check is on the transcript's `last_at`, not only on the mtime).
#[test]
fn newer_mtime_without_activity_after_clean_close_creates_no_run() {
    let mut env = Env::new();
    let path = claude(
        &env,
        "-home-fulano-Exemplo/abc.jsonl",
        "2026-10-20T12:05:30Z",
        &[],
    );
    let session = insert_session(
        &env.conn,
        Tool::Claude,
        "abc",
        Path::new(FIXTURE_CWD),
        Some(&path),
    );
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:05:40Z")),
            end_reason: Some("prompt_input_exit"),
            ended_cleanly: Some(true),
            ..Default::default()
        },
    );
    grow(&path, "2026-10-20T12:06:30Z");
    set_mtime(&path, "2026-10-20T12:30:00Z");

    collect(&mut env, "2026-10-20T12:31:00Z");

    assert_eq!(count(&env.conn, "session_runs"), 1);
}
