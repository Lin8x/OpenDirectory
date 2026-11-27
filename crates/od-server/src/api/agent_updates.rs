//! Endpoint Agent updates. The Management Server offers agents older than
//! itself the matching agent build, following a rollout ring (off, pilot
//! OUs only, or every endpoint). Builds come from this version's GitHub
//! release (or a mirror set with OD_AGENT_UPDATE_URL), or an admin uploads
//! them for networks without internet access. Agents download from the
//! Management Server over their pinned connection and check the SHA-256.
//!
//! The same cached builds are what new computers install: the enrollment
//! command on the Enrollment page downloads the agent from this Management
//! Server (`/agent/downloads/{platform}`, gated by the enrollment token) and checks
//! its SHA-256 before running it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::{Admin, EndpointCaller};
use crate::crypto::sha256_hex;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

pub const PLATFORMS: &[&str] = &["windows-x64", "macos-arm64", "macos-x64", "linux-x64"];
const DEFAULT_SOURCE: &str = "https://github.com/Lin8x/OpenDirectory/releases/download";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UpdatePolicy {
    /// "off", "pilot" (only endpoints in pilot OUs) or "all".
    pub mode: String,
    #[serde(default)]
    pub pilot_ou_ids: Vec<Uuid>,
}
impl Default for UpdatePolicy {
    fn default() -> Self {
        UpdatePolicy { mode: "all".into(), pilot_ou_ids: vec![] }
    }
}

pub fn policy(s: &AppState) -> UpdatePolicy {
    s.db.meta_get("agent_updates").ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

fn asset(platform: &str) -> String {
    format!("opendirectory-agent-{platform}{}", if platform.starts_with("windows") { ".exe" } else { "" })
}
fn cache_dir(s: &AppState) -> PathBuf {
    PathBuf::from(&s.info.data_dir).join("agent-updates").join(crate::VERSION)
}

/// The cached build for a platform: (path, sha256, size).
pub(crate) fn cached(s: &AppState, platform: &str) -> Option<(PathBuf, String, u64)> {
    let p = cache_dir(s).join(asset(platform));
    let sha = std::fs::read_to_string(p.with_extension("sha256")).ok()?;
    let size = std::fs::metadata(&p).ok()?.len();
    Some((p, sha.trim().to_string(), size))
}

fn store(s: &AppState, platform: &str, bytes: &[u8]) -> anyhow::Result<String> {
    anyhow::ensure!(bytes.len() > 100 * 1024, "That file is too small to be an Endpoint Agent build");
    let dir = cache_dir(s);
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(asset(platform));
    let tmp = p.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &p)?;
    let sha = sha256_hex(bytes);
    std::fs::write(p.with_extension("sha256"), &sha)?;
    Ok(sha)
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static S: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Download this version's agent build for a platform.
pub async fn fetch(s: &AppState, platform: &str) -> anyhow::Result<String> {
    let base = std::env::var("OD_AGENT_UPDATE_URL").unwrap_or_else(|_| DEFAULT_SOURCE.into());
    let url = format!("{}/v{}/{}", base.trim_end_matches('/'), crate::VERSION, asset(platform));
    let resp = reqwest::Client::builder().timeout(std::time::Duration::from_secs(600)).build()?.get(&url).send().await?;
    anyhow::ensure!(resp.status().is_success(), "{url} returned {}", resp.status());
    let bytes = resp.bytes().await?;
    let sha = store(s, platform, &bytes)?;
    tracing::info!("cached Endpoint Agent {} for {platform} ({} bytes)", crate::VERSION, bytes.len());
    Ok(sha)
}

fn fetch_in_background(s: &AppState, platform: &str) {
    if !in_flight().lock().unwrap_or_else(|e| e.into_inner()).insert(platform.to_string()) {
        return;
    }
    let (s, platform) = (s.clone(), platform.to_string());
    tokio::spawn(async move {
        if let Err(e) = fetch(&s, &platform).await {
            tracing::warn!("couldn't download the Endpoint Agent build for {platform}: {e:#}");
            // Try again later rather than on every check-in.
            tokio::time::sleep(std::time::Duration::from_secs(1800)).await;
        }
        in_flight().lock().unwrap_or_else(|e| e.into_inner()).remove(&platform);
    });
}

fn in_ou_tree(ou: Option<Uuid>, targets: &[Uuid], ou_parent: &HashMap<Uuid, Option<Uuid>>) -> bool {
    let mut cur = ou;
    let mut guard = 0;
    while let Some(o) = cur {
        if targets.contains(&o) || guard > 64 {
            return targets.contains(&o);
        }
        cur = ou_parent.get(&o).copied().flatten();
        guard += 1;
    }
    false
}

/// Decide whether to offer this agent an update.
pub fn offer(
    s: &AppState,
    agent_version: &str,
    platform: &str,
    ou: Option<Uuid>,
    ou_parent: &HashMap<Uuid, Option<Uuid>>,
) -> Option<od_core::protocol::AgentUpdate> {
    if !PLATFORMS.contains(&platform) || od_core::policy::version_at_least(agent_version, crate::VERSION) {
        return None;
    }
    let p = policy(s);
    let eligible = match p.mode.as_str() {
        "all" => true,
        "pilot" => in_ou_tree(ou, &p.pilot_ou_ids, ou_parent),
        _ => false,
    };
    if !eligible {
        return None;
    }
    match cached(s, platform) {
        Some((_, sha256, size)) => Some(od_core::protocol::AgentUpdate { version: crate::VERSION.into(), sha256, size }),
        None => {
            fetch_in_background(s, platform);
            None
        }
    }
}

/// Agents download the build here.
pub async fn download(
    State(s): State<AppState>,
    EndpointCaller(_auth): EndpointCaller,
    Path(platform): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let (path, sha, _) = cached(&s, &platform).ok_or_else(|| ApiError::not_found("Agent build"))?;
    let bytes = tokio::fs::read(path).await.map_err(anyhow::Error::from)?;
    Ok(([(header::CONTENT_TYPE, "application/octet-stream".to_string()), (header::ETAG, format!("\"{sha}\""))], bytes))
}

#[derive(Deserialize)]
pub struct EnrollDownload {
    token: String,
}

/// New computers download the agent here during enrollment. Needs a valid
/// enrollment token (it isn't used up; enrolling does that). When this
/// version's build isn't cached yet, the server fetches it once.
pub async fn enroll_download(
    State(s): State<AppState>,
    Path(platform): Path<String>,
    Query(q): Query<EnrollDownload>,
) -> ApiResult<impl IntoResponse> {
    if s.db.valid_token(q.token.trim())?.is_none() {
        return Err(ApiError::forbidden("That enrollment token is invalid, expired or used up"));
    }
    if !PLATFORMS.contains(&platform.as_str()) {
        return Err(ApiError::not_found("Agent platform"));
    }
    if cached(&s, &platform).is_none() {
        fetch(&s, &platform).await.map_err(|e| {
            tracing::warn!("couldn't fetch the Endpoint Agent build for {platform}: {e:#}");
            ApiError::new(
                StatusCode::NOT_FOUND,
                "This Management Server doesn't have the Endpoint Agent build for that platform yet. Get it in Settings > Endpoint Agent updates",
            )
        })?;
    }
    let (path, sha, _) = cached(&s, &platform).ok_or_else(|| ApiError::not_found("Agent build"))?;
    let bytes = tokio::fs::read(path).await.map_err(anyhow::Error::from)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", asset(&platform))),
            (header::ETAG, format!("\"{sha}\"")),
        ],
        bytes,
    ))
}

#[derive(Serialize)]
pub struct StatusView {
    policy: UpdatePolicy,
    version: &'static str,
    builds: Vec<serde_json::Value>,
    /// Endpoints per agent version.
    versions: Vec<(String, usize)>,
    outdated: usize,
    /// Agents too old to update themselves (before 0.12); update them once by hand.
    manual: usize,
}

pub async fn status(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<StatusView>> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let eps = s.db.list_endpoints()?;
    for e in &eps {
        *counts.entry(if e.agent_version.is_empty() { "unknown".into() } else { e.agent_version.clone() }).or_default() += 1;
    }
    let mut versions: Vec<(String, usize)> = counts.into_iter().collect();
    versions.sort_by_key(|v| std::cmp::Reverse(od_core::policy::parse_version(&v.0)));
    let outdated = eps.iter().filter(|e| !od_core::policy::version_at_least(&e.agent_version, crate::VERSION)).count();
    let manual = eps.iter().filter(|e| !od_core::policy::version_at_least(&e.agent_version, "0.12.0")).count();
    let builds = PLATFORMS
        .iter()
        .map(|p| match cached(&s, p) {
            Some((_, sha, size)) => serde_json::json!({ "platform": p, "cached": true, "sha256": sha, "size": size }),
            None => serde_json::json!({ "platform": p, "cached": false }),
        })
        .collect();
    Ok(Json(StatusView { policy: policy(&s), version: crate::VERSION, builds, versions, outdated, manual }))
}

pub async fn set_policy(State(s): State<AppState>, admin: Admin, Json(p): Json<UpdatePolicy>) -> ApiResult<Json<UpdatePolicy>> {
    if !["off", "pilot", "all"].contains(&p.mode.as_str()) {
        return Err(ApiError::bad_request("Mode must be off, pilot or all"));
    }
    if p.mode == "pilot" && p.pilot_ou_ids.is_empty() {
        return Err(ApiError::bad_request("Choose at least one pilot OU"));
    }
    s.db.meta_set("agent_updates", &serde_json::to_string(&p).map_err(anyhow::Error::from)?)?;
    s.db.audit(admin.name(), "agent_updates.policy", &p.mode, &format!("{} pilot OU(s)", p.pilot_ou_ids.len()));
    Ok(Json(p))
}

/// Download every platform's build now.
pub async fn fetch_all(State(s): State<AppState>, _a: Admin) -> Json<Vec<serde_json::Value>> {
    let mut out = vec![];
    for p in PLATFORMS {
        out.push(match fetch(&s, p).await {
            Ok(sha) => serde_json::json!({ "platform": p, "ok": true, "sha256": sha }),
            Err(e) => serde_json::json!({ "platform": p, "ok": false, "error": format!("{e:#}") }),
        });
    }
    Json(out)
}

/// Upload a build by hand (networks without internet access).
pub async fn upload(
    State(s): State<AppState>,
    admin: Admin,
    Path(platform): Path<String>,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    if !PLATFORMS.contains(&platform.as_str()) {
        return Err(ApiError::bad_request("Unknown platform"));
    }
    let sha = store(&s, &platform, &body).map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    s.db.audit(admin.name(), "agent_updates.upload", &platform, &format!("sha256 {sha}"));
    Ok((StatusCode::CREATED, Json(serde_json::json!({ "platform": platform, "sha256": sha }))))
}
