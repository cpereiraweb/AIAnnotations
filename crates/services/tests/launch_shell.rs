//! Shell dialects: quoting, resume/ongoing commands and the argv that runs a command.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use aiannotations_core::model::Tool;
use aiannotations_services::launch::shell::{
    interactive, kind_of, ongoing_command, quote, resume_command, run_and_stay,
};
use aiannotations_services::launch::{Shell, ShellKind};

fn sh(path: &str, kind: ShellKind) -> Shell {
    Shell {
        path: PathBuf::from(path),
        kind,
    }
}

fn os(items: &[&str]) -> Vec<OsString> {
    items.iter().map(OsString::from).collect()
}

#[test]
fn quote_per_dialect() {
    assert_eq!(quote("it's", ShellKind::Posix), r"'it'\''s'");
    assert_eq!(quote("it's", ShellKind::Fish), r"'it\'s'");
    assert_eq!(quote(r"a\b", ShellKind::Fish), r"'a\\b'");
    assert_eq!(quote("it's", ShellKind::PowerShell), "'it''s'");
    assert_eq!(quote("it's", ShellKind::Cmd), "\"it's\"");
}

#[test]
fn powershell_doubles_typographic_quotes() {
    assert_eq!(quote("a’b‘c", ShellKind::PowerShell), "'a’’b‘‘c'");
    assert_eq!(quote("a‚b‛c", ShellKind::PowerShell), "'a‚‚b‛‛c'");
}

#[test]
fn resume_command_per_dialect() {
    let r = |tool, id, kind| resume_command(tool, id, "/p q", kind);
    assert_eq!(
        r(Tool::Claude, "abc-1", ShellKind::Posix),
        "cd '/p q' && claude --resume abc-1"
    );
    assert_eq!(
        r(Tool::Claude, "abc-1", ShellKind::Fish),
        "cd '/p q' && claude --resume abc-1"
    );
    assert_eq!(
        r(Tool::Claude, "abc-1", ShellKind::PowerShell),
        "Set-Location -LiteralPath '/p q'; claude --resume abc-1"
    );
    assert_eq!(
        r(Tool::Claude, "abc-1", ShellKind::Cmd),
        "cd /d \"/p q\" && claude --resume abc-1"
    );
    assert_eq!(
        r(Tool::Codex, "abc-1", ShellKind::Posix),
        "cd '/p q' && codex resume abc-1"
    );
    // Ids outside [A-Za-z0-9._-] are quoted in the dialect.
    assert_eq!(
        r(Tool::Claude, "a'b", ShellKind::Posix),
        r"cd '/p q' && claude --resume 'a'\''b'"
    );
    assert_eq!(
        r(Tool::Claude, "a b", ShellKind::PowerShell),
        "Set-Location -LiteralPath '/p q'; claude --resume 'a b'"
    );
    assert_eq!(
        r(Tool::Claude, "a b", ShellKind::Cmd),
        "cd /d \"/p q\" && claude --resume \"a b\""
    );
    assert_eq!(
        r(Tool::Claude, "A.b_c-9", ShellKind::Posix),
        "cd '/p q' && claude --resume A.b_c-9"
    );
}

#[test]
fn ongoing_command_per_dialect() {
    assert_eq!(
        ongoing_command("/p", "nome", ShellKind::Posix),
        "cd '/p' && claude '/ongoing-restart nome'"
    );
    assert_eq!(
        ongoing_command("/p", "nome", ShellKind::Fish),
        "cd '/p' && claude '/ongoing-restart nome'"
    );
    assert_eq!(
        ongoing_command("/p", "nome", ShellKind::PowerShell),
        "Set-Location -LiteralPath '/p'; claude '/ongoing-restart nome'"
    );
    assert_eq!(
        ongoing_command("/p", "nome", ShellKind::Cmd),
        "cd /d \"/p\" && claude \"/ongoing-restart nome\""
    );
}

#[test]
fn run_and_stay_argv() {
    assert_eq!(
        run_and_stay(&sh("/bin/zsh", ShellKind::Posix), "X"),
        os(&["/bin/zsh", "-l", "-c", "X; exec '/bin/zsh' -l"])
    );
    assert_eq!(
        run_and_stay(&sh("/usr/bin/fish", ShellKind::Fish), "X"),
        os(&["/usr/bin/fish", "-l", "-c", "X; exec '/usr/bin/fish' -l"])
    );
    assert_eq!(
        run_and_stay(&sh("pwsh", ShellKind::PowerShell), "X"),
        os(&["pwsh", "-NoLogo", "-NoExit", "-Command", "X"])
    );
    assert_eq!(
        run_and_stay(&sh("cmd.exe", ShellKind::Cmd), "X"),
        os(&["cmd.exe", "/K", "X"])
    );
}

#[test]
fn interactive_argv() {
    assert_eq!(
        interactive(&sh("/bin/zsh", ShellKind::Posix)),
        os(&["/bin/zsh", "-l"])
    );
    assert_eq!(
        interactive(&sh("/usr/bin/fish", ShellKind::Fish)),
        os(&["/usr/bin/fish", "-l"])
    );
    assert_eq!(
        interactive(&sh("pwsh", ShellKind::PowerShell)),
        os(&["pwsh", "-NoLogo"])
    );
    assert_eq!(
        interactive(&sh("cmd.exe", ShellKind::Cmd)),
        os(&["cmd.exe"])
    );
}

#[test]
fn kind_of_names() {
    let k = |p: &str| kind_of(Path::new(p));
    assert_eq!(k(r"pwsh.exe"), ShellKind::PowerShell);
    assert_eq!(k("powershell"), ShellKind::PowerShell);
    assert_eq!(k("/usr/bin/fish"), ShellKind::Fish);
    assert_eq!(k("CMD.EXE"), ShellKind::Cmd);
    assert_eq!(k("/bin/bash"), ShellKind::Posix);
}

#[test]
fn default_shell_matches_os() {
    let expected = if cfg!(windows) {
        ShellKind::PowerShell
    } else {
        ShellKind::Posix
    };
    assert_eq!(
        aiannotations_services::launch::shell::default_shell(),
        expected
    );
}
