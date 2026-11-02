//! Pure argv assembly for editors, terminals and the manual template. Nothing here touches the
//! file system or spawns anything: the caller checks paths and runs the resulting `LaunchSpec`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::catalog::Family;
use super::detect::DetectedApp;
use super::shell::{kind_of, run_and_stay};
use super::{AppKind, Os, Shell};

/// Everything needed to start a process. `raw_args` is appended verbatim and only used on Windows
/// (`CommandExt::raw_arg`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub raw_args: Option<String>,
    pub cwd: Option<PathBuf>,
    pub new_console: bool,
}

/// What a terminal launch does: run a command or just open a shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    Command,
    ShellOnly,
}

/// Why a manual template was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    UnbalancedQuote,
    EditorWithCommand,
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid template")
    }
}

impl std::error::Error for TemplateError {}

// ---- file manager --------------------------------------------------------------------------

/// Windows form of a path for `explorer`: `\` separators and no trailing separator, except on
/// a drive root (`C:\`). Pure, so every OS compiles and tests it.
pub fn explorer_path(path: &str) -> String {
    let native = path.replace('/', "\\");
    let trimmed = native.trim_end_matches('\\');
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        format!("{trimmed}\\")
    } else {
        trimmed.to_string()
    }
}

/// The file-manager command for `dir` on `os`. Does not check that `dir` exists.
pub fn file_manager_argv(os: Os, dir: &Path) -> Vec<OsString> {
    match os {
        Os::Windows => vec![
            OsString::from("explorer"),
            OsString::from(explorer_path(&dir.to_string_lossy())),
        ],
        Os::MacOs => vec![OsString::from("open"), dir.as_os_str().to_owned()],
        Os::Linux => vec![OsString::from("xdg-open"), dir.as_os_str().to_owned()],
    }
}

// ---- helpers -------------------------------------------------------------------------------

fn spec_from(argv: Vec<OsString>) -> LaunchSpec {
    let mut it = argv.into_iter();
    LaunchSpec {
        program: it.next().unwrap_or_default(),
        args: it.collect(),
        raw_args: None,
        cwd: None,
        new_console: false,
    }
}

fn os(s: &str) -> OsString {
    OsString::from(s)
}

/// Quotes one argument by the MSVC rules: quoted when empty or when it has a space, tab or `"`;
/// backslashes before a quote (or before the closing quote) are doubled.
fn msvc_quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    msvc_quote_always(arg)
}

/// Same, but always wrapped in quotes (for paths the caller wants quoted regardless).
fn msvc_quote_always(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

fn msvc_join(argv: &[OsString]) -> String {
    argv.iter()
        .map(|a| msvc_quote(&a.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

// ---- editors -------------------------------------------------------------------------------

/// Opens `target` (the workspace file for VS Code, the folder for everything else; the caller
/// chooses) in the editor. The file manager is the OS opener.
pub fn editor_spec(app: &DetectedApp, os: Os, target: &Path) -> LaunchSpec {
    if app.family == Family::FileManager {
        return spec_from(file_manager_argv(os, target));
    }
    let mut argv = app.program.clone();
    argv.push(target.as_os_str().to_owned());
    spec_from(argv)
}

// ---- terminals -----------------------------------------------------------------------------

/// The shell to build the resume command for: the console terminals are their own shell.
pub fn effective_shell(app: &DetectedApp, configured: &Shell) -> Shell {
    match (app.family, app.program.first()) {
        (Family::PowerShellConsole | Family::CmdConsole, Some(exe)) => {
            let path = PathBuf::from(exe);
            Shell {
                kind: kind_of(&path),
                path,
            }
        }
        _ => configured.clone(),
    }
}

/// Opens `dir` in the terminal, running `run` (shell, command) when given. The command is
/// expected to carry its own `cd`; the shell comes from `effective_shell`.
pub fn terminal_spec(
    app: &DetectedApp,
    _os: Os,
    dir: &Path,
    run: Option<(&Shell, &str)>,
) -> (LaunchSpec, Ran) {
    let ran = if run.is_some() {
        Ran::Command
    } else {
        Ran::ShellOnly
    };
    let d = dir.as_os_str().to_owned();
    let dstr = dir.to_string_lossy().into_owned();
    let s: Option<Vec<OsString>> = run.map(|(sh, cmd)| run_and_stay(sh, cmd));
    let cmd = run.map(|(_, c)| c);

    // Program prefix plus the family's arguments.
    let with_prefix = |args: Vec<OsString>| {
        let mut argv = app.program.clone();
        argv.extend(args);
        spec_from(argv)
    };
    let flag_eq = |name: &str| {
        let mut v = os(name);
        v.push("=");
        v.push(&d);
        v
    };
    let dir_flag = |name: &str| vec![os(name), d.clone()];
    let tail = |sep: Option<&str>| -> Vec<OsString> {
        match &s {
            Some(s) => sep.into_iter().map(os).chain(s.iter().cloned()).collect(),
            None => Vec::new(),
        }
    };

    let spec = match app.family {
        Family::Kitty => with_prefix([dir_flag("--directory"), tail(None)].concat()),
        Family::Ghostty => {
            with_prefix([vec![flag_eq("--working-directory")], tail(Some("-e"))].concat())
        }
        Family::Alacritty => {
            with_prefix([dir_flag("--working-directory"), tail(Some("-e"))].concat())
        }
        Family::WezTerm => {
            with_prefix([vec![os("start")], dir_flag("--cwd"), tail(Some("--"))].concat())
        }
        Family::Ptyxis => with_prefix(
            [
                vec![os("--new-window"), flag_eq("--working-directory")],
                tail(Some("--")),
            ]
            .concat(),
        ),
        Family::GnomeTerminal => {
            with_prefix([vec![flag_eq("--working-directory")], tail(Some("--"))].concat())
        }
        Family::Konsole => with_prefix([dir_flag("--workdir"), tail(Some("-e"))].concat()),
        Family::TerminalApp | Family::ITerm2 => {
            let iterm = app.family == Family::ITerm2;
            match cmd {
                Some(c) => {
                    let c = applescript_escape(c);
                    let lines: Vec<String> = if iterm {
                        vec![
                            "tell application \"iTerm\"".into(),
                            "activate".into(),
                            "set w to (create window with default profile)".into(),
                            format!("tell current session of w to write text \"{c}\""),
                            "end tell".into(),
                        ]
                    } else {
                        vec![
                            "tell application \"Terminal\"".into(),
                            "activate".into(),
                            format!("do script \"{c}\""),
                            "end tell".into(),
                        ]
                    };
                    let mut argv = vec![os("osascript")];
                    for l in lines {
                        argv.push(os("-e"));
                        argv.push(OsString::from(l));
                    }
                    spec_from(argv)
                }
                None => with_prefix(vec![d.clone()]),
            }
        }
        Family::WindowsTerminal => {
            let mut raw = format!("-d {}", msvc_quote_always(&dstr));
            if let Some(s) = &s {
                raw.push(' ');
                raw.push_str(&msvc_join(s));
            }
            let mut sp = with_prefix(Vec::new());
            sp.raw_args = Some(raw.replace(';', r"\;"));
            sp
        }
        Family::PowerShellConsole => {
            let mut args = vec![os("-NoLogo"), os("-NoExit")];
            if let Some(c) = cmd {
                args.push(os("-Command"));
                args.push(os(c));
            }
            let mut sp = with_prefix(args);
            sp.cwd = Some(dir.to_path_buf());
            sp.new_console = true;
            sp
        }
        Family::CmdConsole => {
            let mut sp = with_prefix(Vec::new());
            sp.raw_args = cmd.map(|c| format!("/K {c}"));
            sp.cwd = Some(dir.to_path_buf());
            sp.new_console = true;
            sp
        }
        Family::Cmder => {
            let mut sp = with_prefix(Vec::new());
            sp.raw_args = Some(format!("/START {}", msvc_quote_always(&dstr)));
            return (sp, Ran::ShellOnly);
        }
        // Generic, and editor families used as terminals by mistake: `-e S…`. Without a command
        // there is no shell to name, so the terminal opens its own in `cwd`.
        _ => {
            let mut sp = with_prefix(tail(Some("-e")));
            if run.is_none() {
                sp.cwd = Some(dir.to_path_buf());
            }
            sp
        }
    };
    (spec, ran)
}

// ---- manual template -----------------------------------------------------------------------

/// Splits a template into tokens, honoring single and double quotes (no escapes: Windows paths
/// are full of backslashes). Fails on an unclosed quote.
pub fn split_template(s: &str) -> Result<Vec<String>, TemplateError> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '\'' || c == '"' => {
                quote = Some(c);
                started = true;
            }
            None if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(c);
                started = true;
            }
        }
    }
    if quote.is_some() {
        return Err(TemplateError::UnbalancedQuote);
    }
    if started {
        out.push(cur);
    }
    Ok(out)
}

/// Splits and validates a template for an app of `kind`: an editor cannot use `{cmd}`.
pub fn parse_template(s: &str, kind: AppKind) -> Result<Vec<String>, TemplateError> {
    let tokens = split_template(s)?;
    if kind == AppKind::Editor && tokens.iter().any(|t| t.contains("{cmd}")) {
        return Err(TemplateError::EditorWithCommand);
    }
    Ok(tokens)
}

/// Expands a manual template for `exe`. `{dir}` is replaced in every token; a token that is
/// exactly `{cmd}` becomes the run-and-stay argv, inside a larger token it becomes the command.
/// An empty template means `{dir}` for an editor and `-e S…` (Generic) for a terminal. Pure: it
/// does not check that `exe` exists.
pub fn custom_spec(
    exe: &Path,
    tokens: &[String],
    kind: AppKind,
    dir: &Path,
    run: Option<(&Shell, &str)>,
) -> (LaunchSpec, Ran) {
    let dstr = dir.to_string_lossy();
    let mut args: Vec<OsString> = Vec::new();
    let mut has_cmd = false;
    if tokens.is_empty() {
        match kind {
            AppKind::Editor => args.push(dir.as_os_str().to_owned()),
            AppKind::Terminal => {
                if let Some((sh, c)) = run {
                    args.push(os("-e"));
                    args.extend(run_and_stay(sh, c));
                    has_cmd = true;
                }
            }
        }
    }
    for t in tokens {
        if t == "{cmd}" {
            has_cmd = true;
            if let Some((sh, c)) = run {
                args.extend(run_and_stay(sh, c));
            }
            continue;
        }
        let mut t = t.replace("{dir}", &dstr);
        if t.contains("{cmd}") {
            has_cmd = true;
            t = t.replace("{cmd}", run.map_or("", |(_, c)| c));
        }
        args.push(OsString::from(t));
    }
    let ran = if kind == AppKind::Terminal && has_cmd && run.is_some() {
        Ran::Command
    } else {
        Ran::ShellOnly
    };
    (
        LaunchSpec {
            program: exe.as_os_str().to_owned(),
            args,
            raw_args: None,
            cwd: Some(dir.to_path_buf()),
            new_console: false,
        },
        ran,
    )
}
