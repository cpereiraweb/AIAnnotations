//! The first-run wizard without Tauri: what each AI CLI card shows (with the hook diff), the
//! safe write of the hooks, history counts and the user's final choices. Environment values
//! (home, `PATH`, `AIANNOTATIONS_*_BIN`, our CLI path) come in as parameters.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use aiannotations_core::installer::detect::{find_cli, hook_config_path, hook_spec};
use aiannotations_core::installer::fsio::{backup, write_atomic};
use aiannotations_core::installer::hooks::{self, HookSpec, HookStatus};
use aiannotations_core::installer::state;
use aiannotations_core::model::Tool;
use aiannotations_core::summaries::backfill_progress;
use aiannotations_core::user_settings::{self, UserSettings};
use aiannotations_core::{Error, Paths, Result, Settings};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const TOOLS: [Tool; 2] = [Tool::Claude, Tool::Codex];

/// Where the wizard looks: no global state is read here.
pub struct Env<'a> {
    pub home: &'a Path,
    pub path_var: Option<&'a OsStr>,
    /// Value of `AIANNOTATIONS_CLAUDE_BIN`.
    pub claude_override: Option<&'a OsStr>,
    /// Value of `AIANNOTATIONS_CODEX_BIN`.
    pub codex_override: Option<&'a OsStr>,
    /// Our `aiannotations` CLI (beside the app executable); it may not exist.
    pub cli: &'a Path,
    pub windows: bool,
}

#[derive(Debug, Serialize)]
pub struct CliCard {
    pub tool: Tool,
    pub bin: Option<String>,
    pub config_path: String,
    /// `None` when the config is not valid JSON.
    pub hook_status: Option<HookStatus>,
    pub invalid_json: bool,
    /// `None` when current, invalid, or when our CLI or the tool's CLI is missing.
    pub diff: Option<String>,
    pub can_write: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryCounts {
    pub sessions: u64,
    pub projects: u64,
    pub claude: u64,
    pub codex: u64,
}

#[derive(Debug, Serialize)]
pub struct OnboardingView {
    pub done: bool,
    pub cli_found: bool,
    pub cards: Vec<CliCard>,
    pub provider: Tool,
    pub claude_model: String,
    pub claude_briefing_model: String,
    pub codex_model: Option<String>,
    pub codex_briefing_model: Option<String>,
    pub summarize_history: bool,
    pub autostart: bool,
    pub history: HistoryCounts,
    pub pending_backfill: u64,
}

#[derive(Debug, Serialize)]
pub struct HookWrite {
    pub changed: bool,
    pub backup: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Choices {
    pub provider: Tool,
    pub claude_model: String,
    pub claude_briefing_model: String,
    pub codex_model: String,
    pub codex_briefing_model: String,
    pub summarize_history: bool,
    pub autostart: bool,
}

/// One tool as found on disk.
struct Inspection {
    bin: Option<PathBuf>,
    config: PathBuf,
    /// `None` when the file does not exist.
    text: Option<String>,
    spec: HookSpec,
    /// `None` when the JSON is invalid.
    status: Option<HookStatus>,
}

fn inspect(tool: Tool, paths: &Paths, env: &Env) -> Result<Inspection> {
    let over = match tool {
        Tool::Claude => env.claude_override,
        Tool::Codex => env.codex_override,
    };
    let config = hook_config_path(tool, paths);
    // An unreadable or non-UTF-8 file is shown as invalid (never touched) instead of failing
    // the whole view, which would skip the wizard.
    let (text, unreadable) = match std::fs::read_to_string(&config) {
        Ok(t) => (Some(t), false),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, false),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::InvalidData | std::io::ErrorKind::PermissionDenied
            ) =>
        {
            (None, true)
        }
        Err(e) => return Err(e.into()),
    };
    let spec = hook_spec(env.cli, tool, env.windows);
    let status = if unreadable {
        None
    } else {
        hooks::status(text.as_deref(), &spec).ok()
    };
    Ok(Inspection {
        bin: find_cli(tool, over, env.home, env.path_var),
        config,
        text,
        spec,
        status,
    })
}

impl Inspection {
    fn can_write(&self, cli_found: bool) -> bool {
        cli_found && self.bin.is_some() && self.status.is_some_and(|s| s != HookStatus::Current)
    }
}

fn card(tool: Tool, paths: &Paths, env: &Env, cli_found: bool) -> Result<CliCard> {
    let i = inspect(tool, paths, env)?;
    let can_write = i.can_write(cli_found);
    let label = i.config.to_string_lossy().into_owned();
    let diff = if can_write {
        let merge = hooks::install(i.text.as_deref(), &i.spec)
            .map_err(|_| Error::Installer("invalid file".into()))?;
        Some(hooks::unified_diff(
            &label,
            i.text.as_deref(),
            &merge.contents,
        ))
    } else {
        None
    };
    Ok(CliCard {
        tool,
        bin: i.bin.map(|b| b.to_string_lossy().into_owned()),
        config_path: label,
        hook_status: i.status,
        invalid_json: i.status.is_none(),
        diff,
        can_write,
    })
}

pub fn load(
    conn: &Connection,
    paths: &Paths,
    user: &UserSettings,
    env: &Env,
) -> Result<OnboardingView> {
    let cli_found = env.cli.is_file();
    let cards = TOOLS
        .iter()
        .map(|t| card(*t, paths, env, cli_found))
        .collect::<Result<Vec<_>>>()?;
    let applied = user.apply(Settings::default());
    let provider = user.ai_provider.unwrap_or_else(|| {
        cards
            .iter()
            .find(|c| c.bin.is_some())
            .map_or(applied.ai_provider, |c| c.tool)
    });
    Ok(OnboardingView {
        done: user.onboarding_done,
        cli_found,
        cards,
        provider,
        claude_model: applied.claude_model,
        claude_briefing_model: applied.claude_briefing_model,
        codex_model: applied.codex_model,
        codex_briefing_model: applied.codex_briefing_model,
        summarize_history: user.summarize_history.unwrap_or(false),
        autostart: user.autostart.unwrap_or(true),
        history: history_counts(conn)?,
        pending_backfill: backfill_progress(conn)?.map_or(0, |(done, total)| total - done),
    })
}

/// True when a detected CLI has our hooks `Outdated` or `Legacy` (shown on the Today screen).
pub fn hooks_need_update(paths: &Paths, env: &Env) -> bool {
    // Without our CLI the wizard cannot write anything: do not send the user there.
    env.cli.is_file()
        && TOOLS.iter().any(|t| {
            inspect(*t, paths, env).is_ok_and(|i| {
                i.bin.is_some()
                    && matches!(i.status, Some(HookStatus::Outdated | HookStatus::Legacy))
            })
        })
}

/// Merges our hooks into the tool's config: backup, atomic write, install state. Nothing is
/// touched when the file is invalid, our CLI is missing, or nothing would change.
pub fn write_hooks(paths: &Paths, env: &Env, tool: Tool, stamp: &str) -> Result<HookWrite> {
    if !env.cli.is_file() {
        return Err(Error::Installer("CLI missing".into()));
    }
    let i = inspect(tool, paths, env)?;
    if i.bin.is_none() {
        return Err(Error::Installer("tool CLI missing".into()));
    }
    if i.status.is_none() {
        return Err(Error::Installer("invalid file".into()));
    }
    let merge = hooks::install(i.text.as_deref(), &i.spec)
        .map_err(|_| Error::Installer("invalid file".into()))?;
    if !merge.changed {
        return Ok(HookWrite {
            changed: false,
            backup: None,
        });
    }
    let backup = if i.config.exists() {
        Some(backup(&i.config, stamp)?.to_string_lossy().into_owned())
    } else {
        None
    };
    if let Some(dir) = i.config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_atomic(&i.config, &merge.contents)?;
    let mut st = state::load(&paths.install_state_path)?;
    st.record_created(&i.config, &merge.absent_events);
    state::save(&paths.install_state_path, &st)?;
    Ok(HookWrite {
        changed: true,
        backup,
    })
}

/// The only place that marks the wizard as done.
/// `user` is only replaced after the save succeeded.
pub fn finish(paths: &Paths, user: &mut UserSettings, choices: &Choices) -> Result<()> {
    // An empty Claude model means "the default", never `--model ""`.
    let claude = |m: &str| (!m.is_empty()).then(|| m.to_string());
    let mut next = user.clone();
    next.ai_provider = Some(choices.provider);
    next.claude_model = claude(&choices.claude_model);
    next.claude_briefing_model = claude(&choices.claude_briefing_model);
    next.codex_model = Some(choices.codex_model.clone());
    next.codex_briefing_model = Some(choices.codex_briefing_model.clone());
    next.summarize_history = Some(choices.summarize_history);
    next.autostart = Some(choices.autostart);
    next.onboarding_done = true;
    user_settings::save(&paths.settings_path, &next)?;
    *user = next;
    Ok(())
}

/// Sessions, projects with at least one session, and sessions per tool.
pub fn history_counts(conn: &Connection) -> Result<HistoryCounts> {
    let (sessions, projects, claude, codex): (i64, i64, i64, i64) = conn.query_row(
        "SELECT COUNT(*), COUNT(DISTINCT project_id), \
         COALESCE(SUM(tool = 'claude'), 0), COALESCE(SUM(tool = 'codex'), 0) FROM ai_sessions",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(HistoryCounts {
        sessions: sessions as u64,
        projects: projects as u64,
        claude: claude as u64,
        codex: codex as u64,
    })
}
