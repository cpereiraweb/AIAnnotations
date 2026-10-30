//! Git inspector (port of `app/Git/GitInspector.php`, Inv §3.6).
//!
//! Every call is an argv (never a shell) with a timeout; failures degrade to
//! `None`/empty, never to an error.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;

/// Variables that redirect git to another repository (inherited from a hook or
/// an alias such as `homegit`); stripped from every child.
const ISOLATED_ENV: [&str; 8] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
];

const BASE_CANDIDATES: [&str; 3] = ["develop", "main", "master"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSnapshot {
    /// Project key: parent of the git common dir (the main repository root).
    pub repo_root: Option<PathBuf>,
    /// Top level of the checkout; differs from `repo_root` in a linked worktree.
    pub toplevel: Option<PathBuf>,
    pub remote_url: Option<String>,
    pub branch: Option<String>,
    pub head: Option<String>,
    /// sha1 hex of `git status --porcelain`.
    pub status_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleBranch {
    pub branch: String,
    pub ahead: u32,
    pub last_commit_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub last_commit_ms: Option<i64>,
}

pub struct Git {
    timeout: Duration,
    bin: PathBuf,
}

impl Default for Git {
    fn default() -> Self {
        Git {
            timeout: Duration::from_secs(5),
            bin: PathBuf::from("git"),
        }
    }
}

impl Git {
    /// Git at `bin` (tests use a fake one), with the default 5 s timeout.
    pub fn with_bin(bin: PathBuf) -> Git {
        Git {
            bin,
            ..Git::default()
        }
    }
}

fn isolate_env(cmd: &mut Command) {
    for name in ISOLATED_ENV {
        cmd.env_remove(name);
    }
}

impl Git {
    pub fn snapshot(&self, cwd: &Path) -> GitSnapshot {
        let (Some(common), Some(top)) = (
            self.git(cwd, &["rev-parse", "--git-common-dir"]),
            self.git(cwd, &["rev-parse", "--show-toplevel"]),
        ) else {
            return GitSnapshot::default();
        };

        let top_path = PathBuf::from(&top);
        let top_real = std::fs::canonicalize(&top_path).unwrap_or(top_path);
        let common_path = cwd.join(&common); // absolute `common` replaces cwd
        let repo_root = std::fs::canonicalize(&common_path)
            .ok()
            .and_then(|c| c.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| top_real.clone());

        let mut remote = self
            .git(cwd, &["remote", "get-url", "origin"])
            .filter(|r| !r.is_empty());
        if remote.is_none() {
            remote = self
                .git(cwd, &["remote"])
                .and_then(|l| l.lines().next().map(str::to_string))
                .filter(|n| !n.is_empty())
                .and_then(|n| self.git(cwd, &["remote", "get-url", &n]))
                .filter(|r| !r.is_empty());
        }

        GitSnapshot {
            repo_root: Some(repo_root),
            toplevel: Some(top_real),
            remote_url: remote,
            branch: self
                .git(cwd, &["symbolic-ref", "--short", "-q", "HEAD"])
                .filter(|b| !b.is_empty()),
            head: self
                .git(cwd, &["rev-parse", "HEAD"])
                .filter(|h| !h.is_empty()),
            status_hash: self.status_porcelain(cwd).map(|s| sha1_hex(&s)),
        }
    }

    /// Raw (untrimmed) `status --porcelain`; `None` when git fails.
    pub fn status_porcelain(&self, dir: &Path) -> Option<String> {
        self.run(dir, &["status", "--porcelain"])
    }

    /// Branches ahead of the base (`develop`, `main` or `master`, first that
    /// exists) whose last commit is at least `min_age_days` old at `now_ms`.
    pub fn stale_branches(&self, dir: &Path, min_age_days: u32, now_ms: i64) -> Vec<StaleBranch> {
        let Some(base) = BASE_CANDIDATES.iter().find(|c| {
            self.git(
                dir,
                &["rev-parse", "--verify", "-q", &format!("refs/heads/{c}")],
            )
            .is_some()
        }) else {
            return Vec::new();
        };
        let Some(out) = self.git(
            dir,
            &[
                "for-each-ref",
                "--format=%(refname:short)%09%(committerdate:unix)",
                "refs/heads",
            ],
        ) else {
            return Vec::new();
        };

        let limit_ms = now_ms - i64::from(min_age_days) * 86_400_000;
        let mut result = Vec::new();
        for line in out.lines() {
            let Some((branch, ts)) = line.split_once('\t') else {
                continue;
            };
            if branch == *base {
                continue;
            }
            let Ok(ts) = ts.trim().parse::<i64>() else {
                continue;
            };
            let ahead = self
                .git(
                    dir,
                    &[
                        "rev-list",
                        "--count",
                        &format!("{base}..refs/heads/{branch}"),
                    ],
                )
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0);
            let last_commit_ms = ts * 1000;
            if ahead > 0 && last_commit_ms <= limit_ms {
                result.push(StaleBranch {
                    branch: branch.to_string(),
                    ahead,
                    last_commit_ms,
                });
            }
        }
        result
    }

    pub fn worktrees(&self, dir: &Path) -> Vec<Worktree> {
        let Some(out) = self.git(dir, &["worktree", "list", "--porcelain"]) else {
            return Vec::new();
        };
        let mut list = Vec::new();
        for block in out.split("\n\n") {
            let (mut path, mut head, mut branch) = (None, None, None);
            for line in block.lines() {
                if let Some(p) = line.strip_prefix("worktree ") {
                    path = Some(PathBuf::from(p));
                } else if let Some(h) = line.strip_prefix("HEAD ") {
                    head = Some(h.to_string());
                } else if let Some(b) = line.strip_prefix("branch ") {
                    branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
                }
            }
            let Some(path) = path else { continue };
            let last_commit_ms = head
                .as_deref()
                .and_then(|h| self.git(dir, &["show", "-s", "--format=%ct", h]))
                .and_then(|t| t.parse::<i64>().ok())
                .map(|s| s * 1000);
            list.push(Worktree {
                path,
                head,
                branch,
                last_commit_ms,
            });
        }
        list
    }

    /// Trimmed stdout.
    fn git(&self, cwd: &Path, args: &[&str]) -> Option<String> {
        self.run(cwd, args).map(|s| s.trim().to_string())
    }

    /// Raw stdout of a successful run; `None` on missing dir, spawn failure,
    /// timeout (child and its process group killed) or non-zero exit.
    /// Always returns within about `self.timeout`, even when a descendant
    /// (hook, credential helper, auto-gc...) keeps stdout open.
    fn run(&self, cwd: &Path, args: &[&str]) -> Option<String> {
        if !cwd.is_dir() {
            return None;
        }
        let mut cmd = Command::new(&self.bin);
        cmd.current_dir(cwd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        isolate_env(&mut cmd);
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().ok()?;
        let Some(mut stdout) = child.stdout.take() else {
            kill_tree(&mut child);
            return None;
        };
        let started = Instant::now();
        // Drain concurrently so a large output cannot block the child; the
        // reader is detached on timeout (it ends once the group is killed).
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        let status = match child.wait_timeout(self.timeout) {
            Ok(Some(status)) => status,
            _ => {
                kill_tree(&mut child);
                return None;
            }
        };
        let remaining = self.timeout.saturating_sub(started.elapsed());
        match rx.recv_timeout(remaining) {
            Ok(buf) => status
                .success()
                .then(|| String::from_utf8_lossy(&buf).into_owned()),
            Err(_) => {
                kill_group(&child);
                None
            }
        }
    }
}

/// Kills the child's whole process group (Unix), then the child, and reaps it.
fn kill_tree(child: &mut Child) {
    kill_group(child);
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_group(child: &Child) {
    // The child leads its own group (process_group(0)), so -pid targets it and
    // its descendants. argv only, no shell.
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{}", child.id())])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(unix))]
fn kill_group(_child: &Child) {}

fn sha1_hex(s: &str) -> String {
    sha1_smol::Sha1::from(s).digest().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    const DAY_MS: i64 = 86_400_000;

    fn now_ms() -> i64 {
        chrono::Utc::now().timestamp_millis()
    }

    /// Runs git for test setup, independent of the developer's git config.
    fn git_run(dir: &Path, args: &[&str], date: Option<&str>) -> String {
        let mut cmd = Command::new("git");
        isolate_env(&mut cmd);
        cmd.current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "T")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "T")
            .env("GIT_COMMITTER_EMAIL", "t@example.com");
        if let Some(d) = date {
            cmd.env("GIT_AUTHOR_DATE", d).env("GIT_COMMITTER_DATE", d);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Creates a repo on `main` with one commit (a.txt).
    fn make_repo(dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        git_run(dir, &["init", "-q", "-b", "main"], None);
        fs::write(dir.join("a.txt"), "a").unwrap();
        git_run(dir, &["add", "."], None);
        git_run(dir, &["commit", "-q", "-m", "init"], None);
        fs::canonicalize(dir).unwrap()
    }

    fn iso_days_ago(days: i64) -> String {
        let ts = (now_ms() - days * DAY_MS) / 1000;
        format!("{ts} +0000")
    }

    /// Título original: snapshot de diretório sem git
    #[test]
    fn snapshot_of_directory_without_git() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("plain")).unwrap();
        let snap = Git::default().snapshot(&tmp.path().join("plain"));
        assert_eq!(snap, GitSnapshot::default());
        assert!(snap.repo_root.is_none() && snap.branch.is_none());
    }

    /// Título original: snapshot de worktree agrupa no repositório principal
    #[test]
    fn worktree_snapshot_groups_under_main_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("main-repo"));
        git_run(
            &repo,
            &["worktree", "add", "-q", "../wt-x", "-b", "feat/x"],
            None,
        );
        let git = Git::default();

        let snap = git.snapshot(&tmp.path().join("wt-x"));
        assert_eq!(snap.repo_root.as_deref(), Some(repo.as_path()));
        assert!(snap.toplevel.as_ref().unwrap().ends_with("wt-x"));
        assert_ne!(snap.toplevel, snap.repo_root);
        assert_eq!(snap.branch.as_deref(), Some("feat/x"));

        let main = git.snapshot(&repo);
        assert_eq!(main.toplevel, main.repo_root);
        assert_eq!(main.branch.as_deref(), Some("main"));
        assert!(main.head.is_some());

        let list = git.worktrees(&repo);
        assert_eq!(list.len(), 2);
        let branches: Vec<_> = list.iter().filter_map(|w| w.branch.as_deref()).collect();
        assert!(branches.contains(&"feat/x") && branches.contains(&"main"));
    }

    /// Título original: statusHash muda quando um arquivo é alterado
    #[test]
    fn status_hash_changes_when_a_file_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("r"));
        let git = Git::default();
        let before = git.snapshot(&repo).status_hash;
        assert!(before.is_some());
        assert_eq!(git.status_porcelain(&repo).as_deref(), Some(""));

        fs::write(repo.join("a.txt"), "changed").unwrap();

        assert_ne!(git.snapshot(&repo).status_hash, before);
        assert_eq!(git.status_porcelain(&repo).as_deref(), Some(" M a.txt\n"));
    }

    /// Título original: funciona com caminho com espaço e acento
    #[test]
    fn works_with_path_containing_space_and_accent() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("Grupo Sentia/Condomínios/Bay side"));
        git_run(
            &repo,
            &["remote", "add", "origin", "git@example.com:x/y.git"],
            None,
        );

        let snap = Git::default().snapshot(&repo);
        assert_eq!(snap.repo_root.as_deref(), Some(repo.as_path()));
        assert_eq!(snap.remote_url.as_deref(), Some("git@example.com:x/y.git"));
    }

    /// Título original: remoteUrl cai para o primeiro remote quando não há origin
    #[test]
    fn remote_url_falls_back_to_first_remote_without_origin() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("r2"));
        git_run(
            &repo,
            &["remote", "add", "upstream", "https://example.com/u.git"],
            None,
        );
        let snap = Git::default().snapshot(&repo);
        assert_eq!(
            snap.remote_url.as_deref(),
            Some("https://example.com/u.git")
        );
    }

    /// Título original: cwd inexistente não lança exceção
    #[test]
    fn nonexistent_cwd_yields_empty_snapshot() {
        let tmp = tempfile::tempdir().unwrap();
        let snap = Git::default().snapshot(&tmp.path().join("nao/existe"));
        assert_eq!(snap, GitSnapshot::default());
    }

    /// Título original: staleBranches lista a branch com 2 commits à frente de develop e último commit há 5 dias
    #[test]
    fn stale_branches_lists_branch_two_ahead_with_last_commit_five_days_ago() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("stale"));
        git_run(&repo, &["branch", "develop"], None);
        git_run(&repo, &["checkout", "-q", "-b", "feat/old"], None);
        let date = iso_days_ago(5);
        for f in ["b", "c"] {
            fs::write(repo.join(format!("{f}.txt")), f).unwrap();
            git_run(&repo, &["add", "."], None);
            git_run(&repo, &["commit", "-q", "-m", f], Some(&date));
        }
        git_run(&repo, &["checkout", "-q", "develop"], None);

        let git = Git::default();
        let now = now_ms();
        let found = git.stale_branches(&repo, 3, now);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].branch, "feat/old");
        assert_eq!(found[0].ahead, 2);
        let age_days = (now - found[0].last_commit_ms) / DAY_MS;
        assert!((4..=6).contains(&age_days), "age {age_days}");
        assert!(git.stale_branches(&repo, 10, now).is_empty());
    }

    /// Título original: worktrees traz a data do último commit de cada uma
    #[test]
    fn worktrees_report_last_commit_date_of_each() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("repo"));
        let at = (now_ms() - 6 * DAY_MS) / 1000;
        git_run(
            &repo,
            &["commit", "-q", "--allow-empty", "-m", "velho"],
            Some(&format!("{at} +0000")),
        );
        let wt = tmp.path().join("wt");
        git_run(
            &repo,
            &["worktree", "add", "-q", "-b", "wt", wt.to_str().unwrap()],
            None,
        );

        let list = Git::default().worktrees(&repo);
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].last_commit_ms, Some(at * 1000));
    }

    /// Título original: ignora GIT_DIR, GIT_WORK_TREE, GIT_INDEX_FILE e GIT_COMMON_DIR herdados do ambiente
    ///
    /// Deterministic without mutating the (shared, parallel) process env: the
    /// isolation is a single function (`isolate_env`) applied by every git call;
    /// here it is applied to a command whose env was polluted with all 8 variables.
    #[test]
    fn ignores_redirecting_git_env_vars() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = make_repo(&tmp.path().join("alvo"));
        let other = make_repo(&tmp.path().join("outro"));
        git_run(&other, &["checkout", "-q", "-b", "outra-branch"], None);

        let polluted = |cmd: &mut Command| {
            cmd.env("GIT_DIR", other.join(".git"))
                .env("GIT_WORK_TREE", &other)
                .env("GIT_INDEX_FILE", other.join(".git/index"))
                .env("GIT_COMMON_DIR", other.join(".git"))
                .env("GIT_OBJECT_DIRECTORY", other.join(".git/objects"))
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &other)
                .env("GIT_NAMESPACE", "ns")
                .env("GIT_CEILING_DIRECTORIES", &other);
        };
        let branch_of = |isolate: bool| {
            let mut cmd = Command::new("git");
            cmd.current_dir(&repo)
                .args(["symbolic-ref", "--short", "-q", "HEAD"]);
            polluted(&mut cmd);
            if isolate {
                isolate_env(&mut cmd);
            }
            String::from_utf8_lossy(&cmd.output().unwrap().stdout)
                .trim()
                .to_string()
        };
        // Control: without isolation the pollution really redirects git.
        assert_eq!(branch_of(false), "outra-branch");
        assert_eq!(branch_of(true), "main");

        // And no variable survives in the child's environment.
        let mut env_cmd = Command::new("env");
        polluted(&mut env_cmd);
        isolate_env(&mut env_cmd);
        let out = String::from_utf8_lossy(&env_cmd.output().unwrap().stdout).to_string();
        let leaked: Vec<_> = out
            .lines()
            .filter(|l| ISOLATED_ENV.iter().any(|n| l.starts_with(&format!("{n}="))))
            .collect();
        assert!(leaked.is_empty(), "{leaked:?}");
    }

    /// A git whose descendant keeps stdout open must not defeat the timeout.
    #[cfg(unix)]
    #[test]
    fn timeout_is_enforced_even_when_a_descendant_holds_stdout() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("fake-git");
        fs::write(&bin, "#!/bin/sh\nsleep 30 &\nsleep 30\n").unwrap();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let git = Git {
            timeout: Duration::from_millis(300),
            bin,
        };

        let started = Instant::now();
        assert_eq!(git.status_porcelain(tmp.path()), None);
        assert_eq!(git.snapshot(tmp.path()), GitSnapshot::default());
        assert!(
            started.elapsed() < Duration::from_millis(300 * 2 + 1500),
            "took {:?}",
            started.elapsed()
        );
    }
}
