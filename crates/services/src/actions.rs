//! Data actions behind the screen buttons. Only [`regenerate_briefing`] calls the AI, and the
//! app runs it off the UI thread.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::launch::argv::{LaunchSpec, file_manager_argv};
use crate::launch::spawn::spawn_detached;
use crate::launch::{Os, ShellKind, shell};
use aiannotations_core::ai::AiClient;
use aiannotations_core::briefing::{self, BriefingOutcome};
use aiannotations_core::git::Git;
use aiannotations_core::inbox::{self, GitCache, InboxItem};
use aiannotations_core::model::Tool;
use aiannotations_core::ongoings::{self, Ongoing};
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
    // The Codex desktop app stores the cwd as `\\?\C:\…`; the terminal and the file manager get
    // the plain form, as the project paths already do.
    Ok((
        tool.parse()?,
        id,
        aiannotations_core::paths::plain_path(&cwd),
    ))
}

fn non_empty_dir(path: String, what: &str) -> Result<PathBuf> {
    if path.is_empty() {
        return Err(invalid_folder(format!("{what} has no folder")));
    }
    Ok(PathBuf::from(path))
}

/// [`shell::resume_command`] for the session of `run_id`; fails when the run is unknown or its session
/// has no `cwd`.
pub fn resume_command_for_run(conn: &Connection, run_id: i64, kind: ShellKind) -> Result<String> {
    let (tool, id, cwd) = run_session(conn, run_id)?;
    let cwd = non_empty_dir(cwd, "session")?;
    Ok(shell::resume_command(
        tool,
        &id,
        &cwd.to_string_lossy(),
        kind,
    ))
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

/// Ongoing `file` of a project, only if it is one of the project's listed ongoings. A name with
/// a path separator or `..` is refused before anything is looked up. Error `invalid ongoing`.
pub fn ongoing(conn: &Connection, project_id: i64, file: &str) -> Result<Ongoing> {
    let invalid = || Error::Installer("invalid ongoing".into());
    if file.contains(['/', '\\']) || file.contains("..") {
        return Err(invalid());
    }
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT name, path FROM projects WHERE id = ?1",
            [project_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (name, path) = row.ok_or_else(invalid)?;
    ongoings::for_project(project_id, &name, &path)
        .into_iter()
        .find(|o| o.file == file)
        .ok_or_else(invalid)
}

/// Fails unless `dir` is an absolute, existing directory.
pub fn check_dir(dir: &Path) -> Result<()> {
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
    Ok(())
}

/// The OS file-manager command for `dir`; it fails unless `dir` is an absolute, existing directory.
pub fn open_folder_argv(dir: &Path) -> Result<Vec<OsString>> {
    check_dir(dir)?;
    Ok(file_manager_argv(Os::current(), dir))
}

/// Opens `dir` in the OS file manager, detached from the app (no stdio; reaped off-thread).
pub fn open_folder(dir: &Path) -> Result<()> {
    let mut argv = open_folder_argv(dir)?.into_iter();
    spawn_detached(&LaunchSpec {
        program: argv.next().unwrap_or_default(),
        args: argv.collect(),
        raw_args: None,
        cwd: None,
        new_console: false,
    })
}

#[cfg(test)]
mod tests {
    use crate::launch::argv::explorer_path;

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
