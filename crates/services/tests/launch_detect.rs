//! Detection of editors, terminals and shells, with an in-memory probe for every OS.

use std::ffi::OsString;

use aiannotations_services::launch::catalog::Family;
use aiannotations_services::launch::detect::{
    DetectedApp, default_shell, detect_apps, detect_shells, which,
};
use aiannotations_services::launch::testing::FakeProbe;
use aiannotations_services::launch::{Os, ShellKind};

fn argv(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn app<'a>(apps: &'a [DetectedApp], id: &str) -> Option<&'a DetectedApp> {
    apps.iter().find(|a| a.id == id)
}

#[test]
fn linux_finds_path_and_desktop() {
    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("PATH", "/usr/local/bin:/usr/bin")
        .file("/usr/bin/code", "")
        .file(
            "/usr/share/applications/cursor.desktop",
            "[Desktop Entry]\nName=Cursor\nExec=/opt/cursor/cursor %F\n",
        );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "vscode").unwrap().program,
        argv(&["/usr/bin/code"])
    );
    assert_eq!(
        app(&apps, "cursor").unwrap().program,
        argv(&["/opt/cursor/cursor"])
    );
    assert!(app(&apps, "zed").is_none());
}

#[test]
fn linux_flatpak_exec_keeps_prefix() {
    let p = FakeProbe::new(Os::Linux).env("HOME", "/home/u").file(
        "/var/lib/flatpak/exports/share/applications/com.visualstudio.code.desktop",
        "[Desktop Entry]\nExec=/usr/bin/flatpak run --branch=stable com.visualstudio.code --new-window %F\n",
    );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "vscode").unwrap().program,
        argv(&[
            "/usr/bin/flatpak",
            "run",
            "--branch=stable",
            "com.visualstudio.code",
            "--new-window"
        ])
    );
}

#[test]
fn desktop_exec_respects_quotes() {
    let p = FakeProbe::new(Os::Linux).env("HOME", "/home/u").file(
        "/usr/share/applications/cursor.desktop",
        "[Desktop Entry]\nExec=\"/opt/My App/app\" %U\n\n[Desktop Action new]\nExec=other\n",
    );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "cursor").unwrap().program,
        argv(&["/opt/My App/app"])
    );
}

#[test]
fn desktop_dirs_follow_xdg_variables() {
    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("XDG_DATA_HOME", "/data/home")
        .env("XDG_DATA_DIRS", "/a/share:/b/share")
        .file(
            "/b/share/applications/zed.desktop",
            "[Desktop Entry]\nExec=/b/zed %U\n",
        )
        .file(
            "/data/home/applications/windsurf.desktop",
            "[Desktop Entry]\nExec=/w/windsurf\n",
        );
    let apps = detect_apps(&p);
    assert_eq!(app(&apps, "zed").unwrap().program, argv(&["/b/zed"]));
    assert_eq!(
        app(&apps, "windsurf").unwrap().program,
        argv(&["/w/windsurf"])
    );
}

#[test]
fn linux_jetbrains_toolbox_script() {
    let p = FakeProbe::new(Os::Linux).env("HOME", "/home/u").file(
        "/home/u/.local/share/JetBrains/Toolbox/scripts/phpstorm",
        "",
    );
    let apps = detect_apps(&p);
    let a = app(&apps, "phpstorm").unwrap();
    assert_eq!(a.family, Family::JetBrains);
    assert_eq!(
        a.program,
        argv(&["/home/u/.local/share/JetBrains/Toolbox/scripts/phpstorm"])
    );
}

#[test]
fn x_terminal_emulator_takes_target_family() {
    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("PATH", "/usr/bin")
        .file("/usr/bin/ghostty", "")
        .link(
            "/usr/bin/x-terminal-emulator",
            "/etc/alternatives/x-terminal-emulator",
        )
        .link("/etc/alternatives/x-terminal-emulator", "/usr/bin/ghostty");
    let apps = detect_apps(&p);
    let a = app(&apps, "x-terminal-emulator").unwrap();
    assert_eq!(a.family, Family::Ghostty);
    assert_eq!(a.program, argv(&["/usr/bin/x-terminal-emulator"]));

    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("PATH", "/usr/bin")
        .file("/usr/bin/foot", "")
        .link("/usr/bin/x-terminal-emulator", "/usr/bin/foot");
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "x-terminal-emulator").unwrap().family,
        Family::Generic
    );
}

#[test]
fn linux_does_not_offer_other_platform_terminals() {
    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("PATH", "/usr/bin")
        .file("/usr/bin/cmd", "");
    let apps = detect_apps(&p);
    for id in [
        "cmd",
        "windows-terminal",
        "powershell",
        "terminal-app",
        "iterm2",
    ] {
        assert!(app(&apps, id).is_none(), "{id}");
    }
}

#[test]
fn macos_bundles() {
    let p = FakeProbe::new(Os::MacOs)
        .env("HOME", "/Users/u")
        .dir("/Applications/Visual Studio Code.app")
        .dir("/Applications/kitty.app")
        .dir("/Users/u/Applications/Ghostty.app")
        .dir("/System/Applications/Utilities/Terminal.app")
        .dir("/Applications/PyCharm CE.app");
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "vscode").unwrap().program,
        argv(&["open", "-a", "/Applications/Visual Studio Code.app"])
    );
    assert_eq!(
        app(&apps, "kitty").unwrap().program,
        argv(&["/Applications/kitty.app/Contents/MacOS/kitty"])
    );
    assert_eq!(
        app(&apps, "ghostty").unwrap().program,
        argv(&["open", "-na", "/Users/u/Applications/Ghostty.app", "--args"])
    );
    assert_eq!(
        app(&apps, "terminal-app").unwrap().program,
        argv(&["open", "-a", "/System/Applications/Utilities/Terminal.app"])
    );
    assert_eq!(
        app(&apps, "pycharm").unwrap().program,
        argv(&["open", "-a", "/Applications/PyCharm CE.app"])
    );
    assert!(app(&apps, "wezterm").is_none());
}

#[test]
fn windows_localappdata_and_pathext() {
    let p = FakeProbe::new(Os::Windows)
        .env("LOCALAPPDATA", r"C:\Users\u\AppData\Local")
        .env("PATH", r"C:\Windows\System32;C:\bin")
        .env("PATHEXT", ".COM;.EXE")
        .file(
            r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe",
            "",
        )
        .file(r"C:\bin\wt.exe", "");
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "vscode").unwrap().program,
        argv(&[r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe"])
    );
    assert_eq!(
        app(&apps, "windows-terminal").unwrap().program,
        argv(&[r"C:\bin\wt.exe"])
    );
    assert_eq!(
        which(&p, "wt").unwrap(),
        std::path::PathBuf::from(r"C:\bin\wt.exe")
    );
    assert!(which(&p, "nothing").is_none());
}

#[test]
fn windows_path_ignores_extensionless_scripts() {
    let p = FakeProbe::new(Os::Windows)
        .env("PATH", r"C:\vs\bin")
        .env("PATHEXT", ".COM;.EXE;.CMD")
        .file(r"C:\vs\bin\code", "#!/bin/sh")
        .file(r"C:\vs\bin\code.cmd", "");
    assert_eq!(
        which(&p, "code").unwrap(),
        std::path::PathBuf::from(r"C:\vs\bin\code.cmd")
    );
}

#[test]
fn windows_registry_app_paths() {
    let p = FakeProbe::new(Os::Windows)
        .env("PATH", r"C:\Windows")
        .app_path("sublime_text.exe", r"D:\Tools\Sublime\sublime_text.exe");
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "sublime").unwrap().program,
        argv(&[r"D:\Tools\Sublime\sublime_text.exe"])
    );
}

#[test]
fn windows_program_files_and_toolbox() {
    let p = FakeProbe::new(Os::Windows)
        .env("ProgramFiles", r"C:\Program Files")
        .env("LOCALAPPDATA", r"C:\L")
        .file(r"C:\Program Files\Sublime Text\sublime_text.exe", "")
        .file(r"C:\L\JetBrains\Toolbox\scripts\rider.cmd", "");
    let apps = detect_apps(&p);
    assert!(app(&apps, "sublime").is_some());
    assert_eq!(
        app(&apps, "rider").unwrap().program,
        argv(&[r"C:\L\JetBrains\Toolbox\scripts\rider.cmd"])
    );
}

#[test]
fn files_always_present_and_no_duplicates() {
    let p = FakeProbe::new(Os::Linux)
        .env("HOME", "/home/u")
        .env("PATH", "/usr/bin")
        .file("/usr/bin/code", "")
        .file(
            "/usr/share/applications/code.desktop",
            "[Desktop Entry]\nExec=/other/code %F\n",
        );
    let apps = detect_apps(&p);
    let codes: Vec<_> = apps.iter().filter(|a| a.id == "vscode").collect();
    assert_eq!(codes.len(), 1);
    assert_eq!(codes[0].program, argv(&["/usr/bin/code"]));
    let files = app(&apps, "files").unwrap();
    assert_eq!(files.family, Family::FileManager);
    assert_eq!(files.program, argv(&["xdg-open"]));

    let mut ids: Vec<_> = apps.iter().map(|a| a.id.clone()).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n);

    for (os, prog) in [(Os::MacOs, "open"), (Os::Windows, "explorer")] {
        let apps = detect_apps(&FakeProbe::new(os));
        assert_eq!(app(&apps, "files").unwrap().program, argv(&[prog]));
    }
}

#[test]
fn shells_unix_and_default() {
    let p = FakeProbe::new(Os::Linux)
        .env("SHELL", "/usr/bin/zsh")
        .file(
            "/etc/shells",
            "# comment\n/bin/bash\n/usr/bin/fish\n/bin/ksh\n/usr/sbin/nologin\n",
        )
        .file("/bin/bash", "")
        .file("/usr/bin/fish", "")
        .file("/usr/sbin/nologin", "");
    let shells = detect_shells(&p);
    let got: Vec<_> = shells
        .iter()
        .map(|s| (s.path.to_str().unwrap().to_string(), s.kind))
        .collect();
    assert_eq!(
        got,
        vec![
            ("/bin/bash".to_string(), ShellKind::Posix),
            ("/usr/bin/fish".to_string(), ShellKind::Fish),
            ("/usr/bin/zsh".to_string(), ShellKind::Posix),
        ]
    );
    assert_eq!(
        default_shell(&p).path,
        std::path::PathBuf::from("/usr/bin/zsh")
    );

    let p = FakeProbe::new(Os::Linux)
        .file("/etc/shells", "/bin/bash\n")
        .file("/bin/bash", "");
    let d = default_shell(&p);
    assert_eq!(d.path, std::path::PathBuf::from("/bin/sh"));
    assert_eq!(d.kind, ShellKind::Posix);
}

#[test]
fn shells_windows_default_prefers_pwsh() {
    let p = FakeProbe::new(Os::Windows)
        .env("PATH", r"C:\ps7;C:\Windows\System32\WindowsPowerShell\v1.0")
        .env("PATHEXT", ".EXE")
        .env("ComSpec", r"C:\Windows\System32\cmd.exe")
        .env("ProgramFiles", r"C:\Program Files")
        .file(r"C:\ps7\pwsh.exe", "")
        .file(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "",
        )
        .file(r"C:\Windows\System32\cmd.exe", "")
        .file(r"C:\Program Files\Git\bin\bash.exe", "");
    let shells = detect_shells(&p);
    let kinds: Vec<_> = shells.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        vec![
            ShellKind::PowerShell,
            ShellKind::PowerShell,
            ShellKind::Cmd,
            ShellKind::Posix
        ]
    );
    assert_eq!(
        default_shell(&p).path,
        std::path::PathBuf::from(r"C:\ps7\pwsh.exe")
    );

    let p = FakeProbe::new(Os::Windows)
        .env("PATH", r"C:\w")
        .env("PATHEXT", ".EXE")
        .file(r"C:\w\powershell.exe", "");
    assert_eq!(
        default_shell(&p).path,
        std::path::PathBuf::from(r"C:\w\powershell.exe")
    );
}

#[test]
fn desktop_exec_ignores_other_sections_before_entry() {
    let p = FakeProbe::new(Os::Linux).env("HOME", "/home/u").file(
        "/usr/share/applications/cursor.desktop",
        "[Desktop Action new]\nExec=wrong --new\n\n[Desktop Entry]\nName=Cursor\nExec=/right/cursor %F\n",
    );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "cursor").unwrap().program,
        argv(&["/right/cursor"])
    );
}

#[test]
fn flatpak_file_forwarding_markers_are_dropped() {
    let p = FakeProbe::new(Os::Linux).env("HOME", "/home/u").file(
        "/var/lib/flatpak/exports/share/applications/com.visualstudio.code.desktop",
        "[Desktop Entry]\nExec=/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=code --file-forwarding com.visualstudio.code --new-window @@ %F @@\n",
    );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "vscode").unwrap().program,
        argv(&[
            "/usr/bin/flatpak",
            "run",
            "--branch=stable",
            "--arch=x86_64",
            "--command=code",
            "com.visualstudio.code",
            "--new-window"
        ])
    );
}

#[test]
fn jetbrains_desktop_fallback_works_without_home() {
    let p = FakeProbe::new(Os::Linux).file(
        "/usr/share/applications/jetbrains-phpstorm.desktop",
        "[Desktop Entry]\nExec=/opt/phpstorm/bin/phpstorm.sh %f\n",
    );
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "phpstorm").unwrap().program,
        argv(&["/opt/phpstorm/bin/phpstorm.sh"])
    );
}

#[test]
fn windows_terminal_prefers_installed_pwsh_over_system_powershell() {
    let p = FakeProbe::new(Os::Windows)
        .env("PATH", r"C:\Windows\System32\WindowsPowerShell\v1.0")
        .env("PATHEXT", ".EXE")
        .env("ProgramFiles", r"C:\Program Files")
        .env("SystemRoot", r"C:\Windows")
        .file(
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "",
        )
        .file(r"C:\Program Files\PowerShell\7\pwsh.exe", "");
    let apps = detect_apps(&p);
    assert_eq!(
        app(&apps, "powershell").unwrap().program,
        argv(&[r"C:\Program Files\PowerShell\7\pwsh.exe"])
    );
}
