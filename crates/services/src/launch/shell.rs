//! Shell dialects: quoting, the commands the app copies or runs, and the argv that runs them.

use std::ffi::OsString;
use std::path::Path;

use aiannotations_core::model::Tool;

use super::{Shell, ShellKind};

/// PowerShell on Windows, POSIX everywhere else (until the configured shell is wired in).
pub fn default_shell() -> ShellKind {
    if cfg!(windows) {
        ShellKind::PowerShell
    } else {
        ShellKind::Posix
    }
}

/// Dialect of a shell executable, from its file name without extension, lowercased.
pub fn kind_of(path: &Path) -> ShellKind {
    // Split by hand: `Path` on Linux does not treat `\` as a separator.
    let raw = path.to_string_lossy();
    let file = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let stem = file
        .rsplit_once('.')
        .map_or(file, |(s, _)| s)
        .to_lowercase();
    match stem.as_str() {
        "fish" => ShellKind::Fish,
        "pwsh" | "powershell" => ShellKind::PowerShell,
        "cmd" => ShellKind::Cmd,
        _ => ShellKind::Posix,
    }
}

/// Quotes `s` as one literal argument of the dialect.
pub fn quote(s: &str, kind: ShellKind) -> String {
    match kind {
        ShellKind::Posix => format!("'{}'", s.replace('\'', "'\\''")),
        ShellKind::Fish => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        ShellKind::PowerShell => {
            // PowerShell also treats the typographic single quotes as quote characters.
            let mut out = String::with_capacity(s.len() + 2);
            out.push('\'');
            for c in s.chars() {
                out.push(c);
                if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
                    out.push(c);
                }
            }
            out.push('\'');
            out
        }
        ShellKind::Cmd => format!("\"{}\"", s.replace('"', "\"\"")),
    }
}

/// `cd` into `dir` and run `bare`, in the dialect.
fn in_dir(dir: &str, bare: &str, kind: ShellKind) -> String {
    match kind {
        ShellKind::Posix | ShellKind::Fish => format!("cd {} && {bare}", quote(dir, kind)),
        ShellKind::PowerShell => {
            format!("Set-Location -LiteralPath {}; {bare}", quote(dir, kind))
        }
        ShellKind::Cmd => format!("cd /d {} && {bare}", quote(dir, kind)),
    }
}

/// The command that resumes a session in the directory it ran in. The id is quoted only when
/// it has characters outside `[A-Za-z0-9._-]` (Laravel `ResumeCommand`).
pub fn resume_command(tool: Tool, tool_session_id: &str, cwd: &str, kind: ShellKind) -> String {
    let plain = !tool_session_id.is_empty()
        && tool_session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let id = if plain {
        tool_session_id.to_string()
    } else {
        quote(tool_session_id, kind)
    };
    let bare = match tool {
        Tool::Claude => format!("claude --resume {id}"),
        Tool::Codex => format!("codex resume {id}"),
    };
    in_dir(cwd, &bare, kind)
}

/// The command that restarts an ongoing session of a project with Claude.
pub fn ongoing_command(project_path: &str, name: &str, kind: ShellKind) -> String {
    let bare = format!(
        "claude {}",
        quote(&format!("/ongoing-restart {name}"), kind)
    );
    in_dir(project_path, &bare, kind)
}

/// argv that runs `cmd` in `shell` and leaves the shell open afterwards.
pub fn run_and_stay(shell: &Shell, cmd: &str) -> Vec<OsString> {
    let path = shell.path.as_os_str().to_owned();
    match shell.kind {
        ShellKind::Posix | ShellKind::Fish => {
            let me = quote(&shell.path.to_string_lossy(), shell.kind);
            vec![
                path,
                "-l".into(),
                "-c".into(),
                format!("{cmd}; exec {me} -l").into(),
            ]
        }
        ShellKind::PowerShell => vec![
            path,
            "-NoLogo".into(),
            "-NoExit".into(),
            "-Command".into(),
            cmd.into(),
        ],
        ShellKind::Cmd => vec![path, "/K".into(), cmd.into()],
    }
}

/// argv that opens `shell` interactively.
pub fn interactive(shell: &Shell) -> Vec<OsString> {
    let path = shell.path.as_os_str().to_owned();
    match shell.kind {
        ShellKind::Posix | ShellKind::Fish => vec![path, "-l".into()],
        ShellKind::PowerShell => vec![path, "-NoLogo".into()],
        ShellKind::Cmd => vec![path],
    }
}
