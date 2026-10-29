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
