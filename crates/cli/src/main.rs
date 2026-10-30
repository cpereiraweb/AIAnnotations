use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::io::Read;

#[derive(Parser)]
#[command(name = "aiannotations", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Developer tools.
    #[command(hide = true, subcommand)]
    Dev(DevCommand),
    /// Import the Laravel app's database (read-only on it).
    #[command(hide = true)]
    ImportLaravel {
        /// Path of the Laravel `database.sqlite`.
        #[arg(long)]
        db: std::path::PathBuf,
        /// Import even though the database already has sessions.
        #[arg(long)]
        merge: bool,
    },
}

#[derive(Subcommand)]
enum DevCommand {
    /// Drain the hook spool into the database.
    Ingest,
    /// Record the last days' sessions the hooks missed and close the idle ones.
    Collect,
    /// Import the whole Claude Code and Codex session history.
    Backfill,
    /// Delete old processed and failed spool files.
    Prune,
    /// Recompute the sessions marked empty with the current rule.
    Reclassify,
    /// Print database counts as JSON.
    Stats,
    /// Search the sessions (full text) and print the hits as JSON.
    Search {
        query: String,
        /// Maximum number of hits.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Rebuild the search index and print `{"indexed": N}`.
    Reindex,
    /// Print the active resume documents of every project as JSON.
    Ongoings,
    /// Print the inbox as JSON (`--grouped` by project), or dismiss an item.
    Inbox {
        /// Group by project, split into recent and older.
        #[arg(long)]
        grouped: bool,
        #[command(subcommand)]
        action: Option<InboxAction>,
    },
    /// Summarize the pending runs with the AI and print the report as JSON.
    Summarize {
        /// Maximum number of runs in this batch.
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Put the failed runs back in the queue first.
        #[arg(long)]
        retry_failed: bool,
        /// AI CLI to use (default: the configured one).
        #[arg(long, value_parser = ["claude", "codex"])]
        provider: Option<String>,
        /// Summary language, `en` or `pt-BR` (default: `$LANG`, else the OS locale).
        #[arg(long)]
        lang: Option<String>,
    },
    /// Generate today's AI briefing and print `{"outcome", "briefing"}` as JSON.
    Briefing {
        /// Regenerate even if today's briefing is done.
        #[arg(long)]
        force: bool,
        /// AI CLI to use (default: the configured one).
        #[arg(long, value_parser = ["claude", "codex"])]
        provider: Option<String>,
        /// Briefing language, `en` or `pt-BR` (default: `$LANG`, else the OS locale).
        #[arg(long)]
        lang: Option<String>,
        /// IANA time zone of the briefing's day (default: the system's, else UTC).
        #[arg(long)]
        tz: Option<String>,
    },
}

#[derive(Subcommand)]
enum InboxAction {
    /// Dismiss an item at its current state (it returns when the state changes).
    Dismiss { key: String, state_hash: String },
}

fn main() {
    // The hook is handled before clap so nothing can ever be printed on its path.
    if std::env::args_os().nth(1).is_some_and(|a| a == "hook") {
        let args: Vec<OsString> = std::env::args_os().skip(2).collect();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::panic::catch_unwind(|| run_hook(&args));
        std::process::exit(0);
    }
    let code = match Cli::parse().command {
        Some(Command::Dev(DevCommand::Ingest)) => run_dev_ingest(),
        Some(Command::Dev(DevCommand::Collect)) => {
            run_dev_collect(aiannotations_core::sessions::collector::CollectMode::Window)
        }
        Some(Command::Dev(DevCommand::Backfill)) => {
            run_dev_collect(aiannotations_core::sessions::collector::CollectMode::Backfill)
        }
        Some(Command::Dev(DevCommand::Prune)) => run_dev_prune(),
        Some(Command::Dev(DevCommand::Reclassify)) => run_dev_reclassify(),
        Some(Command::Dev(DevCommand::Stats)) => run_dev_stats(),
        Some(Command::Dev(DevCommand::Search { query, limit })) => run_dev_search(&query, limit),
        Some(Command::Dev(DevCommand::Reindex)) => run_dev_reindex(),
        Some(Command::Dev(DevCommand::Ongoings)) => run_dev_ongoings(),
        Some(Command::Dev(DevCommand::Inbox { grouped, action })) => match action {
            Some(InboxAction::Dismiss { key, state_hash }) => {
                run_dev_inbox_dismiss(&key, &state_hash)
            }
            None => run_dev_inbox(grouped),
        },
        Some(Command::Dev(DevCommand::Summarize {
            limit,
            retry_failed,
            provider,
            lang,
        })) => run_dev_summarize(limit, retry_failed, provider.as_deref(), lang.as_deref()),
        Some(Command::Dev(DevCommand::Briefing {
            force,
            provider,
            lang,
            tz,
        })) => run_dev_briefing(force, provider.as_deref(), lang.as_deref(), tz.as_deref()),
        Some(Command::ImportLaravel { db, merge }) => run_import_laravel(&db, merge),
        None => 0,
    };
    std::process::exit(code);
}

fn run_dev_ingest() -> i32 {
    use aiannotations_core::{Paths, Settings, db, git::Git, sessions::ingest};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|mut conn| {
        ingest::run(
            &mut conn,
            &paths,
            &Settings::default(),
            &Git::default(),
            chrono::Utc::now().timestamp_millis(),
        )
    });
    match result {
        Ok(r) => {
            println!(
                "processed={} failed={} deferred={}",
                r.processed, r.failed, r.deferred
            );
            for w in &r.warnings {
                println!("warning: {w}");
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_dev_collect(mode: aiannotations_core::sessions::collector::CollectMode) -> i32 {
    use aiannotations_core::{Paths, Settings, db, git::Git, sessions::collector};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|mut conn| {
        collector::run(
            &mut conn,
            &paths,
            &Settings::default(),
            &Git::default(),
            mode,
            chrono::Utc::now().timestamp_millis(),
            &mut |_, _| {},
        )
    });
    match result {
        Ok(r) => {
            println!(
                "seen={} closed_idle={} errors={} warnings={}",
                r.seen,
                r.closed_idle,
                r.errors.len(),
                r.warnings.len()
            );
            for e in &r.errors {
                println!("error: {e}");
            }
            for w in &r.warnings {
                println!("warning: {w}");
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_dev_prune() -> i32 {
    use aiannotations_core::{Paths, Settings, sessions::maintenance};
    let paths = Paths::resolve();
    let now = chrono::Utc::now().timestamp_millis();
    match maintenance::prune_spool(&paths.spool_dir, &Settings::default(), now) {
        Ok(r) => {
            for w in &r.warnings {
                println!("warning: {w}");
            }
            println!(
                "done_removed={} failed_removed={}",
                r.done_removed, r.failed_removed
            );
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_dev_reclassify() -> i32 {
    use aiannotations_core::{Paths, Settings, db, sessions::maintenance};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|mut conn| {
        maintenance::reclassify(
            &mut conn,
            &paths,
            &Settings::default(),
            chrono::Utc::now().timestamp_millis(),
        )
    });
    match result {
        Ok(r) => {
            for w in &r.warnings {
                println!("warning: {w}");
            }
            println!("reclassified={}", r.reclassified);
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_dev_stats() -> i32 {
    use aiannotations_core::{Paths, db, sessions::maintenance};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|conn| maintenance::stats(&conn));
    match result {
        Ok(stats) => match serde_json::to_string_pretty(&stats) {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_dev_search(query: &str, limit: usize) -> i32 {
    use aiannotations_core::{Paths, db, search};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|conn| search::query(&conn, query, limit));
    print_json(result)
}

fn run_dev_ongoings() -> i32 {
    use aiannotations_core::{Paths, db, ongoings};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path).and_then(|conn| ongoings::for_projects(&conn));
    print_json(result)
}

fn run_dev_inbox(grouped: bool) -> i32 {
    use aiannotations_core::{Paths, Settings, db, git::Git, inbox};
    let paths = Paths::resolve();
    let settings = Settings::default();
    let now = chrono::Utc::now().timestamp_millis();
    let result = db::open(&paths.db_path).and_then(|conn| {
        let items = inbox::build(
            &conn,
            &Git::default(),
            &inbox::GitCache::new(),
            &settings,
            now,
        )?;
        Ok(if grouped {
            serde_json::to_value(inbox::grouped(&conn, items, &settings, now)?)?
        } else {
            serde_json::to_value(items)?
        })
    });
    print_json(result)
}

fn run_dev_inbox_dismiss(key: &str, state_hash: &str) -> i32 {
    use aiannotations_core::{Paths, db, inbox};
    let paths = Paths::resolve();
    let now = chrono::Utc::now().timestamp_millis();
    let result = db::open(&paths.db_path)
        .and_then(|mut conn| inbox::dismiss(&mut conn, key, state_hash, now))
        .map(|()| serde_json::json!({ "dismissed": key }));
    print_json(result)
}

fn run_dev_reindex() -> i32 {
    use aiannotations_core::{Paths, db, search};
    let paths = Paths::resolve();
    let result = db::open(&paths.db_path)
        .and_then(|mut conn| search::rebuild(&mut conn))
        .map(|indexed| serde_json::json!({ "indexed": indexed }));
    print_json(result)
}

fn run_dev_summarize(
    limit: usize,
    retry_failed: bool,
    provider: Option<&str>,
    lang: Option<&str>,
) -> i32 {
    use aiannotations_core::{Paths, Settings, ai, db, home_dir, summaries};
    let paths = Paths::resolve();
    let settings = Settings {
        lang: lang_or_system(lang),
        ai_provider: provider
            .and_then(|p| p.parse().ok())
            .unwrap_or(Settings::default().ai_provider),
        ..Settings::default()
    };
    let env = |k: &str| std::env::var_os(k);
    let client = ai::client_for(&settings, &paths, env, &home_dir(env));
    let clock = || chrono::Utc::now().timestamp_millis();
    let result = db::open(&paths.db_path).and_then(|mut conn| {
        summaries::run_batch(
            &mut conn,
            client.as_ref(),
            &paths,
            &settings,
            limit,
            retry_failed,
            &clock,
        )
    });
    print_json(result)
}

fn run_dev_briefing(
    force: bool,
    provider: Option<&str>,
    lang: Option<&str>,
    tz: Option<&str>,
) -> i32 {
    use aiannotations_core::{Paths, Settings, ai, briefing, db, git::Git, home_dir, inbox};
    let tz: chrono_tz::Tz = match tz {
        Some(name) => match name.parse() {
            Ok(tz) => tz,
            Err(_) => {
                eprintln!("error: unknown time zone: {name}");
                return 1;
            }
        },
        None => iana_time_zone::get_timezone()
            .ok()
            .and_then(|name| name.parse().ok())
            .unwrap_or(chrono_tz::UTC),
    };
    let paths = Paths::resolve();
    let settings = Settings {
        lang: lang_or_system(lang),
        ai_provider: provider
            .and_then(|p| p.parse().ok())
            .unwrap_or(Settings::default().ai_provider),
        ..Settings::default()
    };
    let env = |k: &str| std::env::var_os(k);
    let client = ai::client_for(&settings, &paths, env, &home_dir(env));
    let clock = || chrono::Utc::now().timestamp_millis();
    let result = db::open(&paths.db_path).and_then(|mut conn| {
        let titles: Vec<String> = inbox::build(
            &conn,
            &Git::default(),
            &inbox::GitCache::new(),
            &settings,
            clock(),
        )?
        .into_iter()
        .map(|item| item.title)
        .collect();
        let outcome = briefing::run(
            &mut conn,
            client.as_ref(),
            &titles,
            &settings,
            tz,
            force,
            &clock,
        )?;
        Ok(serde_json::json!({
            "outcome": outcome,
            "briefing": briefing::today(&conn, tz, clock())?,
        }))
    });
    print_json(result)
}

/// Prints `value` as pretty JSON (exit 0), or the error on stderr (exit 1).
fn print_json<T: serde::Serialize>(value: aiannotations_core::Result<T>) -> i32 {
    match value
        .map_err(|e| e.to_string())
        .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
    {
        Ok(json) => {
            println!("{json}");
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// `projects=N sessions=N runs=N dismissals=N briefings=N skipped=N`, then one `skipped: …` line
/// per record left out.
fn run_import_laravel(laravel_db: &std::path::Path, merge: bool) -> i32 {
    use aiannotations_core::{Paths, db, import_laravel};
    let paths = Paths::resolve();
    let options = import_laravel::ImportOptions {
        merge,
        ..Default::default()
    };
    let result = db::open(&paths.db_path)
        .and_then(|mut conn| import_laravel::run(laravel_db, &mut conn, &options));
    match result {
        Ok(r) => {
            println!(
                "projects={} sessions={} runs={} dismissals={} briefings={} skipped={}",
                r.projects,
                r.sessions,
                r.runs,
                r.dismissals,
                r.briefings,
                r.skipped.len()
            );
            for s in &r.skipped {
                println!("skipped: {s}");
            }
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

/// Port of `bin/aiannotations-hook` (Inv §3.1): drop the event in the spool, silently.
fn run_hook(args: &[OsString]) {
    let mut stdin = Vec::new();
    let _ = std::io::stdin().lock().read_to_end(&mut stdin);

    if std::env::var_os("AIANNOTATIONS_INTERNAL").is_some_and(|v| v == "1") {
        return;
    }
    let (Some(tool), Some(event)) = (
        args.first().and_then(|a| a.to_str()),
        args.get(1).and_then(|a| a.to_str()),
    ) else {
        return;
    };
    if !matches!(tool, "claude" | "codex") || !matches!(event, "start" | "end") {
        return;
    }
    let paths = aiannotations_core::Paths::resolve();
    let _ = aiannotations_core::spool::write_envelope(
        &paths.spool_dir,
        tool,
        event,
        &stdin,
        chrono::Utc::now(),
    );
}

/// `--lang` when given, else `$LANG`, else the OS locale.
fn lang_or_system(flag: Option<&str>) -> aiannotations_core::settings::Lang {
    use aiannotations_core::settings::Lang;
    flag.map_or_else(
        || {
            Lang::detect(
                std::env::var("LANG").ok().as_deref(),
                sys_locale::get_locale().as_deref(),
            )
        },
        Lang::from_tag,
    )
}
