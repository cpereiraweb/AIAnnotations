//! `search_index` maintenance (Inv §9, port of `SessionSearch::index`/`reindex`): one row per
//! run with project name, branch, session title, summary, next step and `compact` (all of them
//! joined by spaces, hyphens removed).

mod support;

use aiannotations_core::model::Tool;
use aiannotations_core::{db, search};
use rusqlite::{Connection, params};
use std::path::Path;
use support::*;

/// Run on project "Fiscal-BR" (session titled "Emissão de notas"), branch `feat/nfs-e`.
fn seed(conn: &Connection) -> i64 {
    let session = insert_session(
        conn,
        Tool::Claude,
        "s1",
        Path::new("/home/u/Code/Fiscal-BR"),
        None,
    );
    conn.execute(
        "UPDATE ai_sessions SET title = 'Emissão de notas' WHERE id = ?1",
        [session],
    )
    .unwrap();
    let run = insert_run(
        conn,
        session,
        RunSeed {
            started_at: now_ms(),
            ..Default::default()
        },
    );
    conn.execute(
        "UPDATE session_runs SET branch = 'feat/nfs-e', summary = 'Emitir NFS-e', \
         next_step = 'Testar' WHERE id = ?1",
        [run],
    )
    .unwrap();
    run
}

fn index(conn: &mut Connection, run: i64) {
    let tx = db::write_tx(conn).unwrap();
    search::index_run(&tx, run).unwrap();
    tx.commit().unwrap();
}

fn hits(conn: &Connection, matching: &str) -> Vec<i64> {
    conn.prepare("SELECT run_id FROM search_index WHERE search_index MATCH ?1 ORDER BY run_id")
        .unwrap()
        .query_map([matching], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

type Row = (i64, String, String, String, String, String, String);

fn rows(conn: &Connection) -> Vec<Row> {
    conn.prepare(
        "SELECT run_id, project, branch, title, summary, next_step, compact FROM search_index \
         ORDER BY run_id",
    )
    .unwrap()
    .query_map([], |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
        ))
    })
    .unwrap()
    .collect::<rusqlite::Result<_>>()
    .unwrap()
}

#[test]
fn index_run_stores_fields_and_compact() {
    let mut env = Env::new();
    let run = seed(&env.conn);
    index(&mut env.conn, run);

    assert_eq!(
        rows(&env.conn),
        vec![(
            run,
            "Fiscal-BR".into(),
            "feat/nfs-e".into(),
            "Emissão de notas".into(),
            "Emitir NFS-e".into(),
            "Testar".into(),
            "FiscalBR feat/nfse Emissão de notas Emitir NFSe Testar".into(),
        )]
    );
}

/// "nfse" acha "NFS-e" (deferred minor from Task 2): through `compact`, by prefix, without
/// accents.
#[test]
fn nfse_matches_nfs_e_through_compact() {
    let mut env = Env::new();
    let run = seed(&env.conn);
    index(&mut env.conn, run);

    assert_eq!(hits(&env.conn, "compact : \"nfse\"*"), vec![run]);
    // Without `compact` the hyphenated term is split into "nfs" and "e": no "nfse" token.
    assert_eq!(hits(&env.conn, "summary : \"nfse\"*"), Vec::<i64>::new());
    assert_eq!(hits(&env.conn, "\"emissao\"*"), vec![run]);
}

#[test]
fn index_run_replaces_the_previous_row() {
    let mut env = Env::new();
    let run = seed(&env.conn);
    index(&mut env.conn, run);
    env.conn
        .execute(
            "UPDATE session_runs SET summary = 'Outro resumo' WHERE id = ?1",
            [run],
        )
        .unwrap();
    index(&mut env.conn, run);

    let all = rows(&env.conn);
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].4, "Outro resumo");
}

/// A run without branch/summary/next step/title indexes empty strings (PHP `(string) null`).
#[test]
fn index_run_with_null_fields_uses_empty_strings() {
    let mut env = Env::new();
    let session = insert_session(&env.conn, Tool::Codex, "s2", Path::new("/p/Alpha"), None);
    let run = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms(),
            ..Default::default()
        },
    );
    index(&mut env.conn, run);

    assert_eq!(
        rows(&env.conn),
        vec![(
            run,
            "Alpha".into(),
            "".into(),
            "".into(),
            "".into(),
            "".into(),
            "Alpha    ".into()
        )]
    );
}

/// Indexing a run that no longer exists only drops its row (`SessionSearch::forget`).
#[test]
fn index_run_of_a_missing_run_removes_its_row() {
    let mut env = Env::new();
    let run = seed(&env.conn);
    index(&mut env.conn, run);
    env.conn
        .execute("DELETE FROM session_runs WHERE id = ?1", [run])
        .unwrap();
    index(&mut env.conn, run);

    assert_eq!(count(&env.conn, "search_index"), 0);
}

/// `aiannotations:reindex`: rebuilds every run's row, drops orphans, returns the total.
#[test]
fn rebuild_reindexes_every_run_and_drops_orphans() {
    let mut env = Env::new();
    let a = seed(&env.conn);
    let session: i64 = env
        .conn
        .query_row("SELECT ai_session_id FROM session_runs", [], |r| r.get(0))
        .unwrap();
    let b = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms() + 1,
            ..Default::default()
        },
    );
    env.conn
        .execute(
            "INSERT INTO search_index (run_id, project, branch, title, summary, next_step, \
             compact) VALUES (?1, 'x', '', '', 'órfão', '', 'x')",
            params![999],
        )
        .unwrap();

    assert_eq!(search::rebuild(&mut env.conn).unwrap(), 2);
    let ids: Vec<i64> = rows(&env.conn).into_iter().map(|r| r.0).collect();
    assert_eq!(ids, vec![a, b]);
    assert_eq!(hits(&env.conn, "compact : \"nfse\"*"), vec![a]);
}

fn found(conn: &Connection, text: &str) -> Vec<i64> {
    search::query(conn, text, 20)
        .unwrap()
        .into_iter()
        .map(|h| h.run_id)
        .collect()
}

/// `SessionSearch::toMatchExpression`: one quoted prefix phrase per term, plus the same term
/// without hyphens on `compact`; terms without letters or digits are dropped.
#[test]
fn match_expression_quotes_terms_and_adds_the_compact_alternative() {
    assert_eq!(
        search::match_expression("nfse"),
        Some(r#"("nfse"* OR compact : "nfse"*)"#.to_string())
    );
    assert_eq!(
        search::match_expression("NFS-e foo"),
        Some(r#"("NFS-e"* OR compact : "NFSe"*) AND ("foo"* OR compact : "foo"*)"#.to_string())
    );
    assert_eq!(search::match_expression(r#"" -- !!"#), None);
    assert_eq!(search::match_expression(""), None);
    assert_eq!(
        search::match_expression("a\"b"),
        Some(r#"("ab"* OR compact : "ab"*)"#.to_string())
    );
    assert_eq!(
        search::match_expression("- foo"),
        Some(r#"("foo"* OR compact : "foo"*)"#.to_string())
    );
}

#[test]
fn new_run_is_indexed_by_trigger() {
    let env = Env::new();
    let session = insert_session(&env.conn, Tool::Claude, "s1", Path::new("/p/Alpha"), None);
    let run = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms(),
            ..Default::default()
        },
    );
    env.conn
        .execute(
            "UPDATE session_runs SET branch = 'branchname' WHERE id = ?1",
            [run],
        )
        .unwrap();

    let hits = search::query(&env.conn, "branchname", 20).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].run_id, run);
    assert_eq!(hits[0].ai_session_id, session);
    assert_eq!(hits[0].project_name, "Alpha");
    assert_eq!(hits[0].tool, Tool::Claude);
    assert_eq!(hits[0].branch.as_deref(), Some("branchname"));
    assert_eq!(hits[0].started_at_ms, now_ms());
    assert_eq!(
        hits[0].project_id,
        env.conn
            .query_row("SELECT id FROM projects", [], |r| r.get::<_, i64>(0))
            .unwrap()
    );
}

#[test]
fn insert_alone_is_indexed_without_index_run() {
    let env = Env::new();
    let session = insert_session(&env.conn, Tool::Codex, "s1", Path::new("/p/Alpha"), None);
    let run = insert_run(
        &env.conn,
        session,
        RunSeed {
            started_at: now_ms(),
            ..Default::default()
        },
    );
    assert_eq!(found(&env.conn, "alpha"), vec![run]);
}

#[test]
fn updated_summary_is_reindexed() {
    let env = Env::new();
    let run = seed(&env.conn);
    env.conn
        .execute(
            "UPDATE session_runs SET summary = 'Emissão de NFS-e' WHERE id = ?1",
            [run],
        )
        .unwrap();

    assert_eq!(found(&env.conn, "nfse"), vec![run]);
    assert_eq!(found(&env.conn, "emissao"), vec![run]);
    assert_eq!(count(&env.conn, "search_index"), 1);
}

#[test]
fn renamed_project_and_retitled_session_are_reindexed() {
    let env = Env::new();
    let run = seed(&env.conn);
    assert_eq!(found(&env.conn, "fiscal"), vec![run]);

    env.conn
        .execute("UPDATE projects SET name = 'Contabil'", [])
        .unwrap();
    assert_eq!(found(&env.conn, "contabil"), vec![run]);
    assert!(found(&env.conn, "fiscal").is_empty());

    env.conn
        .execute("UPDATE ai_sessions SET title = 'Fechamento mensal'", [])
        .unwrap();
    assert_eq!(found(&env.conn, "fechamento"), vec![run]);
    assert!(found(&env.conn, "emissao").is_empty());
    assert_eq!(count(&env.conn, "search_index"), 1);
}

#[test]
fn cascade_delete_removes_rows() {
    let env = Env::new();
    seed(&env.conn);
    assert_eq!(count(&env.conn, "search_index"), 1);

    env.conn.execute("DELETE FROM projects", []).unwrap();
    assert_eq!(count(&env.conn, "search_index"), 0);
}

#[test]
fn migration_0002_indexes_existing_runs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.sqlite");
    let old = Connection::open(&path).unwrap();
    old.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    old.execute_batch(include_str!("../src/db/migrations/0001_initial.sql"))
        .unwrap();
    old.pragma_update(None, "user_version", 1).unwrap();
    let run = seed(&old);
    assert_eq!(count(&old, "search_index"), 0);
    drop(old);

    let conn = db::open(&path).unwrap();
    assert_eq!(found(&conn, "nfse"), vec![run]);
}

#[test]
fn query_orders_by_bm25_and_respects_limit() {
    let env = Env::new();
    let session = insert_session(&env.conn, Tool::Claude, "s1", Path::new("/p/Alpha"), None);
    let mk = |offset: i64, summary: &str| {
        let run = insert_run(
            &env.conn,
            session,
            RunSeed {
                started_at: now_ms() + offset,
                ..Default::default()
            },
        );
        env.conn
            .execute(
                "UPDATE session_runs SET summary = ?2 WHERE id = ?1",
                params![run, summary],
            )
            .unwrap();
        run
    };
    let long = mk(
        0,
        "zebra with plenty of other words around it to dilute the match weight a lot",
    );
    let short = mk(1, "zebra");

    assert_eq!(found(&env.conn, "zebra"), vec![short, long]);
    let limited = search::query(&env.conn, "zebra", 1).unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].run_id, short);
}

#[test]
fn query_with_no_terms_returns_empty() {
    let env = Env::new();
    seed(&env.conn);
    assert!(search::query(&env.conn, " -- ", 20).unwrap().is_empty());
    assert!(search::query(&env.conn, "", 20).unwrap().is_empty());
}

#[test]
fn search_hit_serializes_tool_as_a_lowercase_string() {
    let env = Env::new();
    seed(&env.conn);
    let hit = search::query(&env.conn, "nfse", 20).unwrap().remove(0);
    let json = serde_json::to_value(&hit).unwrap();
    assert_eq!(json["tool"], "claude");
    assert_eq!(json["project_name"], "Fiscal-BR");
}

#[test]
fn session_moved_to_another_project_is_reindexed() {
    let env = Env::new();
    let run = seed(&env.conn);
    let other = insert_project(&env.conn, "/home/u/Code/Contabil");
    assert_eq!(found(&env.conn, "fiscal"), vec![run]);

    env.conn
        .execute("UPDATE ai_sessions SET project_id = ?1", [other])
        .unwrap();
    assert_eq!(found(&env.conn, "contabil"), vec![run]);
    assert!(found(&env.conn, "fiscal").is_empty());
    assert_eq!(count(&env.conn, "search_index"), 1);
}

/// Drift guard: the triggers repeat the indexing SELECT of `search::ROWS`; after plain SQL
/// writes (no `index_run`), a full `rebuild` must not change a single cell.
#[test]
fn triggers_and_rebuild_build_the_same_index() {
    let mut env = Env::new();
    env.conn
        .execute_batch(
            "INSERT INTO projects (id, key, name, path) VALUES
                 (1, '/p/Fiscal-BR', 'Fiscal-BR', '/p/Fiscal-BR'),
                 (2, '/p/Contábil', 'Contábil', '/p/Contábil');
             INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, title, last_activity_at)
             VALUES (1, 1, 'claude', 's1', '/p/Fiscal-BR', 'Emissão de NFS-e', 1),
                    (2, 1, 'codex', 's2', '/p/Fiscal-BR', NULL, 1),
                    (3, 2, 'claude', 's3', '/p/Contábil', 'Balanço', 1);
             INSERT INTO session_runs (id, ai_session_id, started_at, branch, summary, next_step)
             VALUES (1, 1, 1, 'feat/nfs-e', 'Emitir NFS-e', 'Testar'),
                    (2, 2, 2, NULL, NULL, NULL),
                    (3, 3, 3, 'main', 'Fechar o mês', NULL),
                    (4, 3, 4, 'fix/x-y', NULL, 'Revisar - depois');
             UPDATE session_runs SET summary = 'Resumo-novo' WHERE id = 2;
             UPDATE session_runs SET ai_session_id = 1 WHERE id = 4;
             UPDATE ai_sessions SET title = 'Notas fiscais' WHERE id = 1;
             UPDATE ai_sessions SET project_id = 2 WHERE id = 2;
             UPDATE projects SET name = 'Fiscal-PT' WHERE id = 1;
             DELETE FROM session_runs WHERE id = 3;",
        )
        .unwrap();
    let by_triggers = rows(&env.conn);
    assert_eq!(by_triggers.len(), 3);

    search::rebuild(&mut env.conn).unwrap();
    assert_eq!(rows(&env.conn), by_triggers);
}
