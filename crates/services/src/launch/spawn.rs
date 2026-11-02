//! Starting a `LaunchSpec` detached from the app: no stdio, its own process group, and reaped
//! off-thread so it never stays a zombie.

use std::process::{Command, Stdio};

use aiannotations_core::Result;

use super::argv::LaunchSpec;

/// Windows creation flags (spec §7): console terminals need a window of their own.
#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

/// Starts `spec` detached. `raw_args` is only applied on Windows (`CommandExt::raw_arg`).
pub fn spawn_detached(spec: &LaunchSpec) -> Result<()> {
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
        let flags = if spec.new_console {
            CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP
        } else {
            DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
        };
        cmd.creation_flags(flags);
    }
    spawn_reaped(&mut cmd)?;
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
