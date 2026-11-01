//! Today screen query, search and data actions, over a real database built with the core's test
//! builders.

use std::path::Path;

use aiannotations_core::inbox::GitCache;
use aiannotations_core::model::{BriefingStatus, Tool};
use aiannotations_core::testing::*;
use aiannotations_services::views::{self, today};
use aiannotations_services::{actions, views::today::TodayView};
use chrono_tz::Tz;
use rusqlite::params;

const DAY: i64 = 86_400_000;

fn load(env: &mut Env) -> TodayView {
    today::load(
        &mut env.conn,
        &env.git,
        &GitCache::new(),
        &env.settings,
        Tz::UTC,
        now_ms(),
    )
    .unwrap()
}

/// A project directory with `docs/ongoing-sessions/<file>.md` modified `age_days` ago.
fn ongoing(env: &Env, project: &str, file: &str, age_days: i64) -> std::path::PathBuf {
    let dir = env.base.join(project);
    let docs = dir.join("docs/ongoing-sessions");
    std::fs::create_dir_all(&docs).unwrap();
    let path = docs.join(file);
    std::fs::write(&path, format!("# Em andamento — {file}\n")).unwrap();
    let at = std::time::SystemTime::UNIX_EPOCH
        + std::time::Duration::from_millis((now_ms() - age_days * DAY) as u64);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
    dir
}

/// A session whose project counts as active (the inbox only looks at active projects).
fn add_session(env: &Env, sid: &str, cwd: &Path) -> i64 {
    let session = insert_session(&env.conn, Tool::Claude, sid, cwd, None);
    env.conn
        .execute("UPDATE projects SET last_activity_at = ?1", [now_ms()])
        .unwrap();
    session
}

#[test]
fn today_on_empty_db() {
    let mut env = Env::new();
    let view = load(&mut env);
    assert!(view.is_empty_db);
    assert!(view.inbox.recent.is_empty() && view.inbox.older.is_empty());
    assert!(view.inbox_last_runs.is_empty());
    assert!(view.ongoings_recent.is_empty() && view.ongoings_stale.is_empty());
    assert!(view.backfill.is_none());
    assert!(!view.briefing_running);
    let briefing = view.briefing.as_ref().expect("today's record is created");
    assert_eq!(briefing.status, BriefingStatus::Generating);
    assert!(view.next_steps.is_empty());
    serde_json::to_string(&view).unwrap();
}

#[test]
fn ongoings_split_at_30_days() {
    let mut env = Env::new();
    for (name, file, age) in [
        ("a", "fresh.md", 1),
        ("a", "edge.md", 29),
        ("b", "old.md", 31),
    ] {
        let dir = ongoing(&env, name, file, age);
        add_session(&env, &format!("{name}{file}"), &dir);
    }
    let view = load(&mut env);
    assert!(!view.is_empty_db);
    assert_eq!(view.ongoings_recent.len(), 1);
    let group = &view.ongoings_recent[0];
    assert_eq!(group.project_name, "a");
    let files: Vec<_> = group.items.iter().map(|o| o.file.as_str()).collect();
    assert_eq!(files, ["fresh.md", "edge.md"]);
    assert_eq!(view.ongoings_stale.len(), 1);
    assert_eq!(view.ongoings_stale[0].project_name, "b");
    assert_eq!(view.ongoings_stale[0].items[0].file, "old.md");
}

#[test]
fn next_steps_only_when_done() {
    let mut env = Env::new();
    let project = add_session(&env, "s", &env.base.clone());
    let _ = project;
    let steps = r#"[{"project_id":1,"projeto":"x","passo":"Do it"}]"#;
    let first = load(&mut env);
    assert!(first.next_steps.is_empty());
    env.conn
        .execute("UPDATE briefings SET next_steps = ?1", [steps])
        .unwrap();
    assert!(load(&mut env).next_steps.is_empty(), "still generating");
    env.conn
        .execute("UPDATE briefings SET status = 'done', body = 'b'", [])
        .unwrap();
    let view = load(&mut env);
    assert_eq!(view.next_steps.len(), 1);
    assert_eq!(view.next_steps[0].step, "Do it");
}

#[test]
fn inbox_last_runs_has_summary_of_session_items() {
    let mut env = Env::new();
    let cwd = env.base.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let session = add_session(&env, "s1", &cwd);
    let end = now_ms() - 2 * DAY;
    for (start, summary) in [(end - 3 * DAY, "older"), (end - DAY, "latest")] {
        let run = insert_run(
            &env.conn,
            session,
            RunSeed {
                started_at: start,
                ended_at: Some(end),
                end_reason: Some("other"),
                ended_cleanly: Some(false),
                ..Default::default()
            },
        );
        env.conn
            .execute(
                "UPDATE session_runs SET summary = ?1, summary_status = 'done', \
                 changed_files = 1 WHERE id = ?2",
                params![summary, run],
            )
            .unwrap();
    }
    let view = load(&mut env);
    let item = view
        .inbox
        .recent
        .iter()
        .chain(&view.inbox.older)
        .flat_map(|g| &g.items)
        .find(|i| i.session_id == Some(session))
        .expect("interrupted session in the inbox");
    let card = &view.inbox_last_runs[&item.session_id.unwrap()];
    assert_eq!(card.summary.as_deref(), Some("latest"));
    assert_eq!(view.inbox_last_runs.len(), 1);
}

#[test]
fn search_limits_12() {
    let mut env = Env::new();
    let cwd = env.base.join("p");
    std::fs::create_dir_all(&cwd).unwrap();
    for i in 0..15 {
        let session = add_session(&env, &format!("s{i}"), &cwd);
        let run = insert_run(
            &env.conn,
            session,
            RunSeed {
                started_at: now_ms() - i,
                ..Default::default()
            },
        );
        env.conn
            .execute(
                "UPDATE session_runs SET summary = 'refatoracao do modulo', \
                 summary_status = 'done' WHERE id = ?1",
                [run],
            )
            .unwrap();
    }
    aiannotations_core::search::rebuild(&mut env.conn).unwrap();
    let hits = views::search(&env.conn, "refatoracao").unwrap();
    assert_eq!(hits.len(), 12);
    assert!(views::search(&env.conn, "   ").unwrap().is_empty());
}

#[test]
fn dismiss_and_dismiss_group_hide_items() {
    let mut env = Env::new();
    let cwd = env.base.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    for sid in ["a", "b"] {
        let session = add_session(&env, sid, &cwd);
        let run = insert_run(
            &env.conn,
            session,
            RunSeed {
                started_at: now_ms() - 2 * DAY,
                ended_at: Some(now_ms() - 2 * DAY),
                end_reason: Some("other"),
                ended_cleanly: Some(false),
                ..Default::default()
            },
        );
        // One front per branch: each session is its own interrupted item.
        env.conn
            .execute(
                "UPDATE session_runs SET branch = ?1 WHERE id = ?2",
                params![sid, run],
            )
            .unwrap();
    }
    let items = |env: &mut Env| -> Vec<_> {
        let v = load(env);
        v.inbox
            .recent
            .into_iter()
            .chain(v.inbox.older)
            .flat_map(|g| g.items)
            .collect()
    };
    let all = items(&mut env);
    assert_eq!(all.len(), 2);
    actions::dismiss(&mut env.conn, &all[0].key, &all[0].state_hash, now_ms()).unwrap();
    let rest = items(&mut env);
    assert_eq!(rest.len(), 1);
    actions::dismiss_group(&mut env.conn, &rest, now_ms()).unwrap();
    assert!(items(&mut env).is_empty());
}

#[test]
fn import_history_runs_backfill() {
    let mut env = Env::new();
    let path = env
        .paths
        .claude_dir
        .join("projects/-home-fulano-Exemplo/0199aaaa-0000-7000-8000-000000000001.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, CLAUDE_FIXTURE).unwrap();
    assert!(load(&mut env).is_empty_db);

    let mut calls = Vec::new();
    let report = actions::import_history(
        &mut env.conn,
        &env.paths,
        &env.settings,
        &env.git,
        now_ms(),
        &mut |done, total| calls.push((done, total)),
    )
    .unwrap();
    assert_eq!(report.seen, 1);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(calls.last(), Some(&(1, 1)));
    let backfilled: i64 = env
        .conn
        .query_row(
            "SELECT COUNT(*) FROM session_runs WHERE is_backfill = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(backfilled, 1);
    let view = load(&mut env);
    assert!(!view.is_empty_db);
    assert!(view.backfill.is_some(), "pending runs report progress");
}

#[test]
fn should_start_briefing_only_when_generating_and_idle() {
    use aiannotations_core::briefing;
    use aiannotations_core::db::app_state;

    let mut env = Env::new();
    // Today's record was just created as `generating` and nothing holds the lock (v1 mount).
    let view = load(&mut env);
    assert!(today::should_start_briefing(&view));

    // A generation holds the lock: the screen waits for it.
    let tx = aiannotations_core::db::write_tx(&mut env.conn).unwrap();
    assert!(app_state::try_lock(&tx, briefing::LOCK_KEY, 60_000, now_ms()).unwrap());
    tx.commit().unwrap();
    let view = load(&mut env);
    assert!(view.briefing_running);
    assert!(!today::should_start_briefing(&view));
    let tx = aiannotations_core::db::write_tx(&mut env.conn).unwrap();
    app_state::unlock(&tx, briefing::LOCK_KEY).unwrap();
    tx.commit().unwrap();

    // Done or failed: only "regerar" generates again.
    for status in ["done", "failed"] {
        env.conn
            .execute("UPDATE briefings SET status = ?1", [status])
            .unwrap();
        assert!(!today::should_start_briefing(&load(&mut env)), "{status}");
    }
}

#[test]
fn paused_summaries_or_a_failed_start_do_not_start_the_briefing() {
    let mut env = Env::new();
    let view = load(&mut env);
    assert!(today::should_start_briefing(&view));

    // "Pausar resumos" in the tray: opening the screen must not spend quota.
    let mut paused = load(&mut env);
    paused.summaries_paused = true;
    assert!(!today::should_start_briefing(&paused));

    // The automatic start already failed before `briefing::run` (the record stays
    // `generating`): starting again on every "changed" would loop.
    let mut failed = load(&mut env);
    failed.briefing_start_failed = true;
    assert!(!today::should_start_briefing(&failed));
}

#[test]
fn tray_counts_match_the_screen() {
    let mut env = Env::new();
    // In progress: only the ongoings of the last 30 days count.
    for (file, age) in [("fresh.md", 1), ("old.md", 31)] {
        let dir = ongoing(&env, "a", file, age);
        add_session(&env, &format!("a{file}"), &dir);
    }
    // Need you: an interrupted session ended yesterday.
    let cwd = env.base.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let session = add_session(&env, "s1", &cwd);
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms() - 2 * DAY,
            ended_at: Some(now_ms() - DAY),
            end_reason: Some("other"),
            ended_cleanly: Some(false),
            ..Default::default()
        },
    );

    let counts = today::tray_counts(
        &env.conn,
        &env.git,
        &GitCache::new(),
        &env.settings,
        now_ms(),
    )
    .unwrap();
    let view = load(&mut env);
    let need: usize = view.inbox.recent.iter().map(|g| g.items.len()).sum();
    assert!(need >= 1, "the interrupted session needs you");
    assert_eq!(counts, (need, 1));
    assert_eq!(view.tray_counts(), counts);
}

#[test]
fn backfill_progress_hidden_when_history_summaries_declined() {
    let mut env = Env::new();
    let path = env
        .paths
        .claude_dir
        .join("projects/-home-fulano-Exemplo/0199aaaa-0000-7000-8000-000000000001.jsonl");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, CLAUDE_FIXTURE).unwrap();
    actions::import_history(
        &mut env.conn,
        &env.paths,
        &env.settings,
        &env.git,
        now_ms(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(load(&mut env).backfill.is_some());
    env.settings.summarize_history = false;
    assert!(load(&mut env).backfill.is_none());
}

#[test]
fn today_flags_login_first() {
    let env = Env::new();
    env.conn
        .execute(
            "INSERT INTO app_state (key, value) VALUES (?1, 'boom')",
            [aiannotations_core::briefing::LAST_ERROR_KEY],
        )
        .unwrap();
    let got = |reason: Option<&str>| {
        today::ai_attention(&env.conn, &env.settings, reason, now_ms()).unwrap()
    };
    assert_eq!(
        got(Some("login:claude: Not logged in")),
        Some(today::AiAttention::Login { tool: Tool::Claude })
    );
    // The CLI that failed, not the provider selected now.
    assert_eq!(
        got(Some("login:codex: Not logged in")),
        Some(today::AiAttention::Login { tool: Tool::Codex })
    );
    assert_eq!(
        got(Some("unavailable: command not found")),
        Some(today::AiAttention::Unavailable {
            detail: "command not found".into()
        })
    );
    assert_eq!(
        got(None),
        Some(today::AiAttention::BriefingFailed {
            detail: "boom".into()
        })
    );
    assert_eq!(got(Some("rate_limited")), got(None));
}

#[test]
fn today_has_no_attention_by_default() {
    let env = Env::new();
    assert_eq!(
        today::ai_attention(&env.conn, &env.settings, None, now_ms()).unwrap(),
        None
    );
}
