//! The id-based actions behind the app commands: the UI only knows run and project ids.

use aiannotations_core::inbox::{self, GitCache};
use aiannotations_core::model::Tool;
use aiannotations_core::testing::*;
use aiannotations_services::actions::{self, ShellKind};
use rusqlite::params;

const DAY: i64 = 86_400_000;

fn run_in(env: &Env, tool: Tool, sid: &str, cwd: &std::path::Path) -> i64 {
    let session = insert_session(&env.conn, tool, sid, cwd, None);
    insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms() - DAY,
            ..Default::default()
        },
    )
}

#[test]
fn resume_command_for_run_uses_session_tool_id_and_cwd() {
    let env = Env::new();
    let cwd = env.base.join("meu projeto");
    std::fs::create_dir_all(&cwd).unwrap();
    let run = run_in(&env, Tool::Codex, "abc-1", &cwd);
    let cmd = actions::resume_command_for_run(&env.conn, run, ShellKind::Posix).unwrap();
    assert_eq!(cmd, format!("cd '{}' && codex resume abc-1", cwd.display()));
}

#[test]
fn resume_command_for_run_rejects_unknown_run_or_missing_cwd() {
    let env = Env::new();
    assert!(actions::resume_command_for_run(&env.conn, 999, ShellKind::Posix).is_err());
    let run = run_in(&env, Tool::Claude, "s", &env.base.join("p"));
    env.conn
        .execute("UPDATE ai_sessions SET cwd = ''", [])
        .unwrap();
    assert!(actions::resume_command_for_run(&env.conn, run, ShellKind::Posix).is_err());
}

#[test]
fn run_and_project_folders() {
    let env = Env::new();
    let cwd = env.base.join("proj");
    std::fs::create_dir_all(&cwd).unwrap();
    let run = run_in(&env, Tool::Claude, "s", &cwd);
    let project: i64 = env
        .conn
        .query_row("SELECT project_id FROM ai_sessions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(actions::run_folder(&env.conn, run).unwrap(), cwd);
    assert_eq!(actions::project_folder(&env.conn, project).unwrap(), cwd);
    assert!(actions::run_folder(&env.conn, 999).is_err());
    assert!(actions::project_folder(&env.conn, 999).is_err());

    // A session without cwd and a project without path: an error, never "" (the current dir).
    env.conn
        .execute("UPDATE ai_sessions SET cwd = ''", [])
        .unwrap();
    env.conn
        .execute("UPDATE projects SET path = ''", [])
        .unwrap();
    assert!(actions::run_folder(&env.conn, run).is_err());
    assert!(actions::project_folder(&env.conn, project).is_err());
}

#[test]
fn dismiss_project_hides_only_that_projects_items() {
    let mut env = Env::new();
    let mut projects = Vec::new();
    for name in ["a", "b"] {
        let cwd = env.base.join(name);
        std::fs::create_dir_all(&cwd).unwrap();
        for sid in ["x", "y"] {
            let session =
                insert_session(&env.conn, Tool::Claude, &format!("{name}{sid}"), &cwd, None);
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
            env.conn
                .execute(
                    "UPDATE session_runs SET branch = ?1 WHERE id = ?2",
                    params![sid, run],
                )
                .unwrap();
        }
        projects.push(
            env.conn
                .query_row(
                    "SELECT project_id FROM ai_sessions WHERE session_id = ?1",
                    [format!("{name}x")],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap(),
        );
    }
    env.conn
        .execute("UPDATE projects SET last_activity_at = ?1", [now_ms()])
        .unwrap();
    let cache = GitCache::new();
    let items =
        |env: &Env| inbox::build(&env.conn, &env.git, &cache, &env.settings, now_ms()).unwrap();
    assert_eq!(items(&env).len(), 4);

    let n = actions::dismiss_project(
        &mut env.conn,
        &env.git,
        &cache,
        &env.settings,
        projects[0],
        false,
        now_ms(),
    )
    .unwrap();
    assert_eq!(n, 2);
    let rest = items(&env);
    assert_eq!(rest.len(), 2);
    assert!(rest.iter().all(|i| i.project_id == projects[1]));
}

/// `docs/ongoing-sessions/<name>` in `dir`, modified `age_days` ago.
fn ongoing_doc(dir: &std::path::Path, name: &str, age_days: i64) {
    let docs = dir.join("docs/ongoing-sessions");
    std::fs::create_dir_all(&docs).unwrap();
    let path = docs.join(name);
    std::fs::write(&path, "# Doc\n").unwrap();
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis((now_ms() - age_days * DAY) as u64);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

#[test]
fn dismiss_project_only_touches_its_section() {
    for older in [false, true] {
        let mut env = Env::new();
        let cwd = env.base.join("proj");
        ongoing_doc(&cwd, "recent-doc.md", 2);
        ongoing_doc(&cwd, "older-doc.md", 45);
        insert_session(&env.conn, Tool::Claude, "s", &cwd, None);
        env.conn
            .execute("UPDATE projects SET last_activity_at = ?1", [now_ms()])
            .unwrap();
        let project: i64 = env
            .conn
            .query_row("SELECT id FROM projects", [], |r| r.get(0))
            .unwrap();
        let cache = GitCache::new();
        let titles = |env: &Env| -> Vec<String> {
            let mut t: Vec<String> =
                inbox::build(&env.conn, &env.git, &cache, &env.settings, now_ms())
                    .unwrap()
                    .into_iter()
                    .map(|i| i.title)
                    .collect();
            t.sort();
            t
        };
        assert_eq!(titles(&env), ["older-doc", "recent-doc"]);

        let n = actions::dismiss_project(
            &mut env.conn,
            &env.git,
            &cache,
            &env.settings,
            project,
            older,
            now_ms(),
        )
        .unwrap();
        assert_eq!(n, 1);
        let left = if older { "recent-doc" } else { "older-doc" };
        assert_eq!(titles(&env), [left], "older = {older}");
    }
}

#[test]
fn regenerate_briefing_forces_a_new_generation() {
    use aiannotations_core::briefing::{self, BriefingOutcome};
    use aiannotations_core::db::app_state;
    use aiannotations_core::model::BriefingStatus;
    use chrono_tz::Tz;

    let mut env = Env::new();
    let ai = FakeAi::new(vec![]);
    let cache = GitCache::new();
    let at = now_ms();
    let regenerate = |env: &mut Env| {
        actions::regenerate_briefing(
            &mut env.conn,
            &ai,
            &env.git,
            &cache,
            &env.settings,
            Tz::UTC,
            &|| at,
        )
        .unwrap()
    };
    assert_eq!(regenerate(&mut env), BriefingOutcome::Generated);
    // Already done today: "regerar" generates it again.
    assert_eq!(regenerate(&mut env), BriefingOutcome::Generated);
    let today = briefing::today(&env.conn, Tz::UTC, at).unwrap().unwrap();
    assert_eq!(today.status, BriefingStatus::Done);

    // Another generation holds the lock.
    let tx = aiannotations_core::db::write_tx(&mut env.conn).unwrap();
    assert!(app_state::try_lock(&tx, briefing::LOCK_KEY, 60_000, at).unwrap());
    tx.commit().unwrap();
    assert_eq!(regenerate(&mut env), BriefingOutcome::AlreadyRunning);
}

#[test]
fn generate_briefing_is_not_forced() {
    use aiannotations_core::briefing::BriefingOutcome;
    use chrono_tz::Tz;

    let mut env = Env::new();
    let ai = FakeAi::new(vec![]);
    let cache = GitCache::new();
    let at = now_ms();
    let generate = |env: &mut Env| {
        actions::generate_briefing(
            &mut env.conn,
            &ai,
            &env.git,
            &cache,
            &env.settings,
            Tz::UTC,
            &|| at,
        )
        .unwrap()
    };
    assert_eq!(generate(&mut env), BriefingOutcome::Generated);
    // Opening Today again after it is done generates nothing.
    assert_eq!(generate(&mut env), BriefingOutcome::AlreadyDone);
}
