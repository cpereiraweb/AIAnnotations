//! Queries of the Projects, Project and Sessions screens, over a real database built with the
//! core's test builders.

use aiannotations_core::model::Tool;
use aiannotations_core::settings::Lang;
use aiannotations_core::testing::*;
use aiannotations_services::views::projects;
use aiannotations_services::views::runs::{self, HiddenLabel, RunKind, SessionFilter};
use rusqlite::{Connection, params};

const DAY: i64 = 86_400_000;

/// A session in project `cwd` with one run started at `started`.
fn add_run(env: &Env, tool: Tool, sid: &str, cwd: &std::path::Path, started: i64) -> (i64, i64) {
    let session = insert_session(&env.conn, tool, sid, cwd, None);
    let run = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: started,
            ..Default::default()
        },
    );
    (session, run)
}

fn project_dir(env: &Env, name: &str) -> std::path::PathBuf {
    let dir = env.base.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn exec(conn: &Connection, sql: &str, p: impl rusqlite::Params) {
    conn.execute(sql, p).unwrap();
}

#[test]
fn sessions_paginates_30_and_filters_by_tool_project_kind() {
    let env = Env::new();
    let (a, b) = (project_dir(&env, "a"), project_dir(&env, "b"));
    for i in 0..35 {
        let tool = if i % 2 == 0 {
            Tool::Claude
        } else {
            Tool::Codex
        };
        let cwd = if i < 20 { &a } else { &b };
        let (_, run) = add_run(&env, tool, &format!("s{i}"), cwd, 1_000 + i);
        // Runs 0..5 changed files; run 5 is unknown (NULL); the rest are talk-only.
        let changed: Option<i64> = match i {
            0..5 => Some(1),
            5 => None,
            _ => Some(0),
        };
        exec(
            &env.conn,
            "UPDATE session_runs SET changed_files = ?1 WHERE id = ?2",
            params![changed, run],
        );
    }
    let all = SessionFilter::default();
    let p1 = runs::sessions(&env.conn, &all, 1).unwrap();
    assert_eq!(
        (p1.items.len(), p1.page, p1.pages, p1.total),
        (30, 1, 2, 35)
    );
    let started: Vec<i64> = p1.items.iter().map(|c| c.started_at_ms).collect();
    assert!(started.windows(2).all(|w| w[0] > w[1]), "newest first");
    assert_eq!(runs::sessions(&env.conn, &all, 2).unwrap().items.len(), 5);

    let codex = SessionFilter {
        tool: Some(Tool::Codex),
        ..all
    };
    let r = runs::sessions(&env.conn, &codex, 1).unwrap();
    assert_eq!(r.total, 17);
    assert!(r.items.iter().all(|c| c.tool == Tool::Codex));

    let project_b = insert_project(&env.conn, &b.to_string_lossy());
    let r = runs::sessions(
        &env.conn,
        &SessionFilter {
            project_id: Some(project_b),
            ..all
        },
        1,
    )
    .unwrap();
    assert_eq!(r.total, 15);
    assert!(
        r.items
            .iter()
            .all(|c| c.project_id == project_b && c.project_name == "b")
    );

    let changed = runs::sessions(
        &env.conn,
        &SessionFilter {
            kind: Some(RunKind::Changed),
            ..all
        },
        1,
    )
    .unwrap();
    assert_eq!(changed.total, 5);
    let talk = runs::sessions(
        &env.conn,
        &SessionFilter {
            kind: Some(RunKind::Talk),
            ..all
        },
        1,
    )
    .unwrap();
    assert_eq!(talk.total, 30, "false and NULL both count as talk");

    let none = runs::sessions(
        &env.conn,
        &SessionFilter {
            tool: Some(Tool::Codex),
            kind: Some(RunKind::Changed),
            ..all
        },
        1,
    )
    .unwrap();
    assert_eq!(none.total, 2);
}

#[test]
fn sessions_ignore_hidden_and_empty_lists_one_page() {
    let env = Env::new();
    let empty = runs::sessions(&env.conn, &SessionFilter::default(), 1).unwrap();
    assert_eq!((empty.items.len(), empty.pages, empty.total), (0, 1, 0));

    let a = project_dir(&env, "a");
    let (s, _) = add_run(&env, Tool::Claude, "x", &a, 10);
    exec(
        &env.conn,
        "UPDATE ai_sessions SET origin = 'automation' WHERE id = ?1",
        [s],
    );
    add_run(&env, Tool::Claude, "y", &a, 20);
    let r = runs::sessions(&env.conn, &SessionFilter::default(), 1).unwrap();
    assert_eq!(r.total, 1);
}

#[test]
fn hidden_runs_only_with_flag_and_labelled() {
    let env = Env::new();
    let a = project_dir(&env, "a");
    let project = insert_project(&env.conn, &a.to_string_lossy());
    let (_, _) = add_run(&env, Tool::Claude, "visible", &a, 100);
    let kinds = [
        ("auto", "origin = 'automation'", HiddenLabel::Automation, 90),
        ("sub", "origin = 'subagent'", HiddenLabel::Subagent, 80),
        ("int", "origin = 'internal'", HiddenLabel::Internal, 70),
        ("emp", "is_empty = 1", HiddenLabel::Empty, 60),
        // Empty wins over the origin.
        (
            "empauto",
            "is_empty = 1, origin = 'automation'",
            HiddenLabel::Empty,
            50,
        ),
    ];
    for (sid, set, _, at) in &kinds {
        let (s, _) = add_run(&env, Tool::Claude, sid, &a, *at);
        exec(
            &env.conn,
            &format!("UPDATE ai_sessions SET {set} WHERE id = ?1"),
            [s],
        );
    }

    let shown = runs::project_runs(&env.conn, project, false, 1).unwrap();
    assert_eq!(shown.total, 1);
    assert_eq!(shown.items[0].hidden_label, None);

    let all = runs::project_runs(&env.conn, project, true, 1).unwrap();
    assert_eq!(all.total, 6);
    let labels: Vec<_> = all.items.iter().map(|c| c.hidden_label).collect();
    let mut expected = vec![None];
    expected.extend(kinds.iter().map(|k| Some(k.2)));
    assert_eq!(labels, expected);
}

#[test]
fn project_runs_paginate_by_20_and_card_fields() {
    let env = Env::new();
    let a = project_dir(&env, "a");
    let project = insert_project(&env.conn, &a.to_string_lossy());
    for i in 0..21 {
        add_run(&env, Tool::Codex, &format!("s{i}"), &a, 1_000 + i);
    }
    let (s, run) = add_run(&env, Tool::Claude, "rich", &a, 5_000);
    exec(
        &env.conn,
        "UPDATE ai_sessions SET title = 'T' WHERE id = ?1",
        [s],
    );
    exec(
        &env.conn,
        "UPDATE session_runs SET branch = 'main', summary = 'did', next_step = 'next', \
         summary_status = 'done', ended_at = 6000, end_reason = 'clear', ended_cleanly = 1, \
         changed_files = 1, files_touched = '[\"a.rs\",\"b.rs\"]' WHERE id = ?1",
        [run],
    );
    let page = runs::project_runs(&env.conn, project, false, 1).unwrap();
    assert_eq!((page.items.len(), page.pages, page.total), (20, 2, 22));
    let card = &page.items[0];
    assert_eq!(card.run_id, run);
    assert_eq!(card.session_id, s);
    assert_eq!(card.tool_session_id, "rich");
    assert_eq!(card.title.as_deref(), Some("T"));
    assert_eq!(card.branch.as_deref(), Some("main"));
    assert_eq!(card.summary.as_deref(), Some("did"));
    assert_eq!(card.next_step.as_deref(), Some("next"));
    assert_eq!(card.ended_at_ms, Some(6000));
    assert!(card.ended_cleanly);
    assert_eq!(card.changed_files, Some(true));
    assert_eq!(card.files_count, 2);
    assert_eq!(card.cwd.as_deref(), Some(&*a.to_string_lossy()));
    assert_eq!(card.root_session_id, s);
    assert_eq!(
        runs::project_runs(&env.conn, project, false, 2)
            .unwrap()
            .items
            .len(),
        2
    );
    let json = serde_json::to_value(card).unwrap();
    assert_eq!(json["end_reason"], "clear");
    assert_eq!(json["summary_status"], "done");
    assert_eq!(json["tool"], "claude");
}

#[test]
fn root_session_follows_previous_session_chain() {
    let env = Env::new();
    let a = project_dir(&env, "a");
    let project = insert_project(&env.conn, &a.to_string_lossy());
    let (s1, _) = add_run(&env, Tool::Claude, "c1", &a, 10);
    let (s2, _) = add_run(&env, Tool::Claude, "c2", &a, 20);
    let (s3, _) = add_run(&env, Tool::Claude, "c3", &a, 30);
    let (lone, _) = add_run(&env, Tool::Claude, "lone", &a, 40);
    exec(
        &env.conn,
        "UPDATE ai_sessions SET previous_session_id = ?1 WHERE id = ?2",
        [s1, s2],
    );
    exec(
        &env.conn,
        "UPDATE ai_sessions SET previous_session_id = ?1 WHERE id = ?2",
        [s2, s3],
    );

    let page = runs::project_runs(&env.conn, project, false, 1).unwrap();
    let root = |sid| {
        page.items
            .iter()
            .find(|c| c.session_id == sid)
            .unwrap()
            .root_session_id
    };
    assert_eq!(
        (root(s1), root(s2), root(s3), root(lone)),
        (s1, s1, s1, lone)
    );

    // The same through the global list, and a cycle must not loop forever.
    exec(
        &env.conn,
        "UPDATE ai_sessions SET previous_session_id = ?1 WHERE id = ?2",
        [s3, s1],
    );
    let page = runs::sessions(&env.conn, &SessionFilter::default(), 1).unwrap();
    assert_eq!(page.total, 4);
}

#[test]
fn projects_order_nulls_last_and_tools_last_14_days() {
    let env = Env::new();
    let now = now_ms();
    let (old, mid, never) = (
        project_dir(&env, "old"),
        project_dir(&env, "mid"),
        project_dir(&env, "never"),
    );
    for (dir, at) in [(&old, Some(1_000)), (&mid, Some(2_000)), (&never, None)] {
        let id = insert_project(&env.conn, &dir.to_string_lossy());
        exec(
            &env.conn,
            "UPDATE projects SET last_activity_at = ?1 WHERE id = ?2",
            params![at, id],
        );
    }
    // mid: Claude 3 days ago (changed), Codex 1 day ago (talk), Claude 15 days ago (outside).
    let (_, r) = add_run(&env, Tool::Claude, "m1", &mid, now - 3 * DAY);
    exec(
        &env.conn,
        "UPDATE session_runs SET changed_files = 1, branch = 'feat' WHERE id = ?1",
        [r],
    );
    add_run(&env, Tool::Codex, "m2", &mid, now - DAY);
    add_run(&env, Tool::Claude, "m3", &mid, now - 15 * DAY);
    // old: only a run 15 days ago.
    add_run(&env, Tool::Codex, "o1", &old, now - 15 * DAY);

    let list = projects::list(&env.conn, now).unwrap();
    let names: Vec<_> = list.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["mid", "old", "never"]);

    let mid = &list[0];
    assert_eq!(mid.tools_14d.len(), 2);
    let claude = mid
        .tools_14d
        .iter()
        .find(|t| t.tool == Tool::Claude)
        .unwrap();
    let codex = mid
        .tools_14d
        .iter()
        .find(|t| t.tool == Tool::Codex)
        .unwrap();
    assert!(claude.changed_files && !codex.changed_files);
    assert_eq!(mid.last_activity_ms, Some(2_000));
    let last = mid.last_run.as_ref().unwrap();
    assert_eq!(last.tool_session_id, "m2", "latest run by started_at");
    assert!(
        list[1].tools_14d.is_empty(),
        "the 15-day-old run is outside the window"
    );
    assert!(list[1].last_run.is_some(), "but it is still the last run");
    assert!(list[2].last_run.is_none());
    assert!(list[2].tools_14d.is_empty());
}

#[test]
fn last_run_ignores_hidden_sessions() {
    let env = Env::new();
    let a = project_dir(&env, "a");
    add_run(&env, Tool::Claude, "shown", &a, 100);
    let (s, _) = add_run(&env, Tool::Claude, "later-auto", &a, 200);
    exec(
        &env.conn,
        "UPDATE ai_sessions SET origin = 'automation' WHERE id = ?1",
        [s],
    );
    let list = projects::list(&env.conn, now_ms()).unwrap();
    assert_eq!(list[0].last_run.as_ref().unwrap().tool_session_id, "shown");
}

#[test]
fn project_card_counts_ongoings() {
    let env = Env::new();
    let a = project_dir(&env, "a");
    let id = insert_project(&env.conn, &a.to_string_lossy());
    let dir = a.join("docs/ongoing-sessions");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("um.md"), "# Em andamento — Primeiro\n").unwrap();
    std::fs::write(dir.join("dois.md"), "# Segundo\n").unwrap();
    std::fs::write(dir.join("ignorado.txt"), "x").unwrap();
    project_dir(&env, "b");
    insert_project(&env.conn, &env.base.join("b").to_string_lossy());

    let list = projects::list(&env.conn, now_ms()).unwrap();
    let card = list.iter().find(|p| p.id == id).unwrap();
    assert_eq!(card.ongoing_count, 2);
    assert_eq!(list.iter().find(|p| p.id != id).unwrap().ongoing_count, 0);

    let detail = projects::detail(&env.conn, id, now_ms()).unwrap().unwrap();
    assert_eq!(detail.card, *card);
    let mut titles: Vec<_> = detail.ongoings.iter().map(|o| o.title.as_str()).collect();
    titles.sort();
    assert_eq!(titles, ["Primeiro", "Segundo"]);
    assert!(
        projects::detail(&env.conn, id + 100, now_ms())
            .unwrap()
            .is_none()
    );
}

#[test]
fn empty_database_yields_empty_screens() {
    let env = Env::new();
    assert!(projects::list(&env.conn, now_ms()).unwrap().is_empty());
    let page = runs::sessions(&env.conn, &SessionFilter::default(), 3).unwrap();
    assert_eq!(
        (page.items.len(), page.page, page.pages, page.total),
        (0, 3, 1, 0)
    );
}

#[test]
fn core_enums_serialize_snake_case() {
    use aiannotations_core::briefing::BriefingOutcome;
    use aiannotations_core::inbox::{Badge, CountKey, ItemKind};
    use aiannotations_core::summaries::SummaryOutcome;
    let js = |v: &dyn Erased| v.json();
    assert_eq!(js(&Badge::HalfDone), "half_done");
    assert_eq!(js(&Badge::UncommittedChanges), "uncommitted_changes");
    assert_eq!(js(&ItemKind::StaleBranch), "stale_branch");
    assert_eq!(js(&CountKey::HalfDone), "half_done");
    assert_eq!(js(&SummaryOutcome::Rescheduled), "rescheduled");
    assert_eq!(js(&BriefingOutcome::AlreadyDone), "already_done");
    assert_eq!(js(&Lang::PtBr), "pt-BR");
    assert_eq!(js(&Lang::En), "en");
    assert_eq!(js(&HiddenLabel::Automation), "automation");
}

trait Erased {
    fn json(&self) -> String;
}
impl<T: serde::Serialize> Erased for T {
    fn json(&self) -> String {
        serde_json::to_value(self)
            .unwrap()
            .as_str()
            .unwrap()
            .to_string()
    }
}
