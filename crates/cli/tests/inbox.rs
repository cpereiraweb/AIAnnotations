//! `dev inbox [--grouped]` and `dev inbox dismiss <key> <state_hash>`.
use assert_cmd::Command;
use tempfile::TempDir;

fn cli(tmp: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("aiannotations").unwrap();
    cmd.env("AIANNOTATIONS_DATA_DIR", tmp.path().join("data"))
        .env_remove("AIANNOTATIONS_INTERNAL");
    cmd
}

/// A project active now with one pending reminder.
fn setup() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("proj");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::write(
        project.join(".claude/pending-reminder.json"),
        r#"{"items":[{"ref":"1","text":"Do it"}]}"#,
    )
    .unwrap();
    let conn = aiannotations_core::db::open(&tmp.path().join("data/aiannotations.sqlite")).unwrap();
    conn.execute(
        "INSERT INTO projects (key, name, path, last_activity_at) VALUES (?1, 'proj', ?1, ?2)",
        rusqlite::params![
            project.to_string_lossy(),
            chrono::Utc::now().timestamp_millis()
        ],
    )
    .unwrap();
    tmp
}

fn json(cmd: &mut Command) -> serde_json::Value {
    let out = cmd.assert().success();
    serde_json::from_slice(&out.get_output().stdout).unwrap()
}

#[test]
fn inbox_lists_groups_and_dismisses() {
    let tmp = setup();
    let items = json(cli(&tmp).args(["dev", "inbox"]));
    assert_eq!(items.as_array().unwrap().len(), 1);
    assert_eq!(items[0]["kind"], "reminder");
    let (key, hash) = (
        items[0]["key"].as_str().unwrap().to_string(),
        items[0]["state_hash"].as_str().unwrap().to_string(),
    );

    let grouped = json(cli(&tmp).args(["dev", "inbox", "--grouped"]));
    assert_eq!(grouped["recent"][0]["name"], "proj");
    assert!(grouped["older"].as_array().unwrap().is_empty());

    cli(&tmp)
        .args(["dev", "inbox", "dismiss", &key, &hash])
        .assert()
        .success();
    let items = json(cli(&tmp).args(["dev", "inbox"]));
    assert!(items.as_array().unwrap().is_empty());
}
