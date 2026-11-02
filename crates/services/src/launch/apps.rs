//! The user's launch settings applied to what was detected: the Settings screen's view, "open
//! with" and the resume that opens a terminal. Manual apps (`CustomApp`) count as editors or
//! terminals like detected ones, after them. Spawning and the clipboard come in as closures,
//! so nothing here starts a process by itself.

use std::path::{Path, PathBuf};

use aiannotations_core::user_settings::UserSettings;
use aiannotations_core::{Error, Result};
use serde::Serialize;

use super::argv::{
    LaunchSpec, Ran, custom_spec, editor_spec, effective_shell, parse_template, terminal_spec,
};
use super::catalog::Family;
use super::detect::{DetectedApp, Probe, default_shell, detect_apps, detect_shells};
use super::shell::kind_of;
use super::workspaces::workspace_for;
use super::{AppKind, CustomApp, Os, Shell, ShellKind};
use crate::actions::check_dir;

/// Everything detection found; the app caches it until "detect again".
#[derive(Debug, Clone)]
pub struct Detected {
    pub apps: Vec<DetectedApp>,
    pub shells: Vec<Shell>,
    pub default_shell: Shell,
}

impl Detected {
    pub fn probe(p: &dyn Probe) -> Detected {
        Detected {
            apps: detect_apps(p),
            shells: detect_shells(p),
            default_shell: default_shell(p),
        }
    }
}

/// One option of the editor or terminal select.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppEntry {
    pub id: String,
    pub label: String,
    /// Added by hand (`CustomApp`), not detected.
    pub custom: bool,
}

/// The "Editores e terminais" section of the Settings screen.
#[derive(Debug, Clone, Serialize)]
pub struct AppsView {
    pub editors: Vec<AppEntry>,
    pub terminals: Vec<AppEntry>,
    /// Detected shell paths, plus the configured one when detection missed it.
    pub shells: Vec<String>,
    /// Effective default editor id (the choice if it still exists, else the first one).
    pub editor: String,
    /// Effective default terminal id; `None` when there is no terminal at all.
    pub terminal: Option<String>,
    /// Effective shell path.
    pub shell: String,
    /// The user's own choices; `None` is automatic.
    pub editor_choice: Option<String>,
    pub terminal_choice: Option<String>,
    pub shell_choice: Option<String>,
    pub custom_apps: Vec<CustomApp>,
    pub workspaces_dir: Option<String>,
}

/// What "open with" opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The effective default editor.
    Default,
    /// Any editor or terminal, by id.
    App(String),
}

/// What the resume did after copying the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Opened {
    /// The terminal opened running the command.
    Ran,
    /// The terminal opened a shell; the user pastes the command.
    ShellOnly,
    /// No terminal: only copied.
    None,
    /// The terminal did not open (`error` says why); the command is copied.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumeOutcome {
    pub command: String,
    pub opened: Opened,
    pub error: Option<String>,
}

/// Starts a launch spec (the app passes `spawn::spawn_detached`).
pub type Spawn<'a> = &'a dyn Fn(&LaunchSpec) -> Result<()>;

#[derive(Clone, Copy)]
enum App<'a> {
    Detected(&'a DetectedApp),
    Custom(&'a CustomApp),
}

impl App<'_> {
    fn id(&self) -> &str {
        match self {
            App::Detected(a) => &a.id,
            App::Custom(c) => &c.id,
        }
    }

    fn entry(&self) -> AppEntry {
        let (label, custom) = match self {
            App::Detected(a) => (&a.label, false),
            App::Custom(c) => (&c.label, true),
        };
        AppEntry {
            id: self.id().to_string(),
            label: label.clone(),
            custom,
        }
    }
}

fn code(c: &str) -> Error {
    Error::Installer(c.into())
}

/// Detected apps of `kind`, then the manual ones.
fn of_kind<'a>(detected: &'a [DetectedApp], user: &'a UserSettings, kind: AppKind) -> Vec<App<'a>> {
    detected
        .iter()
        .filter(|a| a.kind == kind)
        .map(App::Detected)
        .chain(
            user.custom_apps
                .iter()
                .filter(|c| c.kind == kind)
                .map(App::Custom),
        )
        .collect()
}

/// The user's choice if it still exists, else the first app of the kind.
fn effective<'a>(
    detected: &'a [DetectedApp],
    user: &'a UserSettings,
    kind: AppKind,
) -> Option<App<'a>> {
    let list = of_kind(detected, user, kind);
    let choice = match kind {
        AppKind::Editor => user.editor.as_deref(),
        AppKind::Terminal => user.terminal.as_deref(),
    };
    choice
        .and_then(|c| list.iter().find(|a| a.id() == c).copied())
        .or_else(|| list.first().copied())
}

fn find<'a>(detected: &'a [DetectedApp], user: &'a UserSettings, id: &str) -> Option<App<'a>> {
    detected
        .iter()
        .find(|a| a.id == id)
        .map(App::Detected)
        .or_else(|| {
            user.custom_apps
                .iter()
                .find(|c| c.id == id)
                .map(App::Custom)
        })
}

/// The shell the user configured, else the detected default.
pub fn configured_shell(user: &UserSettings, default_shell: &Shell) -> Shell {
    match &user.shell {
        Some(path) => Shell {
            kind: kind_of(path),
            path: path.clone(),
        },
        None => default_shell.clone(),
    }
}

fn path_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn view(
    detected: &[DetectedApp],
    shells: &[Shell],
    default_shell: &Shell,
    user: &UserSettings,
) -> AppsView {
    let entries = |kind| -> Vec<AppEntry> {
        of_kind(detected, user, kind)
            .iter()
            .map(App::entry)
            .collect()
    };
    let mut shell_list: Vec<String> = shells.iter().map(|s| path_string(&s.path)).collect();
    let shell = path_string(&configured_shell(user, default_shell).path);
    if user.shell.is_some() && !shell_list.contains(&shell) {
        shell_list.push(shell.clone());
    }
    AppsView {
        editors: entries(AppKind::Editor),
        terminals: entries(AppKind::Terminal),
        shells: shell_list,
        editor: effective(detected, user, AppKind::Editor)
            .map(|a| a.id().to_string())
            .unwrap_or_default(),
        terminal: effective(detected, user, AppKind::Terminal).map(|a| a.id().to_string()),
        shell,
        editor_choice: user.editor.clone(),
        terminal_choice: user.terminal.clone(),
        shell_choice: user.shell.as_deref().map(path_string),
        custom_apps: user.custom_apps.clone(),
        workspaces_dir: user.workspaces_dir.as_deref().map(path_string),
    }
}

/// Drops the `\\?\` prefix `canonicalize` adds on Windows (`\\?\C:\x` → `C:\x`); editors do
/// not all accept it. `\\?\UNC\…` is left alone.
pub fn strip_verbatim(p: &Path) -> PathBuf {
    let raw = p.to_string_lossy();
    match raw.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with(r"UNC\") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

/// What an editor opens: the project's workspace file for the VS Code family when a workspaces
/// folder is set and one lists it, else the folder.
fn editor_target(app: &DetectedApp, user: &UserSettings, dir: &Path) -> PathBuf {
    if app.family == Family::VsCode
        && let Some(ws_dir) = &user.workspaces_dir
        && let Some(file) = workspace_for(ws_dir, dir)
    {
        return strip_verbatim(&file);
    }
    dir.to_path_buf()
}

/// A manual app's spec; its executable must still be an absolute path to an existing file (as
/// checked on save: a relative one would be searched in `PATH`) and its template parse.
fn custom_launch(
    c: &CustomApp,
    os: Os,
    dir: &Path,
    run: Option<(&Shell, &str)>,
) -> Result<(LaunchSpec, Ran)> {
    if !(c.exe.is_absolute() && c.exe.is_file()) {
        return Err(code("executable not found"));
    }
    let tokens = parse_template(&c.args, c.kind).map_err(|e| code(&e.to_string()))?;
    Ok(custom_spec(&c.exe, &tokens, c.kind, os, dir, run))
}

/// "Open with": `dir` (absolute, existing) in the default editor or in the app `id`. A terminal
/// opens a shell there. Errors: `app not found`, `executable not found`, `invalid template`.
pub fn open_dir(
    detected: &[DetectedApp],
    user: &UserSettings,
    os: Os,
    dir: &Path,
    target: Target,
    spawn: Spawn<'_>,
) -> Result<()> {
    check_dir(dir)?;
    let app = match &target {
        Target::Default => effective(detected, user, AppKind::Editor),
        Target::App(id) => find(detected, user, id),
    }
    .ok_or_else(|| code("app not found"))?;
    let spec = match app {
        App::Detected(a) if a.kind == AppKind::Editor => {
            editor_spec(a, os, &editor_target(a, user, dir))
        }
        App::Detected(a) => terminal_spec(a, os, dir, None).0,
        App::Custom(c) => custom_launch(c, os, dir, None)?.0,
    };
    spawn(&spec)
}

/// "▶ retomar": builds the command for the shell the default terminal will use, copies it and
/// only then opens the terminal in `dir`. Only `build` and `copy` failures are an `Err`; a
/// terminal that does not open is `Opened::Failed`, the command still copied.
#[allow(clippy::too_many_arguments)]
pub fn resume_in_terminal(
    detected: &[DetectedApp],
    user: &UserSettings,
    os: Os,
    default_shell: &Shell,
    dir: &Path,
    build: &dyn Fn(ShellKind) -> Result<String>,
    copy: &mut dyn FnMut(&str) -> Result<()>,
    spawn: Spawn<'_>,
) -> Result<ResumeOutcome> {
    let configured = configured_shell(user, default_shell);
    let terminal = effective(detected, user, AppKind::Terminal);
    let shell = match terminal {
        Some(App::Detected(a)) => effective_shell(a, &configured),
        _ => configured,
    };
    let command = build(shell.kind)?;
    copy(&command)?;
    let Some(terminal) = terminal else {
        return Ok(ResumeOutcome {
            command,
            opened: Opened::None,
            error: None,
        });
    };
    let run = Some((&shell, command.as_str()));
    let launched = check_dir(dir)
        .and_then(|()| match terminal {
            App::Detected(a) => Ok(terminal_spec(a, os, dir, run)),
            App::Custom(c) => custom_launch(c, os, dir, run),
        })
        .and_then(|(spec, ran)| spawn(&spec).map(|()| ran));
    let (opened, error) = match launched {
        Ok(Ran::Command) => (Opened::Ran, None),
        Ok(Ran::ShellOnly) => (Opened::ShellOnly, None),
        Err(e) => (Opened::Failed, Some(e.to_string())),
    };
    Ok(ResumeOutcome {
        command,
        opened,
        error,
    })
}
