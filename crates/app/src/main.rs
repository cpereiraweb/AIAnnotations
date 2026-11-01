//! AIAnnotations desktop app: a thin Tauri shell over `aiannotations-services`. It owns the
//! window, the tray, the background worker thread and the system clock; every rule lives in the
//! services and core crates.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod startup_error;
mod tray;

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

use aiannotations_core::git::Git;
use aiannotations_core::inbox::GitCache;
use aiannotations_core::installer::detect::cli_beside;
use aiannotations_core::settings::Lang;
use aiannotations_core::user_settings::{self, UserSettings};
use aiannotations_core::{Paths, Settings, ai, db, home_dir};
use aiannotations_services::onboarding;
use aiannotations_services::worker::{Worker, WorkerStatus};
use chrono::TimeZone;
use chrono_tz::Tz;
use rusqlite::Connection;
use startup_error::StartupError;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};

/// Event emitted to the webview after something was written; the screens reload.
pub const CHANGED_EVENT: &str = "changed";
const TICK_EVERY: Duration = Duration::from_secs(15);
/// How long quitting waits for an idle worker to release its lock.
const STOP_WAIT: Duration = Duration::from_secs(2);

/// Messages to the worker thread.
pub enum WorkerMsg {
    /// "Pausar/Retomar resumos" from the tray.
    SetPaused(bool),
    /// New settings after the first-run wizard; the worker builds the matching AI client.
    Reload(Settings),
    Stop,
}

/// Tauri-managed state shared by the commands and the tray.
pub struct AppState {
    /// The UI's connection; the worker has its own.
    pub conn: Mutex<Connection>,
    pub paths: Paths,
    /// `Settings::default()` with the system language and the user's `settings.json` applied.
    pub settings: RwLock<Settings>,
    /// The user's `settings.json` as loaded at start, updated by the wizard.
    pub user: Mutex<UserSettings>,
    pub tz: Tz,
    pub git: Git,
    pub cache: GitCache,
    pub worker_status: Arc<Mutex<WorkerStatus>>,
    pub worker_tx: Mutex<Sender<WorkerMsg>>,
    /// "Pausar resumos" is on (the tray's toggle, or the first-run pause).
    pub summaries_paused: AtomicBool,
    /// The pause came from the unfinished wizard, not from the user: finishing it resumes.
    pub paused_by_onboarding: AtomicBool,
    pub importing: Arc<AtomicBool>,
    /// A briefing generation started by the app (Today or "regerar") is in flight.
    pub briefing_spawned: Arc<AtomicBool>,
    /// The last generation this app started failed with an error (not an AI failure, which
    /// marks the record `failed`); the screen stops starting it until one succeeds.
    pub briefing_start_failed: Arc<AtomicBool>,
    worker_done: Mutex<Option<Receiver<()>>>,
}

/// Locks ignoring poisoning: a panicked command must not take the whole UI down.
pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn read<T>(l: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    l.read().unwrap_or_else(|e| e.into_inner())
}

pub fn write<T>(l: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    l.write().unwrap_or_else(|e| e.into_inner())
}

/// Local `%Y%m%d%H%M%S` stamp for backups and files moved aside.
pub fn stamp(tz: Tz) -> String {
    tz.timestamp_millis_opt(now_ms())
        .single()
        .map(|d| d.format("%Y%m%d%H%M%S").to_string())
        .unwrap_or_else(|| "00000000000000".into())
}

/// The process environment the wizard reads, owned so an [`onboarding::Env`] can borrow it.
pub struct EnvValues {
    home: PathBuf,
    path_var: Option<OsString>,
    claude_override: Option<OsString>,
    codex_override: Option<OsString>,
    cli: PathBuf,
}

impl EnvValues {
    pub fn read() -> EnvValues {
        let env = |k: &str| std::env::var_os(k);
        EnvValues {
            home: home_dir(env),
            path_var: env("PATH"),
            claude_override: env("AIANNOTATIONS_CLAUDE_BIN"),
            codex_override: env("AIANNOTATIONS_CODEX_BIN"),
            // No executable path: our CLI counts as missing.
            cli: std::env::current_exe()
                .map(|exe| cli_beside(&exe))
                .unwrap_or_default(),
        }
    }

    pub fn env(&self) -> onboarding::Env<'_> {
        onboarding::Env {
            home: &self.home,
            path_var: self.path_var.as_deref(),
            claude_override: self.claude_override.as_deref(),
            codex_override: self.codex_override.as_deref(),
            cli: &self.cli,
            windows: cfg!(windows),
        }
    }
}

/// The app layer reads the system clock; the libraries receive it.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn system_tz() -> Tz {
    iana_time_zone::get_timezone()
        .ok()
        .and_then(|name| name.parse().ok())
        .unwrap_or(chrono_tz::UTC)
}

/// Language of the AI output and the tray: `LANG`, else the OS locale.
pub(crate) fn system_lang() -> Lang {
    Lang::detect(
        std::env::var("LANG").ok().as_deref(),
        sys_locale::get_locale().as_deref(),
    )
}

/// Runs a tick every 15 s on its own connection until `Stop` (or the app is gone). Updates the
/// tray counts at start and, with `"changed"`, after a tick that wrote something.
fn run_worker(
    app: AppHandle,
    paths: Paths,
    settings: Settings,
    tz: Tz,
    status: Arc<Mutex<WorkerStatus>>,
    rx: Receiver<WorkerMsg>,
    done: Sender<()>,
) {
    let conn = match db::open(&paths.db_path) {
        Ok(conn) => conn,
        Err(e) => {
            lock(&status).last_errors.push(format!("worker: {e}"));
            return;
        }
    };
    let env = |k: &str| std::env::var_os(k);
    let client = ai::client_for(&settings, &paths, env, &home_dir(env));
    let mut worker = Worker::new(conn, paths.clone(), settings, client, Git::default(), tz);
    // Before the first tick: the first-run pause (no AI call until the wizard finishes).
    let paused = app
        .state::<AppState>()
        .summaries_paused
        .load(Ordering::SeqCst);
    worker.set_summaries_paused(paused);
    update_tray(&app, &worker);
    loop {
        match worker.tick(&now_ms) {
            Ok(report) if report.changed => {
                update_tray(&app, &worker);
                let _ = app.emit(CHANGED_EVENT, ());
            }
            Ok(_) => {}
            Err(e) => eprintln!("aiannotations: worker tick failed: {e}"),
        }
        *lock(&status) = worker.status();
        match rx.recv_timeout(TICK_EVERY) {
            Ok(WorkerMsg::SetPaused(paused)) => {
                worker.set_summaries_paused(paused);
                *lock(&status) = worker.status();
            }
            Ok(WorkerMsg::Reload(settings)) => {
                let client = ai::client_for(&settings, &paths, env, &home_dir(env));
                worker.set_settings(settings, client);
            }
            Ok(WorkerMsg::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    // Dropping the worker releases the `worker:running` lock for the next instance.
    drop(worker);
    let _ = done.send(());
}

/// Tray counts from the worker's connection, so they follow the data with the window closed.
fn update_tray(app: &AppHandle, worker: &Worker) {
    match worker.tray_counts(now_ms()) {
        Ok((need, ongoing)) => tray::set_counts(app, need, ongoing),
        Err(e) => eprintln!("aiannotations: tray counts failed: {e}"),
    }
}

/// Asks the worker to stop and waits briefly; a worker in the middle of an AI call is left to
/// the lock TTL.
fn stop_worker(app: &AppHandle) {
    let state = app.state::<AppState>();
    let _ = lock(&state.worker_tx).send(WorkerMsg::Stop);
    if let Some(done) = lock(&state.worker_done).take() {
        let _ = done.recv_timeout(STOP_WAIT);
    }
}

fn main() {
    let paths = Paths::resolve();
    let tz = system_tz();
    let user = user_settings::load(&paths.settings_path, &stamp(tz)).unwrap_or_else(|e| {
        eprintln!(
            "aiannotations: cannot read {}: {e}",
            paths.settings_path.display()
        );
        UserSettings::default()
    });
    let start_paused = user.starts_paused();
    let settings = user.apply(Settings {
        lang: user.effective_lang(system_lang()),
        ..Settings::default()
    });
    let conn = match db::open(&paths.db_path) {
        Ok(conn) => conn,
        Err(e) => startup_error::show_and_exit(
            settings.lang,
            StartupError::Database {
                path: paths.db_path.display().to_string(),
                detail: e.to_string(),
            },
        ),
    };
    let (worker_tx, worker_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let worker_status = Arc::new(Mutex::new(WorkerStatus::default()));
    let lang = settings.lang;
    let state = AppState {
        conn: Mutex::new(conn),
        paths: paths.clone(),
        settings: RwLock::new(settings.clone()),
        user: Mutex::new(user),
        tz,
        git: Git::default(),
        cache: GitCache::new(),
        worker_status: worker_status.clone(),
        worker_tx: Mutex::new(worker_tx),
        summaries_paused: AtomicBool::new(start_paused),
        paused_by_onboarding: AtomicBool::new(start_paused),
        importing: Arc::new(AtomicBool::new(false)),
        briefing_spawned: Arc::new(AtomicBool::new(false)),
        briefing_start_failed: Arc::new(AtomicBool::new(false)),
        worker_done: Mutex::new(Some(done_rx)),
    };

    let built = tauri::Builder::default()
        // First plugin: a second launch only focuses the running window.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        // Used from Rust only (`commands::copy_*`): the webview gets no clipboard permission.
        .plugin(tauri_plugin_clipboard_manager::init())
        // Also Rust only (`commands::finish_onboarding`): no autostart permission either.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::today,
            commands::projects,
            commands::project_detail,
            commands::sessions,
            commands::project_runs,
            commands::search,
            commands::dismiss,
            commands::dismiss_group,
            commands::regenerate_briefing,
            commands::import_history,
            commands::copy_resume_command,
            commands::copy_text,
            commands::open_run_folder,
            commands::open_project_folder,
            commands::worker_status,
            commands::onboarding_state,
            commands::write_hooks,
            commands::remove_hooks,
            commands::finish_onboarding,
            commands::save_settings,
            commands::set_summaries_paused,
        ])
        // Closing the window hides it; quitting is only through the tray.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            tray::create(app.handle(), lang, start_paused)?;
            let handle = app.handle().clone();
            std::thread::Builder::new()
                .name("aiannotations-worker".into())
                .spawn(move || {
                    run_worker(
                        handle,
                        paths,
                        settings,
                        tz,
                        worker_status,
                        worker_rx,
                        done_tx,
                    )
                })?;
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = built.unwrap_or_else(|e| {
        startup_error::show_and_exit(
            lang,
            StartupError::Window {
                detail: e.to_string(),
            },
        )
    });

    app.run(|app, event| match event {
        RunEvent::Exit => stop_worker(app),
        // Clicking the Dock icon with the window hidden brings it back.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::show_main(app),
        _ => {}
    });
}
