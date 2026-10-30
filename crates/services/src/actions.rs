//! Data actions behind the Today screen buttons. None of them calls the AI.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, InboxItem};
use aiannotations_core::model::Tool;
use aiannotations_core::sessions::collector::{self, CollectMode, CollectReport};
use aiannotations_core::{Error, Paths, Result, Settings};
use rusqlite::Connection;

/// Dismisses one inbox item at its current state; it returns when the state changes.
pub fn dismiss(conn: &mut Connection, key: &str, state_hash: &str, now_ms: i64) -> Result<()> {
    inbox::dismiss(conn, key, state_hash, now_ms)
}

/// Dismisses every item of a group in one transaction.
pub fn dismiss_group(conn: &mut Connection, items: &[InboxItem], now_ms: i64) -> Result<()> {
    inbox::dismiss_all(conn, items, now_ms)
}

/// "Import history": the whole on-disk history, runs marked `is_backfill`. Summaries are left
/// pending for the worker. `progress(done, total)` is passed to the collector.
pub fn import_history(
    conn: &mut Connection,
    paths: &Paths,
    settings: &Settings,
    git: &Git,
    now_ms: i64,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<CollectReport> {
    collector::run(
        conn,
        paths,
        settings,
        git,
        CollectMode::Backfill,
        now_ms,
        progress,
    )
}

/// Shell dialect of the copied resume command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    PowerShell,
}

/// PowerShell on Windows, POSIX everywhere else.
pub fn default_shell() -> ShellKind {
    if cfg!(windows) {
        ShellKind::PowerShell
    } else {
        ShellKind::Posix
    }
}

/// Single-quotes `s` for the shell; an embedded `'` becomes `'\''` (POSIX) or `''` (PowerShell).
fn quote(s: &str, shell: ShellKind) -> String {
    let escaped = match shell {
        ShellKind::Posix => s.replace('\'', "'\\''"),
        ShellKind::PowerShell => s.replace('\'', "''"),
    };
    format!("'{escaped}'")
}

/// The command that resumes a session in the directory it ran in. The id is quoted only when
/// it has characters outside `[A-Za-z0-9._-]` (Laravel `ResumeCommand`).
pub fn resume_command(tool: Tool, tool_session_id: &str, cwd: &str, shell: ShellKind) -> String {
    let plain = !tool_session_id.is_empty()
        && tool_session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    let id = if plain {
        tool_session_id.to_string()
    } else {
        quote(tool_session_id, shell)
    };
    let bare = match tool {
        Tool::Claude => format!("claude --resume {id}"),
        Tool::Codex => format!("codex resume {id}"),
    };
    match shell {
        ShellKind::Posix => format!("cd {} && {bare}", quote(cwd, shell)),
        ShellKind::PowerShell => {
            format!("Set-Location -LiteralPath {}; {bare}", quote(cwd, shell))
        }
    }
}

fn invalid_folder(msg: String) -> Error {
    Error::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, msg))
}

/// The OS file-manager command for `dir`; it fails unless `dir` is an absolute, existing directory.
pub fn open_folder_argv(dir: &Path) -> Result<Vec<OsString>> {
    if !dir.is_absolute() {
        return Err(invalid_folder(format!(
            "folder path is not absolute: {}",
            dir.display()
        )));
    }
    if !dir.is_dir() {
        return Err(invalid_folder(format!(
            "folder does not exist: {}",
            dir.display()
        )));
    }
    if cfg!(windows) {
        // `explorer` opens the default folder when it cannot parse its argument.
        let path = explorer_path(&dir.to_string_lossy());
        return Ok(vec![OsString::from("explorer"), OsString::from(path)]);
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    Ok(vec![OsString::from(opener), dir.as_os_str().to_owned()])
}

/// Windows form of a path for `explorer`: `\` separators and no trailing separator, except on
/// a drive root (`C:\`). Pure, so every OS compiles and tests it.
fn explorer_path(path: &str) -> String {
    let native = path.replace('/', "\\");
    let trimmed = native.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

/// Opens `dir` in the OS file manager, detached from the app (no stdio, never waited on).
pub fn open_folder(dir: &Path) -> Result<()> {
    let argv = open_folder_argv(dir)?;
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        cmd.creation_flags(0x0000_0008 | 0x0000_0200);
    }
    cmd.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::explorer_path;

    #[test]
    fn explorer_path_normalizes() {
        assert_eq!(explorer_path("C:/Users/x/proj"), r"C:\Users\x\proj");
        assert_eq!(explorer_path(r"C:\a b\"), r"C:\a b");
        assert_eq!(explorer_path(r"C:\a b\\"), r"C:\a b");
        assert_eq!(explorer_path("C:/a b/"), r"C:\a b");
        assert_eq!(explorer_path(r"C:\"), r"C:\");
        assert_eq!(explorer_path("C:/"), r"C:\");
        assert_eq!(explorer_path(r"\\srv\share\d\"), r"\\srv\share\d");
    }
}
