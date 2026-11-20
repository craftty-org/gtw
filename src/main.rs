use std::fs::OpenOptions;
use std::io::IsTerminal;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::writer::BoxMakeWriter;

use crate::app::RunOptions;
use crate::core::Config;

#[derive(Parser, Debug)]
#[command(
    name = "gtw",
    version,
    about = "Wikipedia in your terminal: cached reading, rich media, offline lists",
    long_about = None
)]
struct Args {
    /// Read only from the local cache; never contact Wikipedia.
    #[arg(long)]
    offline: bool,
    /// Disable startup, page, feed, and image transitions.
    #[arg(long)]
    no_animations: bool,
    /// Start with a bundled theme name (for example "Chadrula").
    #[arg(long)]
    theme: Option<String>,
    /// Append diagnostic logs to this file (default: cache directory/gtw.log).
    #[arg(long)]
    log_file: Option<PathBuf>,
    /// Load configuration from this path instead of the platform default.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Print environment, cache, media, and terminal capabilities, then exit.
    #[arg(long)]
    doctor: bool,
    /// Print shell completions for this shell and exit.
    #[arg(long, value_name = "SHELL", value_enum)]
    completions: Option<clap_complete::Shell>,
    /// Print the roff man page and exit.
    #[arg(long)]
    man: bool,
}

fn main() -> Result<()> {
    restore_sigpipe();
    let args = Args::parse();
    if let Some(shell) = args.completions {
        let mut command = Args::command();
        clap_complete::generate(shell, &mut command, "gtw", &mut std::io::stdout());
        return Ok(());
    }
    if args.man {
        clap_mangen::Man::new(Args::command()).render(&mut std::io::stdout())?;
        return Ok(());
    }
    let (mut config, config_path) = Config::load(args.config.as_deref());
    if args.offline {
        config.general.offline = true;
    }
    if args.no_animations {
        config.general.no_animations = true;
        config.effects.enabled = false;
    }
    if let Some(theme) = &args.theme {
        config.theme.name = theme.clone();
    }

    let log_path = args
        .log_file
        .clone()
        .unwrap_or_else(|| Config::cache_dir().join("gtw.log"));
    let _log_guard = init_logging(&log_path);
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        offline = config.general.offline,
        doctor = args.doctor,
        "gtw starting"
    );

    if args.doctor {
        return crate::app::doctor::run(&config, config_path.as_deref());
    }

    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        anyhow::bail!(
            "gtw needs an interactive terminal. Run it directly in your terminal, or use `gtw --doctor` for diagnostics."
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(async {
        crate::app::run(RunOptions {
            config,
            offline: args.offline,
        })
        .await
    })
}

/// Restore the default SIGPIPE behavior so a closed pipeline (for example
/// `gtw --doctor | head`) terminates quietly instead of aborting on a failed
/// stdout write. Rust ignores SIGPIPE in `main` by default.
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: restoring the default disposition for one signal before any
    // threads or I/O are started does not violate any aliasing rules.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

fn init_logging(path: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("gtw=info,gtw::app=info,warn"));
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "gtw: cannot create log directory {}: {error}",
            parent.display()
        );
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::sink)
            .with_ansi(false)
            .init();
        return None;
    }
    let file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("gtw: cannot open log file {}: {error}", path.display());
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::sink)
                .with_ansi(false)
                .init();
            return None;
        }
    };
    let (writer, guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(BoxMakeWriter::new(writer))
        .with_ansi(false)
        .with_target(false)
        .init();
    Some(guard)
}

mod api;
mod app;
mod cache;
mod core;
mod media;
mod render;
