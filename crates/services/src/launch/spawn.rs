//! Starting a `LaunchSpec` detached from the app: no stdio, its own process group, and reaped
//! off-thread so it never stays a zombie.

use std::process::{Command, Stdio};

use aiannotations_core::Result;

use super::argv::LaunchSpec;

/// Windows creation flags (spec §7); console terminals get their own window in
/// [`spawn_new_console`].
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Starts `spec` detached. `raw_args` is only applied on Windows (`CommandExt::raw_arg`).
pub fn spawn_detached(spec: &LaunchSpec) -> Result<()> {
    #[cfg(windows)]
    if spec.new_console {
        return spawn_new_console(spec);
    }
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Some(raw) = &spec.raw_args {
            cmd.raw_arg(raw);
        }
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    spawn_reaped(&mut cmd)?;
    Ok(())
}

/// Starts a console terminal (PowerShell, cmd) in a console window of its own.
///
/// Not through `std::process::Command`: it always hands the child standard handles, and with
/// NUL ones the shell reads an empty stdin and exits at once (the window flashes and closes),
/// while inherited ones would tie it to the app's own console in development. Without
/// `STARTF_USESTDHANDLES`, Windows gives the child the handles of its new console.
#[cfg(windows)]
fn spawn_new_console(spec: &LaunchSpec) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CREATE_NEW_CONSOLE, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let wide = |s: &std::ffi::OsStr| -> Vec<u16> { s.encode_wide().chain([0]).collect() };
    let mut line = wide(std::ffi::OsStr::new(&super::argv::windows_command_line(
        spec,
    )));
    let cwd = spec.cwd.as_ref().map(|d| wide(d.as_os_str()));
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every pointer is either null or points to a NUL-terminated buffer, a zeroed
    // STARTUPINFOW with its size set, or a PROCESS_INFORMATION, all alive for the call.
    let ok = unsafe {
        CreateProcessW(
            std::ptr::null(),
            line.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP,
            std::ptr::null(),
            cwd.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &startup,
            &mut info,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: both handles were just returned by CreateProcessW and are closed once.
    unsafe {
        CloseHandle(info.hThread);
        CloseHandle(info.hProcess);
    }
    Ok(())
}

/// Spawns `cmd` and returns its pid. A detached thread waits on the child so that it does not
/// stay a zombie on Unix until the app quits.
pub(crate) fn spawn_reaped(cmd: &mut Command) -> Result<u32> {
    let mut child = cmd.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(pid)
}

// Every test here needs a Unix process (`true`): on Windows the module would be dead code.
#[cfg(all(test, unix))]
mod tests {
    use std::ffi::OsString;

    use super::*;

    /// The opener exits at once; the app must not keep it as a zombie until it quits.
    #[cfg(target_os = "linux")]
    #[test]
    fn spawned_child_is_reaped() {
        let pid = spawn_reaped(&mut Command::new("true")).unwrap();
        let stat = format!("/proc/{pid}/stat");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::path::Path::new(&stat).exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "still in the process table: {}",
                std::fs::read_to_string(&stat).unwrap_or_default()
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    fn spec(program: &str, cwd: Option<std::path::PathBuf>) -> LaunchSpec {
        LaunchSpec {
            program: OsString::from(program),
            args: Vec::new(),
            raw_args: None,
            cwd,
            new_console: false,
        }
    }

    #[test]
    fn spawn_detached_runs_in_cwd_and_reports_missing_program() {
        let dir = tempfile::tempdir().unwrap();
        spawn_detached(&spec("true", Some(dir.path().to_path_buf()))).unwrap();
        assert!(spawn_detached(&spec("true", Some(dir.path().join("missing")))).is_err());
        assert!(spawn_detached(&spec("/nonexistent/terminal", None)).is_err());
    }
}
