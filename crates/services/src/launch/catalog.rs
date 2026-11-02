//! The editors and terminals the app knows how to find, in the order of preference of the default.
//! Pure data: where to look is in `detect`, how to run them is in `argv`.

use super::AppKind;

/// Launch behavior group: apps of one family share their argv shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    // Editors
    VsCode,
    Zed,
    Sublime,
    JetBrains,
    FileManager,
    // Terminals
    WindowsTerminal,
    PowerShellConsole,
    CmdConsole,
    Cmder,
    TerminalApp,
    ITerm2,
    Kitty,
    Ghostty,
    WezTerm,
    Alacritty,
    Ptyxis,
    GnomeTerminal,
    Konsole,
    Generic,
}

/// One catalog entry. Every list is optional: an empty one means "not on this OS".
#[derive(Debug, Clone, Copy)]
pub struct Known {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: AppKind,
    pub family: Family,
    /// Executable names looked up in `PATH` (Windows: without extension, `PATHEXT` is applied).
    pub path_names: &'static [&'static str],
    /// `.desktop` file names (Linux).
    pub desktop_ids: &'static [&'static str],
    /// Bundle names in the applications folders (macOS).
    pub mac_bundles: &'static [&'static str],
    /// Install locations (Windows), with `%VAR%` placeholders.
    pub win_paths: &'static [&'static str],
    /// Executable names registered under the `App Paths` registry key (Windows).
    pub win_app_paths: &'static [&'static str],
}

const EMPTY: Known = Known {
    id: "",
    label: "",
    kind: AppKind::Editor,
    family: Family::Generic,
    path_names: &[],
    desktop_ids: &[],
    mac_bundles: &[],
    win_paths: &[],
    win_app_paths: &[],
};

/// A JetBrains IDE: `$id` is also its command-line script name (and Toolbox script name).
macro_rules! jetbrains {
    ($id:literal, $label:literal, $bundle:literal) => {
        Known {
            id: $id,
            label: $label,
            family: Family::JetBrains,
            path_names: &[$id],
            desktop_ids: &[concat!("jetbrains-", $id, ".desktop")],
            mac_bundles: &[$bundle],
            win_paths: &[concat!(
                r"%LOCALAPPDATA%\JetBrains\Toolbox\scripts\",
                $id,
                ".cmd"
            )],
            ..EMPTY
        }
    };
}

pub const KNOWN: &[Known] = &[
    // Editors
    Known {
        id: "vscode",
        label: "Visual Studio Code",
        family: Family::VsCode,
        path_names: &["code"],
        desktop_ids: &[
            "code.desktop",
            "com.visualstudio.code.desktop",
            "visual-studio-code.desktop",
        ],
        mac_bundles: &["Visual Studio Code.app"],
        win_paths: &[
            r"%LOCALAPPDATA%\Programs\Microsoft VS Code\Code.exe",
            r"%ProgramFiles%\Microsoft VS Code\Code.exe",
            r"%ProgramFiles(x86)%\Microsoft VS Code\Code.exe",
        ],
        win_app_paths: &["Code.exe"],
        ..EMPTY
    },
    Known {
        id: "cursor",
        label: "Cursor",
        family: Family::VsCode,
        path_names: &["cursor"],
        desktop_ids: &["cursor.desktop"],
        mac_bundles: &["Cursor.app"],
        win_paths: &[r"%LOCALAPPDATA%\Programs\cursor\Cursor.exe"],
        win_app_paths: &["Cursor.exe"],
        ..EMPTY
    },
    Known {
        id: "windsurf",
        label: "Windsurf",
        family: Family::VsCode,
        path_names: &["windsurf"],
        desktop_ids: &["windsurf.desktop"],
        mac_bundles: &["Windsurf.app"],
        win_paths: &[r"%LOCALAPPDATA%\Programs\Windsurf\Windsurf.exe"],
        win_app_paths: &["Windsurf.exe"],
        ..EMPTY
    },
    Known {
        id: "antigravity",
        label: "Antigravity",
        family: Family::VsCode,
        path_names: &["antigravity"],
        desktop_ids: &["antigravity.desktop"],
        mac_bundles: &["Antigravity.app"],
        win_paths: &[r"%LOCALAPPDATA%\Programs\Antigravity\Antigravity.exe"],
        win_app_paths: &["Antigravity.exe"],
        ..EMPTY
    },
    Known {
        id: "zed",
        label: "Zed",
        family: Family::Zed,
        path_names: &["zed", "zeditor"],
        desktop_ids: &["dev.zed.Zed.desktop", "zed.desktop"],
        mac_bundles: &["Zed.app"],
        win_paths: &[r"%LOCALAPPDATA%\Programs\Zed\Zed.exe"],
        win_app_paths: &["Zed.exe"],
        ..EMPTY
    },
    Known {
        id: "sublime",
        label: "Sublime Text",
        family: Family::Sublime,
        path_names: &["subl", "sublime_text"],
        desktop_ids: &["sublime_text.desktop", "sublime-text.desktop"],
        mac_bundles: &["Sublime Text.app"],
        win_paths: &[r"%ProgramFiles%\Sublime Text\sublime_text.exe"],
        win_app_paths: &["sublime_text.exe"],
        ..EMPTY
    },
    // JetBrains: the script name is also the Toolbox script (Linux and Windows).
    jetbrains!("phpstorm", "PhpStorm", "PhpStorm.app"),
    jetbrains!("idea", "IntelliJ IDEA", "IntelliJ IDEA.app"),
    jetbrains!("webstorm", "WebStorm", "WebStorm.app"),
    jetbrains!("pycharm", "PyCharm", "PyCharm.app"),
    jetbrains!("goland", "GoLand", "GoLand.app"),
    jetbrains!("rider", "Rider", "Rider.app"),
    jetbrains!("clion", "CLion", "CLion.app"),
    jetbrains!("rubymine", "RubyMine", "RubyMine.app"),
    jetbrains!("rustrover", "RustRover", "RustRover.app"),
    // Always available: the system's file manager.
    Known {
        id: "files",
        label: "File manager",
        family: Family::FileManager,
        ..EMPTY
    },
    // Terminals
    Known {
        id: "windows-terminal",
        label: "Windows Terminal",
        kind: AppKind::Terminal,
        family: Family::WindowsTerminal,
        path_names: &["wt"],
        win_paths: &[r"%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe"],
        ..EMPTY
    },
    Known {
        id: "powershell",
        label: "PowerShell",
        kind: AppKind::Terminal,
        family: Family::PowerShellConsole,
        path_names: &["pwsh", "powershell"],
        win_paths: &[
            r"%ProgramFiles%\PowerShell\7\pwsh.exe",
            r"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe",
        ],
        ..EMPTY
    },
    Known {
        id: "cmd",
        label: "Command Prompt",
        kind: AppKind::Terminal,
        family: Family::CmdConsole,
        path_names: &["cmd"],
        win_paths: &[r"%SystemRoot%\System32\cmd.exe"],
        ..EMPTY
    },
    Known {
        id: "cmder",
        label: "Cmder",
        kind: AppKind::Terminal,
        family: Family::Cmder,
        win_paths: &[r"%CMDER_ROOT%\Cmder.exe"],
        ..EMPTY
    },
    Known {
        id: "terminal-app",
        label: "Terminal",
        kind: AppKind::Terminal,
        family: Family::TerminalApp,
        mac_bundles: &["Terminal.app"],
        ..EMPTY
    },
    Known {
        id: "iterm2",
        label: "iTerm2",
        kind: AppKind::Terminal,
        family: Family::ITerm2,
        mac_bundles: &["iTerm.app"],
        ..EMPTY
    },
    Known {
        id: "kitty",
        label: "kitty",
        kind: AppKind::Terminal,
        family: Family::Kitty,
        path_names: &["kitty"],
        desktop_ids: &["kitty.desktop", "net.kovidgoyal.kitty.desktop"],
        mac_bundles: &["kitty.app"],
        ..EMPTY
    },
    Known {
        id: "ghostty",
        label: "Ghostty",
        kind: AppKind::Terminal,
        family: Family::Ghostty,
        path_names: &["ghostty"],
        desktop_ids: &["com.mitchellh.ghostty.desktop"],
        mac_bundles: &["Ghostty.app"],
        ..EMPTY
    },
    Known {
        id: "wezterm",
        label: "WezTerm",
        kind: AppKind::Terminal,
        family: Family::WezTerm,
        path_names: &["wezterm"],
        desktop_ids: &["org.wezfurlong.wezterm.desktop"],
        mac_bundles: &["WezTerm.app"],
        win_paths: &[r"%ProgramFiles%\WezTerm\wezterm-gui.exe"],
        ..EMPTY
    },
    Known {
        id: "alacritty",
        label: "Alacritty",
        kind: AppKind::Terminal,
        family: Family::Alacritty,
        path_names: &["alacritty"],
        desktop_ids: &["Alacritty.desktop", "alacritty.desktop"],
        mac_bundles: &["Alacritty.app"],
        win_paths: &[r"%ProgramFiles%\Alacritty\alacritty.exe"],
        ..EMPTY
    },
    Known {
        id: "ptyxis",
        label: "Ptyxis",
        kind: AppKind::Terminal,
        family: Family::Ptyxis,
        path_names: &["ptyxis"],
        desktop_ids: &["org.gnome.Ptyxis.desktop"],
        ..EMPTY
    },
    Known {
        id: "gnome-terminal",
        label: "GNOME Terminal",
        kind: AppKind::Terminal,
        family: Family::GnomeTerminal,
        path_names: &["gnome-terminal"],
        desktop_ids: &["org.gnome.Terminal.desktop"],
        ..EMPTY
    },
    Known {
        id: "konsole",
        label: "Konsole",
        kind: AppKind::Terminal,
        family: Family::Konsole,
        path_names: &["konsole"],
        desktop_ids: &["org.kde.konsole.desktop"],
        ..EMPTY
    },
    Known {
        id: "x-terminal-emulator",
        label: "Default terminal",
        kind: AppKind::Terminal,
        family: Family::Generic,
        path_names: &["x-terminal-emulator"],
        ..EMPTY
    },
];
