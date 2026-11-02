//! `aiannotations import-laravel --db <path>` output contract (spec §10.6).
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

const SCHEMA: &str = include_str!("../../core/tests/fixtures/laravel_schema.sql");

/// A Laravel database with one project, one session, one run and a dismissal whose project is
/// gone.
fn laravel(tmp: &TempDir) -> std::path::PathBuf {
    let path = tmp.path().join("laravel.sqlite");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(SCHEMA).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (id, key, name, path) VALUES (4, '/p', 'p', '/p');
         INSERT INTO ai_sessions (id, project_id, tool, session_id, cwd, last_activity_at)
           VALUES (8, 4, 'codex', 't1', '/p', '2026-10-29 15:14:50');
         INSERT INTO session_runs (ai_session_id, started_at) VALUES (8, '2026-10-29 15:00:00');
         INSERT INTO inbox_dismissals (item_key, state_hash) VALUES ('dirty:77', 'h');",
    )
    .unwrap();
    path
}

fn cli(tmp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

#[test]
fn import_laravel_prints_the_report_then_the_skipped_records() {
    let tmp = TempDir::new().unwrap();
    let db = laravel(&tmp);

    let out = cli(&tmp)
        .args(["import-laravel", "--db"])
        .arg(&db)
        .assert()
        .success();

    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "projects=1 sessions=1 runs=1 dismissals=0 briefings=0 skipped=1",
            "skipped: inbox_dismissals #1 'dirty:77': project #77 not imported",
        ],
        "{stdout}"
    );

    // Again: the database now has sessions, so a plain re-run is refused and writes nothing...
    cli(&tmp)
        .args(["import-laravel", "--db"])
        .arg(&db)
        .assert()
        .code(1)
        .stdout("")
        .stderr(
            predicate::str::starts_with("error: import error: the target database already has")
                .and(predicate::str::contains(
                    "import before the first collect/hook run, or pass --merge",
                )),
        );

    // ...and with --merge nothing is new, the same record still left out.
    cli(&tmp)
        .args(["import-laravel", "--merge", "--db"])
        .arg(&db)
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "projects=0 sessions=0 runs=0 dismissals=0 briefings=0 skipped=1\n",
        ));
}

#[test]
fn import_laravel_with_a_missing_database_fails() {
    let tmp = TempDir::new().unwrap();
    cli(&tmp)
        .args(["import-laravel", "--db"])
        .arg(tmp.path().join("nope.sqlite"))
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::starts_with(
            "error: import error: Laravel database not found: ",
        ));
}

/// The command stays out of the help.
#[test]
fn import_laravel_is_hidden_from_help() {
    Command::cargo_bin("aiannotations")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("import-laravel").not());
}
