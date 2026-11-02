//! Settings-driven launching: the apps view, "open with" and the resume that opens a terminal.
//! Nothing is spawned: `spawn` and `copy` are closures that record what they get.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use aiannotations_core::Error;
use aiannotations_core::model::Tool;
use aiannotations_core::testing::{Env, insert_project};
use aiannotations_core::user_settings::{CustomApp, UserSettings};
use aiannotations_services::actions;
use aiannotations_services::launch::apps::{self, Opened, Target};
use aiannotations_services::launch::argv::LaunchSpec;
use aiannotations_services::launch::catalog::Family;
use aiannotations_services::launch::detect::DetectedApp;
use aiannotations_services::launch::{AppKind, Os, Shell, ShellKind, shell};

fn det(id: &str, kind: AppKind, family: Family, program: &[&str]) -> DetectedApp {
    DetectedApp {
        id: id.into(),
        label: id.to_uppercase(),
        kind,
        family,
        program: program.iter().map(OsString::from).collect(),
    }
}

fn vscode() -> DetectedApp {
    det(
        "vscode",
        AppKind::Editor,
        Family::VsCode,
        &["/usr/bin/code"],
    )
}

fn kitty() -> DetectedApp {
    det(
        "kitty",
        AppKind::Terminal,
        Family::Kitty,
        &["/usr/bin/kitty"],
    )
}

fn bash() -> Shell {
    Shell {
        path: PathBuf::from("/bin/bash"),
        kind: ShellKind::Posix,
    }
}

fn custom(id: &str, kind: AppKind, exe: &Path, args: &str) -> CustomApp {
    CustomApp {
        id: id.into(),
        label: format!("Mine {id}"),
        kind,
        exe: exe.to_path_buf(),
        args: args.into(),
    }
}

fn installer_code(e: &Error) -> &str {
    match e {
        Error::Installer(m) => m,
        _ => "",
    }
}

fn strs(v: &[OsString]) -> Vec<String> {
    v.iter().map(|x| x.to_string_lossy().into_owned()).collect()
}

/// `build` for a Claude resume of id `x` in `dir`.
fn resume_in(dir: &Path) -> impl Fn(ShellKind) -> aiannotations_core::Result<String> + '_ {
    move |k| {
        Ok(shell::resume_command(
            Tool::Claude,
            "x",
            &dir.to_string_lossy(),
            k,
        ))
    }
}

// ---- view ----------------------------------------------------------------------------------

#[test]
fn view_falls_back_when_choice_uninstalled() {
    let detected = [vscode(), kitty()];
    let user = UserSettings {
        editor: Some("cursor".into()),
        terminal: Some("kitty".into()),
        ..Default::default()
    };
    let v = apps::view(&detected, &[bash()], &bash(), &user);
    assert_eq!(v.editor, "vscode");
    assert_eq!(v.editor_choice.as_deref(), Some("cursor"));
    assert_eq!(v.terminal.as_deref(), Some("kitty"));
    assert_eq!(v.terminal_choice.as_deref(), Some("kitty"));
}

#[test]
fn view_terminal_none_when_nothing_detected() {
    let v = apps::view(&[vscode()], &[], &bash(), &UserSettings::default());
    assert!(v.terminals.is_empty());
    assert_eq!(v.terminal, None);
    assert_eq!(v.terminal_choice, None);
    assert_eq!(v.shell, "/bin/bash");
    assert_eq!(v.shell_choice, None);
}

#[test]
fn view_lists_custom_apps_after_detected() {
    let exe = PathBuf::from("/opt/foot");
    let user = UserSettings {
        terminal: Some("custom-2".into()),
        shell: Some("/usr/bin/fish".into()),
        workspaces_dir: Some("/w".into()),
        custom_apps: vec![
            custom("custom-1", AppKind::Editor, &exe, ""),
            custom("custom-2", AppKind::Terminal, &exe, ""),
        ],
        ..Default::default()
    };
    let v = apps::view(&[vscode(), kitty()], &[bash()], &bash(), &user);
    let ids = |l: &[apps::AppEntry]| -> Vec<(String, bool)> {
        l.iter().map(|e| (e.id.clone(), e.custom)).collect()
    };
    assert_eq!(
        ids(&v.editors),
        [("vscode".into(), false), ("custom-1".into(), true)]
    );
    assert_eq!(
        ids(&v.terminals),
        [("kitty".into(), false), ("custom-2".into(), true)]
    );
    assert_eq!(v.terminal.as_deref(), Some("custom-2"));
    assert_eq!(v.shell, "/usr/bin/fish");
    assert_eq!(v.shell_choice.as_deref(), Some("/usr/bin/fish"));
    // The configured shell is offered even when detection missed it.
    assert_eq!(v.shells, ["/bin/bash", "/usr/bin/fish"]);
    assert_eq!(v.workspaces_dir.as_deref(), Some("/w"));
    assert_eq!(v.custom_apps.len(), 2);
}

#[test]
fn view_serializes_for_the_ui() {
    let v = apps::view(&[vscode()], &[bash()], &bash(), &UserSettings::default());
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["editor"], "vscode");
    assert_eq!(json["terminal"], serde_json::Value::Null);
    assert_eq!(json["editors"][0]["custom"], false);
    let outcome = apps::ResumeOutcome {
        command: "c".into(),
        opened: Opened::ShellOnly,
        error: None,
    };
    assert_eq!(
        serde_json::to_value(&outcome).unwrap()["opened"],
        "shell_only"
    );
}

// ---- resume --------------------------------------------------------------------------------

#[test]
fn resume_copies_even_without_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let copied = RefCell::new(Vec::<String>::new());
    let out = apps::resume_in_terminal(
        &[vscode()],
        &UserSettings::default(),
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |c| {
            copied.borrow_mut().push(c.to_string());
            Ok(())
        },
        &|_| panic!("nothing to spawn"),
    )
    .unwrap();
    assert_eq!(out.opened, Opened::None);
    assert_eq!(out.error, None);
    assert_eq!(*copied.borrow(), std::slice::from_ref(&out.command));
    assert!(
        out.command.ends_with("claude --resume x"),
        "{}",
        out.command
    );
}

#[test]
fn resume_spawn_failure_still_copied() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("removed-terminal");
    let user = UserSettings {
        terminal: Some("custom-1".into()),
        custom_apps: vec![custom("custom-1", AppKind::Terminal, &gone, "")],
        ..Default::default()
    };
    let copied = RefCell::new(Vec::<String>::new());
    let out = apps::resume_in_terminal(
        &[kitty()],
        &user,
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |c| {
            copied.borrow_mut().push(c.to_string());
            Ok(())
        },
        &|_| panic!("a missing executable is not spawned"),
    )
    .unwrap();
    assert_eq!(out.opened, Opened::Failed);
    assert_eq!(out.error.as_deref(), Some("executable not found"));
    assert_eq!(*copied.borrow(), std::slice::from_ref(&out.command));

    // The spawn itself failing (detected terminal): same outcome, the spawn error kept.
    let copied = RefCell::new(0);
    let out = apps::resume_in_terminal(
        &[kitty()],
        &UserSettings::default(),
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |_| {
            *copied.borrow_mut() += 1;
            Ok(())
        },
        &|_| Err(Error::Io(std::io::Error::other("boom"))),
    )
    .unwrap();
    assert_eq!(out.opened, Opened::Failed);
    assert!(out.error.as_deref().unwrap().contains("boom"));
    assert_eq!(*copied.borrow(), 1);
}

#[test]
fn resume_copy_failure_is_an_error_and_nothing_opens() {
    let dir = tempfile::tempdir().unwrap();
    let got = apps::resume_in_terminal(
        &[kitty()],
        &UserSettings::default(),
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |_| Err(Error::Io(std::io::Error::other("clipboard"))),
        &|_| panic!("not opened when the copy failed"),
    );
    assert!(got.is_err());
}

#[test]
fn resume_copies_before_opening_with_configured_shell() {
    let dir = tempfile::tempdir().unwrap();
    let user = UserSettings {
        shell: Some("/usr/bin/fish".into()),
        ..Default::default()
    };
    let log = RefCell::new(Vec::<String>::new());
    let spawned = RefCell::new(None::<LaunchSpec>);
    let out = apps::resume_in_terminal(
        &[vscode(), kitty()],
        &user,
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |c| {
            log.borrow_mut().push(format!("copy {c}"));
            Ok(())
        },
        &|spec| {
            log.borrow_mut().push("spawn".into());
            *spawned.borrow_mut() = Some(spec.clone());
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(out.opened, Opened::Ran);
    assert_eq!(
        *log.borrow(),
        [format!("copy {}", out.command), "spawn".to_string()]
    );
    let spec = spawned.borrow().clone().unwrap();
    assert_eq!(spec.program, OsString::from("/usr/bin/kitty"));
    let args = strs(&spec.args);
    assert_eq!(args[2], "/usr/bin/fish", "{args:?}");
    assert_eq!(
        args.last().unwrap(),
        &format!("{}; exec '/usr/bin/fish' -l", out.command)
    );
}

#[test]
fn resume_quotes_for_console_shell() {
    let dir = tempfile::tempdir().unwrap();
    let ps = det(
        "powershell",
        AppKind::Terminal,
        Family::PowerShellConsole,
        &[r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"],
    );
    let user = UserSettings {
        shell: Some("/bin/bash".into()),
        ..Default::default()
    };
    let spawned = RefCell::new(None::<LaunchSpec>);
    let out = apps::resume_in_terminal(
        &[ps],
        &user,
        Os::Windows,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |_| Ok(()),
        &|spec| {
            *spawned.borrow_mut() = Some(spec.clone());
            Ok(())
        },
    )
    .unwrap();
    assert!(out.command.starts_with("Set-Location"), "{}", out.command);
    assert_eq!(out.opened, Opened::Ran);
    assert!(spawned.borrow().as_ref().unwrap().new_console);
}

#[test]
fn resume_with_shell_only_terminal() {
    let dir = tempfile::tempdir().unwrap();
    // A manual terminal without `{cmd}` cannot run the command: the user pastes it.
    let exe = std::env::current_exe().unwrap();
    let user = UserSettings {
        terminal: Some("custom-1".into()),
        custom_apps: vec![custom("custom-1", AppKind::Terminal, &exe, "--dir {dir}")],
        ..Default::default()
    };
    let out = apps::resume_in_terminal(
        &[],
        &user,
        Os::Linux,
        &bash(),
        dir.path(),
        &resume_in(dir.path()),
        &mut |_| Ok(()),
        &|_| Ok(()),
    )
    .unwrap();
    assert_eq!(out.opened, Opened::ShellOnly);
}

// ---- open with -----------------------------------------------------------------------------

fn open(
    detected: &[DetectedApp],
    user: &UserSettings,
    dir: &Path,
    target: Target,
) -> (aiannotations_core::Result<()>, Option<LaunchSpec>) {
    let spawned = RefCell::new(None::<LaunchSpec>);
    let r = apps::open_dir(detected, user, Os::Linux, dir, target, &|spec| {
        *spawned.borrow_mut() = Some(spec.clone());
        Ok(())
    });
    (r, spawned.into_inner())
}

#[test]
fn open_dir_default_editor_and_named_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let (r, spec) = open(
        &[vscode(), kitty()],
        &UserSettings::default(),
        dir.path(),
        Target::Default,
    );
    r.unwrap();
    let spec = spec.unwrap();
    assert_eq!(spec.program, OsString::from("/usr/bin/code"));
    assert_eq!(spec.args, [dir.path().as_os_str().to_owned()]);

    let (r, spec) = open(
        &[vscode(), kitty()],
        &UserSettings::default(),
        dir.path(),
        Target::App("kitty".into()),
    );
    r.unwrap();
    let args = strs(&spec.unwrap().args);
    assert_eq!(
        args,
        [
            "--directory".to_string(),
            dir.path().to_string_lossy().into_owned()
        ]
    );
}

#[test]
fn open_dir_unknown_app_or_bad_dir_fails() {
    let dir = tempfile::tempdir().unwrap();
    let (r, spec) = open(
        &[vscode()],
        &UserSettings::default(),
        dir.path(),
        Target::App("nope".into()),
    );
    assert_eq!(installer_code(&r.unwrap_err()), "app not found");
    assert!(spec.is_none());

    let (r, spec) = open(
        &[vscode()],
        &UserSettings::default(),
        Path::new("relative"),
        Target::Default,
    );
    assert!(r.is_err());
    assert!(spec.is_none());
    let (r, _) = open(
        &[vscode()],
        &UserSettings::default(),
        &dir.path().join("missing"),
        Target::Default,
    );
    assert!(r.is_err());
}

#[test]
fn open_dir_custom_app_checks_exe() {
    let dir = tempfile::tempdir().unwrap();
    let gone = dir.path().join("gone");
    let exe = std::env::current_exe().unwrap();
    let user = UserSettings {
        custom_apps: vec![
            custom("custom-1", AppKind::Editor, &gone, ""),
            custom("custom-2", AppKind::Editor, &exe, "--open {dir}"),
        ],
        ..Default::default()
    };
    let (r, spec) = open(&[], &user, dir.path(), Target::App("custom-1".into()));
    assert_eq!(installer_code(&r.unwrap_err()), "executable not found");
    assert!(spec.is_none());

    let (r, spec) = open(&[], &user, dir.path(), Target::App("custom-2".into()));
    r.unwrap();
    let spec = spec.unwrap();
    assert_eq!(spec.program, exe.as_os_str());
    assert_eq!(
        strs(&spec.args),
        [
            "--open".to_string(),
            dir.path().to_string_lossy().into_owned()
        ]
    );
}

#[test]
fn open_dir_uses_workspace_for_vscode_only() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("proj");
    let workspaces = root.path().join("ws");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&workspaces).unwrap();
    let file = workspaces.join("proj.code-workspace");
    let folder = serde_json::json!({"folders": [{"path": project.to_string_lossy()}]});
    std::fs::write(&file, folder.to_string()).unwrap();
    let user = UserSettings {
        workspaces_dir: Some(workspaces.clone()),
        ..Default::default()
    };
    let zed = det("zed", AppKind::Editor, Family::Zed, &["/usr/bin/zed"]);

    let (r, spec) = open(&[vscode(), zed], &user, &project, Target::Default);
    r.unwrap();
    let expected = apps::strip_verbatim(&std::fs::canonicalize(&file).unwrap());
    assert_eq!(spec.unwrap().args, [expected.into_os_string()]);

    let zed = det("zed", AppKind::Editor, Family::Zed, &["/usr/bin/zed"]);
    let (r, spec) = open(&[vscode(), zed], &user, &project, Target::App("zed".into()));
    r.unwrap();
    assert_eq!(spec.unwrap().args, [project.as_os_str().to_owned()]);
}

#[test]
fn strip_verbatim_keeps_unc() {
    let s = |p: &str| {
        apps::strip_verbatim(Path::new(p))
            .to_string_lossy()
            .into_owned()
    };
    assert_eq!(
        s(r"\\?\C:\Users\x\a.code-workspace"),
        r"C:\Users\x\a.code-workspace"
    );
    assert_eq!(s(r"\\?\UNC\srv\share\a"), r"\\?\UNC\srv\share\a");
    assert_eq!(s("/home/x/a"), "/home/x/a");
}

// ---- ongoing lookup ------------------------------------------------------------------------

#[test]
fn resume_ongoing_rejects_traversal() {
    let env = Env::new();
    let project = env.base.join("proj");
    let ongoing_dir = project.join("docs").join("ongoing-sessions");
    std::fs::create_dir_all(&ongoing_dir).unwrap();
    std::fs::write(ongoing_dir.join("a.md"), "# A\n").unwrap();
    std::fs::write(project.join("docs").join("x.md"), "# X\n").unwrap();
    let id = insert_project(&env.conn, &project.to_string_lossy());

    for bad in ["../x.md", "a/b.md", r"a\b.md", "..", "missing.md", "b.txt"] {
        let err = actions::ongoing(&env.conn, id, bad).unwrap_err();
        assert_eq!(installer_code(&err), "invalid ongoing", "{bad}");
    }
    assert_eq!(
        installer_code(&actions::ongoing(&env.conn, 999, "a.md").unwrap_err()),
        "invalid ongoing"
    );

    let found = actions::ongoing(&env.conn, id, "a.md").unwrap();
    assert_eq!(found.name, "a");
    assert_eq!(Path::new(&found.project_path), project);
}
