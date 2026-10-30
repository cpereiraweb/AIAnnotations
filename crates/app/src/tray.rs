//! Tray icon and menu: Abrir · Hoje (N precisam de você, M em andamento) · Pausar/Retomar
//! resumos · Sair. The worker thread updates the counts at start and after every tick that
//! wrote something; a Today load (`commands::today`) updates them too.

use std::sync::atomic::Ordering;

use aiannotations_core::settings::Lang;
use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::{AppState, WorkerMsg, lock};

const OPEN: &str = "open";
const TODAY: &str = "today";
const PAUSE: &str = "pause";
const QUIT: &str = "quit";

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

pub fn create(app: &AppHandle, lang: Lang) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN, open_text(lang), true, None::<&str>)?;
    let today = MenuItem::with_id(app, TODAY, today_text(lang, 0, 0), true, None::<&str>)?;
    let pause = MenuItem::with_id(app, PAUSE, pause_text(lang, false), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, QUIT, quit_text(lang), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &today, &separator, &pause, &quit])?;

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
        OPEN | TODAY => show_main(app),
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
    let paused = !state.summaries_paused.fetch_xor(true, Ordering::SeqCst);
    let _ = lock(&state.worker_tx).send(WorkerMsg::SetPaused(paused));
    let tray = app.state::<Tray>();
    let _ = tray.pause.set_text(pause_text(tray.lang, paused));
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
    fn pause_text_toggles() {
        assert_eq!(pause_text(Lang::PtBr, false), "Pausar resumos");
        assert_eq!(pause_text(Lang::PtBr, true), "Retomar resumos");
        assert_eq!(pause_text(Lang::En, true), "Resume summaries");
    }
}
