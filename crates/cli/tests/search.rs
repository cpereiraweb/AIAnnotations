//! `dev search` / `dev reindex` output contract.
use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

fn cli(tmp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env("CLAUDE_CONFIG_DIR", tmp.path().join("claude"))
        .env("CODEX_HOME", tmp.path().join("codex"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

/// Database with one run (id 1) on project "Fiscal-BR", summary "Emitir NFS-e".
fn seed(tmp: &TempDir) {
    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    conn.execute_batch(
        "INSERT INTO projects (key, name, path) VALUES ('k', 'Fiscal-BR', '/p/Fiscal-BR');
         INSERT INTO ai_sessions (project_id, tool, session_id, cwd, last_activity_at)
             VALUES (1, 'codex', 's1', '/p/Fiscal-BR', 1);
         INSERT INTO session_runs (ai_session_id, started_at, summary) VALUES (1, 5, 'Emitir NFS-e');",
    )
    .unwrap();
}

fn json(out: &[u8]) -> Value {
    serde_json::from_slice(out).unwrap()
}

#[test]
fn search_prints_the_hits_as_json() {
    let tmp = TempDir::new().unwrap();
    seed(&tmp);
    let out = cli(&tmp).args(["dev", "search", "nfse"]).assert().success();
    let hits = json(&out.get_output().stdout);
    assert_eq!(hits.as_array().unwrap().len(), 1);
    assert_eq!(hits[0]["run_id"], 1);
    assert_eq!(hits[0]["tool"], "codex");
    assert_eq!(hits[0]["project_name"], "Fiscal-BR");

    let out = cli(&tmp)
        .args(["dev", "search", "zzz", "--limit", "5"])
        .assert()
        .success();
    assert_eq!(json(&out.get_output().stdout), serde_json::json!([]));
}

#[test]
fn reindex_prints_the_indexed_count() {
    let tmp = TempDir::new().unwrap();
    seed(&tmp);
    let out = cli(&tmp).args(["dev", "reindex"]).assert().success();
    assert_eq!(
        json(&out.get_output().stdout),
        serde_json::json!({"indexed": 1})
    );
}
