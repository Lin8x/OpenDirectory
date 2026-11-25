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
    /// Started by the Windows Service Control Manager.
    #[command(hide = true)]
    WindowsService,
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
