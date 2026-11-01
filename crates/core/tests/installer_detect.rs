use aiannotations_core::installer::detect::{
    cli_beside, find_cli, hook_command, hook_config_path, hook_spec, powershell_command,
};
use aiannotations_core::installer::hooks::{HookEvent, is_ours};
use aiannotations_core::model::Tool;
use aiannotations_core::paths::Paths;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

#[test]
fn find_cli_none_when_absent() {
    let home = tempfile::tempdir().unwrap();
    let empty = tempfile::tempdir().unwrap();
    let found = find_cli(
        Tool::Codex,
        None,
        home.path(),
        Some(empty.path().as_os_str()),
    );
    // The system folders may hold a real codex on the dev machine; the contract is that
    // the result is never the bare name.
    if let Some(p) = found {
        assert!(p.is_absolute());
    }
}

#[test]
fn find_cli_uses_override() {
    let home = tempfile::tempdir().unwrap();
    let found = find_cli(
        Tool::Claude,
        Some(OsStr::new("/custom/claude")),
        home.path(),
        None,
    );
    assert_eq!(found, Some(PathBuf::from("/custom/claude")));
}

#[test]
fn find_cli_finds_in_home_local_bin() {
    let home = tempfile::tempdir().unwrap();
    let bin = home.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let file = bin.join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::write(&file, "x").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(
        find_cli(Tool::Codex, None, home.path(), Some(OsStr::new(""))),
        Some(file)
    );
}

#[test]
fn config_paths_follow_claude_dir_and_codex_home() {
    let home = Path::new("/h");
    let paths = Paths::from_env(
        |k| match k {
            "CLAUDE_CONFIG_DIR" => Some("/cfg/claude".into()),
            "CODEX_HOME" => Some("/cfg/codex".into()),
            _ => None,
        },
        home,
        Path::new("/data"),
    );
    assert_eq!(
        hook_config_path(Tool::Claude, &paths),
        PathBuf::from("/cfg/claude/settings.json")
    );
    assert_eq!(
        hook_config_path(Tool::Codex, &paths),
        PathBuf::from("/cfg/codex/hooks.json")
    );
}

#[test]
fn cli_beside_adds_exe_on_windows() {
    let beside = cli_beside(Path::new("/opt/app/AIAnnotations"));
    let expected = if cfg!(windows) {
        "/opt/app/aiannotations.exe"
    } else {
        "/opt/app/aiannotations"
    };
    assert_eq!(beside, PathBuf::from(expected));
}

#[test]
fn hook_command_unix_form() {
    let cmd = hook_command(
        Path::new("/opt/AI Annotations/aiannotations"),
        Tool::Codex,
        HookEvent::End,
        false,
    );
    assert_eq!(cmd, "'/opt/AI Annotations/aiannotations' hook codex end");
}

#[test]
fn hook_command_windows_form() {
    let cmd = hook_command(
        Path::new(r"C:\Program Files\AI Annotations\aiannotations.exe"),
        Tool::Claude,
        HookEvent::Start,
        true,
    );
    assert_eq!(
        cmd,
        r#""C:/Program Files/AI Annotations/aiannotations.exe" hook claude start"#
    );
}

#[test]
fn hook_spec_commands_are_ours() {
    for windows in [false, true] {
        for tool in [Tool::Claude, Tool::Codex] {
            let cli = if windows {
                Path::new("C:/Apps/Ana D'Ávila/aiannotations.exe")
            } else {
                Path::new("/tmp/Ana D'Ávila/aiannotations")
            };
            let spec = hook_spec(cli, tool, windows);
            assert_eq!(spec.tool, tool);
            assert!(is_ours(&spec.start), "{}", spec.start);
            assert!(is_ours(&spec.end), "{}", spec.end);
            assert!(
                spec.start
                    .ends_with(&format!("hook {} start", tool.as_str()))
            );
            assert!(spec.end.ends_with(&format!("hook {} end", tool.as_str())));
        }
    }
}

#[cfg(unix)]
#[test]
fn hook_command_quotes_hostile_paths() {
    use std::os::unix::fs::PermissionsExt;
    let base = tempfile::tempdir().unwrap();
    for segment in ["joão/Meus Apps", "Ana D'Ávila", "a$b `c` \"d\" ;e"] {
        let dir = base.path().join(segment);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.txt");
        let script = dir.join("aiannotations");
        std::fs::write(
            &script,
            "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/out.txt\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let cmd = hook_command(&script, Tool::Claude, HookEvent::Start, false);
        assert!(is_ours(&cmd), "{cmd}");
        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .status()
            .unwrap();
        assert!(status.success(), "{cmd}");
        assert_eq!(
            std::fs::read_to_string(&out).unwrap().trim(),
            "hook claude start",
            "{cmd}"
        );
    }
}

#[test]
fn powershell_command_uses_call_operator_and_single_quotes() {
    let cmd = powershell_command(
        Path::new(r"C:\Program Files\Ana D'Ávila $x\aiannotations.exe"),
        Tool::Codex,
        HookEvent::Start,
    );
    assert_eq!(
        cmd,
        "& 'C:/Program Files/Ana D''Ávila $x/aiannotations.exe' hook codex start"
    );
}

#[test]
fn only_codex_on_windows_gets_windows_commands() {
    let cli = Path::new("C:/Apps/aiannotations.exe");
    let spec = hook_spec(cli, Tool::Codex, true);
    assert_eq!(
        spec.start_windows.as_deref(),
        Some("& 'C:/Apps/aiannotations.exe' hook codex start")
    );
    assert_eq!(
        spec.end_windows.as_deref(),
        Some("& 'C:/Apps/aiannotations.exe' hook codex end")
    );
    for (tool, windows) in [
        (Tool::Claude, true),
        (Tool::Codex, false),
        (Tool::Claude, false),
    ] {
        let spec = hook_spec(cli, tool, windows);
        assert_eq!(spec.start_windows, None, "{tool:?} windows={windows}");
        assert_eq!(spec.end_windows, None, "{tool:?} windows={windows}");
    }
}
