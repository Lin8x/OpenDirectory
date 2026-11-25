//! OpenDirectory Admin Center: the desktop app for administrators.
//!
//! It either hosts the Management Server inside the app ("host the
//! directory on this computer") or connects to one elsewhere on the
//! network. All API traffic goes through the Rust side, which handles TLS
//! certificate pinning, so the web view never talks to the network itself.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use od_client::{Client, ServerTarget};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, State, WindowEvent};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SavedConnection {
    mode: Option<String>,
    url: Option<String>,
    fingerprint: Option<String>,
    listen_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Default)]
struct ConnectionState {
    mode: Option<String>,
    url: Option<String>,
    fingerprint: Option<String>,
    data_dir: Option<String>,
    endpoint_url: Option<String>,
    error: Option<String>,
}

#[derive(Default)]
struct Inner {
    state: ConnectionState,
    client: Option<Client>,
    server: Option<od_server::RunningServer>,
}

struct App {
    inner: Mutex<Inner>,
    config_path: PathBuf,
    data_dir: PathBuf,
}

impl App {
    fn save(&self, c: &SavedConnection) {
        if let Some(dir) = self.config_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.config_path, serde_json::to_vec_pretty(c).unwrap_or_default());
    }
    fn load(&self) -> SavedConnection {
        std::fs::read(&self.config_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
}

async fn start_local(app: &App, port: u16) -> Result<ConnectionState, String> {
    let mut inner = app.inner.lock().await;
    if let Some(old) = inner.server.take() {
        old.shutdown();
    }
    let cfg = od_server::ServerConfig {
        data_dir: app.data_dir.clone(),
        listen: format!("0.0.0.0:{port}").parse().map_err(|e| format!("{e}"))?,
        tls: true,
        local_listen: Some("127.0.0.1:0".parse().unwrap()),
        public_host: None,
    };
    let server = od_server::start(cfg).await.map_err(|e| {
        let msg = format!("{e:#}");
        if msg.contains("in use") || msg.contains("AddrInUse") {
            format!("Port {port} is already in use. Is another Management Server running on this computer?")
        } else {
            msg
        }
    })?;
    let local = server.local_url().ok_or("local listener missing")?;
    let client = Client::new(&ServerTarget { url: local.clone(), fingerprint: None }).map_err(|e| e.to_string())?;
    inner.state = ConnectionState {
        mode: Some("local".into()),
        url: Some(local),
        fingerprint: server.info.tls_fingerprint.clone(),
        data_dir: Some(server.info.data_dir.clone()),
        endpoint_url: Some(server.info.endpoint_url.clone()),
        error: None,
    };
    inner.client = Some(client);
    inner.server = Some(server);
    Ok(inner.state.clone())
}

async fn connect(app: &App, url: &str, fingerprint: Option<String>) -> Result<ConnectionState, String> {
    let target = ServerTarget { url: url.trim_end_matches('/').to_string(), fingerprint: fingerprint.filter(|f| !f.trim().is_empty()) };
    let client = Client::new(&target).map_err(|e| e.to_string())?;
    let mut inner = app.inner.lock().await;
    if let Some(old) = inner.server.take() {
        old.shutdown();
    }
    let error = client.request(od_client::Method::GET, "/health", None).await.err().map(|e| e.to_string());
    inner.state = ConnectionState {
        mode: Some("remote".into()),
        url: Some(target.url.clone()),
        fingerprint: target.fingerprint.clone(),
        data_dir: None,
        endpoint_url: Some(target.url.clone()),
        error,
    };
    inner.client = Some(client);
    Ok(inner.state.clone())
}

#[tauri::command]
async fn connection_status(app: State<'_, Arc<App>>) -> Result<ConnectionState, String> {
    Ok(app.inner.lock().await.state.clone())
}

#[tauri::command]
async fn host_locally(app: State<'_, Arc<App>>, listen_port: u16) -> Result<ConnectionState, String> {
    let st = start_local(&app, listen_port).await?;
    app.save(&SavedConnection { mode: Some("local".into()), listen_port: Some(listen_port), ..Default::default() });
    Ok(st)
}

#[tauri::command]
async fn connect_remote(app: State<'_, Arc<App>>, url: String, fingerprint: String) -> Result<ConnectionState, String> {
    let st = connect(&app, &url, Some(fingerprint.clone())).await?;
    if let Some(e) = &st.error {
        return Err(e.clone());
    }
    app.save(&SavedConnection { mode: Some("remote".into()), url: Some(url), fingerprint: Some(fingerprint), listen_port: None });
    Ok(st)
}

#[tauri::command]
async fn disconnect(app: State<'_, Arc<App>>) -> Result<ConnectionState, String> {
    let mut inner = app.inner.lock().await;
    if let Some(s) = inner.server.take() {
        s.shutdown();
    }
    *inner = Inner::default();
    app.save(&SavedConnection::default());
    Ok(inner.state.clone())
}

#[derive(Serialize)]
struct RawResponse {
    status: u16,
    body: Value,
}

/// Saves an export (e.g. the audit log as CSV) to the user's Downloads folder
/// and returns the full path. A numbered suffix avoids overwriting.
#[tauri::command]
fn save_export(handle: tauri::AppHandle, file_name: String, contents: String, base64: Option<bool>) -> Result<String, String> {
    let dir = handle.path().download_dir().or_else(|_| handle.path().document_dir()).map_err(|e| e.to_string())?;
    let safe: String = file_name.chars().map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' }).collect();
    let (stem, ext) = safe.rsplit_once('.').map(|(a, b)| (a.to_string(), format!(".{b}"))).unwrap_or((safe.clone(), String::new()));
    let mut path = dir.join(&safe);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}){ext}"));
        n += 1;
    }
    let bytes = if base64.unwrap_or(false) {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(contents.trim()).map_err(|e| e.to_string())?
    } else {
        contents.into_bytes()
    };
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
async fn api_request(
    app: State<'_, Arc<App>>,
    method: String,
    path: String,
    body: Option<Value>,
    token: Option<String>,
) -> Result<RawResponse, String> {
    if !path.starts_with('/') || path.contains("..") {
        return Err("invalid path".into());
    }
    let client = app.inner.lock().await.client.clone().ok_or("Not connected to a Management Server")?;
    let client = match token {
        Some(t) => client.with_session(&t),
        None => client,
    };
    let m = od_client::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    match client.request(m, &path, body.as_ref()).await {
        Ok(v) => Ok(RawResponse { status: 200, body: v }),
        Err(e) => match e.status {
            Some(s) => Ok(RawResponse { status: s.as_u16(), body: serde_json::json!({ "error": e.message }) }),
            None => Err(e.message),
        },
    }
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=warn".into()))
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // "Launch at startup": the OS login item starts us hidden in the tray.
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .setup(|app| {
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            let config_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?.join("server");
            let state = Arc::new(App { inner: Mutex::new(Inner::default()), config_path: config_dir.join("connection.json"), data_dir });
            app.manage(state.clone());

            // Reconnect to whatever was used last time.
            let saved = state.load();
            tauri::async_runtime::block_on(async {
                match saved.mode.as_deref() {
                    Some("local") => {
                        if let Err(e) = start_local(&state, saved.listen_port.unwrap_or(8443)).await {
                            let mut inner = state.inner.lock().await;
                            inner.state = ConnectionState { mode: Some("local".into()), error: Some(e), ..Default::default() };
                        }
                    }
                    Some("remote") => {
                        if let Some(url) = &saved.url {
                            let _ = connect(&state, url, saved.fingerprint.clone()).await;
                        }
                    }
                    _ => {}
                }
            });

            // Tray icon: when this computer hosts the directory, closing the
            // window keeps the Management Server running in the background.
            let open = MenuItem::with_id(app, "open", "Open Admin Center", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit (stops a locally hosted server)", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::new().tooltip("OpenDirectory").menu(&menu).on_menu_event(|app, ev| match ev.id().as_ref() {
                "open" => show_main(app),
                "quit" => app.exit(0),
                _ => {}
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let hosting = app
                    .try_state::<Arc<App>>()
                    .map(|s| tauri::async_runtime::block_on(async { s.inner.lock().await.server.is_some() }))
                    .unwrap_or(false);
                if hosting {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![connection_status, host_locally, connect_remote, disconnect, api_request, save_export])
        .run(tauri::generate_context!())
        .expect("error while running OpenDirectory Admin Center");
}
