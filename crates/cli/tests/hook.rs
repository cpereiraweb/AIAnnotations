//! Parity with Laravel `tests/Feature/Hook/HookScriptTest.php` (Inv §3.1).
use assert_cmd::Command;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn hook(args: &[&str], stdin: impl Into<Vec<u8>>, data_dir: &Path) -> assert_cmd::assert::Assert {
    Command::cargo_bin("aiannotations")
        .unwrap()
        .env("AIANNOTATIONS_DATA_DIR", data_dir)
        .env_remove("AIANNOTATIONS_INTERNAL")
        .args(args)
        .write_stdin(stdin)
        .timeout(Duration::from_secs(10))
        .assert()
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut v: Vec<PathBuf> = rd
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    v.sort();
    v
}

fn read(p: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap()
}

fn assert_received_at_shape(s: &str) {
    // 2026-10-29T18:14:50.123456789Z
    let b = s.as_bytes();
    assert_eq!(b.len(), 30, "{s}");
    assert_eq!(b[10], b'T');
    assert_eq!(b[19], b'.');
    assert_eq!(b[29], b'Z');
    assert!(b[20..29].iter().all(u8::is_ascii_digit), "{s}");
}

/// grava envelope com tool, event e payload
#[test]
fn hook_writes_envelope_with_tool_event_and_payload() {
    let tmp = TempDir::new().unwrap();
    let payload = json!({"session_id":"abc","cwd":"/home/x/Condomínios/Bay side","reason":"prompt_input_exit","msg":"100% \"aspas\"\n\\"});

    hook(&["hook", "claude", "end"], payload.to_string(), tmp.path())
        .success()
        .stdout("")
        .stderr("");

    let spool = tmp.path().join("spool");
    let files = json_files(&spool);
    assert_eq!(files.len(), 1);
    let name = files[0].file_name().unwrap().to_str().unwrap();
    let (epoch, rest) = name.split_once('-').unwrap();
    assert!(epoch.len() >= 18 && epoch.bytes().all(|c| c.is_ascii_digit()));
    let pid = rest
        .strip_prefix("claude-end-")
        .and_then(|r| r.strip_suffix(".json"))
        .unwrap();
    assert!(!pid.is_empty() && pid.bytes().all(|c| c.is_ascii_digit()));
    let leftovers: Vec<_> = std::fs::read_dir(&spool)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .filter(|n| n.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());

    let env = read(&files[0]);
    assert_eq!(env["tool"], "claude");
    assert_eq!(env["event"], "end");
    assert_received_at_shape(env["received_at"].as_str().unwrap());
    assert_eq!(env["payload"], payload);
}

/// não imprime nada no stdout
#[test]
fn hook_prints_nothing() {
    let tmp = TempDir::new().unwrap();
    hook(
        &["hook", "codex", "start"],
        r#"{"session_id":"x","cwd":"/tmp"}"#,
        tmp.path(),
    )
    .code(0)
    .stdout("")
    .stderr("");
}

/// sai com 0 quando a pasta de fila não é gravável
#[test]
fn hook_exits_zero_when_spool_not_writable() {
    let tmp = TempDir::new().unwrap();
    // A regular file where the data dir should be makes create_dir_all fail.
    let blocker = tmp.path().join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    hook(
        &["hook", "claude", "start"],
        r#"{"session_id":"x"}"#,
        &blocker,
    )
    .code(0)
    .stdout("")
    .stderr("");
}

/// AIANNOTATIONS_INTERNAL=1 não grava
#[test]
fn hook_internal_env_does_not_write() {
    let tmp = TempDir::new().unwrap();
    Command::cargo_bin("aiannotations")
        .unwrap()
        .env("AIANNOTATIONS_DATA_DIR", tmp.path())
        .env("AIANNOTATIONS_INTERNAL", "1")
        .args(["hook", "claude", "end"])
        .write_stdin(r#"{"session_id":"x"}"#)
        .assert()
        .code(0)
        .stdout("")
        .stderr("");
    assert!(!tmp.path().join("spool").exists());
}

/// sem AIANNOTATIONS_SPOOL usa a pasta padrão dentro do HOME
#[cfg(target_os = "linux")]
#[test]
fn hook_default_dir_inside_home() {
    let home = TempDir::new().unwrap();
    let xdg = TempDir::new().unwrap();
    Command::cargo_bin("aiannotations")
        .unwrap()
        .env("HOME", home.path())
        .env("XDG_DATA_HOME", xdg.path())
        .env_remove("AIANNOTATIONS_DATA_DIR")
        .env_remove("AIANNOTATIONS_INTERNAL")
        .args(["hook", "codex", "end"])
        .write_stdin(r#"{"session_id":"x"}"#)
        .assert()
        .code(0)
        .stdout("")
        .stderr("");
    let spool = xdg
        .path()
        .join("io.github.cpereiraweb.aiannotations")
        .join("spool");
    let files = json_files(&spool);
    assert_eq!(files.len(), 1);
    assert!(
        files[0]
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("-codex-end-")
    );
}

/// argumentos desconhecidos não gravam nada
#[test]
fn hook_unknown_arguments_do_not_write() {
    let tmp = TempDir::new().unwrap();
    for args in [
        vec!["hook", "../evil", "end"],
        vec!["hook", "claude", "bogus"],
        vec!["hook", "claude"],
        vec!["hook"],
    ] {
        hook(&args, r#"{"session_id":"x"}"#, tmp.path())
            .code(0)
            .stdout("")
            .stderr("");
    }
    assert!(!tmp.path().join("spool").exists());
}

#[test]
fn hook_handles_5mb_payload() {
    let tmp = TempDir::new().unwrap();
    let big = "a".repeat(5 * 1024 * 1024);
    let payload = json!({"session_id":"x","blob":big});
    let started = Instant::now();
    hook(
        &["hook", "claude", "start"],
        payload.to_string(),
        tmp.path(),
    )
    .code(0)
    .stdout("")
    .stderr("");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
    let files = json_files(&tmp.path().join("spool"));
    assert_eq!(files.len(), 1);
    assert_eq!(read(&files[0])["payload"], payload);
}

#[test]
fn hook_spool_dir_with_space_and_accent() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path().join("Dados do Cláudio");
    hook(&["hook", "claude", "start"], r#"{"session_id":"x"}"#, &dir)
        .code(0)
        .stdout("")
        .stderr("");
    assert_eq!(json_files(&dir.join("spool")).len(), 1);
}

#[test]
fn hook_invalid_json_stdin_records_null_payload() {
    let tmp = TempDir::new().unwrap();
    for stdin in ["{not json", "", "  \n"] {
        hook(&["hook", "claude", "end"], stdin, tmp.path())
            .code(0)
            .stdout("")
            .stderr("");
    }
    let files = json_files(&tmp.path().join("spool"));
    assert_eq!(files.len(), 3);
    for f in files {
        let env = read(&f);
        assert_eq!(env["tool"], "claude");
        assert!(env["payload"].is_null());
    }
}
