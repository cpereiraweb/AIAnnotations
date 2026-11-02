//! Finds installed editors, terminals and shells. All filesystem and environment access goes
//! through `Probe`, and the OS is a value, so every platform's rules run in tests on any host.
//!
//! Paths of the probed OS are built as strings with that OS's separator and only then turned
//! into `PathBuf`: `Path::join` would use the host's separator.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::catalog::{Family, KNOWN, Known};
use super::shell::kind_of;
use super::{AppKind, Os, Shell};

/// Everything detection needs from the machine.
pub trait Probe {
    fn os(&self) -> Os;
    fn env(&self, key: &str) -> Option<OsString>;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// Full paths of the entries of a directory (empty when unreadable).
    fn read_dir(&self, path: &Path) -> Vec<PathBuf>;
    fn read_to_string(&self, path: &Path) -> Option<String>;
    fn canonicalize(&self, path: &Path) -> Option<PathBuf>;
    /// Registry `App Paths` lookup; `None` outside Windows.
    fn app_path(&self, exe: &str) -> Option<PathBuf>;
}

/// The real machine.
pub struct SystemProbe;

impl Probe for SystemProbe {
    fn os(&self) -> Os {
        Os::current()
    }

    fn env(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_dir(&self, path: &Path) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = std::fs::read_dir(path)
            .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default();
        out.sort();
        out
    }

    fn read_to_string(&self, path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    fn canonicalize(&self, path: &Path) -> Option<PathBuf> {
        std::fs::canonicalize(path).ok()
    }

    #[cfg(windows)]
    fn app_path(&self, exe: &str) -> Option<PathBuf> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

        let sub = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\{exe}");
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            if let Ok(key) = RegKey::predef(hive).open_subkey(&sub) {
                if let Ok(value) = key.get_value::<String, _>("") {
                    let value = value.trim().trim_matches('"');
                    if !value.is_empty() {
                        return Some(PathBuf::from(value));
                    }
                }
            }
        }
        None
    }

    #[cfg(not(windows))]
    fn app_path(&self, _exe: &str) -> Option<PathBuf> {
        None
    }
}

/// An app found on this machine. `program` is the argv prefix that starts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedApp {
    pub id: String,
    pub label: String,
    pub kind: AppKind,
    pub family: Family,
    pub program: Vec<OsString>,
}

// ---- path helpers --------------------------------------------------------------------------

fn sep(os: Os) -> char {
    if os == Os::Windows { '\\' } else { '/' }
}

fn join(os: Os, base: &str, name: &str) -> String {
    format!("{}{}{}", base.trim_end_matches(['/', '\\']), sep(os), name)
}

fn env_string(p: &dyn Probe, key: &str) -> Option<String> {
    p.env(key)
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
}

fn home(p: &dyn Probe) -> Option<String> {
    env_string(p, "HOME")
}

/// Replaces every `%VAR%`; `None` when a variable is not set.
fn expand_env(p: &dyn Probe, template: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('%')?;
        out.push_str(&env_string(p, &after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn split_path_var(os: Os, value: &str) -> Vec<String> {
    let delimiter = if os == Os::Windows { ';' } else { ':' };
    value
        .split(delimiter)
        .map(|d| d.trim().trim_matches('"'))
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .collect()
}

/// File names to try for `name` in a `PATH` directory.
fn path_candidates(p: &dyn Probe, name: &str) -> Vec<String> {
    if p.os() != Os::Windows {
        return vec![name.to_string()];
    }
    let pathext = env_string(p, "PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
    let exts: Vec<&str> = pathext.split(';').filter(|e| e.starts_with('.')).collect();
    let lower = name.to_lowercase();
    // A bare name (like VS Code's `code` shell script) is not runnable on Windows.
    if exts.iter().any(|e| lower.ends_with(&e.to_lowercase())) {
        return vec![name.to_string()];
    }
    let mut out = Vec::new();
    for ext in exts {
        for cand in [
            format!("{name}{ext}"),
            format!("{name}{}", ext.to_lowercase()),
        ] {
            if !out.contains(&cand) {
                out.push(cand);
            }
        }
    }
    out
}

/// First runnable `name` in `PATH`.
pub fn which(p: &dyn Probe, name: &str) -> Option<PathBuf> {
    let os = p.os();
    let path_var = env_string(p, "PATH")?;
    let names = path_candidates(p, name);
    for dir in split_path_var(os, &path_var) {
        for n in &names {
            let candidate = PathBuf::from(join(os, &dir, n));
            if p.is_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn os_str(s: impl AsRef<std::ffi::OsStr>) -> OsString {
    s.as_ref().to_os_string()
}

// ---- apps ----------------------------------------------------------------------------------

/// Whether the family exists on the OS at all (so a `cmd` in a Linux PATH is not offered).
fn available_on(family: Family, os: Os) -> bool {
    match family {
        Family::WindowsTerminal
        | Family::PowerShellConsole
        | Family::CmdConsole
        | Family::Cmder => os == Os::Windows,
        Family::TerminalApp | Family::ITerm2 => os == Os::MacOs,
        Family::Ptyxis | Family::GnomeTerminal | Family::Konsole | Family::Generic => {
            os == Os::Linux
        }
        _ => true,
    }
}

/// Every catalog app found on this machine, in catalog order, one entry per id.
pub fn detect_apps(p: &dyn Probe) -> Vec<DetectedApp> {
    let os = p.os();
    let mut out = Vec::new();
    for known in KNOWN {
        if !available_on(known.family, os) {
            continue;
        }
        let Some(program) = find_program(p, known) else {
            continue;
        };
        let family = if known.id == "x-terminal-emulator" {
            generic_terminal_family(p, &program)
        } else {
            known.family
        };
        out.push(DetectedApp {
            id: known.id.to_string(),
            label: known.label.to_string(),
            kind: known.kind,
            family,
            program,
        });
    }
    out
}

fn find_program(p: &dyn Probe, k: &Known) -> Option<Vec<OsString>> {
    if k.family == Family::FileManager {
        let opener = match p.os() {
            Os::Linux => "xdg-open",
            Os::MacOs => "open",
            Os::Windows => "explorer",
        };
        return Some(vec![os_str(opener)]);
    }
    match p.os() {
        Os::Linux => linux_program(p, k),
        Os::MacOs => mac_program(p, k),
        Os::Windows => windows_program(p, k),
    }
}

fn from_path(p: &dyn Probe, k: &Known) -> Option<PathBuf> {
    k.path_names.iter().find_map(|name| which(p, name))
}

// -- Linux

fn linux_program(p: &dyn Probe, k: &Known) -> Option<Vec<OsString>> {
    if let Some(found) = from_path(p, k) {
        return Some(vec![found.into_os_string()]);
    }
    if k.family == Family::JetBrains {
        let script = join(
            Os::Linux,
            &home(p)?,
            ".local/share/JetBrains/Toolbox/scripts",
        );
        let script = PathBuf::from(join(Os::Linux, &script, k.path_names.first()?));
        if p.is_file(&script) {
            return Some(vec![script.into_os_string()]);
        }
    }
    let dirs = desktop_dirs(p);
    for dir in &dirs {
        for id in k.desktop_ids {
            let file = PathBuf::from(join(Os::Linux, dir, id));
            if let Some(exec) = p.read_to_string(&file).and_then(|c| desktop_exec(&c)) {
                return Some(exec);
            }
        }
    }
    None
}

fn desktop_dirs(p: &dyn Probe) -> Vec<String> {
    let home = home(p);
    let mut bases = Vec::new();
    match env_string(p, "XDG_DATA_HOME") {
        Some(d) => bases.push(d),
        None => bases.extend(home.as_ref().map(|h| join(Os::Linux, h, ".local/share"))),
    }
    let data_dirs =
        env_string(p, "XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    bases.extend(split_path_var(Os::Linux, &data_dirs));
    bases.push("/var/lib/flatpak/exports/share".into());
    bases.extend(home.map(|h| join(Os::Linux, &h, ".local/share/flatpak/exports/share")));
    bases.push("/var/lib/snapd/desktop".into());

    let mut out: Vec<String> = Vec::new();
    for base in bases {
        let dir = join(Os::Linux, &base, "applications");
        if !out.contains(&dir) {
            out.push(dir);
        }
    }
    out
}

/// The `Exec=` of the `[Desktop Entry]` section as an argv, without field codes.
fn desktop_exec(content: &str) -> Option<Vec<OsString>> {
    let mut in_entry = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry && let Some(exec) = line.strip_prefix("Exec=") {
            let argv: Vec<OsString> = split_exec(exec)
                .into_iter()
                .filter(|t| !is_field_code(t))
                .map(OsString::from)
                .collect();
            return (!argv.is_empty()).then_some(argv);
        }
    }
    None
}

fn is_field_code(token: &str) -> bool {
    matches!(
        token,
        "%f" | "%F" | "%u" | "%U" | "%i" | "%c" | "%k" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m"
    )
}

/// Splits an `Exec` value on whitespace, honoring double quotes and backslash escapes in them.
fn split_exec(exec: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if quoted => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    tokens.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        tokens.push(cur);
    }
    tokens
}

/// Family of the system default terminal: the catalog terminal its symlink points to, else generic.
fn generic_terminal_family(p: &dyn Probe, program: &[OsString]) -> Family {
    let Some(first) = program.first() else {
        return Family::Generic;
    };
    let Some(target) = p.canonicalize(Path::new(first)) else {
        return Family::Generic;
    };
    let raw = target.to_string_lossy();
    let file = raw.rsplit(['/', '\\']).next().unwrap_or("");
    KNOWN
        .iter()
        .filter(|k| k.kind == AppKind::Terminal && k.family != Family::Generic)
        .find(|k| k.path_names.contains(&file))
        .map_or(Family::Generic, |k| k.family)
}

// -- macOS

fn mac_dirs(p: &dyn Probe) -> Vec<String> {
    let mut dirs = vec!["/Applications".to_string()];
    dirs.extend(home(p).map(|h| join(Os::MacOs, &h, "Applications")));
    dirs.push("/System/Applications/Utilities".into());
    dirs.push("/Applications/Utilities".into());
    dirs
}

fn mac_program(p: &dyn Probe, k: &Known) -> Option<Vec<OsString>> {
    let dirs = mac_dirs(p);
    for bundle in k.mac_bundles {
        for dir in &dirs {
            let path = join(Os::MacOs, dir, bundle);
            if p.is_dir(Path::new(&path)) {
                return Some(mac_argv(k.family, &path, bundle));
            }
        }
    }
    if k.family == Family::JetBrains {
        // Versioned or edition-suffixed bundles ("PyCharm CE.app", "WebStorm 2025.1.app").
        for bundle in k.mac_bundles {
            let stem = bundle.trim_end_matches(".app").to_lowercase();
            for dir in &dirs {
                for entry in p.read_dir(Path::new(dir)) {
                    let name = entry
                        .to_string_lossy()
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if name.to_lowercase().starts_with(&stem)
                        && name.ends_with(".app")
                        && p.is_dir(&entry)
                    {
                        let path = entry.to_string_lossy().into_owned();
                        return Some(mac_argv(k.family, &path, &name));
                    }
                }
            }
        }
    }
    None
}

fn mac_argv(family: Family, bundle_path: &str, bundle_name: &str) -> Vec<OsString> {
    match family {
        // These run their own binary so that arguments reach them.
        Family::Kitty | Family::WezTerm | Family::Alacritty => {
            let bin = bundle_name.trim_end_matches(".app").to_lowercase();
            vec![os_str(format!("{bundle_path}/Contents/MacOS/{bin}"))]
        }
        Family::Ghostty => ["open", "-na", bundle_path, "--args"].map(os_str).to_vec(),
        _ => ["open", "-a", bundle_path].map(os_str).to_vec(),
    }
}

// -- Windows

fn windows_program(p: &dyn Probe, k: &Known) -> Option<Vec<OsString>> {
    if let Some(found) = from_path(p, k) {
        return Some(vec![found.into_os_string()]);
    }
    for template in k.win_paths {
        if let Some(path) = expand_env(p, template) {
            let path = PathBuf::from(path);
            if p.is_file(&path) {
                return Some(vec![path.into_os_string()]);
            }
        }
    }
    k.win_app_paths
        .iter()
        .find_map(|exe| p.app_path(exe))
        .map(|path| vec![path.into_os_string()])
}

// ---- shells --------------------------------------------------------------------------------

fn shell_of(path: PathBuf) -> Shell {
    let kind = kind_of(&path);
    Shell { path, kind }
}

fn push_unique(shells: &mut Vec<Shell>, path: PathBuf) {
    if !shells.iter().any(|s| s.path == path) {
        shells.push(shell_of(path));
    }
}

/// Shells installed on the machine, for the shell picker.
pub fn detect_shells(p: &dyn Probe) -> Vec<Shell> {
    let mut shells = Vec::new();
    if p.os() == Os::Windows {
        windows_shells(p, &mut shells);
        return shells;
    }
    if let Some(listing) = p.read_to_string(Path::new("/etc/shells")) {
        for line in listing.lines().map(str::trim) {
            let path = Path::new(line);
            let name = line.rsplit('/').next().unwrap_or("");
            // `nologin` and `false` are listed on some systems but are not usable shells.
            if line.starts_with('/') && !matches!(name, "nologin" | "false") && p.is_file(path) {
                push_unique(&mut shells, path.to_path_buf());
            }
        }
    }
    if let Some(shell) = env_string(p, "SHELL") {
        push_unique(&mut shells, PathBuf::from(shell));
    }
    shells
}

fn windows_shells(p: &dyn Probe, shells: &mut Vec<Shell>) {
    let existing = |template: &str| {
        expand_env(p, template)
            .map(PathBuf::from)
            .filter(|path| p.is_file(path))
    };
    let pwsh = which(p, "pwsh").or_else(|| existing(r"%ProgramFiles%\PowerShell\7\pwsh.exe"));
    let powershell = which(p, "powershell")
        .or_else(|| existing(r"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"));
    let cmd = env_string(p, "ComSpec")
        .map(PathBuf::from)
        .filter(|path| p.is_file(path))
        .or_else(|| which(p, "cmd"));
    let git_bash = existing(r"%ProgramFiles%\Git\bin\bash.exe");
    for path in [pwsh, powershell, cmd, git_bash].into_iter().flatten() {
        push_unique(shells, path);
    }
}

/// The shell used when none is configured: `$SHELL` (else `/bin/sh`), or `pwsh` then
/// Windows PowerShell on Windows.
pub fn default_shell(p: &dyn Probe) -> Shell {
    if p.os() == Os::Windows {
        let shells = detect_shells(p);
        let preferred = ["pwsh", "powershell"].iter().find_map(|stem| {
            shells.iter().find(|s| {
                let raw = s.path.to_string_lossy().to_lowercase();
                let file = raw.rsplit(['/', '\\']).next().unwrap_or("").to_string();
                file.trim_end_matches(".exe") == *stem
            })
        });
        return preferred
            .or(shells.first())
            .cloned()
            .unwrap_or_else(|| shell_of(PathBuf::from("cmd.exe")));
    }
    shell_of(PathBuf::from(
        env_string(p, "SHELL").unwrap_or_else(|| "/bin/sh".to_string()),
    ))
}
