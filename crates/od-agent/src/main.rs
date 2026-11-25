//! OpenDirectory Endpoint Agent.

mod accounts;
mod agent;
mod inventory;
mod jobs;
mod policy;
mod policy_scripts;
mod service;
mod state;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
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
    },
    /// Run in the foreground (what the service runs).
    Run,
    /// Install and start the background service.
    Install,
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
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async_main(cli.command))
}

async fn async_main(cmd: Command) -> Result<()> {
    match cmd {
        Command::Enroll { server, token, fingerprint, install } => {
            if let Some(existing) = state::AgentState::load()? {
                println!("Note: replacing the existing enrollment with {}", existing.server.url);
            }
            let st = agent::enroll(&server, &token, fingerprint).await?;
            println!("Enrolled as endpoint {} with {}", st.endpoint_id, st.server.url);
            if install {
                service::install()?;
                println!("{} installed and running.", service::DISPLAY_NAME);
            } else {
                println!("Next: run `opendirectory-agent install` to keep it running in the background.");
            }
        }
        Command::Run => {
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
        Command::Install => {
            if state::AgentState::load()?.is_none() {
                anyhow::bail!("Enroll first: opendirectory-agent enroll --server <url> --token <token> --fingerprint <fp>");
            }
            service::install()?;
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
            println!("Service:    {}", service::status());
            println!("Version:    {}", agent::VERSION);
        }
        Command::Inventory => {
            println!("{}", serde_json::to_string_pretty(&inventory::collect().await)?);
        }
        Command::Service => unreachable!(),
    }
    Ok(())
}
