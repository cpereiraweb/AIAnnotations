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
