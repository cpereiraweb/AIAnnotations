//! Per-run AI summaries: queue, usage-limit pause, backoff, backfill pacing, optimistic write
//! (Inv §4.2, Review Focus 3).

mod support;

use std::path::{Path, PathBuf};

use aiannotations_core::ai::{AiError, ModelRole};
use aiannotations_core::db::app_state;
use aiannotations_core::model::Tool;
use aiannotations_core::settings::Lang;
use aiannotations_core::summaries::{self, SummaryOutcome, prompt};
use aiannotations_core::transcripts::factory;
use rusqlite::Connection;
use support::*;

const SID: &str = "11111111-aaaa-4bbb-8ccc-000000000001";
type Row = (
    String,
    String,
    Option<String>,
    bool,
    String,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

const MIN: i64 = 60_000;

fn t0() -> i64 {
    ts("2026-10-29T12:00:00Z")
}

/// Env plus the transcript fixture every run points to.
fn setup() -> (Env, PathBuf) {
    let env = Env::new();
    let path = env.transcript("t", &[]);
    (env, path)
}

/// Closed interactive run covering the fixture's first minute (so only `contato.html` falls in
/// its interval).
fn seed_run(env: &Env, path: &Path, sid: &str, seed: RunSeed) -> i64 {
    let cwd = env.base.join("proj");
    let session = insert_session(&env.conn, Tool::Claude, sid, &cwd, Some(path));
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:01:00Z")),
            ..seed
        },
    )
}

fn pending(env: &Env, path: &Path, sid: &str) -> i64 {
    seed_run(env, path, sid, RunSeed::default())
}

fn run_one(env: &mut Env, ai: &FakeAi, id: i64, now: i64) -> Result<SummaryOutcome, AiError> {
    let clock = || now;
    summaries::summarize_run(&mut env.conn, ai, &env.paths, &env.settings, id, &clock)
}

fn batch(
    env: &mut Env,
    ai: &FakeAi,
    limit: usize,
    retry: bool,
    now: i64,
) -> summaries::SummarizeReport {
    let clock = || now;
    summaries::run_batch(
        &mut env.conn,
        ai,
        &env.paths,
        &env.settings,
        limit,
        retry,
        &clock,
    )
    .unwrap()
}

fn status(conn: &Connection, id: i64) -> String {
    conn.query_row(
        "SELECT summary_status FROM session_runs WHERE id = ?1",
        [id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn summarizes_and_maps_kind() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let ai = FakeAi::new(vec![ai_ok(
        serde_json::json!({"summary": "  Fez X  ", "next_step": "", "unfinished": false, "kind": "code"}),
    )]);

    let outcome = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(outcome, SummaryOutcome::Summarized));
    assert_eq!(ai.roles(), vec![ModelRole::Summary]);

    let (status, summary, next, unfinished, kind, stored_fp, available, updated): Row = env
        .conn
        .query_row(
            "SELECT summary_status, summary, next_step, unfinished, kind, summarized_fingerprint, \
             summary_available_at, updated_at FROM session_runs WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .unwrap();
    let fingerprint = factory::read(Tool::Claude, SID, Some(&path), &env.paths)
        .unwrap()
        .unwrap()
        .fingerprint();
    assert_eq!(status, "done");
    assert_eq!(summary, "Fez X");
    assert_eq!(next, None);
    assert!(!unfinished);
    assert_eq!(kind, "codigo");
    assert_eq!(stored_fp.as_deref(), Some(fingerprint.as_str()));
    assert_eq!(available, None);
    assert_eq!(updated, Some(t0()));
}

#[test]
fn conversation_kind_maps_to_conversa_and_long_texts_are_cut() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let long = "á".repeat(2500);
    let ai = FakeAi::new(vec![ai_ok(
        serde_json::json!({"summary": long, "next_step": long, "unfinished": true, "kind": "conversation"}),
    )]);
    run_one(&mut env, &ai, id, t0()).unwrap();
    let (kind, summary, next, unfinished): (String, String, String, bool) = env
        .conn
        .query_row(
            "SELECT kind, summary, next_step, unfinished FROM session_runs WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(kind, "conversa");
    assert_eq!(summary.chars().count(), 2000);
    assert_eq!(next.chars().count(), 2000);
    assert!(unfinished);
}

#[test]
fn search_follows_the_summary() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let ai = FakeAi::new(vec![summary_ok("Emitir NFS-e", "code")]);
    run_one(&mut env, &ai, id, t0()).unwrap();
    let hits = aiannotations_core::search::query(&env.conn, "nfse", 10).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn missing_transcript_is_skipped_without_attempt() {
    let (mut env, _) = setup();
    let gone = env.base.join("nowhere.jsonl");
    let id = seed_run(
        &env,
        &gone,
        SID,
        RunSeed {
            summary_available_at: Some(t0() + MIN),
            ..Default::default()
        },
    );
    let ai = FakeAi::new(vec![]);
    let outcome = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(outcome, SummaryOutcome::Skipped));
    assert_eq!(ai.call_count(), 0);
    let run = &runs(&env.conn, "1")[0];
    assert_eq!(run.summary_status, "skipped");
    assert_eq!(run.summary_attempts, 0);
    assert_eq!(run.summary_available_at, None);
}

#[test]
fn invalid_reply_reschedules_with_backoff() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let invalid = || ai_ok(serde_json::json!({"summary": ""}));
    let ai = FakeAi::new(vec![invalid(), invalid(), invalid(), invalid(), invalid()]);

    let outcome = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(outcome, SummaryOutcome::Rescheduled));
    let run = &runs(&env.conn, "1")[0];
    assert_eq!(run.summary_status, "pending");
    assert_eq!(run.summary_attempts, 1);
    assert_eq!(run.summary_available_at, Some(t0() + 2 * MIN));
    assert_eq!(run.updated_at, Some(t0()));

    run_one(&mut env, &ai, id, t0()).unwrap();
    assert_eq!(
        runs(&env.conn, "1")[0].summary_available_at,
        Some(t0() + 4 * MIN)
    );
    for _ in 0..2 {
        let o = run_one(&mut env, &ai, id, t0()).unwrap();
        assert!(matches!(o, SummaryOutcome::Rescheduled));
    }
    let o = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(o, SummaryOutcome::Failed));
    let run = &runs(&env.conn, "1")[0];
    assert_eq!(run.summary_status, "failed");
    assert_eq!(run.summary_attempts, 5);
    assert_eq!(run.summary_available_at, None);
}

#[test]
fn malformed_replies_and_generic_failures_count_as_attempts() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let ai = FakeAi::new(vec![
        ai_ok(
            serde_json::json!({"summary": "x", "next_step": 3, "unfinished": false, "kind": "code"}),
        ),
        ai_ok(serde_json::json!({"summary": "x", "unfinished": "no", "kind": "code"})),
        ai_ok(serde_json::json!({"summary": "x", "unfinished": false, "kind": "codigo"})),
        Err(AiError::Failed("boom".into())),
    ]);
    for n in 1..=4 {
        let o = run_one(&mut env, &ai, id, t0()).unwrap();
        assert!(matches!(o, SummaryOutcome::Rescheduled), "call {n}");
    }
    assert_eq!(runs(&env.conn, "1")[0].summary_attempts, 4);
}

#[test]
fn run_changed_during_call_discards_result() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let db_path = env.paths.db_path.clone();
    let ai = FakeAi::new(vec![summary_ok("Fez X", "code")]);
    ai.on_call(move |_| {
        let other = aiannotations_core::db::open(&db_path).unwrap();
        other
            .execute(
                "UPDATE session_runs SET ended_at = NULL, updated_at = COALESCE(updated_at, 0) + 1",
                [],
            )
            .unwrap();
    });
    let outcome = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(outcome, SummaryOutcome::Skipped));
    let run = &runs(&env.conn, "1")[0];
    assert_eq!(run.summary_status, "pending");
    assert_eq!(run.summary_attempts, 0);
    assert_eq!(run.summary_available_at, None);
    let summary: Option<String> = env
        .conn
        .query_row("SELECT summary FROM session_runs", [], |r| r.get(0))
        .unwrap();
    assert_eq!(summary, None);
}

#[test]
fn failure_on_a_changed_run_is_not_counted_either() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let db_path = env.paths.db_path.clone();
    let ai = FakeAi::new(vec![Err(AiError::Failed("boom".into()))]);
    ai.on_call(move |_| {
        let other = aiannotations_core::db::open(&db_path).unwrap();
        other
            .execute("UPDATE session_runs SET ended_at = NULL", [])
            .unwrap();
    });
    let outcome = run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(matches!(outcome, SummaryOutcome::Skipped));
    assert_eq!(runs(&env.conn, "1")[0].summary_attempts, 0);
}

#[test]
fn rate_limit_error_is_returned_without_counting() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let ai = FakeAi::new(vec![
        Err(AiError::RateLimited("limit".into())),
        Err(AiError::Unavailable("no login".into())),
    ]);
    assert_eq!(
        run_one(&mut env, &ai, id, t0()).unwrap_err(),
        AiError::RateLimited("limit".into())
    );
    assert_eq!(
        run_one(&mut env, &ai, id, t0()).unwrap_err(),
        AiError::Unavailable("no login".into())
    );
    assert_eq!(runs(&env.conn, "1")[0].summary_attempts, 0);
}

#[test]
fn rate_limit_pauses_batch() {
    let (mut env, path) = setup();
    for i in 0..3 {
        pending(&env, &path, &format!("s{i}"));
    }
    let ai = FakeAi::new(vec![Err(AiError::RateLimited("limit".into()))]);
    let report = batch(&mut env, &ai, 10, false, t0());
    assert_eq!(report.paused_until_ms, Some(t0() + 30 * MIN));
    assert_eq!(report.pause_reason.as_deref(), Some("rate_limited"));
    assert!(!report.was_paused);
    assert_eq!(ai.call_count(), 1);
    assert!(runs(&env.conn, "1").iter().all(|r| r.summary_attempts == 0));
    assert_eq!(
        app_state::get(&env.conn, "ai:paused-until", t0()).unwrap(),
        Some((t0() + 30 * MIN).to_string())
    );

    let again = batch(&mut env, &ai, 10, false, t0());
    assert!(again.was_paused);
    assert_eq!(again.paused_until_ms, Some(t0() + 30 * MIN));
    assert_eq!(ai.call_count(), 1);

    // After the pause the queue runs again.
    ai.push(summary_ok("a", "code"));
    ai.push(summary_ok("b", "code"));
    ai.push(summary_ok("c", "code"));
    let later = batch(&mut env, &ai, 10, false, t0() + 31 * MIN);
    assert!(!later.was_paused);
    assert_eq!(later.summarized, 3);
}

#[test]
fn unavailable_pause_reason_is_cut_to_200_chars() {
    let (mut env, path) = setup();
    pending(&env, &path, SID);
    let ai = FakeAi::new(vec![Err(AiError::Unavailable("é".repeat(300)))]);
    let report = batch(&mut env, &ai, 10, false, t0());
    let reason = report.pause_reason.unwrap();
    assert_eq!(reason, format!("unavailable: {}", "é".repeat(200)));
}

#[test]
fn new_runs_before_backfill_and_backfill_throttled() {
    let (mut env, path) = setup();
    // Backfill runs end later than the new ones: ordering must still put new runs first.
    let new_a = seed_run(&env, &path, "n1", RunSeed::default());
    let new_b = seed_run(&env, &path, "n2", RunSeed::default());
    let bf_a = seed_run(
        &env,
        &path,
        "b1",
        RunSeed {
            is_backfill: true,
            ..Default::default()
        },
    );
    let bf_b = seed_run(
        &env,
        &path,
        "b2",
        RunSeed {
            is_backfill: true,
            ..Default::default()
        },
    );
    let ai = FakeAi::new(vec![
        summary_ok("1", "code"),
        summary_ok("2", "code"),
        summary_ok("3", "code"),
    ]);

    let first = batch(&mut env, &ai, 5, false, t0());
    assert_eq!(first.summarized, 3);
    assert_eq!(ai.call_count(), 3);
    assert_eq!(status(&env.conn, new_a), "done");
    assert_eq!(status(&env.conn, new_b), "done");
    let done_backfill = [bf_a, bf_b]
        .iter()
        .filter(|id| status(&env.conn, **id) == "done")
        .count();
    assert_eq!(done_backfill, 1);

    let second = batch(&mut env, &ai, 5, false, t0() + 60_000);
    assert_eq!(second.summarized, 0);
    assert_eq!(ai.call_count(), 3);

    ai.push(summary_ok("4", "code"));
    let third = batch(&mut env, &ai, 5, false, t0() + 121_000);
    assert_eq!(third.summarized, 1);
    assert_eq!(status(&env.conn, bf_a), "done");
    assert_eq!(status(&env.conn, bf_b), "done");
}

#[test]
fn failed_backfill_counts_for_interval() {
    let (mut env, path) = setup();
    let backfill = || RunSeed {
        is_backfill: true,
        ..Default::default()
    };
    let first = seed_run(&env, &path, "b1", backfill());
    let second = seed_run(&env, &path, "b2", backfill());
    let ai = FakeAi::new(vec![ai_ok(serde_json::json!({"summary": ""}))]);

    let report = batch(&mut env, &ai, 5, false, t0());
    assert_eq!(report.rescheduled, 1);
    assert_eq!(ai.call_count(), 1);
    // The second candidate was not even tried: the failure used the interval.
    assert_eq!(status(&env.conn, first), "pending");
    assert_eq!(status(&env.conn, second), "pending");
    let untouched = runs(&env.conn, "1")
        .iter()
        .filter(|r| r.summary_attempts == 0)
        .count();
    assert_eq!(untouched, 1);
}

#[test]
fn open_and_skipped_runs_are_not_candidates() {
    let (mut env, path) = setup();
    seed_run(
        &env,
        &path,
        "open",
        RunSeed {
            ..Default::default()
        },
    );
    env.conn
        .execute("UPDATE session_runs SET ended_at = NULL", [])
        .unwrap();
    seed_run(
        &env,
        &path,
        "skipped",
        RunSeed {
            summary_status: Some("skipped"),
            ..Default::default()
        },
    );
    seed_run(
        &env,
        &path,
        "done",
        RunSeed {
            summary_status: Some("done"),
            ..Default::default()
        },
    );
    seed_run(
        &env,
        &path,
        "later",
        RunSeed {
            summary_available_at: Some(t0() + 1),
            ..Default::default()
        },
    );
    let due = seed_run(
        &env,
        &path,
        "due",
        RunSeed {
            summary_available_at: Some(t0()),
            ..Default::default()
        },
    );
    let ai = FakeAi::new(vec![summary_ok("ok", "code")]);
    let report = batch(&mut env, &ai, 10, false, t0());
    assert_eq!(report.summarized, 1);
    assert_eq!(ai.call_count(), 1);
    assert_eq!(status(&env.conn, due), "done");
}

#[test]
fn limit_caps_the_batch_and_zero_means_one() {
    let (mut env, path) = setup();
    for i in 0..3 {
        pending(&env, &path, &format!("s{i}"));
    }
    let ai = FakeAi::new(vec![summary_ok("1", "code"), summary_ok("2", "code")]);
    assert_eq!(batch(&mut env, &ai, 0, false, t0()).summarized, 1);
    assert_eq!(batch(&mut env, &ai, 1, false, t0()).summarized, 1);
}

#[test]
fn prompt_lists_only_files_touched_in_the_run() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    let ai = FakeAi::new(vec![summary_ok("ok", "code")]);
    run_one(&mut env, &ai, id, t0()).unwrap();
    let prompt = &ai.prompts()[0];
    assert!(prompt.contains("contato.html"), "{prompt}");
    assert!(!prompt.contains("valida.js"), "{prompt}");
}

#[test]
fn prompt_header_uses_run_session_and_extract_data() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    env.conn
        .execute("UPDATE session_runs SET branch = 'feat/x'", [])
        .unwrap();
    env.conn
        .execute("UPDATE ai_sessions SET title = 'Meu título'", [])
        .unwrap();
    env.conn
        .execute("UPDATE projects SET name = 'Projeto Z'", [])
        .unwrap();
    let ai = FakeAi::new(vec![summary_ok("ok", "code")]);
    run_one(&mut env, &ai, id, t0()).unwrap();
    let p = &ai.prompts()[0];
    assert!(p.contains("Project: Projeto Z\n"), "{p}");
    assert!(p.contains("Branch: feat/x\n"), "{p}");
    assert!(p.contains("Title: Meu título\n"), "{p}");
    assert!(p.contains(&format!("Directory: {}\n", env.base.join("proj").display())));

    // Without run branch / session title the extract fills them in.
    env.conn
        .execute(
            "UPDATE session_runs SET branch = NULL, summary_status = 'pending'",
            [],
        )
        .unwrap();
    env.conn
        .execute("UPDATE ai_sessions SET title = NULL", [])
        .unwrap();
    let x = factory::read(Tool::Claude, SID, Some(&path), &env.paths)
        .unwrap()
        .unwrap();
    ai.push(summary_ok("ok", "code"));
    run_one(&mut env, &ai, id, t0()).unwrap();
    let p = &ai.prompts()[1];
    assert!(
        p.contains(&format!("Branch: {}\n", x.branch.clone().unwrap())),
        "{p}"
    );
    assert!(
        p.contains(&format!("Title: {}\n", x.title.clone().unwrap())),
        "{p}"
    );
}

#[test]
fn prompt_language_follows_lang() {
    let en = prompt::for_run(Lang::En, "P", None, Tool::Claude, None, "/cwd", "BODY");
    assert!(en.contains("<transcript>\nBODY\n</transcript>"));
    assert!(en.contains("Tool: Claude Code"));
    assert!(en.contains("Branch: (no branch)"));
    assert!(!en.contains("Title:"));
    assert!(en.contains("in English"));

    let pt = prompt::for_run(
        Lang::PtBr,
        "P",
        Some("main"),
        Tool::Codex,
        Some("T"),
        "/cwd",
        "BODY",
    );
    assert!(pt.contains("<extrato>\nBODY\n</extrato>"));
    assert!(pt.contains("Ferramenta: Codex"));
    assert!(pt.contains("Branch: main"));
    assert!(pt.contains("Título: T"));
    assert!(pt.contains("Diretório: /cwd"));
    let none = prompt::for_run(Lang::PtBr, "P", None, Tool::Codex, None, "/cwd", "BODY");
    assert!(none.contains("Branch: (sem branch)"));
    assert!(!none.contains("Título:"));
    assert!(pt.starts_with("Você resume sessões de trabalho"));
}

#[test]
fn settings_lang_selects_the_prompt() {
    let (mut env, path) = setup();
    let id = pending(&env, &path, SID);
    env.settings.lang = Lang::PtBr;
    let ai = FakeAi::new(vec![summary_ok("ok", "code")]);
    run_one(&mut env, &ai, id, t0()).unwrap();
    assert!(ai.prompts()[0].contains("<extrato>"));
}

#[test]
fn retry_failed_requeues() {
    let (mut env, path) = setup();
    let id = seed_run(
        &env,
        &path,
        SID,
        RunSeed {
            summary_status: Some("failed"),
            summary_attempts: 5,
            summary_available_at: Some(t0() + 99 * MIN),
            ..Default::default()
        },
    );
    let ai = FakeAi::new(vec![]);
    let untouched = batch(&mut env, &ai, 5, false, t0());
    assert_eq!(untouched.retried, 0);
    assert_eq!(status(&env.conn, id), "failed");

    ai.push(summary_ok("ok", "code"));
    let report = batch(&mut env, &ai, 5, true, t0());
    assert_eq!(report.retried, 1);
    assert_eq!(report.summarized, 1);
    let run = &runs(&env.conn, "1")[0];
    assert_eq!(run.summary_status, "done");
    assert_eq!(run.summary_attempts, 0, "the budget starts over");
}

#[test]
fn backfill_progress_counts() {
    let (env, path) = setup();
    let bf = |status: &'static str, sid: &str| {
        seed_run(
            &env,
            &path,
            sid,
            RunSeed {
                is_backfill: true,
                summary_status: Some(status),
                ..Default::default()
            },
        )
    };
    assert_eq!(summaries::backfill_progress(&env.conn).unwrap(), None);
    bf("done", "a");
    assert_eq!(summaries::backfill_progress(&env.conn).unwrap(), None);
    bf("pending", "b");
    bf("skipped", "c");
    pending(&env, &path, "new");
    assert_eq!(
        summaries::backfill_progress(&env.conn).unwrap(),
        Some((1, 2))
    );
}
