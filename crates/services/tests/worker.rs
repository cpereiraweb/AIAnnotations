//! Background worker: schedule, per-database lock, job isolation, pauses and the change flag.

use std::collections::HashMap;
use std::sync::Arc;

use aiannotations_core::ai::{AiClient, AiError, ModelRole};
use aiannotations_core::db;
use aiannotations_core::model::Tool;
use aiannotations_core::spool;
use aiannotations_core::testing::*;
use aiannotations_services::worker::{Job, Schedule, TickReport, Worker};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value, json};

const MIN: i64 = 60_000;

/// Lets the test keep a handle on the `FakeAi` the worker owns.
struct Shared(Arc<FakeAi>);

impl AiClient for Shared {
    fn complete_json(&self, prompt: &str, role: ModelRole) -> Result<Map<String, Value>, AiError> {
        self.0.complete_json(prompt, role)
    }
}

fn worker_at(env: &Env, ai: &Arc<FakeAi>, tz: Tz) -> Worker {
    Worker::new(
        db::open(&env.paths.db_path).unwrap(),
        env.paths.clone(),
        env.settings.clone(),
        Box::new(Shared(ai.clone())),
        aiannotations_core::git::Git::default(),
        tz,
    )
}

fn worker(env: &Env, ai: &Arc<FakeAi>) -> Worker {
    worker_at(env, ai, Tz::UTC)
}

fn tick(w: &mut Worker, at: i64) -> TickReport {
    w.tick(&|| at).unwrap()
}

fn queue_start(env: &Env, sid: &str, at: i64) {
    let cwd = env.base.join("proj");
    let when = DateTime::<Utc>::from_timestamp_millis(at).unwrap();
    let payload = json!({"session_id": sid, "cwd": cwd, "source": "startup"});
    spool::write_envelope(
        &env.paths.spool_dir,
        "claude",
        "start",
        payload.to_string().as_bytes(),
        when,
    )
    .unwrap();
}

/// A closed run waiting for its summary.
fn pending_run(env: &Env, sid: &str) {
    let path = env.transcript("t", &[]);
    let session = insert_session(
        &env.conn,
        Tool::Claude,
        sid,
        &env.base.join("old"),
        Some(&path),
    );
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: ts("2026-10-20T12:00:00Z"),
            ended_at: Some(ts("2026-10-20T12:01:00Z")),
            ..Default::default()
        },
    );
}

fn no_ai() -> Arc<FakeAi> {
    Arc::new(FakeAi::new(vec![]))
}

#[test]
fn schedule_due_on_first_tick_runs_all_but_daily_not_yet() {
    let s = Schedule::default();
    let early = ts("2026-10-29T03:00:00Z");
    assert_eq!(
        s.due(&HashMap::new(), early, Tz::UTC),
        [Job::Ingest, Job::Collect, Job::Summarize, Job::Maintenance]
    );
    let noon = ts("2026-10-29T12:00:00Z");
    assert_eq!(
        s.due(&HashMap::new(), noon, Tz::UTC),
        [
            Job::Ingest,
            Job::Collect,
            Job::Summarize,
            Job::Briefing,
            Job::Maintenance
        ]
    );

    // Intervals: ingest/summarize every minute, collect every 15 minutes.
    let last: HashMap<Job, i64> = [Job::Ingest, Job::Collect, Job::Summarize, Job::Maintenance]
        .into_iter()
        .map(|j| (j, noon))
        .chain([(Job::Briefing, noon)])
        .collect();
    assert!(s.due(&last, noon + 59_000, Tz::UTC).is_empty());
    assert_eq!(
        s.due(&last, noon + MIN, Tz::UTC),
        [Job::Ingest, Job::Summarize]
    );
    assert_eq!(
        s.due(&last, noon + 15 * MIN, Tz::UTC),
        [Job::Ingest, Job::Collect, Job::Summarize]
    );
    assert_eq!(
        s.due(&last, noon + 24 * 60 * MIN, Tz::UTC).last(),
        Some(&Job::Maintenance)
    );
}

#[test]
fn briefing_due_after_six_local_once_per_day() {
    let s = Schedule::default();
    let tz: Tz = "America/Sao_Paulo".parse().unwrap();
    let due = |last: Option<i64>, now: i64| {
        let map: HashMap<Job, i64> = last.map(|l| (Job::Briefing, l)).into_iter().collect();
        s.due(&map, now, tz).contains(&Job::Briefing)
    };
    // 05:59 local (UTC-3) is too early, 06:00 is due.
    assert!(!due(None, ts("2026-10-29T08:59:00Z")));
    assert!(due(None, ts("2026-10-29T09:00:00Z")));
    // Opened late: still due. Already ran today: not due until the next local day's 06:00.
    assert!(due(None, ts("2026-10-29T22:00:00Z")));
    let ran = ts("2026-10-29T09:05:00Z");
    assert!(!due(Some(ran), ts("2026-10-29T22:00:00Z")));
    // 02:00Z on the 30th is still the 29th locally (23:00).
    assert!(!due(Some(ran), ts("2026-10-30T02:00:00Z")));
    assert!(!due(Some(ran), ts("2026-10-30T08:59:00Z")));
    assert!(due(Some(ran), ts("2026-10-30T09:00:00Z")));
}

#[test]
fn briefing_runs_once_per_day_and_again_the_next_day() {
    let env = Env::new();
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    let noon = ts("2026-10-29T12:00:00Z");

    let first = tick(&mut w, noon);
    assert!(first.ran.contains(&Job::Briefing));
    assert!(first.changed, "the briefing was generated");
    // Runs again neither one hour later nor in the evening.
    assert!(!tick(&mut w, noon + 60 * MIN).ran.contains(&Job::Briefing));
    assert!(
        !tick(&mut w, noon + 9 * 60 * MIN)
            .ran
            .contains(&Job::Briefing)
    );
    // Next day 12:00Z.
    assert!(
        tick(&mut w, noon + 24 * 60 * MIN)
            .ran
            .contains(&Job::Briefing)
    );
}

#[test]
fn second_worker_skips_while_lock_held() {
    let env = Env::new();
    let ai = no_ai();
    let mut first = worker(&env, &ai);
    let mut second = worker(&env, &ai);
    let t0 = ts("2026-10-29T03:00:00Z");

    assert!(!tick(&mut first, t0).skipped_locked);
    let skipped = tick(&mut second, t0 + MIN);
    assert!(skipped.skipped_locked);
    assert!(skipped.ran.is_empty() && !skipped.changed);
    assert!(
        second.status().last_errors.is_empty(),
        "skipping is not an error"
    );

    // The holder renews its own lock tick after tick.
    assert!(!tick(&mut first, t0 + 2 * MIN).skipped_locked);
    assert!(tick(&mut second, t0 + 3 * MIN).skipped_locked);

    // A worker that stops releases the lock; a crashed one lets it expire.
    drop(first);
    assert!(!tick(&mut second, t0 + 4 * MIN).skipped_locked);
    let mut third = worker(&env, &ai);
    assert!(tick(&mut third, t0 + 5 * MIN).skipped_locked);
    assert!(!tick(&mut third, t0 + 4 * MIN + 241_000).skipped_locked);
}

#[test]
fn summarize_pause_does_not_block_other_jobs() {
    let env = Env::new();
    pending_run(&env, "old");
    queue_start(&env, "new", ts("2026-10-29T11:59:00Z"));
    let ai = Arc::new(FakeAi::new(vec![Err(AiError::RateLimited("limit".into()))]));
    let mut w = worker(&env, &ai);
    let noon = ts("2026-10-29T12:00:00Z");

    let report = tick(&mut w, noon);
    assert!(report.ran.contains(&Job::Ingest) && report.ran.contains(&Job::Collect));
    assert!(report.ran.contains(&Job::Summarize));
    assert!(report.changed, "the event was ingested");
    assert_eq!(
        count(&env.conn, "ai_sessions"),
        2,
        "ingest wrote the new session"
    );

    let status = w.status();
    assert!(status.paused_until_ms.is_some_and(|until| until > noon));
    assert_eq!(status.pause_reason.as_deref(), Some("rate_limited"));
    assert!(status.last_errors.is_empty());
    // The briefing spends quota too: skipped while the AI is paused.
    assert!(!report.ran.contains(&Job::Briefing));

    // Still paused one minute later: nothing calls the AI, the status keeps the pause.
    let again = tick(&mut w, noon + MIN);
    assert!(again.ran.contains(&Job::Ingest));
    assert_eq!(ai.call_count(), 1);
    assert!(w.status().paused_until_ms.is_some());
    // Past the pause the status clears.
    let later = noon + 31 * MIN;
    ai.push(summary_ok("Fez X", "code"));
    ai.push(ai_ok(json!({"paragraph": "Dia bom", "next_steps": []})));
    let resumed = tick(&mut w, later);
    assert!(resumed.ran.contains(&Job::Briefing), "the briefing is back");
    assert_eq!(w.status().paused_until_ms, None);
    assert_eq!(w.status().pause_reason, None);
}

#[test]
fn user_pause_skips_summarize_and_briefing() {
    let env = Env::new();
    pending_run(&env, "old");
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    w.set_summaries_paused(true);
    let noon = ts("2026-10-29T12:00:00Z");

    let report = tick(&mut w, noon);
    assert_eq!(report.ran, [Job::Ingest, Job::Collect, Job::Maintenance]);
    assert_eq!(ai.call_count(), 0);
    assert!(w.status().summarizing_paused_by_user);

    // Resuming brings both back on the next tick.
    w.set_summaries_paused(false);
    ai.push(summary_ok("Fez X", "code"));
    let report = tick(&mut w, noon + MIN);
    assert!(report.ran.contains(&Job::Summarize) && report.ran.contains(&Job::Briefing));
    assert!(!w.status().summarizing_paused_by_user);
}

#[test]
fn job_error_is_recorded_and_others_run() {
    let env = Env::new();
    env.conn.execute_batch("DROP TABLE briefings").unwrap();
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    let noon = ts("2026-10-29T12:00:00Z");

    let report = tick(&mut w, noon);
    assert_eq!(
        report.ran,
        [
            Job::Ingest,
            Job::Collect,
            Job::Summarize,
            Job::Briefing,
            Job::Maintenance
        ]
    );
    let errors = w.status().last_errors;
    assert_eq!(errors.len(), 1);
    assert!(errors[0].starts_with("briefing: "), "{errors:?}");
    assert_eq!(w.status().last_tick_ms, Some(noon));

    // A failed briefing is retried every 30 minutes, not every tick.
    assert!(!tick(&mut w, noon + 29 * MIN).ran.contains(&Job::Briefing));
    assert!(tick(&mut w, noon + 31 * MIN).ran.contains(&Job::Briefing));
    assert_eq!(w.status().last_errors.len(), 2);
}

#[test]
fn only_the_last_ten_errors_are_kept() {
    let env = Env::new();
    env.conn.execute_batch("DROP TABLE briefings").unwrap();
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    let mut at = ts("2026-10-29T12:00:00Z");
    for _ in 0..12 {
        tick(&mut w, at);
        at += 31 * MIN;
    }
    assert_eq!(w.status().last_errors.len(), 10);
}

#[test]
fn changed_true_only_when_something_was_written() {
    let env = Env::new();
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    // Before 06:00 local: no briefing; an empty database has nothing to write.
    let t0 = ts("2026-10-29T03:00:00Z");

    let idle = tick(&mut w, t0);
    assert_eq!(
        idle.ran,
        [Job::Ingest, Job::Collect, Job::Summarize, Job::Maintenance]
    );
    assert!(!idle.changed);

    queue_start(&env, "s1", t0 + 30_000);
    let written = tick(&mut w, t0 + MIN);
    assert!(written.changed, "ingest recorded an event");

    let idle_again = tick(&mut w, t0 + 2 * MIN);
    assert!(idle_again.ran.contains(&Job::Ingest));
    assert!(!idle_again.changed);
}

#[test]
fn lock_is_kept_through_long_ai_jobs() {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    let env = Env::new();
    pending_run(&env, "a");
    pending_run(&env, "b");
    // Before 06:00 local: no briefing, only the two summary calls.
    let t0 = ts("2026-10-29T03:00:00Z");
    let clock_ms = Arc::new(AtomicI64::new(t0));
    let ai = Arc::new(FakeAi::new(vec![
        summary_ok("Fez A", "code"),
        summary_ok("Fez B", "code"),
    ]));
    let mut other = worker(&env, &ai);
    // If the lock were lost the intruder must not re-enter the AI (and the hook) and hang.
    other.set_summaries_paused(true);
    let intruder = Arc::new(Mutex::new(other));
    let skipped = Arc::new(Mutex::new(Vec::new()));
    {
        let (clock_ms, intruder, skipped) = (clock_ms.clone(), intruder.clone(), skipped.clone());
        ai.on_call(move |_| {
            // Each call takes 200 s: past the old 120 s TTL, and the second one past the
            // 240 s the first tick took the lock for.
            let at = clock_ms.fetch_add(200_000, Ordering::SeqCst) + 200_000;
            let report = intruder.lock().unwrap().tick(&|| at).unwrap();
            skipped.lock().unwrap().push(report.skipped_locked);
        });
    }
    let mut w = worker(&env, &ai);
    let c = clock_ms.clone();
    let report = w.tick(&move || c.load(Ordering::SeqCst)).unwrap();

    assert!(report.ran.contains(&Job::Summarize));
    assert_eq!(ai.call_count(), 2);
    assert_eq!(*skipped.lock().unwrap(), [true, true]);
}

#[test]
fn ingest_runs_between_slow_summaries() {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};

    let env = Env::new();
    for sid in ["a", "b", "c"] {
        pending_run(&env, sid);
    }
    // Before 06:00 local: no briefing, only the three summary calls.
    let t0 = ts("2026-10-29T03:00:00Z");
    let clock_ms = Arc::new(AtomicI64::new(t0));
    let ai = Arc::new(FakeAi::new(vec![
        summary_ok("Fez A", "code"),
        summary_ok("Fez B", "code"),
        summary_ok("Fez C", "code"),
    ]));
    // Sessions visible in the database at the start of each AI call.
    let seen = Arc::new(Mutex::new(Vec::new()));
    {
        let (clock_ms, seen) = (clock_ms.clone(), seen.clone());
        let (db_path, spool_dir, cwd) = (
            env.paths.db_path.clone(),
            env.paths.spool_dir.clone(),
            env.base.join("proj"),
        );
        ai.on_call(move |n| {
            let conn = db::open(&db_path).unwrap();
            seen.lock().unwrap().push(count(&conn, "ai_sessions"));
            // Each call takes 70 s, past the one-minute ingest interval.
            let at = clock_ms.fetch_add(70_000, Ordering::SeqCst) + 70_000;
            if n == 0 {
                let payload = json!({"session_id": "new", "cwd": cwd, "source": "startup"});
                let when = DateTime::<Utc>::from_timestamp_millis(at).unwrap();
                spool::write_envelope(
                    &spool_dir,
                    "claude",
                    "start",
                    payload.to_string().as_bytes(),
                    when,
                )
                .unwrap();
            }
        });
    }
    let mut w = worker(&env, &ai);
    let c = clock_ms.clone();
    let report = w.tick(&move || c.load(Ordering::SeqCst)).unwrap();

    assert_eq!(ai.call_count(), 3);
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen[2], seen[0] + 1, "ingested before the 3rd AI call");
    assert!(report.changed);
    assert!(report.ran.contains(&Job::Ingest) && report.ran.contains(&Job::Summarize));
    let done: i64 = env
        .conn
        .query_row(
            "SELECT COUNT(*) FROM session_runs WHERE summary_status = 'done'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(done, 3);
}

#[test]
fn collect_changed_only_when_the_data_changed() {
    let env = Env::new();
    let path = env
        .paths
        .claude_dir
        .join("projects/-home-fulano-Exemplo/0199aaaa-0000-7000-8000-000000000001.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, CLAUDE_FIXTURE).unwrap();
    let ai = no_ai();
    let mut w = worker(&env, &ai);
    // No AI calls, no briefing: the collector is the only job that can write.
    w.set_summaries_paused(true);
    let t0 = ts("2026-10-29T03:00:00Z");

    let first = tick(&mut w, t0);
    assert!(first.ran.contains(&Job::Collect));
    assert_eq!(
        count(&env.conn, "ai_sessions"),
        1,
        "the transcript was collected"
    );
    assert!(first.changed);

    let second = tick(&mut w, t0 + 16 * MIN);
    assert!(second.ran.contains(&Job::Collect));
    assert!(
        !second.changed,
        "the same transcript again writes nothing new"
    );
}

#[test]
fn tray_counts_on_the_worker_connection() {
    let env = Env::new();
    let ai = Arc::new(FakeAi::new(vec![]));
    let w = worker(&env, &ai);
    assert_eq!(w.tray_counts(now_ms()).unwrap(), (0, 0));
}

#[test]
fn reload_switches_ai_client() {
    let env = Env::new();
    pending_run(&env, "old");
    let a = no_ai();
    let b = Arc::new(FakeAi::new(vec![summary_ok("Fez X", "code")]));
    let mut w = worker(&env, &a);

    let mut settings = env.settings.clone();
    settings.ai_timeout_seconds += 1;
    w.set_settings(settings, Box::new(Shared(b.clone())));
    // Before 06:00 the briefing is not due: only the summary calls the AI.
    let report = tick(&mut w, ts("2026-10-29T05:00:00Z"));

    assert!(report.ran.contains(&Job::Summarize));
    assert_eq!(a.call_count(), 0, "the old client is gone");
    assert_eq!(b.call_count(), 1, "the next summary uses the new client");
}

/// Switching CLI or model must lift the AI pause: it was earned by the old setup.
#[test]
fn changing_provider_or_model_clears_the_ai_pause() {
    use aiannotations_core::summaries::PAUSE_KEY;
    let env = Env::new();
    pending_run(&env, "old");
    let ai = Arc::new(FakeAi::new(vec![Err(AiError::Unavailable(
        "Failed to authenticate".into(),
    ))]));
    let mut w = worker(&env, &ai);
    let noon = ts("2026-10-29T12:00:00Z");
    tick(&mut w, noon);
    assert!(w.status().paused_until_ms.is_some());
    assert!(w.status().pause_reason.unwrap().starts_with("login:"));

    // Unrelated change: the pause stays.
    let mut same = env.settings.clone();
    same.ai_timeout_seconds += 1;
    w.set_settings(same, Box::new(Shared(ai.clone())));
    assert!(w.status().paused_until_ms.is_some());

    let mut other = env.settings.clone();
    other.ai_provider = Tool::Codex;
    w.set_settings(other, Box::new(Shared(ai.clone())));
    let status = w.status();
    assert_eq!(status.paused_until_ms, None);
    assert_eq!(status.pause_reason, None);
    let key: Option<String> = env
        .conn
        .query_row(
            "SELECT value FROM app_state WHERE key = ?1",
            [PAUSE_KEY],
            |r| r.get(0),
        )
        .ok();
    assert_eq!(key, None);
}
