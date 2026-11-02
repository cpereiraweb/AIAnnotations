//! Launching things outside the app: shells, terminals, editors and file managers.

use std::path::PathBuf;

pub mod apps;
pub mod argv;
pub mod catalog;
pub mod detect;
pub mod shell;
pub mod spawn;
#[doc(hidden)]
pub mod testing;
pub mod workspaces;

pub use aiannotations_core::user_settings::{AppKind, CustomApp};

/// Operating system, as a value so every platform's logic is testable on any host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    MacOs,
    Windows,
}

impl Os {
    /// The OS this binary was compiled for.
    pub fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// Shell dialect: how commands are quoted, chained and run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Posix,
    Fish,
    PowerShell,
    Cmd,
}

impl ShellKind {
    pub const ALL: [ShellKind; 4] = [
        ShellKind::Posix,
        ShellKind::Fish,
        ShellKind::PowerShell,
        ShellKind::Cmd,
    ];
}

/// A shell executable and its dialect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    pub path: PathBuf,
    pub kind: ShellKind,
}
