//! Data actions behind the screen buttons. Only [`regenerate_briefing`] calls the AI, and the
//! app runs it off the UI thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use aiannotations_core::ai::AiClient;
use aiannotations_core::briefing::{self, BriefingOutcome};
use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, GitCache, InboxItem};
use aiannotations_core::model::Tool;
use aiannotations_core::sessions::collector::{self, CollectMode, CollectReport};
use aiannotations_core::{Error, Paths, Result, Settings};
use chrono_tz::Tz;
use rusqlite::{Connection, OptionalExtension};

/// Dismisses one inbox item at its current state; it returns when the state changes.
pub fn dismiss(conn: &mut Connection, key: &str, state_hash: &str, now_ms: i64) -> Result<()> {
    inbox::dismiss(conn, key, state_hash, now_ms)
}

/// Dismisses every item of a group in one transaction.
pub fn dismiss_group(conn: &mut Connection, items: &[InboxItem], now_ms: i64) -> Result<()> {
    inbox::dismiss_all(conn, items, now_ms)
}

/// "Já vi tudo" of a project in one section of the inbox (`older` or recent), as the v1
/// `Today::dismissProject`: every item of that group at its current state. Returns how many
/// items were dismissed.
pub fn dismiss_project(
    conn: &mut Connection,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    project_id: i64,
    older: bool,
    now_ms: i64,
) -> Result<usize> {
    let items = inbox::build(conn, git, cache, settings, now_ms)?;
    let grouped = inbox::grouped(conn, items, settings, now_ms)?;
    let section = if older { grouped.older } else { grouped.recent };
    let items: Vec<InboxItem> = section
        .into_iter()
        .filter(|g| g.project_id == project_id)
        .flat_map(|g| g.items)
        .collect();
    dismiss_group(conn, &items, now_ms)?;
    Ok(items.len())
}

/// "Regerar": today's briefing generated again even if already `done`, with the current inbox
/// titles. `AlreadyRunning` when another generation holds the lock.
pub fn regenerate_briefing(
    conn: &mut Connection,
    ai: &dyn AiClient,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    tz: Tz,
    clock: &dyn Fn() -> i64,
) -> Result<BriefingOutcome> {
    run_briefing(conn, ai, git, cache, settings, tz, true, clock)
}

/// Today's briefing generated if not `done` yet, as the v1 Today mount does when it finds the
/// record `generating` (see [`crate::views::today::should_start_briefing`]).
pub fn generate_briefing(
    conn: &mut Connection,
    ai: &dyn AiClient,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    tz: Tz,
    clock: &dyn Fn() -> i64,
) -> Result<BriefingOutcome> {
    run_briefing(conn, ai, git, cache, settings, tz, false, clock)
}

#[allow(clippy::too_many_arguments)]
fn run_briefing(
    conn: &mut Connection,
    ai: &dyn AiClient,
    git: &Git,
    cache: &GitCache,
    settings: &Settings,
    tz: Tz,
    force: bool,
    clock: &dyn Fn() -> i64,
) -> Result<BriefingOutcome> {
    let titles: Vec<String> = inbox::build(conn, git, cache, settings, clock())?
        .into_iter()
        .map(|i| i.title)
        .collect();
    briefing::run(conn, ai, &titles, settings, tz, force, clock)
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

/// Tool, tool session id and cwd of the session a run belongs to.
fn run_session(conn: &Connection, run_id: i64) -> Result<(Tool, String, String)> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT s.tool, s.session_id, s.cwd FROM session_runs r \
             JOIN ai_sessions s ON s.id = r.ai_session_id WHERE r.id = ?1",
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (tool, id, cwd) = row.ok_or_else(|| invalid_folder(format!("run not found: {run_id}")))?;
    Ok((tool.parse()?, id, cwd))
}

fn non_empty_dir(path: String, what: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err(invalid_folder(format!("{what} has no folder")));
    }
    Ok(PathBuf::from(path))
}

/// [`resume_command`] for the session of `run_id`; fails when the run is unknown or its session
/// has no `cwd`.
pub fn resume_command_for_run(conn: &Connection, run_id: i64, shell: ShellKind) -> Result<String> {
    let (tool, id, cwd) = run_session(conn, run_id)?;
    let cwd = non_empty_dir(cwd, "session")?;
    Ok(resume_command(tool, &id, &cwd.to_string_lossy(), shell))
}

/// Folder (`cwd`) of the session of `run_id`.
pub fn run_folder(conn: &Connection, run_id: i64) -> Result<PathBuf> {
    let (_, _, cwd) = run_session(conn, run_id)?;
    non_empty_dir(cwd, "session")
}

/// Folder of a project.
pub fn project_folder(conn: &Connection, project_id: i64) -> Result<PathBuf> {
    let path: Option<String> = conn
        .query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [project_id],
            |r| r.get(0),
        )
        .optional()?;
    let path = path.ok_or_else(|| invalid_folder(format!("project not found: {project_id}")))?;
    non_empty_dir(path, "project")
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
