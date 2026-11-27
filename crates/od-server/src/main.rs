use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// OpenDirectory Management Server: directory, Group Policy, endpoint
/// management and the API used by the Admin Center, `od` CLI and Endpoint
/// Agents.
#[derive(Parser)]
#[command(name = "opendirectory-server", version)]
struct Args {
    /// Where the directory database, keys and certificates are stored.
    #[arg(long, env = "OD_DATA_DIR", default_value = default_data_dir(), global = true)]
    data_dir: PathBuf,
    /// Address Endpoint Agents and remote admins connect to.
    #[arg(long, env = "OD_LISTEN", default_value = "0.0.0.0:8443", global = true)]
    listen: SocketAddr,
    /// Serve plain HTTP instead of TLS (only for development).
    #[arg(long, env = "OD_INSECURE_HTTP", global = true)]
    insecure_http: bool,
    /// Hostname or IP put in enrollment commands (detected if omitted).
    #[arg(long, env = "OD_PUBLIC_HOST", global = true)]
    public_host: Option<String>,
    /// Serve the Admin Center to browsers from this folder (web mode).
    /// Defaults to a `web` folder next to the binary, if there is one.
    #[arg(long, env = "OD_WEB_UI", global = true)]
    web_ui: Option<PathBuf>,
    /// TLS certificate (PEM) to use instead of the self-signed one.
    #[arg(long, env = "OD_TLS_CERT", global = true, requires = "tls_key")]
    tls_cert: Option<PathBuf>,
    /// TLS private key (PEM) for --tls-cert.
    #[arg(long, env = "OD_TLS_KEY", global = true)]
    tls_key: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run in the foreground (the default).
    Run,
    /// Install as a system service that starts at boot (run as root/Administrator).
    InstallService,
    /// Stop and remove the system service. The directory data is kept.
    UninstallService,
    /// Show whether the system service is running.
    ServiceStatus,
    /// Break-glass recovery for Domain Admins, run on the Management Server
    /// host itself (needs root or Administrator; not available over the network).
    #[command(subcommand)]
    Admin(AdminCmd),
    /// Factory reset: erase the directory, GPOs, enrolled endpoints, keys and
    /// certificates so the next start runs the setup wizard. Stop the
    /// Management Server first. Needs root or Administrator.
    FactoryReset {
        /// Skip the prompt by giving the domain name being erased.
        #[arg(long)]
        confirm: Option<String>,
    },
    /// Started by the Windows Service Control Manager.
    #[command(hide = true)]
    WindowsService,
}

#[derive(Subcommand)]
enum AdminCmd {
    /// List the Domain Admins in this directory.
    List,
    /// Remove an account's two-factor sign-in (lost phone). Signs out the
    /// account's sessions and writes an audit log entry.
    ResetMfa {
        /// Logon name, e.g. `admin`, `CORP\admin` or `admin@corp.example.com`.
        user: String,
    },
    /// Reset a Domain Admin's password when nobody can sign in to the Admin
    /// Center. Signs out the account's sessions and writes an audit log entry.
    ResetPassword {
        /// Logon name, e.g. `admin`, `CORP\admin` or `admin@corp.example.com`.
        user: String,
        /// Generate a random temporary password and print it once.
        #[arg(long, conflicts_with = "password_stdin")]
        generate: bool,
        /// Read the new password from standard input instead of prompting.
        #[arg(long)]
        password_stdin: bool,
        /// Also re-enable the account if it's disabled.
        #[arg(long)]
        enable: bool,
    },
}

fn default_data_dir() -> &'static str {
    if cfg!(target_os = "windows") {
        "C:\\ProgramData\\OpenDirectory\\server"
    } else if cfg!(target_os = "macos") {
        "/Library/Application Support/OpenDirectory/server"
    } else {
        "/var/lib/opendirectory"
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()))
        .init();
    let args = Args::parse();
    let config = od_server::ServerConfig {
        data_dir: args.data_dir.clone(),
        listen: args.listen,
        tls: !args.insecure_http,
        local_listen: None,
        public_host: args.public_host.clone(),
        web_ui: args.web_ui.clone().or_else(|| {
            std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("web"))).filter(|p| p.join("index.html").exists())
        }),
        tls_cert: args.tls_cert.clone().zip(args.tls_key.clone()),
        web_assets: None,
    };
    let svc_args = || od_server::service::ServiceArgs {
        data_dir: std::path::absolute(&args.data_dir).unwrap_or(args.data_dir.clone()),
        listen: args.listen.to_string(),
        public_host: args.public_host.clone(),
    };
    match args.cmd.as_ref().unwrap_or(&Cmd::Run) {
        Cmd::Run => tokio::runtime::Runtime::new()?.block_on(run(config)),
        Cmd::InstallService => {
            od_server::service::install(&svc_args())?;
            println!("{} installed and started. It starts automatically at boot.", od_server::service::DISPLAY_NAME);
            println!("Data directory: {}", args.data_dir.display());
            println!("Next: run `od init --server https://<this-host>:{}` to create your domain, or open the Admin Center and connect to this server.", args.listen.port());
            Ok(())
        }
        Cmd::UninstallService => {
            od_server::service::uninstall()?;
            println!("Service removed. The directory data in {} was kept.", args.data_dir.display());
            Ok(())
        }
        Cmd::ServiceStatus => {
            println!("{}", od_server::service::status());
            Ok(())
        }
        Cmd::Admin(AdminCmd::List) => od_server::recovery::list_admins(&args.data_dir),
        Cmd::Admin(AdminCmd::ResetMfa { user }) => od_server::recovery::reset_mfa(&args.data_dir, user),
        Cmd::Admin(AdminCmd::ResetPassword { user, generate, password_stdin, enable }) => {
            use od_server::recovery::{NewPassword, ResetOptions};
            let password = if *generate {
                NewPassword::Generate
            } else if *password_stdin {
                NewPassword::Stdin
            } else {
                NewPassword::Prompt
            };
            od_server::recovery::reset_password(
                &args.data_dir,
                &ResetOptions { username: user.clone(), password, enable: *enable, via: None },
            )
        }
        Cmd::FactoryReset { confirm } => factory_reset(&args.data_dir, args.listen, confirm.clone()),
        Cmd::WindowsService => {
            #[cfg(windows)]
            {
                od_server::service::win::run_dispatcher(config)
            }
            #[cfg(not(windows))]
            {
                let _ = config;
                anyhow::bail!("Only used by the Windows Service Control Manager")
            }
        }
    }
}

fn factory_reset(data_dir: &std::path::Path, listen: SocketAddr, confirm: Option<String>) -> anyhow::Result<()> {
    let Some(domain) = od_server::recovery::directory_domain(data_dir)? else {
        anyhow::bail!("No directory found in {}. Pass the Management Server's data directory with --data-dir.", data_dir.display());
    };
    if std::net::TcpListener::bind(listen).is_err() {
        anyhow::bail!(
            "Port {} is in use, so the Management Server looks like it's running. Stop it first (for example `sudo systemctl stop opendirectory-server`).",
            listen.port()
        );
    }
    let typed = match confirm {
        Some(c) => c,
        None => {
            println!("This erases the directory {domain} in {}: users, groups, OUs, GPOs, enrolled endpoints, scripts, keys, certificates and the audit log.", data_dir.display());
            println!("Enrolled endpoints stop being managed. Make a backup first if you might need any of it.");
            print!("Type the domain name to confirm: ");
            std::io::Write::flush(&mut std::io::stdout())?;
            let mut line = String::new();
            std::io::stdin().read_line(&mut line)?;
            line
        }
    };
    if typed.trim().to_lowercase() != domain {
        anyhow::bail!("That isn't {domain}. Nothing was erased.");
    }
    od_server::recovery::factory_reset(data_dir, None)?;
    println!("Factory reset complete. Start the Management Server and open the Admin Center to run the setup wizard.");
    Ok(())
}

async fn run(config: od_server::ServerConfig) -> anyhow::Result<()> {
    let web = config.web_ui.as_ref().is_some_and(|d| d.join("index.html").exists());
    let server = od_server::start(config).await?;
    println!("OpenDirectory Management Server {}", od_server::VERSION);
    println!("  Endpoint URL:     {}", server.info.endpoint_url);
    if let Some(fp) = &server.info.tls_fingerprint {
        println!("  TLS fingerprint:  {fp}");
    }
    if web {
        println!("  Web Admin Center: {}/", server.info.endpoint_url);
    }
    println!("  Data directory:   {}", server.info.data_dir);
    tokio::select! {
        _ = server.wait() => {}
        _ = tokio::signal::ctrl_c() => { println!("Shutting down"); }
    }
    Ok(())
}
