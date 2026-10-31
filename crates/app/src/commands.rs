//! `invoke` commands: each one adapts a service to the webview, the error becoming a `String`.
//! Arguments are snake_case on both sides (`rename_all`), as in `ui/src/lib/api.ts`. The commands
//! are `async` so they run off the main thread; the AI/IO-heavy work (the briefing, started by
//! `regenerate_briefing` or by `today` as in v1, and `import_history`) runs on a thread that
//! emits `"changed"` when it finishes.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use aiannotations_core::git::Git;
use aiannotations_core::inbox::GitCache;
use aiannotations_core::model::Tool;
use aiannotations_core::search::SearchHit;
use aiannotations_core::{ai, db, home_dir};
use aiannotations_services::actions::{self, default_shell};
use aiannotations_services::onboarding::{self, Choices, HookWrite, OnboardingView};
use aiannotations_services::views;
use aiannotations_services::views::projects::{self, ProjectCard, ProjectDetail};
use aiannotations_services::views::runs::{self, Page, RunCard, RunKind, SessionFilter};
use aiannotations_services::views::today::{self, TodayView};
use aiannotations_services::worker::WorkerStatus;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{
    AppState, CHANGED_EVENT, EnvValues, WorkerMsg, lock, now_ms, read, stamp, tray, write,
};

/// `{done, total}` while `import_history` runs.
pub const IMPORT_PROGRESS_EVENT: &str = "import-progress";
/// [`onboarding::HistoryCounts`] once `import_history` ends.
pub const IMPORT_DONE_EVENT: &str = "import-done";
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

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
    let settings = read(&state.settings).clone();
    let mut view = {
        let mut conn = lock(&state.conn);
        today::load(
            &mut conn,
            &state.git,
            &state.cache,
            &settings,
            state.tz,
            now_ms(),
        )
        .map_err(err)?
    };
    view.summaries_paused = state.summaries_paused.load(Ordering::SeqCst);
    view.briefing_start_failed = state.briefing_start_failed.load(Ordering::SeqCst);
    view.hooks_need_update = onboarding::hooks_need_update(&state.paths, &EnvValues::read().env());
    // A generation this app started counts as running before it takes the lock.
    if state.briefing_spawned.load(Ordering::SeqCst)
        || (today::should_start_briefing(&view) && spawn_briefing(&app, &state, false))
    {
        view.briefing_running = true;
    }
    let (need, ongoing) = view.tray_counts();
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
    let settings = read(&state.settings).clone();
    actions::dismiss_project(
        &mut lock(&state.conn),
        &state.git,
        &state.cache,
        &settings,
        project_id,
        older,
        now_ms(),
    )
    .map_err(err)
}

/// Starts the generation on its own thread and connection (it calls the AI for minutes).
#[tauri::command(rename_all = "snake_case")]
pub async fn regenerate_briefing(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if aiannotations_core::briefing::is_running(&lock(&state.conn), now_ms()).map_err(err)?
        || !spawn_briefing(&app, &state, true)
    {
        return Err(ERR_BRIEFING_RUNNING.into());
    }
    Ok(())
}

/// Starts a briefing generation on its own thread and connection, `force` for "regerar".
/// `false` when one this app started is still in flight. Emits `"changed"` when it ends.
fn spawn_briefing(app: &AppHandle, state: &AppState, force: bool) -> bool {
    if state.briefing_spawned.swap(true, Ordering::SeqCst) {
        return false;
    }
    let spawned = state.briefing_spawned.clone();
    let start_failed = state.briefing_start_failed.clone();
    let (paths, settings, tz) = (state.paths.clone(), read(&state.settings).clone(), state.tz);
    let app = app.clone();
    std::thread::spawn(move || {
        let result = db::open(&paths.db_path).and_then(|mut conn| {
            let env = |k: &str| std::env::var_os(k);
            let client = ai::client_for(&settings, &paths, env, &home_dir(env));
            let generate = if force {
                actions::regenerate_briefing
            } else {
                actions::generate_briefing
            };
            generate(
                &mut conn,
                &*client,
                &Git::default(),
                &GitCache::new(),
                &settings,
                tz,
                &now_ms,
            )
        });
        start_failed.store(result.is_err(), Ordering::SeqCst);
        spawned.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            eprintln!("aiannotations: briefing generation failed: {e}");
        }
        let _ = app.emit(CHANGED_EVENT, ());
    });
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct ImportProgress {
    done: usize,
    total: usize,
}

/// Starts the backfill ("Importar histórico") on its own thread and connection. Emits
/// `"import-progress"` at most every 250 ms (the last one always), then `"import-done"` with the
/// history counts and `"changed"`.
#[tauri::command(rename_all = "snake_case")]
pub async fn import_history(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if state.importing.swap(true, Ordering::SeqCst) {
        return Err(ERR_IMPORT_RUNNING.into());
    }
    let importing = state.importing.clone();
    let (paths, settings) = (state.paths.clone(), read(&state.settings).clone());
    std::thread::spawn(move || {
        let mut last: Option<ImportProgress> = None;
        let mut sent: Option<(Instant, ImportProgress)> = None;
        let result = db::open(&paths.db_path).and_then(|mut conn| {
            let imported = actions::import_history(
                &mut conn,
                &paths,
                &settings,
                &Git::default(),
                now_ms(),
                &mut |done, total| {
                    let p = ImportProgress { done, total };
                    last = Some(p);
                    if sent.is_none_or(|(at, _)| at.elapsed() >= PROGRESS_EVERY) {
                        sent = Some((Instant::now(), p));
                        let _ = app.emit(IMPORT_PROGRESS_EVENT, p);
                    }
                },
            );
            // The counts also after a partial import: the screen must leave its progress state.
            let counts = onboarding::history_counts(&conn);
            imported.and(counts)
        });
        if let Some(p) = last
            && sent.is_none_or(|(_, s)| s != p)
        {
            let _ = app.emit(IMPORT_PROGRESS_EVENT, p);
        }
        importing.store(false, Ordering::SeqCst);
        match result {
            Ok(counts) => {
                let _ = app.emit(IMPORT_DONE_EVENT, counts);
            }
            Err(e) => eprintln!("aiannotations: import history failed: {e}"),
        }
        let _ = app.emit(CHANGED_EVENT, ());
    });
    Ok(())
}

/// "▶ retomar": builds the run's resume command for the OS shell, writes it to the clipboard
/// and returns it. Copying here, not in the webview: WebKit may refuse a clipboard write that
/// comes after an `await`, once the click's user activation is gone.
#[tauri::command(rename_all = "snake_case")]
pub async fn copy_resume_command(
    app: AppHandle,
    state: State<'_, AppState>,
    run_id: i64,
) -> CmdResult<String> {
    let command = actions::resume_command_for_run(&lock(&state.conn), run_id, default_shell())
        .map_err(err)?;
    app.clipboard().write_text(command.as_str()).map_err(err)?;
    Ok(command)
}

/// Writes `text` to the clipboard (the ongoings' "▶ retomar", built in the webview).
#[tauri::command(rename_all = "snake_case")]
pub async fn copy_text(app: AppHandle, text: String) -> CmdResult<()> {
    app.clipboard().write_text(text).map_err(err)
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

/// The first-run wizard: CLI cards with the hook diffs, saved choices and history counts.
#[tauri::command(rename_all = "snake_case")]
pub async fn onboarding_state(state: State<'_, AppState>) -> CmdResult<OnboardingView> {
    let values = EnvValues::read();
    let user = lock(&state.user).clone();
    onboarding::load(&lock(&state.conn), &state.paths, &user, &values.env()).map_err(err)
}

/// "Instalar hooks" after the diff was shown: backup and atomic write of the tool's config.
/// Errors are the service's English codes (`invalid file`, `CLI missing`, `tool CLI missing`).
#[tauri::command(rename_all = "snake_case")]
pub async fn write_hooks(
    app: AppHandle,
    state: State<'_, AppState>,
    tool: Tool,
) -> CmdResult<HookWrite> {
    let values = EnvValues::read();
    let written = onboarding::write_hooks(&state.paths, &values.env(), tool, &stamp(state.tz))
        .map_err(err)?;
    if written.changed {
        let _ = app.emit(CHANGED_EVENT, ());
    }
    Ok(written)
}

/// "Concluir": saves the choices, reloads the worker's settings and AI client, sets the
/// autostart and lifts the first-run pause. `Some(warning)`: the autostart could not be changed
/// (the choices stay saved).
#[tauri::command(rename_all = "snake_case")]
pub async fn finish_onboarding(
    app: AppHandle,
    state: State<'_, AppState>,
    choices: Choices,
) -> CmdResult<Option<String>> {
    let settings = {
        let mut user = lock(&state.user);
        onboarding::finish(&state.paths, &mut user, &choices).map_err(err)?;
        user.apply(read(&state.settings).clone())
    };
    *write(&state.settings) = settings.clone();
    let _ = lock(&state.worker_tx).send(WorkerMsg::Reload(settings));

    let autolaunch = app.autolaunch();
    let autostart = if choices.autostart {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    let warning = autostart.err().map(err);

    if state.paused_by_onboarding.swap(false, Ordering::SeqCst) {
        state.summaries_paused.store(false, Ordering::SeqCst);
        let _ = lock(&state.worker_tx).send(WorkerMsg::SetPaused(false));
        tray::set_paused(&app, false);
    }
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(warning)
}
