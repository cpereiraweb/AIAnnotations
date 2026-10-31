use aiannotations_core::installer::hooks::{
    HookError, HookEvent, HookSpec, HookStatus, install, is_ours, status, unified_diff, uninstall,
};
use aiannotations_core::model::Tool;
use serde_json::{Value, json};

const CODEX_MATCHER: &str = "startup|resume|clear|compact|fork";

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/hooks/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

fn spec(tool: Tool) -> HookSpec {
    let name = if tool == Tool::Claude {
        "claude"
    } else {
        "codex"
    };
    HookSpec {
        tool,
        start: format!("'/opt/AI Annotations/aiannotations' hook {name} start"),
        end: format!("'/opt/AI Annotations/aiannotations' hook {name} end"),
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn commands(config: &Value, event: &str) -> Vec<String> {
    config["hooks"][event]
        .as_array()
        .map(|groups| {
            groups
                .iter()
                .flat_map(|g| g["hooks"].as_array().unwrap().iter())
                .map(|h| h["command"].as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn keys(v: &Value) -> Vec<String> {
    v.as_object().unwrap().keys().cloned().collect()
}

fn installed(name: &str, tool: Tool) -> (String, String) {
    let original = fixture(name);
    let merge = install(Some(&original), &spec(tool)).unwrap();
    (original, merge.contents)
}

#[test]
fn install_preserves_existing_claude_hooks_and_adds_start_end() {
    let (original, contents) = installed("claude-2sp.json", Tool::Claude);
    let (before, after) = (parse(&original), parse(&contents));
    assert_eq!(keys(&before), keys(&after));
    for (key, value) in before.as_object().unwrap() {
        if key != "hooks" {
            assert_eq!(&after[key], value);
        }
    }
    for (event, groups) in before["hooks"].as_object().unwrap() {
        if event != "SessionStart" {
            assert_eq!(&after["hooks"][event], groups);
        }
    }
    let s = spec(Tool::Claude);
    assert_eq!(
        after["hooks"]["SessionStart"][0],
        before["hooks"]["SessionStart"][0]
    );
    assert_eq!(after["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(
        after["hooks"]["SessionStart"][1],
        json!({"hooks": [{"type": "command", "command": s.start, "timeout": 5}]})
    );
    assert_eq!(
        after["hooks"]["SessionEnd"],
        json!([{"hooks": [{"type": "command", "command": s.end, "timeout": 5}]}])
    );
    let mut expected = keys(&before["hooks"]);
    expected.push("SessionEnd".into());
    assert_eq!(keys(&after["hooks"]), expected);
}

#[test]
fn preserves_empty_objects_arrays_unicode_and_slashes() {
    let (_, contents) = installed("claude-2sp.json", Tool::Claude);
    assert!(contents.contains("\"env\": {}"));
    assert!(contents.contains("\"additionalDirectories\": []"));
    assert!(contents.contains("Checando pendências do projeto..."));
    assert!(
        contents
            .contains("\"$schema\": \"https://json.schemastore.org/claude-code-settings.json\"")
    );
    assert!(contents.ends_with("}\n"));
}

#[test]
fn install_is_idempotent() {
    let s = spec(Tool::Claude);
    let first = install(Some(&fixture("claude-2sp.json")), &s).unwrap();
    assert!(first.changed);
    let second = install(Some(&first.contents), &s).unwrap();
    assert!(!second.changed);
    assert_eq!(second.contents, first.contents);
    let config = parse(&second.contents);
    let ours = |event| {
        commands(&config, event)
            .iter()
            .filter(|c| is_ours(c))
            .count()
    };
    assert_eq!((ours("SessionStart"), ours("SessionEnd")), (1, 1));
}

#[test]
fn does_not_duplicate_v1_hook_with_other_path() {
    let mut config = parse(&fixture("claude-2sp.json"));
    config["hooks"]["SessionStart"].as_array_mut().unwrap().push(json!(
        {"hooks": [{"type": "command", "command": "/outro/bin/aiannotations-hook claude start", "timeout": 5}]}
    ));
    let s = spec(Tool::Claude);
    let merge = install(Some(&config.to_string()), &s).unwrap();
    assert!(merge.changed);
    let after = parse(&merge.contents);
    let start = commands(&after, "SessionStart");
    assert_eq!(start.len(), 2);
    assert!(
        start.contains(&s.start),
        "the v1 command is replaced by ours"
    );
    assert!(!start.iter().any(|c| c.contains("bin/aiannotations-hook")));
    assert_eq!(commands(&after, "SessionEnd").len(), 1);
}

#[test]
fn codex_gets_timeout_3_and_matcher() {
    let (original, contents) = installed("codex-2sp.json", Tool::Codex);
    let (before, after) = (parse(&original), parse(&contents));
    let s = spec(Tool::Codex);
    assert_eq!(
        after["hooks"]["SessionStart"][0],
        before["hooks"]["SessionStart"][0]
    );
    assert_eq!(
        after["hooks"]["SessionStart"][1],
        json!({"matcher": CODEX_MATCHER, "hooks": [{"type": "command", "command": s.start, "timeout": 3}]})
    );
    assert_eq!(
        after["hooks"]["SessionEnd"],
        json!([{"hooks": [{"type": "command", "command": s.end, "timeout": 3}]}])
    );
    for event in ["UserPromptSubmit", "PostToolUse", "Stop", "PreCompact"] {
        assert_eq!(after["hooks"][event], before["hooks"][event]);
    }
}

#[test]
fn creates_file_with_hooks_object_when_missing() {
    for tool in [Tool::Claude, Tool::Codex] {
        let merge = install(None, &spec(tool)).unwrap();
        assert!(merge.changed);
        assert!(merge.contents.ends_with("}\n"));
        assert!(
            merge.contents.starts_with("{\n  \"hooks\": {\n"),
            "2 spaces by default"
        );
        let config = parse(&merge.contents);
        assert_eq!(keys(&config), ["hooks"]);
        assert_eq!(keys(&config["hooks"]), ["SessionStart", "SessionEnd"]);
        assert_eq!(merge.absent_events, [HookEvent::Start, HookEvent::End]);
    }
}

#[test]
fn empty_file_counts_as_empty_object() {
    for text in ["", "  \n\t\n"] {
        let merge = install(Some(text), &spec(Tool::Claude)).unwrap();
        assert!(merge.changed);
        assert_eq!(
            merge.contents,
            install(None, &spec(Tool::Claude)).unwrap().contents
        );
        let un = uninstall(Some(text), None).unwrap();
        assert!(!un.changed);
        assert_eq!(un.contents, text);
    }
}

#[test]
fn uninstall_roundtrip_restores_original() {
    for (name, tool) in [
        ("claude-2sp.json", Tool::Claude),
        ("claude-4sp.json", Tool::Claude),
        ("codex-2sp.json", Tool::Codex),
        ("codex-4sp.json", Tool::Codex),
    ] {
        let (original, contents) = installed(name, tool);
        assert_ne!(original, contents, "{name}");
        let un = uninstall(Some(&contents), None).unwrap();
        assert!(un.changed, "{name}");
        assert_eq!(un.contents, original, "{name} must come back byte for byte");
        assert!(!uninstall(Some(&un.contents), None).unwrap().changed);
    }
}

#[test]
fn keeps_indentation_and_missing_trailing_newline_of_the_original() {
    let tabs = "{\n\t\"a\": 1\n}";
    let merge = install(Some(tabs), &spec(Tool::Claude)).unwrap();
    assert!(
        merge
            .contents
            .starts_with("{\n\t\"a\": 1,\n\t\"hooks\": {\n\t\t\"SessionStart\"")
    );
    assert!(merge.contents.ends_with('}'));
    let compact = install(Some("{\"a\":1}\n"), &spec(Tool::Claude)).unwrap();
    assert!(
        compact.contents.starts_with("{\n  \"a\": 1,"),
        "default 2 spaces"
    );
    let crlf = "{\r\n  \"a\": 1\r\n}\r\n";
    let merge = install(Some(crlf), &spec(Tool::Claude)).unwrap();
    assert!(merge.contents.contains("\r\n") && !merge.contents.replace("\r\n", "").contains('\n'));
    let back = uninstall(Some(&merge.contents), Some(&merge.absent_events)).unwrap();
    assert_eq!(
        back.contents,
        "{\r\n  \"a\": 1,\r\n  \"hooks\": {}\r\n}\r\n"
    );
}

#[test]
fn uninstall_keeps_groups_with_other_commands() {
    let mut config = parse(&fixture("claude-2sp.json"));
    config["hooks"]["SessionStart"][0]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!(
            {"type": "command", "command": "/x/bin/aiannotations-hook claude start", "timeout": 5}
        ));
    let un = uninstall(Some(&serde_json::to_string_pretty(&config).unwrap()), None).unwrap();
    assert!(un.changed);
    assert_eq!(parse(&un.contents), parse(&fixture("claude-2sp.json")));
}

#[test]
fn uninstall_of_missing_file_is_noop() {
    let un = uninstall(None, None).unwrap();
    assert!(!un.changed);
    assert_eq!(un.contents, "");
}

#[test]
fn uninstall_without_hooks_key_is_noop() {
    let text = "{\"a\": 1}\n";
    let un = uninstall(Some(text), None).unwrap();
    assert!(!un.changed);
    assert_eq!(un.contents, text);
}

#[test]
fn install_returns_contents_without_touching_input() {
    let original = fixture("claude-2sp.json");
    let merge = install(Some(&original), &spec(Tool::Claude)).unwrap();
    assert!(merge.contents.contains("hook claude start"));
    assert_eq!(original, fixture("claude-2sp.json"));
}

#[test]
fn refuses_invalid_json_untouched() {
    for text in [
        "{ invalido",
        "[]",
        "null",
        "\"x\"",
        "{\"hooks\": []}",
        "{\"hooks\": {\"SessionStart\": {}}}",
    ] {
        assert!(
            matches!(
                install(Some(text), &spec(Tool::Claude)),
                Err(HookError::InvalidJson(_))
            ),
            "{text}"
        );
        assert!(
            matches!(
                status(Some(text), &spec(Tool::Claude)),
                Err(HookError::InvalidJson(_))
            ),
            "{text}"
        );
    }
    for text in [
        "{ invalido",
        "[]",
        "{\"hooks\": 3}",
        "{\"hooks\": {\"Stop\": \"x\"}}",
    ] {
        assert!(
            matches!(uninstall(Some(text), None), Err(HookError::InvalidJson(_))),
            "{text}"
        );
    }
}

#[test]
fn reinstall_updates_old_group_then_idempotent() {
    let s = spec(Tool::Codex);
    let first = install(Some(&fixture("codex-2sp.json")), &s).unwrap();
    let mut config = parse(&first.contents);
    config["hooks"]["SessionStart"][1]["matcher"] = json!("startup|resume|clear|compact");
    config["hooks"]["SessionStart"][1]["hooks"][0]["timeout"] = json!(10);
    let old = serde_json::to_string_pretty(&config).unwrap();
    assert_eq!(status(Some(&old), &s).unwrap(), HookStatus::Outdated);

    let merge = install(Some(&old), &s).unwrap();
    assert!(merge.changed);
    let after = parse(&merge.contents);
    assert_eq!(after["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(
        after["hooks"]["SessionStart"][1],
        json!({"matcher": CODEX_MATCHER, "hooks": [{"type": "command", "command": s.start, "timeout": 3}]})
    );
    let again = install(Some(&merge.contents), &s).unwrap();
    assert!(!again.changed);
    assert_eq!(again.contents, merge.contents);
}

#[test]
fn updates_our_command_in_shared_group_only() {
    let mut config = parse(&fixture("claude-2sp.json"));
    config["hooks"]["SessionEnd"] = json!([{"hooks": [
        {"type": "command", "command": "echo outro", "timeout": 1},
        {"type": "command", "command": "/velho/bin/aiannotations-hook claude end", "timeout": 9},
    ]}]);
    let s = spec(Tool::Claude);
    let merge = install(Some(&config.to_string()), &s).unwrap();
    assert!(merge.changed);
    assert_eq!(
        parse(&merge.contents)["hooks"]["SessionEnd"],
        json!([{"hooks": [
            {"type": "command", "command": "echo outro", "timeout": 1},
            {"type": "command", "command": s.end, "timeout": 5},
        ]}])
    );
}

#[test]
fn absent_events_lists_only_missing_keys() {
    let merge = install(
        Some("{\"hooks\": {\"SessionEnd\": []}}"),
        &spec(Tool::Claude),
    )
    .unwrap();
    assert_eq!(merge.absent_events, [HookEvent::Start]);
    let none = install(Some(&merge.contents), &spec(Tool::Claude)).unwrap();
    assert!(none.absent_events.is_empty());
}

#[test]
fn uninstall_with_created_events_keeps_preexisting_empty_key_as_array() {
    let original = "{\n  \"hooks\": {\n    \"SessionEnd\": []\n  }\n}\n";
    let s = spec(Tool::Claude);
    let merge = install(Some(original), &s).unwrap();
    let un = uninstall(Some(&merge.contents), Some(&merge.absent_events)).unwrap();
    assert_eq!(un.contents, original);
    // without state, the emptied key is dropped
    let stateless = uninstall(Some(&merge.contents), None).unwrap();
    assert_eq!(parse(&stateless.contents), json!({"hooks": {}}));
    // with empty state nothing was created by us: both keys go back to []
    let kept = uninstall(Some(&merge.contents), Some(&[])).unwrap();
    assert_eq!(
        parse(&kept.contents),
        json!({"hooks": {"SessionEnd": [], "SessionStart": []}})
    );
}

#[test]
fn status_reports_legacy_outdated_current() {
    let s = spec(Tool::Claude);
    let base = fixture("claude-2sp.json");
    assert_eq!(status(None, &s).unwrap(), HookStatus::NotInstalled);
    assert_eq!(status(Some(" "), &s).unwrap(), HookStatus::NotInstalled);
    assert_eq!(status(Some(&base), &s).unwrap(), HookStatus::NotInstalled);

    let current = install(Some(&base), &s).unwrap().contents;
    assert_eq!(status(Some(&current), &s).unwrap(), HookStatus::Current);

    let v1 = |cmd_start: &str, cmd_end: &str| {
        json!({"hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": cmd_start, "timeout": 5}]}],
            "SessionEnd": [{"hooks": [{"type": "command", "command": cmd_end, "timeout": 5}]}],
        }})
        .to_string()
    };
    let legacy = v1(
        "/p/bin/aiannotations-hook claude start",
        "/p/bin/aiannotations-hook claude end",
    );
    assert_eq!(status(Some(&legacy), &s).unwrap(), HookStatus::Legacy);

    // new command on one event, v1 on the other: needs an update
    let mixed = v1(&s.start, "/p/bin/aiannotations-hook claude end");
    assert_eq!(status(Some(&mixed), &s).unwrap(), HookStatus::Outdated);
    // different command (moved app), different timeout, missing event
    let moved = v1("'/other/aiannotations' hook claude start", &s.end);
    assert_eq!(status(Some(&moved), &s).unwrap(), HookStatus::Outdated);
    let mut config = parse(&current);
    config["hooks"]["SessionEnd"][0]["hooks"][0]["timeout"] = json!(99);
    assert_eq!(
        status(Some(&config.to_string()), &s).unwrap(),
        HookStatus::Outdated
    );
    let mut config = parse(&current);
    config["hooks"]
        .as_object_mut()
        .unwrap()
        .shift_remove("SessionEnd");
    assert_eq!(
        status(Some(&config.to_string()), &s).unwrap(),
        HookStatus::Outdated
    );

    // Codex: a missing matcher is outdated too
    let c = spec(Tool::Codex);
    let codex = install(None, &c).unwrap().contents;
    assert_eq!(status(Some(&codex), &c).unwrap(), HookStatus::Current);
    let mut config = parse(&codex);
    config["hooks"]["SessionStart"][0]
        .as_object_mut()
        .unwrap()
        .shift_remove("matcher");
    assert_eq!(
        status(Some(&config.to_string()), &c).unwrap(),
        HookStatus::Outdated
    );
}

#[test]
fn is_ours_matches_new_and_v1_commands_only() {
    for yes in [
        "/p/bin/aiannotations-hook claude start",
        "'/opt/AI Annotations/aiannotations' hook claude start",
        "\"C:\\Program Files\\AIAnnotations\\aiannotations.exe\" hook codex end",
        "/usr/bin/aiannotations hook claude start",
        "aiannotations hook claude start",
        "C:\\x\\aiannotations.exe hook claude start",
    ] {
        assert!(is_ours(yes), "{yes}");
    }
    for no in [
        "aiannotations-other hook claude start",
        "/x/aiannotationsfoo hook claude start",
        "echo aiannotations",
        "echo aiannotations hook x",
        "/usr/bin/aiannotations",
        "/usr/bin/aiannotations hooks claude start",
        "'/opt/aiannotations-old' hook claude start",
        "'/unterminated/aiannotations hook x",
        "",
    ] {
        assert!(!is_ours(no), "{no}");
    }
}

#[test]
fn unified_diff_shows_added_lines() {
    let old = "{\n  \"a\": 1\n}\n";
    let new = "{\n  \"a\": 1,\n  \"b\": 2\n}\n";
    let diff = unified_diff("settings.json", Some(old), new);
    assert!(
        diff.starts_with("--- a/settings.json\n+++ b/settings.json\n"),
        "{diff}"
    );
    assert!(diff.contains("@@"));
    assert!(diff.contains("+  \"b\": 2\n"));
    assert!(diff.contains("-  \"a\": 1\n"));
    let created = unified_diff("hooks.json", None, "{}\n");
    assert!(created.contains("+{}\n"));
    assert!(!created.contains("\n-"));
    assert_eq!(unified_diff("x", Some("a\n"), "a\n"), "");
}

#[test]
fn hook_enums_serialize_snake_case() {
    assert_eq!(
        serde_json::to_string(&HookEvent::Start).unwrap(),
        "\"start\""
    );
    assert_eq!(
        serde_json::to_string(&HookStatus::NotInstalled).unwrap(),
        "\"not_installed\""
    );
}
