use std::ffi::OsString;
use std::path::{Path, PathBuf};

use aiannotations_core::model::Tool;
use aiannotations_services::launch::argv::*;
use aiannotations_services::launch::catalog::Family;
use aiannotations_services::launch::detect::DetectedApp;
use aiannotations_services::launch::shell::resume_command;
use aiannotations_services::launch::{AppKind, Os, Shell, ShellKind};

const CMD: &str = "claude --resume x";

fn app(family: Family, kind: AppKind, program: &[&str]) -> DetectedApp {
    DetectedApp {
        id: "t".into(),
        label: "T".into(),
        kind,
        family,
        program: program.iter().map(OsString::from).collect(),
    }
}

fn bash() -> Shell {
    Shell {
        path: PathBuf::from("/bin/bash"),
        kind: ShellKind::Posix,
    }
}

/// `run_and_stay` for bash and CMD.
fn s() -> Vec<&'static str> {
    vec![
        "/bin/bash",
        "-l",
        "-c",
        "claude --resume x; exec '/bin/bash' -l",
    ]
}

fn strs(v: &[OsString]) -> Vec<String> {
    v.iter().map(|x| x.to_string_lossy().into_owned()).collect()
}

fn cat(parts: &[&[&str]]) -> Vec<String> {
    parts
        .iter()
        .flat_map(|p| p.iter().map(|x| x.to_string()))
        .collect()
}

/// Spec for the terminal, with a command and shell only.
fn both(a: &DetectedApp, os: Os) -> ((LaunchSpec, Ran), (LaunchSpec, Ran)) {
    let sh = bash();
    let dir = Path::new("/p q");
    (
        terminal_spec(a, os, dir, Some((&sh, CMD))),
        terminal_spec(a, os, dir, None),
    )
}

fn check(a: DetectedApp, os: Os, with: Vec<String>, only: Vec<String>) {
    let ((w, rw), (o, ro)) = both(&a, os);
    let full = |sp: &LaunchSpec| {
        let mut v = vec![sp.program.to_string_lossy().into_owned()];
        v.extend(strs(&sp.args));
        v
    };
    assert_eq!(full(&w), with);
    assert_eq!(full(&o), only);
    assert_eq!(rw, Ran::Command);
    assert_eq!(ro, Ran::ShellOnly);
}

#[test]
fn terminal_kitty() {
    check(
        app(Family::Kitty, AppKind::Terminal, &["kitty"]),
        Os::Linux,
        cat(&[&["kitty", "--directory", "/p q"], &s()]),
        cat(&[&["kitty", "--directory", "/p q"]]),
    );
}

#[test]
fn terminal_ghostty_linux_and_macos() {
    check(
        app(Family::Ghostty, AppKind::Terminal, &["ghostty"]),
        Os::Linux,
        cat(&[&["ghostty", "--working-directory=/p q", "-e"], &s()]),
        cat(&[&["ghostty", "--working-directory=/p q"]]),
    );
    check(
        app(
            Family::Ghostty,
            AppKind::Terminal,
            &["open", "-na", "/Applications/Ghostty.app", "--args"],
        ),
        Os::MacOs,
        cat(&[
            &["open", "-na", "/Applications/Ghostty.app", "--args"],
            &["--working-directory=/p q", "-e"],
            &s(),
        ]),
        cat(&[
            &["open", "-na", "/Applications/Ghostty.app", "--args"],
            &["--working-directory=/p q"],
        ]),
    );
}

#[test]
fn terminal_alacritty() {
    check(
        app(Family::Alacritty, AppKind::Terminal, &["alacritty"]),
        Os::Linux,
        cat(&[&["alacritty", "--working-directory", "/p q", "-e"], &s()]),
        cat(&[&["alacritty", "--working-directory", "/p q"]]),
    );
}

#[test]
fn terminal_wezterm() {
    check(
        app(Family::WezTerm, AppKind::Terminal, &["wezterm"]),
        Os::Linux,
        cat(&[&["wezterm", "start", "--cwd", "/p q", "--"], &s()]),
        cat(&[&["wezterm", "start", "--cwd", "/p q"]]),
    );
}

#[test]
fn terminal_ptyxis() {
    check(
        app(Family::Ptyxis, AppKind::Terminal, &["ptyxis"]),
        Os::Linux,
        cat(&[
            &["ptyxis", "--new-window", "--working-directory=/p q", "--"],
            &s(),
        ]),
        cat(&[&["ptyxis", "--new-window", "--working-directory=/p q"]]),
    );
}

#[test]
fn terminal_gnome_terminal() {
    check(
        app(
            Family::GnomeTerminal,
            AppKind::Terminal,
            &["gnome-terminal"],
        ),
        Os::Linux,
        cat(&[&["gnome-terminal", "--working-directory=/p q", "--"], &s()]),
        cat(&[&["gnome-terminal", "--working-directory=/p q"]]),
    );
}

#[test]
fn terminal_konsole() {
    check(
        app(Family::Konsole, AppKind::Terminal, &["konsole"]),
        Os::Linux,
        cat(&[&["konsole", "--workdir", "/p q", "-e"], &s()]),
        cat(&[&["konsole", "--workdir", "/p q"]]),
    );
}

#[test]
fn terminal_generic() {
    let a = app(Family::Generic, AppKind::Terminal, &["xterm"]);
    let ((w, rw), (o, ro)) = both(&a, Os::Linux);
    assert_eq!(strs(&w.args), cat(&[&["-e"], &s()]));
    assert_eq!(rw, Ran::Command);
    assert!(o.args.is_empty());
    assert_eq!(o.cwd, Some(PathBuf::from("/p q")));
    assert_eq!(ro, Ran::ShellOnly);
}

#[test]
fn terminal_terminal_app() {
    let a = app(
        Family::TerminalApp,
        AppKind::Terminal,
        &["open", "-a", "/System/Applications/Utilities/Terminal.app"],
    );
    check(
        a,
        Os::MacOs,
        cat(&[
            &[
                "osascript",
                "-e",
                "tell application \"Terminal\"",
                "-e",
                "activate",
            ],
            &["-e", "do script \"claude --resume x\"", "-e", "end tell"],
        ]),
        cat(&[&[
            "open",
            "-a",
            "/System/Applications/Utilities/Terminal.app",
            "/p q",
        ]]),
    );
}

#[test]
fn terminal_iterm2() {
    let a = app(
        Family::ITerm2,
        AppKind::Terminal,
        &["open", "-a", "/Applications/iTerm.app"],
    );
    let ((w, _), (o, _)) = both(&a, Os::MacOs);
    assert_eq!(w.program, OsString::from("osascript"));
    let args = strs(&w.args);
    assert!(args.contains(&"set w to (create window with default profile)".to_string()));
    assert!(
        args.contains(&"tell current session of w to write text \"claude --resume x\"".to_string())
    );
    assert_eq!(o.program, OsString::from("open"));
    assert_eq!(
        strs(&o.args),
        cat(&[&["-a", "/Applications/iTerm.app", "/p q"]])
    );
}

#[test]
fn terminal_windows_terminal() {
    let a = app(Family::WindowsTerminal, AppKind::Terminal, &["wt.exe"]);
    let ((w, rw), (o, ro)) = both(&a, Os::Windows);
    assert_eq!(w.program, OsString::from("wt.exe"));
    assert!(w.args.is_empty());
    assert_eq!(
        w.raw_args.as_deref(),
        Some(r#"-d "/p q" /bin/bash -l -c "claude --resume x\; exec '/bin/bash' -l""#)
    );
    assert_eq!(o.raw_args.as_deref(), Some(r#"-d "/p q""#));
    assert_eq!((rw, ro), (Ran::Command, Ran::ShellOnly));
}

#[test]
fn terminal_powershell_console() {
    let a = app(Family::PowerShellConsole, AppKind::Terminal, &["pwsh.exe"]);
    let ((w, rw), (o, ro)) = both(&a, Os::Windows);
    assert_eq!(w.program, OsString::from("pwsh.exe"));
    assert_eq!(
        strs(&w.args),
        cat(&[&["-NoLogo", "-NoExit", "-Command", CMD]])
    );
    assert_eq!(strs(&o.args), cat(&[&["-NoLogo", "-NoExit"]]));
    for sp in [&w, &o] {
        assert_eq!(sp.cwd, Some(PathBuf::from("/p q")));
        assert!(sp.new_console);
    }
    assert_eq!((rw, ro), (Ran::Command, Ran::ShellOnly));
}

#[test]
fn terminal_cmd_console() {
    let a = app(Family::CmdConsole, AppKind::Terminal, &["cmd.exe"]);
    let ((w, rw), (o, ro)) = both(&a, Os::Windows);
    assert_eq!(w.program, OsString::from("cmd.exe"));
    assert!(w.args.is_empty());
    assert_eq!(w.raw_args.as_deref(), Some("/K claude --resume x"));
    assert!(o.args.is_empty());
    assert_eq!(o.raw_args, None);
    for sp in [&w, &o] {
        assert_eq!(sp.cwd, Some(PathBuf::from("/p q")));
        assert!(sp.new_console);
    }
    assert_eq!((rw, ro), (Ran::Command, Ran::ShellOnly));
}

#[test]
fn terminal_cmder() {
    let a = app(Family::Cmder, AppKind::Terminal, &["Cmder.exe"]);
    let ((w, rw), (o, ro)) = both(&a, Os::Windows);
    assert_eq!(w.raw_args.as_deref(), Some(r#"/START "/p q""#));
    assert_eq!(o.raw_args.as_deref(), Some(r#"/START "/p q""#));
    assert_eq!((rw, ro), (Ran::ShellOnly, Ran::ShellOnly));
}

#[test]
fn windows_terminal_escapes_semicolons() {
    let a = app(Family::WindowsTerminal, AppKind::Terminal, &["wt.exe"]);
    let sh = Shell {
        path: PathBuf::from("pwsh.exe"),
        kind: ShellKind::PowerShell,
    };
    let cmd = r"Set-Location -LiteralPath 'C:\a;b'; claude --resume x";
    let (sp, _) = terminal_spec(&a, Os::Windows, Path::new(r"C:\a;b"), Some((&sh, cmd)));
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(
            r#"-d "C:\a\;b" pwsh.exe -NoLogo -NoExit -Command "Set-Location -LiteralPath 'C:\a\;b'\; claude --resume x""#
        )
    );
}

#[test]
fn terminal_app_escapes_applescript() {
    let a = app(
        Family::TerminalApp,
        AppKind::Terminal,
        &["open", "-a", "T.app"],
    );
    let sh = bash();
    let (sp, _) = terminal_spec(&a, Os::MacOs, Path::new("/p"), Some((&sh, r#"echo "a\b""#)));
    assert!(strs(&sp.args).contains(&r#"do script "echo \"a\\b\"""#.to_string()));
}

#[test]
fn effective_shell_for_consoles() {
    let git_bash = Shell {
        path: PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        kind: ShellKind::Posix,
    };
    let ps = app(
        Family::PowerShellConsole,
        AppKind::Terminal,
        &[r"C:\pwsh.exe"],
    );
    let got = effective_shell(&ps, &git_bash);
    assert_eq!(got.kind, ShellKind::PowerShell);
    assert_eq!(got.path, PathBuf::from(r"C:\pwsh.exe"));
    let cmd = app(Family::CmdConsole, AppKind::Terminal, &["cmd.exe"]);
    assert_eq!(effective_shell(&cmd, &git_bash).kind, ShellKind::Cmd);
    let k = app(Family::Kitty, AppKind::Terminal, &["kitty"]);
    assert_eq!(effective_shell(&k, &git_bash), git_bash);
    // Cmder runs cmd.exe (with clink), whatever shell is configured.
    let cmder = app(Family::Cmder, AppKind::Terminal, &[r"C:\cmder\Cmder.exe"]);
    assert_eq!(
        effective_shell(&cmder, &git_bash),
        Shell {
            path: PathBuf::from("cmd.exe"),
            kind: ShellKind::Cmd,
        }
    );
}

#[test]
fn editor_vscode_uses_workspace_target() {
    let a = app(Family::VsCode, AppKind::Editor, &["code"]);
    let sp = editor_spec(&a, Os::Linux, Path::new("/w/p.code-workspace"));
    assert_eq!(sp.program, OsString::from("code"));
    assert_eq!(strs(&sp.args), vec!["/w/p.code-workspace"]);
}

#[test]
fn editor_jetbrains_uses_dir() {
    let a = app(Family::JetBrains, AppKind::Editor, &["phpstorm"]);
    let sp = editor_spec(&a, Os::Linux, Path::new("/p q"));
    assert_eq!(strs(&sp.args), vec!["/p q"]);
}

#[test]
fn editor_macos_open_a() {
    let a = app(
        Family::Zed,
        AppKind::Editor,
        &["open", "-a", "/Applications/Zed.app"],
    );
    let sp = editor_spec(&a, Os::MacOs, Path::new("/p"));
    assert_eq!(sp.program, OsString::from("open"));
    assert_eq!(strs(&sp.args), vec!["-a", "/Applications/Zed.app", "/p"]);
}

#[test]
fn editor_file_manager_per_os() {
    let a = app(Family::FileManager, AppKind::Editor, &["xdg-open"]);
    let l = editor_spec(&a, Os::Linux, Path::new("/p"));
    assert_eq!(
        (l.program, strs(&l.args)),
        (OsString::from("xdg-open"), vec!["/p".to_string()])
    );
    let m = editor_spec(&a, Os::MacOs, Path::new("/p"));
    assert_eq!(m.program, OsString::from("open"));
    let w = editor_spec(&a, Os::Windows, Path::new("C:/a b/"));
    assert_eq!(
        (w.program, strs(&w.args)),
        (OsString::from("explorer"), vec![r"C:\a b".to_string()])
    );
}

#[test]
fn template_split_and_expand() {
    let toks = split_template(r#"--new "{dir}" -x {cmd}"#).unwrap();
    assert_eq!(toks, vec!["--new", "{dir}", "-x", "{cmd}"]);
    let sh = bash();
    let (sp, ran) = custom_spec(
        Path::new("/usr/bin/foo"),
        &toks,
        AppKind::Terminal,
        Os::Linux,
        Path::new("/p q"),
        Some((&sh, CMD)),
    );
    assert_eq!(strs(&sp.args), cat(&[&["--new", "/p q", "-x"], &s()]));
    assert_eq!(ran, Ran::Command);
}

#[test]
fn template_cmd_inside_token() {
    let toks = split_template("--run=bash:{cmd}").unwrap();
    let sh = bash();
    let (sp, _) = custom_spec(
        Path::new("t"),
        &toks,
        AppKind::Terminal,
        Os::Linux,
        Path::new("/p"),
        Some((&sh, CMD)),
    );
    assert_eq!(strs(&sp.args), vec!["--run=bash:claude --resume x"]);
}

#[test]
fn template_unbalanced_quote_is_error() {
    assert_eq!(
        split_template(r#"--x "abc"#),
        Err(TemplateError::UnbalancedQuote)
    );
}

#[test]
fn template_editor_with_cmd_is_error() {
    assert_eq!(
        parse_template("{dir} {cmd}", AppKind::Editor),
        Err(TemplateError::EditorWithCommand)
    );
    assert!(parse_template("{dir} {cmd}", AppKind::Terminal).is_ok());
}

#[test]
fn template_terminal_without_cmd_is_shell_only() {
    let sh = bash();
    let toks = split_template("--cwd {dir}").unwrap();
    let (sp, ran) = custom_spec(
        Path::new("t"),
        &toks,
        AppKind::Terminal,
        Os::Linux,
        Path::new("/p"),
        Some((&sh, CMD)),
    );
    assert_eq!(strs(&sp.args), vec!["--cwd", "/p"]);
    assert_eq!(ran, Ran::ShellOnly);
}

#[test]
fn template_empty_defaults() {
    let (ed, _) = custom_spec(
        Path::new("e"),
        &[],
        AppKind::Editor,
        Os::Linux,
        Path::new("/p q"),
        None,
    );
    assert_eq!(strs(&ed.args), vec!["/p q"]);
    let sh = bash();
    let (t, ran) = custom_spec(
        Path::new("t"),
        &[],
        AppKind::Terminal,
        Os::Linux,
        Path::new("/p"),
        Some((&sh, CMD)),
    );
    assert_eq!(strs(&t.args), cat(&[&["-e"], &s()]));
    assert_eq!(ran, Ran::Command);
}

// ---- cmd.exe in non-console terminals (Windows) --------------------------------------------

fn cmd_exe() -> Shell {
    Shell {
        path: PathBuf::from("cmd.exe"),
        kind: ShellKind::Cmd,
    }
}

/// `cd /d "C:\a b" && claude --resume x`: cmd.exe would not understand it MSVC-quoted.
fn cmd_resume(dir: &str) -> String {
    resume_command(Tool::Claude, "x", dir, ShellKind::Cmd)
}

#[test]
fn windows_terminal_with_cmd_passes_the_command_verbatim() {
    let a = app(Family::WindowsTerminal, AppKind::Terminal, &["wt.exe"]);
    let sh = cmd_exe();
    let c = cmd_resume(r"C:\a b");
    let (sp, ran) = terminal_spec(&a, Os::Windows, Path::new(r"C:\a b"), Some((&sh, &c)));
    assert!(sp.args.is_empty());
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(r#"-d "C:\a b" cmd.exe /K cd /d "C:\a b" && claude --resume x"#)
    );
    assert_eq!(ran, Ran::Command);
}

#[test]
fn wezterm_with_cmd_on_windows_puts_the_shell_in_raw_args() {
    let a = app(Family::WezTerm, AppKind::Terminal, &["wezterm.exe"]);
    let sh = cmd_exe();
    let c = cmd_resume(r"C:\a b");
    let (sp, ran) = terminal_spec(&a, Os::Windows, Path::new(r"C:\a b"), Some((&sh, &c)));
    assert_eq!(sp.program, OsString::from("wezterm.exe"));
    assert_eq!(strs(&sp.args), vec!["start", "--cwd", r"C:\a b", "--"]);
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(r#"cmd.exe /K cd /d "C:\a b" && claude --resume x"#)
    );
    assert_eq!(ran, Ran::Command);

    // A shell path with spaces is MSVC-quoted; only the command after `/K` is verbatim.
    let spaced = Shell {
        path: PathBuf::from(r"C:\My Tools\cmd.exe"),
        kind: ShellKind::Cmd,
    };
    let (sp, _) = terminal_spec(&a, Os::Windows, Path::new(r"C:\a b"), Some((&spaced, &c)));
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(r#""C:\My Tools\cmd.exe" /K cd /d "C:\a b" && claude --resume x"#)
    );

    // Elsewhere `raw_args` is ignored: the argv stays whole.
    let (sp, _) = terminal_spec(&a, Os::Linux, Path::new("/p"), Some((&sh, CMD)));
    assert_eq!(
        strs(&sp.args),
        vec!["start", "--cwd", "/p", "--", "cmd.exe", "/K", CMD]
    );
    assert_eq!(sp.raw_args, None);
}

#[test]
fn custom_terminal_with_cmd_on_windows_renders_the_tail_raw() {
    let toks = split_template("--cwd {dir} -e {cmd} --hold").unwrap();
    let sh = cmd_exe();
    let c = cmd_resume(r"C:\a b");
    let (sp, ran) = custom_spec(
        Path::new(r"C:\t\term.exe"),
        &toks,
        AppKind::Terminal,
        Os::Windows,
        Path::new(r"C:\a b"),
        Some((&sh, &c)),
    );
    assert_eq!(strs(&sp.args), vec!["--cwd", r"C:\a b", "-e"]);
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(r#"cmd.exe /K cd /d "C:\a b" && claude --resume x --hold"#)
    );
    assert_eq!(ran, Ran::Command);

    // Empty template: `-e` stays an argument, the shell part is raw.
    let (sp, _) = custom_spec(
        Path::new(r"C:\t\term.exe"),
        &[],
        AppKind::Terminal,
        Os::Windows,
        Path::new(r"C:\a b"),
        Some((&sh, &c)),
    );
    assert_eq!(strs(&sp.args), vec!["-e"]);
    assert_eq!(
        sp.raw_args.as_deref(),
        Some(r#"cmd.exe /K cd /d "C:\a b" && claude --resume x"#)
    );

    // A `{cmd}` inside a larger token is not a shell argv: unchanged.
    let toks = split_template("--run={cmd}").unwrap();
    let (sp, _) = custom_spec(
        Path::new(r"C:\t\term.exe"),
        &toks,
        AppKind::Terminal,
        Os::Windows,
        Path::new(r"C:\a b"),
        Some((&sh, &c)),
    );
    assert_eq!(strs(&sp.args), vec![format!("--run={c}")]);
    assert_eq!(sp.raw_args, None);
}
