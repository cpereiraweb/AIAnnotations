//! Startup failures the user must see: a release build on Windows has no console, so a silent
//! exit would look like the app does nothing.

use aiannotations_core::settings::Lang;

/// Why the app could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupError {
    /// The database file could not be opened.
    Database { path: String, detail: String },
    /// The Tauri window / WebView could not be built.
    Window { detail: String },
}

/// Title and body of the dialog for `err`.
pub fn message(lang: Lang, err: &StartupError) -> (String, String) {
    let pt = lang == Lang::PtBr;
    let title = if pt {
        "O AIAnnotations não conseguiu abrir"
    } else {
        "AIAnnotations could not start"
    };
    let body = match (err, pt) {
        (StartupError::Database { path, detail }, true) => format!(
            "Não foi possível abrir o banco de dados:\n{path}\n\n{detail}\n\nVerifique se o arquivo \
             está acessível e se outro programa não o está usando."
        ),
        (StartupError::Database { path, detail }, false) => format!(
            "The database could not be opened:\n{path}\n\n{detail}\n\nCheck that the file is \
             accessible and that no other program is using it."
        ),
        (StartupError::Window { detail }, true) => format!(
            "Não foi possível criar a janela do aplicativo.\n\n{detail}\n\nNo Windows, confira se o \
             WebView2 está instalado."
        ),
        (StartupError::Window { detail }, false) => format!(
            "The application window could not be created.\n\n{detail}\n\nOn Windows, check that \
             WebView2 is installed."
        ),
    };
    (title.to_string(), body)
}

/// Prints the error, shows a native dialog and exits with status 1. Blocks until dismissed.
pub fn show_and_exit(lang: Lang, err: StartupError) -> ! {
    let (title, body) = message(lang, &err);
    eprintln!("aiannotations: {title}: {body}");
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(&title)
        .set_description(&body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> StartupError {
        StartupError::Database {
            path: "/data/aiannotations.db".into(),
            detail: "disk I/O error".into(),
        }
    }

    fn window() -> StartupError {
        StartupError::Window {
            detail: "no webview".into(),
        }
    }

    #[test]
    fn database_message_names_the_file() {
        let (title, body) = message(Lang::PtBr, &database());
        assert_eq!(title, "O AIAnnotations não conseguiu abrir");
        assert!(body.contains("/data/aiannotations.db"));
        assert!(body.contains("disk I/O error"));
    }

    #[test]
    fn window_message_mentions_webview2_on_windows() {
        let (_, body) = message(Lang::PtBr, &window());
        assert!(body.contains("WebView2"));
        assert!(body.contains("no webview"));
    }

    #[test]
    fn english_messages() {
        let (title, body) = message(Lang::En, &database());
        assert_eq!(title, "AIAnnotations could not start");
        assert!(body.contains("/data/aiannotations.db") && body.contains("disk I/O error"));
        let (_, body) = message(Lang::En, &window());
        assert!(body.contains("WebView2") && body.contains("no webview"));
    }
}
