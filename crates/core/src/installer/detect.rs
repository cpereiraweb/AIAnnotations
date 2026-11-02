//! Finds the AI CLIs, where their hook config lives, our own `aiannotations` CLI and the
//! exact command string the hooks run. Pure: environment values come in as parameters.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::ai::binary::resolve;
use crate::installer::hooks::{HookEvent, HookSpec};
use crate::model::Tool;
use crate::paths::Paths;

/// The tool's CLI, or `None` when it is not installed. `override_bin` is the value of
/// `AIANNOTATIONS_CLAUDE_BIN` / `AIANNOTATIONS_CODEX_BIN` (same as `ai::client_for`).
pub fn find_cli(
    tool: Tool,
    override_bin: Option<&OsStr>,
    home: &Path,
    path_var: Option<&OsStr>,
) -> Option<PathBuf> {
    let name = tool.as_str();
    let found = resolve(name, override_bin, home, path_var);
    let overridden = override_bin.is_some_and(|o| !o.to_string_lossy().trim().is_empty());
    (overridden || found != Path::new(name)).then_some(found)
}

/// The file our hooks are merged into: Claude's `settings.json`, Codex's `hooks.json`.
pub fn hook_config_path(tool: Tool, paths: &Paths) -> PathBuf {
    match tool {
        Tool::Claude => paths.claude_dir.join("settings.json"),
        Tool::Codex => paths.codex_home.join("hooks.json"),
    }
}

/// Our CLI, shipped next to the app executable.
pub fn cli_beside(exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "aiannotations.exe"
    } else {
        "aiannotations"
    };
    exe.parent().unwrap_or(Path::new("")).join(name)
}

fn event_word(event: HookEvent) -> &'static str {
    match event {
        HookEvent::Start => "start",
        HookEvent::End => "end",
    }
}

/// The command a hook runs. Unix: `'<cli>' hook <tool> <event>` (single-quoted, `'` as
/// `'\''`). Windows: `"<cli>" hook <tool> <event>` with forward slashes, since Claude Code
/// runs hooks through Git Bash there.
pub fn hook_command(cli: &Path, tool: Tool, event: HookEvent, windows: bool) -> String {
    let path = cli.to_string_lossy();
    let quoted = if windows {
        format!("\"{}\"", path.replace('\\', "/"))
    } else {
        format!("'{}'", path.replace('\'', r"'\''"))
    };
    format!("{quoted} hook {} {}", tool.as_str(), event_word(event))
}

/// The PowerShell form, for Codex's `commandWindows`: `& '<cli>' hook <tool> <event>`, with
/// forward slashes and `'` doubled. Single quotes keep `$` and backticks literal.
pub fn powershell_command(cli: &Path, tool: Tool, event: HookEvent) -> String {
    let path = cli.to_string_lossy().replace('\\', "/").replace('\'', "''");
    format!("& '{path}' hook {} {}", tool.as_str(), event_word(event))
}

pub fn hook_spec(cli: &Path, tool: Tool, windows: bool) -> HookSpec {
    // Only Codex: Claude Code runs hooks through Git Bash on Windows, where `command` works.
    let powershell =
        |event| (windows && tool == Tool::Codex).then(|| powershell_command(cli, tool, event));
    HookSpec {
        tool,
        start: hook_command(cli, tool, HookEvent::Start, windows),
        end: hook_command(cli, tool, HookEvent::End, windows),
        start_windows: powershell(HookEvent::Start),
        end_windows: powershell(HookEvent::End),
    }
}
