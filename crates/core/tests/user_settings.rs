//! User `settings.json`: load, save, and applying it over the defaults.

use aiannotations_core::Settings;
use aiannotations_core::model::Tool;
use aiannotations_core::settings::Lang;
use aiannotations_core::user_settings::{self, UserSettings};

#[test]
fn missing_file_is_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let got = user_settings::load(&path, "20261031120000").unwrap();
    assert_eq!(got, UserSettings::default());
    assert!(!path.exists());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn roundtrip_keeps_every_field() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub").join("settings.json");
    let s = UserSettings {
        onboarding_done: true,
        ai_provider: Some(Tool::Codex),
        claude_model: Some("opus".into()),
        claude_briefing_model: Some("sonnet".into()),
        codex_model: Some("gpt-x".into()),
        codex_briefing_model: Some("gpt-y".into()),
        summarize_history: Some(false),
        autostart: Some(true),
        lang: Some(Lang::PtBr),
    };
    user_settings::save(&path, &s).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\n  \"onboarding_done\": true"), "{text}");
    assert!(text.contains("\"ai_provider\": \"codex\""), "{text}");
    assert_eq!(user_settings::load(&path, "x").unwrap(), s);
}

#[test]
fn invalid_json_is_moved_aside_and_defaults_used() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    let got = user_settings::load(&path, "20261031120000").unwrap();
    assert_eq!(got, UserSettings::default());
    assert!(!path.exists());
    let aside = dir.path().join("settings.json.invalid-20261031120000");
    assert_eq!(std::fs::read_to_string(aside).unwrap(), "{ not json");
}

#[test]
fn apply_overrides_only_some_fields() {
    let base = Settings::default();
    let s = UserSettings {
        claude_model: Some("opus".into()),
        ..Default::default()
    };
    let got = s.apply(base.clone());
    assert_eq!(got.claude_model, "opus");
    assert_eq!(
        got,
        Settings {
            claude_model: "opus".into(),
            ..base
        }
    );
}

#[test]
fn apply_sets_provider_models_and_history() {
    let s = UserSettings {
        ai_provider: Some(Tool::Codex),
        claude_briefing_model: Some("opus".into()),
        codex_model: Some("gpt-x".into()),
        codex_briefing_model: Some("gpt-y".into()),
        summarize_history: Some(false),
        ..Default::default()
    };
    let got = s.apply(Settings::default());
    assert_eq!(got.ai_provider, Tool::Codex);
    assert_eq!(got.claude_briefing_model, "opus");
    assert_eq!(got.codex_model.as_deref(), Some("gpt-x"));
    assert_eq!(got.codex_briefing_model.as_deref(), Some("gpt-y"));
    assert!(!got.summarize_history);
}

#[test]
fn empty_codex_model_means_codex_default() {
    let s = UserSettings {
        codex_model: Some(String::new()),
        codex_briefing_model: Some(String::new()),
        ..Default::default()
    };
    let base = Settings {
        codex_model: Some("old".into()),
        codex_briefing_model: Some("old".into()),
        ..Settings::default()
    };
    let got = s.apply(base);
    assert_eq!(got.codex_model, None);
    assert_eq!(got.codex_briefing_model, None);
}

#[test]
fn pending_onboarding_starts_paused() {
    assert!(UserSettings::default().starts_paused());
    let done = UserSettings {
        onboarding_done: true,
        ..Default::default()
    };
    assert!(!done.starts_paused());
}

#[test]
fn user_settings_lang_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let s = UserSettings {
        lang: Some(Lang::PtBr),
        ..Default::default()
    };
    user_settings::save(&path, &s).unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"lang\": \"pt-BR\"")
    );
    assert_eq!(
        user_settings::load(&path, "x").unwrap().lang,
        Some(Lang::PtBr)
    );

    std::fs::write(&path, r#"{"onboarding_done":true}"#).unwrap();
    assert_eq!(user_settings::load(&path, "x").unwrap().lang, None);
    user_settings::save(&path, &UserSettings::default()).unwrap();
    assert!(!std::fs::read_to_string(&path).unwrap().contains("lang"));
}

#[test]
fn effective_lang_prefers_the_choice() {
    let s = UserSettings {
        lang: Some(Lang::En),
        ..Default::default()
    };
    assert_eq!(s.effective_lang(Lang::PtBr), Lang::En);
    assert_eq!(
        UserSettings::default().effective_lang(Lang::PtBr),
        Lang::PtBr
    );
}

/// A hand-edited unknown language must not invalidate the whole file.
#[test]
fn unknown_lang_loads_as_auto_and_keeps_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"onboarding_done": true, "lang": "fr"}"#).unwrap();
    let got = user_settings::load(&path, "x").unwrap();
    assert_eq!(got.lang, None);
    assert!(got.onboarding_done);
    assert!(path.exists(), "the file is not moved aside");
}
