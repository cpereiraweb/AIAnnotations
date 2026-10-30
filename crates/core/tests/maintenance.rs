//! Port of Laravel `PruneTest.php` (6) and `ReclassifyTest.php` (3), plus stats.

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use aiannotations_core::model::Tool;
use aiannotations_core::sessions::maintenance::{self, PruneReport};
use support::*;

const DAY: u64 = 86_400;

/// Spool under `base/spool` with `done/` and `failed/`.
fn spool(env: &Env) -> PathBuf {
    let spool = env.base.join("spool");
    std::fs::create_dir_all(spool.join("done")).unwrap();
    std::fs::create_dir_all(spool.join("failed")).unwrap();
    spool
}

/// File with `{}` whose mtime is `age_days` days and one minute before `now_ms()`.
fn put(spool: &Path, rel: &str, age_days: u64) -> PathBuf {
    let path = spool.join(rel);
    std::fs::write(&path, "{}").unwrap();
    let now = SystemTime::UNIX_EPOCH + Duration::from_millis(now_ms() as u64);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(now - Duration::from_secs(age_days * DAY + 60))
        .unwrap();
    path
}

/// apaga done/*.json com mais de 7 dias e failed/*.json com mais de 30
#[test]
fn prune_removes_done_over_7_days_and_failed_over_30() {
    let env = Env::new();
    let spool = spool(&env);
    let old_done = put(&spool, "done/1.json", 8);
    let recent_done = put(&spool, "done/2.json", 6);
    let old_failed = put(&spool, "failed/3.json", 31);
    let recent_failed = put(&spool, "failed/4.json", 20);
    let pending = put(&spool, "5.json", 90);
    let other = put(&spool, "done/notas.txt", 90);

    let report = maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert_eq!(
        report,
        PruneReport {
            done_removed: 1,
            failed_removed: 1,
            warnings: vec![]
        }
    );
    assert!(!old_done.exists() && !old_failed.exists());
    assert!(recent_done.exists() && recent_failed.exists());
    assert!(pending.exists() && other.exists());
}

/// respeita spool_done_days e spool_failed_days da config
#[test]
fn prune_respects_configured_days() {
    let mut env = Env::new();
    env.settings.spool_done_days = 1;
    env.settings.spool_failed_days = 2;
    let spool = spool(&env);
    let done = put(&spool, "done/1.json", 1);
    let failed = put(&spool, "failed/2.json", 1);

    maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert!(!done.exists());
    assert!(failed.exists());
}

/// sem as pastas da fila não faz nada
#[test]
fn prune_without_spool_folders_does_nothing() {
    let env = Env::new();
    let report =
        maintenance::prune_spool(&env.base.join("nao-existe"), &env.settings, now_ms()).unwrap();
    assert_eq!(report, PruneReport::default());
}

/// os padrões são 7 e 30 dias
#[test]
fn prune_defaults_are_7_and_30_days() {
    let env = Env::new();
    assert_eq!(env.settings.spool_done_days, 7);
    assert_eq!(env.settings.spool_failed_days, 30);
}

/// limites de dias zerados ou negativos valem 1 dia
#[test]
fn prune_zero_day_limits_count_as_one_day() {
    // Settings holds unsigned days: "negative" in the v1 config is 0 here, the same floor.
    let mut env = Env::new();
    env.settings.spool_done_days = 0;
    env.settings.spool_failed_days = 0;
    let spool = spool(&env);
    let fresh = put(&spool, "done/1.json", 0);
    let fresh_failed = put(&spool, "failed/2.json", 0);
    let old = put(&spool, "done/3.json", 1);

    maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert!(fresh.exists() && fresh_failed.exists());
    assert!(!old.exists());
}

/// pasta done/ ou failed/ que é symlink é pulada com aviso
// Creating symlinks needs Unix (or elevated rights on Windows).
#[cfg(unix)]
#[test]
fn prune_skips_symlinked_folder_with_warning() {
    let env = Env::new();
    let spool = spool(&env);
    let outside = env.base.join("fora");
    std::fs::create_dir_all(&outside).unwrap();
    let outside_file = put(&outside, "x.json", 90);
    std::fs::remove_dir(spool.join("failed")).unwrap();
    std::os::unix::fs::symlink(&outside, spool.join("failed")).unwrap();
    let done = put(&spool, "done/1.json", 8);

    let report = maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert!(outside_file.exists());
    assert!(!done.exists());
    assert_eq!(report.done_removed, 1);
    assert_eq!(report.failed_removed, 0);
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("is a symlink; skipped"));
}

/// file symlink named *.json is never followed (target survives)
// Creating symlinks needs Unix (or elevated rights on Windows).
#[cfg(unix)]
#[test]
fn prune_does_not_follow_file_symlinks() {
    let env = Env::new();
    let spool = spool(&env);
    let outside = env.base.join("fora");
    std::fs::create_dir_all(&outside).unwrap();
    let target = put(&outside, "alvo.json", 90);
    for dir in ["done", "failed"] {
        std::os::unix::fs::symlink(&target, spool.join(dir).join("x.json")).unwrap();
    }

    let report = maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert_eq!(std::fs::read_to_string(&target).unwrap(), "{}");
    // The link itself is not a spool file: left in place.
    assert!(spool.join("done/x.json").symlink_metadata().is_ok());
    assert!(spool.join("failed/x.json").symlink_metadata().is_ok());
    assert_eq!((report.done_removed, report.failed_removed), (0, 0));
}

/// subdirectory named *.json is neither descended into nor deleted
#[test]
fn prune_ignores_directories_named_json() {
    let env = Env::new();
    let spool = spool(&env);
    let sub = spool.join("done/x.json");
    std::fs::create_dir_all(&sub).unwrap();
    let inner = put(&spool, "done/x.json/y.json", 90);
    // The directory itself must be old enough to be pruned.
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    std::fs::File::open(&sub)
        .unwrap()
        .set_modified(old)
        .unwrap();

    let report = maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert!(sub.is_dir() && inner.exists());
    assert_eq!(report.done_removed, 0);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// symlinked done/ is skipped with a warning
// Creating symlinks needs Unix (or elevated rights on Windows).
#[cfg(unix)]
#[test]
fn prune_skips_symlinked_done_folder_with_warning() {
    let env = Env::new();
    let spool = spool(&env);
    let outside = env.base.join("fora");
    std::fs::create_dir_all(&outside).unwrap();
    let file = put(&outside, "x.json", 90);
    std::fs::remove_dir(spool.join("done")).unwrap();
    std::os::unix::fs::symlink(&outside, spool.join("done")).unwrap();
    let failed = put(&spool, "failed/1.json", 31);

    let report = maintenance::prune_spool(&spool, &env.settings, now_ms()).unwrap();

    assert!(file.exists());
    assert!(!failed.exists());
    assert_eq!((report.done_removed, report.failed_removed), (0, 1));
    assert_eq!(report.warnings.len(), 1);
    assert!(report.warnings[0].contains("is a symlink; skipped"));
}

// ---- reclassify ----

/// Claude transcript with one turn (17 s) per prompt.
fn transcript(env: &Env, name: &str, prompts: &[&str]) -> PathBuf {
    let path = env.base.join(format!("{name}.jsonl"));
    let mut lines = vec![serde_json::json!({"type": "permission-mode", "sessionId": name})];
    for (i, p) in prompts.iter().enumerate() {
        lines.push(claude_user(
            name,
            p,
            &format!("2026-10-20T10:{i:02}:00.000Z"),
        ));
        lines.push(claude_assistant(
            name,
            "m",
            "Ok.",
            &format!("2026-10-20T10:{i:02}:17.000Z"),
        ));
    }
    write_jsonl(&path, &lines);
    path
}

/// Empty session plus a skipped run (closed unless `seed` says otherwise); returns (session, run).
fn empty_session(
    env: &Env,
    name: &str,
    path: Option<&Path>,
    origin: &str,
    seed: Option<RunSeed>,
) -> (i64, i64) {
    let s = insert_session(&env.conn, Tool::Claude, name, &env.base, path);
    env.conn
        .execute(
            "UPDATE ai_sessions SET is_empty = 1, origin = ?2 WHERE id = ?1",
            rusqlite::params![s, origin],
        )
        .unwrap();
    let seed = seed.unwrap_or(RunSeed {
        started_at: now_ms() - 3_600_000,
        ended_at: Some(now_ms() - 3_540_000),
        end_reason: Some("prompt_input_exit"),
        ended_cleanly: Some(true),
        summary_status: Some("skipped"),
        summary_attempts: 3,
        summary_available_at: Some(now_ms() + 3_600_000),
        ..Default::default()
    });
    (s, insert_run(&env.conn, s, seed))
}

fn run_of(env: &Env, id: i64) -> RunView {
    runs(&env.conn, &format!("id = {id}")).remove(0)
}

fn is_empty(env: &Env, id: i64) -> bool {
    sessions(&env.conn, &format!("id = {id}"))[0].is_empty
}

/// reclassifica sessões vazias que têm prompt do usuário e devolve as execuções ao resumo
#[test]
fn reclassifies_empty_sessions_with_prompt_and_returns_runs_to_summary() {
    let mut env = Env::new();
    let p = |env: &Env, n: &str, prompts: &[&str]| transcript(env, n, prompts);
    let path = p(&env, "curta", &["Qual a versão do PHP?"]);
    let short = empty_session(&env, "curta", Some(&path), "interactive", None);
    let path = p(&env, "antiga", &["Oi"]);
    let backfill = empty_session(
        &env,
        "antiga",
        Some(&path),
        "interactive",
        Some(RunSeed {
            started_at: now_ms() - 3_600_000,
            ended_at: Some(now_ms() - 3_540_000),
            summary_status: Some("skipped"),
            summary_attempts: 3,
            is_backfill: true,
            ..Default::default()
        }),
    );
    let path = p(&env, "aberta", &["Oi"]);
    let open = empty_session(
        &env,
        "aberta",
        Some(&path),
        "interactive",
        Some(RunSeed {
            started_at: now_ms() - 3_600_000,
            summary_status: Some("skipped"),
            summary_attempts: 3,
            ..Default::default()
        }),
    );
    let path = p(&env, "vazia", &[]);
    let really_empty = empty_session(&env, "vazia", Some(&path), "interactive", None);
    let path = p(&env, "em-branco", &[""]);
    let only_blank = empty_session(&env, "em-branco", Some(&path), "interactive", None);
    let path = p(&env, "sdk", &["Oi"]);
    let automation = empty_session(&env, "sdk", Some(&path), "automation", None);
    let missing = empty_session(
        &env,
        "ausente",
        Some(&env.base.join("nao-existe.jsonl")),
        "interactive",
        None,
    );

    let report =
        maintenance::reclassify(&mut env.conn, &env.paths, &env.settings, now_ms()).unwrap();

    assert_eq!(report.reclassified, 3);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for (s, r) in [short, backfill] {
        let run = run_of(&env, r);
        assert!(!is_empty(&env, s));
        assert_eq!(run.summary_status, "pending");
        assert_eq!(run.summary_attempts, 0);
        assert_eq!(run.summary_available_at, None);
    }
    assert!(run_of(&env, backfill.1).is_backfill);
    assert!(!is_empty(&env, open.0));
    assert_eq!(run_of(&env, open.1).summary_status, "skipped");
    for (s, r) in [really_empty, only_blank, automation, missing] {
        let run = run_of(&env, r);
        assert!(is_empty(&env, s));
        assert_eq!(run.summary_status, "skipped");
        assert_eq!(run.summary_attempts, 3);
    }
}

/// reclassificar é idempotente
#[test]
fn reclassify_is_idempotent() {
    let mut env = Env::new();
    let path = transcript(&env, "curta", &["Qual a versão do PHP?"]);
    empty_session(&env, "curta", Some(&path), "interactive", None);

    let first =
        maintenance::reclassify(&mut env.conn, &env.paths, &env.settings, now_ms()).unwrap();
    let second =
        maintenance::reclassify(&mut env.conn, &env.paths, &env.settings, now_ms()).unwrap();

    assert_eq!(first.reclassified, 1);
    assert_eq!(second.reclassified, 0);
    assert_eq!(sole_run(&env.conn).summary_status, "pending");
}

/// reclassifica todas mesmo com mais sessões que o tamanho do lote
#[test]
fn reclassifies_all_even_with_more_sessions_than_chunk_size() {
    let mut env = Env::new();
    env.settings.chunk_size = 2;
    for i in 1..=5 {
        let name = format!("s{i}");
        let path = transcript(&env, &name, &["Oi"]);
        empty_session(&env, &name, Some(&path), "interactive", None);
    }

    let report =
        maintenance::reclassify(&mut env.conn, &env.paths, &env.settings, now_ms()).unwrap();

    assert_eq!(report.reclassified, 5);
    assert_eq!(sessions(&env.conn, "is_empty = 1").len(), 0);
}

/// contagens por ferramenta e por status
#[test]
fn stats_counts_by_tool_and_status() {
    let env = Env::new();
    let c = insert_session(&env.conn, Tool::Claude, "a", &env.base, None);
    let c2 = insert_session(&env.conn, Tool::Claude, "b", &env.base, None);
    let x = insert_session(&env.conn, Tool::Codex, "c", &env.base, None);
    env.conn
        .execute("UPDATE ai_sessions SET is_empty = 1 WHERE id = ?1", [c2])
        .unwrap();
    env.conn
        .execute(
            "UPDATE ai_sessions SET origin = 'automation' WHERE id = ?1",
            [x],
        )
        .unwrap();
    for (s, status) in [(c, "pending"), (c, "done"), (c2, "pending"), (x, "skipped")] {
        insert_run(
            &env.conn,
            s,
            RunSeed {
                started_at: 1,
                summary_status: Some(status),
                ..Default::default()
            },
        );
    }

    let s = maintenance::stats(&env.conn).unwrap();

    assert_eq!(s.projects, 1);
    assert_eq!(s.sessions_by_tool["claude"], 2);
    assert_eq!(s.sessions_by_tool["codex"], 1);
    assert_eq!(s.runs, 4);
    assert_eq!(s.runs_by_summary_status["pending"], 2);
    assert_eq!(s.runs_by_summary_status["done"], 1);
    assert_eq!(s.runs_by_summary_status["skipped"], 1);
    assert_eq!(s.visible_sessions, 1);
}
