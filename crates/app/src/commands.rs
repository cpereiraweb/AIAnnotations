//! `invoke` commands: each one adapts a service to the webview, the error becoming a `String`.
//! Arguments are snake_case on both sides (`rename_all`), as in `ui/src/lib/api.ts`. The commands
//! are `async` so they run off the main thread; the two AI/IO-heavy ones (`regenerate_briefing`,
//! `import_history`) start a thread and emit `"changed"` when they finish.

use std::sync::atomic::Ordering;

use aiannotations_core::git::Git;
use aiannotations_core::inbox::GitCache;
use aiannotations_core::model::Tool;
use aiannotations_core::search::SearchHit;
use aiannotations_core::{ai, db, home_dir};
use aiannotations_services::actions::{self, default_shell};
use aiannotations_services::views;
use aiannotations_services::views::projects::{self, ProjectCard, ProjectDetail};
use aiannotations_services::views::runs::{self, Page, RunCard, RunKind, SessionFilter};
use aiannotations_services::views::today::{self, TodayView};
use aiannotations_services::worker::WorkerStatus;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, State};

use crate::{AppState, CHANGED_EVENT, lock, now_ms, tray};

/// Error code of `regenerate_briefing` while a generation holds the lock.
pub const ERR_BRIEFING_RUNNING: &str = "already_running";
/// Error code of `import_history` while an import runs.
pub const ERR_IMPORT_RUNNING: &str = "import_running";

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command(rename_all = "snake_case")]
pub async fn today(app: AppHandle, state: State<'_, AppState>) -> CmdResult<TodayView> {
    let view = {
        let mut conn = lock(&state.conn);
        today::load(
            &mut conn,
            &state.git,
            &state.cache,
            &state.settings,
            state.tz,
            now_ms(),
        )
        .map_err(err)?
    };
    let need: usize = view.inbox.recent.iter().map(|g| g.items.len()).sum();
    let ongoing: usize = view.ongoings_recent.iter().map(|g| g.items.len()).sum();
    tray::set_counts(&app, need, ongoing);
    Ok(view)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn projects(state: State<'_, AppState>) -> CmdResult<Vec<ProjectCard>> {
    projects::list(&lock(&state.conn), now_ms()).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_detail(
    state: State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<ProjectDetail>> {
    projects::detail(&lock(&state.conn), id, now_ms()).map_err(err)
}

/// `SessionFilter` as the webview sends it (`types.ts` `SessionFilter`).
#[derive(Debug, Default, Deserialize)]
pub struct SessionFilterArg {
    pub tool: Option<String>,
    pub project_id: Option<i64>,
    pub kind: Option<String>,
}

impl SessionFilterArg {
    fn parse(self) -> CmdResult<SessionFilter> {
        let tool = match self.tool.as_deref() {
            None | Some("") => None,
            Some(t) => Some(t.parse::<Tool>().map_err(err)?),
        };
        let kind = match self.kind.as_deref() {
            None | Some("") => None,
            Some("changed") => Some(RunKind::Changed),
            Some("talk") => Some(RunKind::Talk),
            Some(other) => return Err(format!("unknown run kind: {other:?}")),
        };
        Ok(SessionFilter {
            tool,
            project_id: self.project_id,
            kind,
        })
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn sessions(
    state: State<'_, AppState>,
    filter: Option<SessionFilterArg>,
    page: u32,
) -> CmdResult<Page<RunCard>> {
    let filter = filter.unwrap_or_default().parse()?;
    runs::sessions(&lock(&state.conn), &filter, page).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_runs(
    state: State<'_, AppState>,
    id: i64,
    include_hidden: bool,
    page: u32,
) -> CmdResult<Page<RunCard>> {
    runs::project_runs(&lock(&state.conn), id, include_hidden, page).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search(state: State<'_, AppState>, text: String) -> CmdResult<Vec<SearchHit>> {
    views::search(&lock(&state.conn), &text).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn dismiss(state: State<'_, AppState>, key: String, state_hash: String) -> CmdResult<()> {
    actions::dismiss(&mut lock(&state.conn), &key, &state_hash, now_ms()).map_err(err)
}

/// "Já vi tudo": the project's inbox items of one section (`older` or recent), at their
/// current state.
#[tauri::command(rename_all = "snake_case")]
pub async fn dismiss_group(
    state: State<'_, AppState>,
    project_id: i64,
    older: bool,
) -> CmdResult<usize> {
    actions::dismiss_project(
        &mut lock(&state.conn),
        &state.git,
        &state.cache,
        &state.settings,
        project_id,
        older,
        now_ms(),
    )
    .map_err(err)
}

/// Starts the generation on its own thread and connection (it calls the AI for minutes).
#[tauri::command(rename_all = "snake_case")]
pub async fn regenerate_briefing(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if aiannotations_core::briefing::is_running(&lock(&state.conn), now_ms()).map_err(err)? {
        return Err(ERR_BRIEFING_RUNNING.into());
    }
    let (paths, settings, tz) = (state.paths.clone(), state.settings.clone(), state.tz);
    std::thread::spawn(move || {
        let result = db::open(&paths.db_path).and_then(|mut conn| {
            let env = |k: &str| std::env::var_os(k);
            let client = ai::client_for(&settings, &paths, env, &home_dir(env));
            actions::regenerate_briefing(
                &mut conn,
                &*client,
                &Git::default(),
                &GitCache::new(),
                &settings,
                tz,
                &now_ms,
            )
        });
        if let Err(e) = result {
            eprintln!("aiannotations: regenerate briefing failed: {e}");
        }
        let _ = app.emit(CHANGED_EVENT, ());
    });
    Ok(())
}

/// Starts the backfill ("Importar histórico") on its own thread and connection.
#[tauri::command(rename_all = "snake_case")]
pub async fn import_history(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if state.importing.swap(true, Ordering::SeqCst) {
        return Err(ERR_IMPORT_RUNNING.into());
    }
    let importing = state.importing.clone();
    let (paths, settings) = (state.paths.clone(), state.settings.clone());
    std::thread::spawn(move || {
        let result = db::open(&paths.db_path).and_then(|mut conn| {
            actions::import_history(
                &mut conn,
                &paths,
                &settings,
                &Git::default(),
                now_ms(),
                &mut |_, _| {},
            )
        });
        importing.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            eprintln!("aiannotations: import history failed: {e}");
        }
        let _ = app.emit(CHANGED_EVENT, ());
    });
    Ok(())
}

/// The command "▶ retomar" copies, for the OS shell.
#[tauri::command(rename_all = "snake_case")]
pub async fn resume_command(state: State<'_, AppState>, run_id: i64) -> CmdResult<String> {
    actions::resume_command_for_run(&lock(&state.conn), run_id, default_shell()).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_run_folder(state: State<'_, AppState>, run_id: i64) -> CmdResult<()> {
    let dir = actions::run_folder(&lock(&state.conn), run_id).map_err(err)?;
    actions::open_folder(&dir).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_project_folder(state: State<'_, AppState>, project_id: i64) -> CmdResult<()> {
    let dir = actions::project_folder(&lock(&state.conn), project_id).map_err(err)?;
    actions::open_folder(&dir).map_err(err)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn worker_status(state: State<'_, AppState>) -> CmdResult<WorkerStatus> {
    Ok(lock(&state.worker_status).clone())
}
