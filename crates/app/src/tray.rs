//! Tray icon and menu: Abrir · Hoje (N precisam de você, M em andamento) · Ajustes ·
//! Pausar/Retomar resumos · Sair. The worker thread updates the counts at start and after every tick that
//! wrote something; a Today load (`commands::today`) updates them too.

use std::sync::atomic::Ordering;

use aiannotations_core::settings::Lang;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::{AppState, WorkerMsg, lock};

const OPEN: &str = "open";
const TODAY: &str = "today";
const SETTINGS: &str = "settings";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";

/// Event the webview (`App.svelte`) turns into a route; the payload is the hash.
const NAVIGATE_EVENT: &str = "navigate";
const TODAY_HASH: &str = "#/";
/// The wizard, reopened as "Ajustes" (see `ui/src/lib/router.svelte.ts`).
const SETTINGS_HASH: &str = "#/settings";

/// Menu items whose text changes, kept in the managed state.
struct Tray {
    lang: Lang,
    today: MenuItem<Wry>,
    pause: MenuItem<Wry>,
}

fn open_text(lang: Lang) -> &'static str {
    match lang {
        Lang::PtBr => "Abrir",
        Lang::En => "Open",
    }
}

fn settings_text(lang: Lang) -> &'static str {
    match lang {
        Lang::PtBr => "Ajustes",
        Lang::En => "Settings",
    }
}

fn quit_text(lang: Lang) -> &'static str {
    match lang {
        Lang::PtBr => "Sair",
        Lang::En => "Quit",
    }
}

fn today_text(lang: Lang, need: usize, ongoing: usize) -> String {
    match lang {
        Lang::PtBr => {
            let verb = if need == 1 { "precisa" } else { "precisam" };
            format!("Hoje ({need} {verb} de você, {ongoing} em andamento)")
        }
        Lang::En => {
            let verb = if need == 1 { "needs" } else { "need" };
            format!("Today ({need} {verb} you, {ongoing} in progress)")
        }
    }
}

fn pause_text(lang: Lang, paused: bool) -> &'static str {
    match (lang, paused) {
        (Lang::PtBr, false) => "Pausar resumos",
        (Lang::PtBr, true) => "Retomar resumos",
        (Lang::En, false) => "Pause summaries",
        (Lang::En, true) => "Resume summaries",
    }
}

/// Shows, unminimizes and focuses the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// `paused`: the label the pause item starts with (the first-run pause).
pub fn create(app: &AppHandle, lang: Lang, paused: bool) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, open_text(lang), true, None::<&str>)?;
    let today = MenuItem::with_id(app, TODAY, today_text(lang, 0, 0), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS, settings_text(lang), true, None::<&str>)?;
    let pause = MenuItem::with_id(app, PAUSE, pause_text(lang, paused), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, quit_text(lang), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &today, &settings, &separator, &pause, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("AIAnnotations")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    app.manage(Tray { lang, today, pause });
    Ok(())
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        OPEN => show_main(app),
        TODAY => {
            show_main(app);
            let _ = app.emit(NAVIGATE_EVENT, TODAY_HASH);
        }
        SETTINGS => {
            show_main(app);
            let _ = app.emit(NAVIGATE_EVENT, SETTINGS_HASH);
        }
        PAUSE => toggle_pause(app),
        QUIT => app.exit(0),
        _ => {}
    }
}

/// Left click opens the window (Windows and macOS; Linux trays only show the menu).
fn on_tray_icon_event(tray: &TrayIcon, event: TrayIconEvent) {
    if let TrayIconEvent::Click {
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        show_main(tray.app_handle());
    }
}

fn toggle_pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    let current = state.summaries_paused.load(Ordering::SeqCst);
    let paused = next_paused(current, lock(&state.user).onboarding_done);
    if paused == current {
        return;
    }
    state.summaries_paused.store(paused, Ordering::SeqCst);
    // The user now owns the pause: finishing the wizard no longer lifts it.
    state.paused_by_onboarding.store(false, Ordering::SeqCst);
    let _ = lock(&state.worker_tx).send(WorkerMsg::SetPaused(paused));
    set_paused(app, paused);
}

/// The pause after a click on "Pausar/Retomar resumos". No AI call before the wizard's
/// "Concluir": until then resuming is ignored; pausing is always allowed.
fn next_paused(currently_paused: bool, onboarding_done: bool) -> bool {
    !currently_paused || !onboarding_done
}

/// "Pausar resumos" / "Retomar resumos" after the pause changed.
pub fn set_paused(app: &AppHandle, paused: bool) {
    if let Some(tray) = app.try_state::<Tray>() {
        let _ = tray.pause.set_text(pause_text(tray.lang, paused));
    }
}

/// Updates "Hoje (N precisam de você, M em andamento)".
pub fn set_counts(app: &AppHandle, need: usize, ongoing: usize) {
    if let Some(tray) = app.try_state::<Tray>() {
        let _ = tray.today.set_text(today_text(tray.lang, need, ongoing));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn today_text_counts_and_agreement() {
        assert_eq!(
            today_text(Lang::PtBr, 3, 2),
            "Hoje (3 precisam de você, 2 em andamento)"
        );
        assert_eq!(
            today_text(Lang::PtBr, 1, 0),
            "Hoje (1 precisa de você, 0 em andamento)"
        );
        assert_eq!(
            today_text(Lang::En, 1, 4),
            "Today (1 needs you, 4 in progress)"
        );
        assert_eq!(
            today_text(Lang::En, 0, 0),
            "Today (0 need you, 0 in progress)"
        );
    }

    #[test]
    fn resume_is_ignored_until_onboarding_done() {
        // Before "Concluir": pausing works, resuming does nothing.
        assert!(next_paused(true, false));
        assert!(next_paused(false, false));
        // After: a plain toggle.
        assert!(!next_paused(true, true));
        assert!(next_paused(false, true));
    }

    #[test]
    fn settings_text_per_lang() {
        assert_eq!(settings_text(Lang::PtBr), "Ajustes");
        assert_eq!(settings_text(Lang::En), "Settings");
        assert_eq!(SETTINGS_HASH, "#/settings");
    }

    #[test]
    fn pause_text_toggles() {
        assert_eq!(pause_text(Lang::PtBr, false), "Pausar resumos");
        assert_eq!(pause_text(Lang::PtBr, true), "Retomar resumos");
        assert_eq!(pause_text(Lang::En, true), "Resume summaries");
    }
}
