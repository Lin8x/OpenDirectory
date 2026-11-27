//! OpenDirectory Endpoint Agent.

mod accounts;
mod agent;
mod certs;
mod inventory;
mod jobs;
mod laps;
mod patches;
mod policy;
mod policy_ext;
mod policy_scripts;
mod service;
mod shell;
mod state;
mod updater;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::time::Duration;

use tokio::sync::watch;

#[derive(Parser)]
#[command(
    name = "opendirectory-agent",
    version,
    about = "OpenDirectory Endpoint Agent: keeps this computer managed by your Management Server"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Enroll this computer with a Management Server.
    Enroll {
        /// Management Server URL, e.g. https://10.0.0.4:8443
        #[arg(long)]
        server: String,
        /// Enrollment token from the Admin Center (Endpoints > Enroll).
        #[arg(long)]
        token: String,
        /// TLS certificate fingerprint shown next to the token (required for https).
        #[arg(long)]
        fingerprint: Option<String>,
        /// Also install and start the background service.
        #[arg(long)]
        install: bool,
        /// Service priority when installing.
        #[arg(long, value_enum, default_value_t = service::Priority::Low)]
        priority: service::Priority,
    },
    /// Run in the foreground (what the service runs).
    Run,
    /// Install and start the background service (headless: no tray or window).
    Install {
        /// Low (default) yields CPU and disk to the user's apps; normal uses the OS default.
        #[arg(long, value_enum, default_value_t = service::Priority::Low)]
        priority: service::Priority,
    },
    /// Stop and remove the background service.
    Uninstall {
        /// Also delete enrollment and local state.
        #[arg(long)]
        purge: bool,
    },
    /// Show enrollment and service status.
    Status,
    /// Collect and print this computer's inventory as JSON.
    Inventory,
    /// Entry point for the Windows Service Control Manager.
    #[command(hide = true)]
    Service,
}

fn init_logging(to_file: bool) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());
    if to_file {
        let dir = state::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        // Keep one previous log, so the file can't grow without limit.
        let log = dir.join("agent.log");
        if std::fs::metadata(&log).map(|m| m.len() > 10 * 1024 * 1024).unwrap_or(false) {
            let _ = std::fs::rename(&log, dir.join("agent.log.1"));
        }
        if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("agent.log")) {
            tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(std::sync::Mutex::new(file)).init();
            return;
        }
    }
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Service = cli.command {
        init_logging(true);
        #[cfg(windows)]
        return service::win::run_dispatcher();
        #[cfg(not(windows))]
        anyhow::bail!("The `service` command is only used on Windows; use `run` instead");
    }
    init_logging(false);
    runtime()?.block_on(async_main(cli.command))
}

/// The agent runs on one thread: it spends nearly all its time waiting on
/// the network, so a single-threaded runtime keeps memory and idle CPU
/// wake-ups to a minimum. Slow OS commands run on a small blocking pool.
pub fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(8).thread_keep_alive(Duration::from_secs(10)).build()
}

async fn async_main(cmd: Command) -> Result<()> {
    match cmd {
        Command::Enroll { server, token, fingerprint, install, priority } => {
            if let Some(existing) = state::AgentState::load()? {
                println!("Note: replacing the existing enrollment with {}", existing.server.url);
            }
            let st = agent::enroll(&server, &token, fingerprint).await?;
            println!("Enrolled as endpoint {} with {}", st.endpoint_id, st.server.url);
            if install {
                service::install(priority)?;
                println!("{} installed and running.", service::DISPLAY_NAME);
            } else {
                println!("Next: run `opendirectory-agent install` to keep it running in the background.");
            }
        }
        Command::Run => {
            service::apply_priority(service::Priority::load());
            service::refresh_definition();
            let (tx, rx) = watch::channel(false);
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                let _ = tx.send(true);
            });
            if agent::run(rx).await? {
                // Restart requested: exiting non-zero lets systemd/launchd/SCM start us again cleanly.
                std::process::exit(1);
            }
        }
        Command::Install { priority } => {
            if state::AgentState::load()?.is_none() {
                anyhow::bail!("Enroll first: opendirectory-agent enroll --server <url> --token <token> --fingerprint <fp>");
            }
            service::install(priority)?;
            println!("{} installed and running.", service::DISPLAY_NAME);
        }
        Command::Uninstall { purge } => {
            service::uninstall()?;
            if purge {
                let _ = std::fs::remove_dir_all(state::data_dir());
                println!("Service removed and local state deleted. Local accounts created by OpenDirectory were left in place.");
            } else {
                println!("Service removed. Enrollment kept; run `install` to start it again.");
            }
        }
        Command::Status => {
            match state::AgentState::load()? {
                Some(s) => {
                    println!("Enrolled:   yes ({})", s.server.url);
                    println!("Endpoint:   {}", s.endpoint_id);
                    println!("Since:      {}", s.enrolled_at.format("%Y-%m-%d %H:%M UTC"));
                    println!("Accounts:   {} managed", s.accounts.len());
                }
                None => println!("Enrolled:   no"),
            }
            println!("Service:    {} (headless, {} priority)", service::status(), service::Priority::load().label());
            println!("Version:    {}", agent::VERSION);
        }
        Command::Inventory => {
            println!("{}", serde_json::to_string_pretty(&inventory::collect().await)?);
        }
        Command::Service => unreachable!(),
    }
    Ok(())
}
