//! First-run wizard services: CLI cards, hook writing, history counts and the final save.

use std::path::{Path, PathBuf};

use aiannotations_core::installer::hooks::HookStatus;
use aiannotations_core::installer::state;
use aiannotations_core::model::Tool;
use aiannotations_core::testing::{Env as TestEnv, RunSeed, insert_run, insert_session, now_ms};
use aiannotations_core::user_settings::UserSettings;
use aiannotations_core::{Error, Settings, user_settings};
use aiannotations_services::onboarding::{self, Choices, Env};

struct Setup {
    t: TestEnv,
    cli: PathBuf,
    path_var: std::ffi::OsString,
}

fn fake_exe(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// Home with a fake `claude` (no `codex`) and, when `with_cli`, a fake `aiannotations`.
fn setup(with_cli: bool) -> Setup {
    let t = TestEnv::new();
    // `resolve` only tries `.exe`/`.cmd`/`.bat` on Windows.
    let claude = if cfg!(windows) {
        "claude.cmd"
    } else {
        "claude"
    };
    fake_exe(&t.base.join(".local").join("bin").join(claude));
    let cli = t.base.join("app").join("aiannotations");
    if with_cli {
        fake_exe(&cli);
    }
    let path_var = t.base.join("nowhere").into_os_string();
    Setup { t, cli, path_var }
}

impl Setup {
    fn env(&self) -> Env<'_> {
        Env {
            home: &self.t.base,
            path_var: Some(self.path_var.as_os_str()),
            claude_override: None,
            codex_override: None,
            cli: &self.cli,
            windows: false,
        }
    }
    fn claude_cfg(&self) -> PathBuf {
        self.t.paths.claude_dir.join("settings.json")
    }
    fn card(&self, tool: Tool) -> onboarding::CliCard {
        let view = onboarding::load(
            &self.t.conn,
            &self.t.paths,
            &Default::default(),
            &self.env(),
        )
        .unwrap();
        view.cards.into_iter().find(|c| c.tool == tool).unwrap()
    }
}

#[test]
fn cards_show_found_cli_and_diff() {
    let s = setup(true);
    let view = onboarding::load(&s.t.conn, &s.t.paths, &Default::default(), &s.env()).unwrap();
    assert!(view.cli_found);
    assert!(!view.done);
    assert_eq!(view.cards[0].tool, Tool::Claude);
    assert_eq!(view.cards[1].tool, Tool::Codex);
    let c = &view.cards[0];
    let bin = Path::new(c.bin.as_deref().unwrap());
    assert!(
        bin.parent()
            .unwrap()
            .ends_with(Path::new(".local").join("bin"))
    );
    assert_eq!(bin.file_stem().unwrap(), "claude");
    assert_eq!(c.hook_status, Some(HookStatus::NotInstalled));
    assert!(!c.invalid_json);
    assert!(c.can_write);
    let diff = c.diff.as_deref().unwrap();
    assert!(diff.contains(&s.claude_cfg().to_string_lossy().into_owned()));
    assert!(diff.contains("SessionStart"));
    assert_eq!(view.provider, Tool::Claude);
    assert!(view.autostart);
    assert!(!view.summarize_history);
}

#[test]
fn absent_tool_card_has_no_diff_and_cannot_write() {
    let s = setup(true);
    let c = s.card(Tool::Codex);
    assert!(c.bin.is_none());
    assert!(c.diff.is_none());
    assert!(!c.can_write);
}

#[test]
fn missing_cli_disables_hook_write() {
    let s = setup(false);
    let view = onboarding::load(&s.t.conn, &s.t.paths, &Default::default(), &s.env()).unwrap();
    assert!(!view.cli_found);
    assert!(view.cards[0].diff.is_none());
    assert!(!view.cards[0].can_write);
    let err =
        onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap_err();
    assert!(
        matches!(&err, Error::Installer(m) if m == "CLI missing"),
        "{err:?}"
    );
    assert!(!s.claude_cfg().exists());
}

#[test]
fn write_hooks_refuses_tool_not_installed() {
    let s = setup(true);
    let codex_cfg = s.t.paths.codex_home.join("hooks.json");
    let err =
        onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Codex, "20261031000000").unwrap_err();
    assert!(
        matches!(&err, Error::Installer(m) if m == "tool CLI missing"),
        "{err:?}"
    );
    assert!(!codex_cfg.exists());
    assert!(!s.t.paths.install_state_path.exists());
}

#[test]
fn invalid_config_card_has_no_diff() {
    let s = setup(true);
    std::fs::write(s.claude_cfg(), "{ not json").unwrap();
    let c = s.card(Tool::Claude);
    assert!(c.invalid_json);
    assert_eq!(c.hook_status, None);
    assert!(c.diff.is_none());
    assert!(!c.can_write);
    let err =
        onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap_err();
    assert!(
        matches!(&err, Error::Installer(m) if m == "invalid file"),
        "{err:?}"
    );
    assert_eq!(
        std::fs::read_to_string(s.claude_cfg()).unwrap(),
        "{ not json"
    );
}

#[test]
fn write_hooks_backs_up_writes_and_records_state() {
    let s = setup(true);
    let original = "{\n  \"theme\": \"dark\"\n}\n";
    std::fs::write(s.claude_cfg(), original).unwrap();
    let w = onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap();
    assert!(w.changed);
    let backup = w.backup.expect("backup of the existing file");
    assert!(backup.contains(".bak-aiannotations-20261031000000"));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
    let written = std::fs::read_to_string(s.claude_cfg()).unwrap();
    assert!(written.contains("\"theme\""));
    assert!(written.contains("aiannotations"));
    assert_eq!(s.card(Tool::Claude).hook_status, Some(HookStatus::Current));
    let st = state::load(&s.t.paths.install_state_path).unwrap();
    assert_eq!(st.created(&s.claude_cfg()).map(<[_]>::len), Some(2));
}

#[test]
fn write_hooks_creates_missing_file_without_backup() {
    let s = setup(true);
    let w = onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap();
    assert!(w.changed);
    assert!(w.backup.is_none());
    assert!(s.claude_cfg().exists());
}

#[test]
fn second_write_is_unchanged_without_backup() {
    let s = setup(true);
    onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap();
    let before = std::fs::read_to_string(s.claude_cfg()).unwrap();
    let w = onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000001").unwrap();
    assert!(!w.changed);
    assert!(w.backup.is_none());
    assert_eq!(std::fs::read_to_string(s.claude_cfg()).unwrap(), before);
    let baks = std::fs::read_dir(&s.t.paths.claude_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".bak-aiannotations-")
        })
        .count();
    assert_eq!(baks, 0);
}

#[test]
fn legacy_v1_hook_needs_update() {
    let s = setup(true);
    assert!(!onboarding::hooks_need_update(&s.t.paths, &s.env()));
    let legacy = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"/x/bin/aiannotations-hook start","timeout":5}]}]}}"#;
    std::fs::write(s.claude_cfg(), legacy).unwrap();
    assert!(onboarding::hooks_need_update(&s.t.paths, &s.env()));
    onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap();
    assert!(!onboarding::hooks_need_update(&s.t.paths, &s.env()));
}

#[test]
fn finish_is_the_only_writer_of_done() {
    let mut s = setup(true);
    let mut user = user_settings::load(&s.t.paths.settings_path, "x").unwrap();
    onboarding::load(&s.t.conn, &s.t.paths, &user, &s.env()).unwrap();
    onboarding::write_hooks(&s.t.paths, &s.env(), Tool::Claude, "20261031000000").unwrap();
    aiannotations_services::actions::import_history(
        &mut s.t.conn,
        &s.t.paths,
        &Settings::default(),
        &s.t.git,
        now_ms(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(
        !user_settings::load(&s.t.paths.settings_path, "x")
            .unwrap()
            .onboarding_done
    );

    let choices = Choices {
        provider: Tool::Codex,
        claude_model: "opus".into(),
        claude_briefing_model: "haiku".into(),
        codex_model: "".into(),
        codex_briefing_model: "gpt-x".into(),
        summarize_history: true,
        autostart: false,
    };
    onboarding::finish(&s.t.paths, &mut user, &choices).unwrap();
    let back = user_settings::load(&s.t.paths.settings_path, "x").unwrap();
    assert!(back.onboarding_done);
    assert_eq!(back.ai_provider, Some(Tool::Codex));
    assert_eq!(back.claude_model.as_deref(), Some("opus"));
    assert_eq!(back.claude_briefing_model.as_deref(), Some("haiku"));
    assert_eq!(back.codex_model.as_deref(), Some(""));
    assert_eq!(back.codex_briefing_model.as_deref(), Some("gpt-x"));
    assert_eq!(back.summarize_history, Some(true));
    assert_eq!(back.autostart, Some(false));
    assert_eq!(user, back);

    let view = onboarding::load(&s.t.conn, &s.t.paths, &back, &s.env()).unwrap();
    assert!(view.done);
    assert_eq!(view.provider, Tool::Codex);
    assert!(!view.autostart);
    assert!(view.summarize_history);
    assert_eq!(view.codex_model, None);
    assert_eq!(view.codex_briefing_model.as_deref(), Some("gpt-x"));
}

#[test]
fn history_counts_by_tool() {
    let s = setup(true);
    let c = onboarding::history_counts(&s.t.conn).unwrap();
    assert_eq!((c.sessions, c.projects, c.claude, c.codex), (0, 0, 0, 0));
    insert_session(&s.t.conn, Tool::Claude, "a", Path::new("/p/one"), None);
    insert_session(&s.t.conn, Tool::Claude, "b", Path::new("/p/one"), None);
    insert_session(&s.t.conn, Tool::Codex, "c", Path::new("/p/two"), None);
    // A project without sessions does not count.
    aiannotations_core::testing::insert_project(&s.t.conn, "/p/empty");
    let c = onboarding::history_counts(&s.t.conn).unwrap();
    assert_eq!((c.sessions, c.projects, c.claude, c.codex), (3, 2, 2, 1));
}

#[test]
fn pending_backfill_counts_pending_runs() {
    let s = setup(true);
    let load = || {
        onboarding::load(&s.t.conn, &s.t.paths, &Default::default(), &s.env())
            .unwrap()
            .pending_backfill
    };
    assert_eq!(load(), 0);
    let sess = insert_session(&s.t.conn, Tool::Claude, "a", Path::new("/p"), None);
    let run = |status: &'static str| RunSeed {
        started_at: 1,
        summary_status: Some(status),
        is_backfill: true,
        ..Default::default()
    };
    insert_run(&s.t.conn, sess, run("done"));
    insert_run(&s.t.conn, sess, run("pending"));
    insert_run(&s.t.conn, sess, run("pending"));
    insert_run(&s.t.conn, sess, run("skipped"));
    assert_eq!(load(), 2);
}

fn choices() -> Choices {
    Choices {
        provider: Tool::Claude,
        claude_model: "opus".into(),
        claude_briefing_model: "haiku".into(),
        codex_model: "".into(),
        codex_briefing_model: "".into(),
        summarize_history: false,
        autostart: true,
    }
}

#[test]
fn non_utf8_config_is_an_invalid_card() {
    let s = setup(true);
    std::fs::write(s.claude_cfg(), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    let c = s.card(Tool::Claude);
    assert!(c.invalid_json);
    assert!(c.diff.is_none());
    assert!(!c.can_write);
}

#[cfg(unix)]
#[test]
fn unreadable_config_is_an_invalid_card() {
    use std::os::unix::fs::PermissionsExt;
    let s = setup(true);
    std::fs::write(s.claude_cfg(), "{}").unwrap();
    std::fs::set_permissions(s.claude_cfg(), std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read_to_string(s.claude_cfg()).is_ok() {
        return; // running as root: the permission cannot be denied
    }
    let c = s.card(Tool::Claude);
    assert!(c.invalid_json);
    assert!(c.diff.is_none());
    assert!(!c.can_write);
}

#[test]
fn failed_save_leaves_user_unchanged() {
    let s = setup(true);
    let mut paths = s.t.paths.clone();
    let blocker = s.t.base.join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    paths.settings_path = blocker.join("settings.json");
    let mut user = UserSettings::default();
    let err = onboarding::finish(&paths, &mut user, &choices());
    assert!(err.is_err());
    assert_eq!(user, UserSettings::default());
    assert!(!user.onboarding_done);
}

#[test]
fn empty_claude_models_become_default() {
    let s = setup(true);
    let mut user = UserSettings::default();
    let mut c = choices();
    c.claude_model = "".into();
    c.claude_briefing_model = "".into();
    onboarding::finish(&s.t.paths, &mut user, &c).unwrap();
    assert_eq!(user.claude_model, None);
    assert_eq!(user.claude_briefing_model, None);
    let applied = user.apply(Settings::default());
    assert!(!applied.claude_model.is_empty());
    assert!(!applied.claude_briefing_model.is_empty());
}

#[test]
fn hooks_need_update_requires_our_cli() {
    let s = setup(false);
    let legacy = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"/x/bin/aiannotations-hook start","timeout":5}]}]}}"#;
    std::fs::write(s.claude_cfg(), legacy).unwrap();
    assert!(!onboarding::hooks_need_update(&s.t.paths, &s.env()));
}
