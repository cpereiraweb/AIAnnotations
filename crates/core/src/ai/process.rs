//! Running an AI CLI to completion: stdin in, stdout/stderr out, with a timeout.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

/// How long, in total, to wait for both reader threads after the child is gone. The pipes
/// close with the child (and, on unix, with its process group); if something still holds
/// them we give up and return what we have.
const READER_GRACE: Duration = Duration::from_secs(2);

/// What a finished (or killed) process left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// Exit code; `None` when killed by a signal (including our timeout kill).
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Spawns `bin` in `cwd` with the inherited environment plus `env`, feeds it `stdin`
/// and closes it, and reads both outputs on threads (no deadlock on large output or
/// large input). On timeout the child is killed (on unix its whole process group, so
/// grandchildren die too), `timed_out` is set, `status` is `None` and the output read so
/// far is returned. Non-UTF-8 output is decoded lossily.
pub fn run(
    bin: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &[(OsString, OsString)],
    stdin: &str,
    timeout: Duration,
) -> std::io::Result<ProcessOutput> {
    let mut child = spawn(bin, args, cwd, env)?;

    // The child may not read its stdin (or may exit early): write on a thread and
    // ignore a broken pipe.
    let writer = child.stdin.take().map(|mut pipe| {
        let data = stdin.as_bytes().to_vec();
        thread::spawn(move || {
            let _ = pipe.write_all(&data);
        })
    });
    let out = child.stdout.take().map(reader);
    let err = child.stderr.take().map(reader);

    let (status, timed_out) = match child.wait_timeout(timeout) {
        Ok(Some(status)) => (status.code(), false),
        Ok(None) => {
            kill_tree(&mut child);
            (None, true)
        }
        Err(e) => {
            kill_tree(&mut child);
            return Err(e);
        }
    };

    // The writer ends when the child dies (broken pipe); it is detached, never joined.
    drop(writer);
    let deadline = Instant::now() + READER_GRACE;
    Ok(ProcessOutput {
        status,
        stdout: collect(out, deadline),
        stderr: collect(err, deadline),
        timed_out,
    })
}

/// Kills the child and reaps it. On unix the child leads its own process group
/// (see `spawn`), so the whole group is killed.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        // SAFETY: plain syscall; a negative pid targets the child's process group.
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn spawn(
    bin: &Path,
    args: &[OsString],
    cwd: &Path,
    env: &[(OsString, OsString)],
) -> std::io::Result<Child> {
    #[cfg(unix)]
    let mut attempts = 0;
    loop {
        let mut cmd = Command::new(bin);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        for (k, v) in env {
            cmd.env::<&OsStr, &OsStr>(k, v);
        }
        match cmd.spawn() {
            // ETXTBSY: a script written just before may still be open for writing in a
            // concurrently forked process; transient, so retry briefly.
            #[cfg(unix)]
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempts < 20 => {
                attempts += 1;
                thread::sleep(Duration::from_millis(25));
            }
            other => return other,
        }
    }
}

fn reader<R: Read + Send + 'static>(mut pipe: R) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

fn collect(rx: Option<mpsc::Receiver<Vec<u8>>>, deadline: Instant) -> String {
    let bytes = rx
        .and_then(|rx| {
            rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .ok()
        })
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn sh(script: &str, stdin: &str, timeout: Duration) -> ProcessOutput {
        let args = vec![OsString::from("-c"), OsString::from(script)];
        run(
            Path::new("/bin/sh"),
            &args,
            Path::new("/"),
            &[],
            stdin,
            timeout,
        )
        .unwrap()
    }

    #[test]
    fn captures_streams_status_and_stdin() {
        let out = sh(
            "cat; echo err >&2; exit 3",
            "hello",
            Duration::from_secs(10),
        );
        assert_eq!(out.stdout, "hello");
        assert_eq!(out.stderr, "err\n");
        assert_eq!(out.status, Some(3));
        assert!(!out.timed_out);
    }

    #[test]
    fn adds_env_to_the_inherited_one() {
        let args = vec![OsString::from("-c"), OsString::from("printf %s \"$FOO\"")];
        let env = vec![(OsString::from("FOO"), OsString::from("bar"))];
        let out = run(
            Path::new("/bin/sh"),
            &args,
            Path::new("/"),
            &env,
            "",
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(out.stdout, "bar");
    }

    #[test]
    fn timeout_kills_the_child() {
        let started = std::time::Instant::now();
        let out = sh("sleep 30", "", Duration::from_millis(300));
        assert!(out.timed_out);
        assert_eq!(out.status, None);
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn non_utf8_output_is_lossy() {
        let out = sh("printf '\\377ok'", "", Duration::from_secs(10));
        assert_eq!(out.stdout, "\u{fffd}ok");
    }

    #[test]
    fn missing_binary_is_not_found() {
        let err = run(
            Path::new("/nonexistent/bin"),
            &[],
            Path::new("/"),
            &[],
            "",
            Duration::from_secs(1),
        )
        .unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
