//! Child processes started by the app.

use std::process::Command;

/// Windows `CREATE_NO_WINDOW`.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Keeps a console program (git, claude, codex) from opening a window of its own on Windows.
/// The release app is a GUI program with no console, so without this every background call
/// flashes a terminal window; in development `tauri dev`'s console hid the problem.
pub fn hide_console(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}
