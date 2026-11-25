use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;

/// OpenDirectory Management Server: directory, Group Policy, endpoint
/// management and the API used by the Admin Center, `od` CLI and Endpoint
/// Agents.
#[derive(Parser)]
#[command(name = "opendirectory-server", version)]
struct Args {
    /// Where the directory database, keys and certificates are stored.
    #[arg(long, env = "OD_DATA_DIR", default_value = default_data_dir())]
    data_dir: PathBuf,
    /// Address Endpoint Agents and remote admins connect to.
    #[arg(long, env = "OD_LISTEN", default_value = "0.0.0.0:8443")]
    listen: SocketAddr,
    /// Serve plain HTTP instead of TLS (only for development).
    #[arg(long, env = "OD_INSECURE_HTTP")]
    insecure_http: bool,
    /// Hostname or IP put in enrollment commands (detected if omitted).
    #[arg(long, env = "OD_PUBLIC_HOST")]
    public_host: Option<String>,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()))
        .init();
    let args = Args::parse();
    let server = od_server::start(od_server::ServerConfig {
        data_dir: args.data_dir,
        listen: args.listen,
        tls: !args.insecure_http,
        local_listen: None,
        public_host: args.public_host,
    })
    .await?;
    println!("OpenDirectory Management Server {}", od_server::VERSION);
    println!("  Endpoint URL:     {}", server.info.endpoint_url);
    if let Some(fp) = &server.info.tls_fingerprint {
        println!("  TLS fingerprint:  {fp}");
    }
    println!("  Data directory:   {}", server.info.data_dir);
    tokio::select! {
        _ = server.wait() => {}
        _ = tokio::signal::ctrl_c() => { println!("Shutting down"); }
    }
    Ok(())
}
