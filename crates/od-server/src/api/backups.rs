//! Configuration backup and restore: backup plans (destination, schedule,
//! retention, encryption), on-demand backups and restores.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use chrono::{DateTime, Datelike, Duration, Local, NaiveTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::required;
use crate::auth::Admin;
use crate::backup;
use crate::destinations::{Destination, RemoteFile, KEEP};
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Frequency {
    Manual,
    Hourly,
    Daily,
    Weekly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub frequency: Frequency,
    /// Local server time, "HH:MM", for daily and weekly.
    #[serde(default = "two_am")]
    pub time: String,
    /// 0 = Monday … 6 = Sunday, for weekly.
    #[serde(default)]
    pub weekday: u32,
}
fn two_am() -> String {
    "02:00".into()
}

impl Schedule {
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let t = NaiveTime::parse_from_str(&self.time, "%H:%M").unwrap_or(NaiveTime::from_hms_opt(2, 0, 0).expect("valid"));
        let local = after.with_timezone(&Local);
        match self.frequency {
            Frequency::Manual => None,
            Frequency::Hourly => Some(after + Duration::hours(1)),
            Frequency::Daily | Frequency::Weekly => {
                for d in 0..9 {
                    let day = local.date_naive() + Duration::days(d);
                    if self.frequency == Frequency::Weekly && day.weekday().num_days_from_monday() != self.weekday % 7 {
                        continue;
                    }
                    if let Some(at) = Local.from_local_datetime(&day.and_time(t)).earliest() {
                        let at = at.with_timezone(&Utc);
                        if at > after {
                            return Some(at);
                        }
                    }
                }
                None
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Retention {
    /// Keep this many most recent backups (0 = no limit).
    #[serde(default = "fourteen")]
    pub keep_last: u32,
    /// Delete backups older than this many days (0 = no limit).
    #[serde(default)]
    pub max_age_days: u32,
}
fn fourteen() -> u32 {
    14
}

/// A backup plan as stored (destination secrets included). Never sent to clients.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPlan {
    pub id: Uuid,
    pub name: String,
    pub destination: Destination,
    pub schedule: Schedule,
    pub retention: Retention,
    pub encrypt: bool,
    #[serde(default)]
    pub passphrase: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub last_ok: Option<bool>,
    #[serde(default)]
    pub last_message: String,
    #[serde(default)]
    pub next_run_at: Option<DateTime<Utc>>,
}

/// What the Admin Center sees.
#[derive(Serialize)]
pub struct PlanView {
    id: Uuid,
    name: String,
    kind: &'static str,
    destination: Destination,
    schedule: Schedule,
    retention: Retention,
    encrypt: bool,
    has_passphrase: bool,
    enabled: bool,
    created_at: DateTime<Utc>,
    last_run_at: Option<DateTime<Utc>>,
    last_ok: Option<bool>,
    last_message: String,
    next_run_at: Option<DateTime<Utc>>,
    warnings: Vec<String>,
}

impl From<&StoredPlan> for PlanView {
    fn from(p: &StoredPlan) -> Self {
        let mut warnings = p.destination.warnings();
        if !p.encrypt {
            warnings.push("Backups from this plan aren't encrypted. They contain the key that decrypts every user's password.".into());
        }
        PlanView {
            id: p.id,
            name: p.name.clone(),
            kind: p.destination.kind(),
            destination: p.destination.redacted(),
            schedule: p.schedule.clone(),
            retention: p.retention.clone(),
            encrypt: p.encrypt,
            has_passphrase: !p.passphrase.is_empty(),
            enabled: p.enabled,
            created_at: p.created_at,
            last_run_at: p.last_run_at,
            last_ok: p.last_ok,
            last_message: p.last_message.clone(),
            next_run_at: p.next_run_at,
            warnings,
        }
    }
}

fn load(s: &AppState, id: Uuid) -> ApiResult<StoredPlan> {
    let sealed = s.db.get_backup_plan(id)?.ok_or_else(|| ApiError::not_found("Backup plan"))?;
    Ok(serde_json::from_slice(&s.sealer.open(&sealed)?).map_err(anyhow::Error::from)?)
}
pub fn load_all(s: &AppState) -> anyhow::Result<Vec<StoredPlan>> {
    let mut v = vec![];
    for (_, sealed) in s.db.list_backup_plans()? {
        if let Ok(p) = s.sealer.open(&sealed).and_then(|b| Ok(serde_json::from_slice::<StoredPlan>(&b)?)) {
            v.push(p);
        }
    }
    v.sort_by_key(|p| p.name.to_lowercase());
    Ok(v)
}
fn save(s: &AppState, p: &StoredPlan) -> anyhow::Result<()> {
    s.db.upsert_backup_plan(p.id, &p.name, &s.sealer.seal(&serde_json::to_vec(p)?)?)
}

pub async fn list(State(s): State<AppState>, _a: Admin) -> ApiResult<Json<Vec<PlanView>>> {
    Ok(Json(load_all(&s)?.iter().map(PlanView::from).collect()))
}

#[derive(Deserialize)]
pub struct PlanInput {
    name: String,
    destination: Destination,
    schedule: Schedule,
    #[serde(default = "default_retention")]
    retention: Retention,
    #[serde(default = "yes")]
    encrypt: bool,
    /// Empty or the placeholder keeps the stored passphrase.
    #[serde(default)]
    passphrase: String,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}
fn default_retention() -> Retention {
    Retention { keep_last: 14, max_age_days: 0 }
}

fn apply_input(p: &mut StoredPlan, inp: PlanInput) -> ApiResult<()> {
    p.name = required("Name", &inp.name)?;
    let mut d = inp.destination;
    d.merge_secrets(&p.destination);
    d.validate().map_err(|e| ApiError::bad_request(e.to_string()))?;
    p.destination = d;
    if NaiveTime::parse_from_str(&inp.schedule.time, "%H:%M").is_err() {
        return Err(ApiError::bad_request("Enter the time as HH:MM, e.g. 02:00"));
    }
    p.schedule = inp.schedule;
    p.retention = inp.retention;
    p.encrypt = inp.encrypt;
    if !inp.passphrase.is_empty() && inp.passphrase != KEEP {
        if inp.passphrase.chars().count() < 12 {
            return Err(ApiError::bad_request("Use a backup passphrase of at least 12 characters"));
        }
        p.passphrase = inp.passphrase;
    }
    if p.encrypt && p.passphrase.is_empty() {
        return Err(ApiError::bad_request(
            "Set a passphrase for encrypted backups. Store it somewhere safe: without it a backup can't be restored.",
        ));
    }
    if matches!(p.destination, Destination::Ftp { tls: false, .. }) {
        // Allowed, but loudly flagged in the plan's warnings.
    }
    p.enabled = inp.enabled;
    p.next_run_at = if p.enabled { p.schedule.next_after(Utc::now()) } else { None };
    Ok(())
}

pub async fn create(State(s): State<AppState>, admin: Admin, Json(inp): Json<PlanInput>) -> ApiResult<(StatusCode, Json<PlanView>)> {
    let mut p = StoredPlan {
        id: Uuid::new_v4(),
        name: String::new(),
        destination: Destination::Local { path: String::new() },
        schedule: Schedule { frequency: Frequency::Manual, time: two_am(), weekday: 0 },
        retention: default_retention(),
        encrypt: true,
        passphrase: String::new(),
        enabled: true,
        created_at: Utc::now(),
        last_run_at: None,
        last_ok: None,
        last_message: String::new(),
        next_run_at: None,
    };
    apply_input(&mut p, inp)?;
    save(&s, &p)?;
    s.db.audit(admin.name(), "backup_plan.create", &p.name, p.destination.kind());
    Ok((StatusCode::CREATED, Json((&p).into())))
}

pub async fn update(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<PlanInput>,
) -> ApiResult<Json<PlanView>> {
    let mut p = load(&s, id)?;
    apply_input(&mut p, inp)?;
    save(&s, &p)?;
    s.db.audit(admin.name(), "backup_plan.update", &p.name, "");
    Ok(Json((&p).into()))
}

pub async fn remove(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<StatusCode> {
    let p = load(&s, id)?;
    s.db.delete_backup_plan(id)?;
    s.db.audit(admin.name(), "backup_plan.delete", &p.name, "Backups already made are kept at the destination");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize)]
pub struct RunResult {
    ok: bool,
    message: String,
    file: Option<String>,
    deleted: Vec<String>,
}

/// Make a backup for a plan, upload it and apply retention.
pub async fn run_plan(s: &AppState, mut p: StoredPlan, actor: &str) -> RunResult {
    let started = Utc::now();
    let data_dir = std::path::PathBuf::from(&s.info.data_dir);
    let name = backup::file_name(&s.db);
    let res: anyhow::Result<(usize, Vec<String>)> = async {
        let bytes = backup::create(&s.db, &data_dir, if p.encrypt { Some(p.passphrase.as_str()) } else { None })?;
        p.destination.upload(&name, &bytes).await?;
        let mut deleted = vec![];
        if p.retention.keep_last > 0 || p.retention.max_age_days > 0 {
            if let Ok(files) = p.destination.list().await {
                for f in retention_victims(&files, &p.retention, &name, started) {
                    if p.destination.delete(&f).await.is_ok() {
                        deleted.push(f);
                    }
                }
            }
        }
        Ok((bytes.len(), deleted))
    }
    .await;
    let (ok, message, deleted) = match res {
        Ok((size, deleted)) => (
            true,
            format!(
                "Saved {name} ({}) to {}{}",
                if size < 1_048_576 { format!("{} KB", size.div_ceil(1024)) } else { format!("{:.1} MB", size as f64 / 1_048_576.0) },
                p.destination.kind(),
                if deleted.is_empty() { String::new() } else { format!("; removed {} old backup(s)", deleted.len()) }
            ),
            deleted,
        ),
        Err(e) => (false, format!("{e:#}"), vec![]),
    };
    p.last_run_at = Some(started);
    p.last_ok = Some(ok);
    p.last_message = message.clone();
    p.next_run_at = if p.enabled { p.schedule.next_after(Utc::now()) } else { None };
    // Keep anything learned during the run (e.g. a pinned SFTP host key).
    if let Ok(current) = load(s, p.id) {
        p.name = current.name;
        p.enabled = current.enabled;
    }
    let _ = save(s, &p);
    s.db.audit(actor, if ok { "backup.success" } else { "backup.failed" }, &p.name, &message);
    RunResult { ok, message, file: ok.then_some(name), deleted }
}

/// Which files retention removes. The backup just made is never removed.
pub fn retention_victims(files: &[RemoteFile], r: &Retention, just_made: &str, now: DateTime<Utc>) -> Vec<String> {
    let mut sorted: Vec<&RemoteFile> =
        files.iter().filter(|f| crate::destinations::is_backup(&f.name) && !f.name.contains("connection-test")).collect();
    // Names embed the timestamp, so name order is age order.
    sorted.sort_by(|a, b| b.name.cmp(&a.name));
    let mut out = vec![];
    for (i, f) in sorted.iter().enumerate() {
        if f.name == just_made {
            continue;
        }
        let too_many = r.keep_last > 0 && i >= r.keep_last as usize;
        let too_old = r.max_age_days > 0 && f.modified.map(|m| now - m > Duration::days(r.max_age_days as i64)).unwrap_or(false);
        if too_many || too_old {
            out.push(f.name.clone());
        }
    }
    out
}

pub async fn run_now(State(s): State<AppState>, admin: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<RunResult>> {
    let p = load(&s, id)?;
    Ok(Json(run_plan(&s, p, admin.name()).await))
}

#[derive(Deserialize)]
pub struct TestInput {
    /// Test an unsaved destination; omit with a plan id to test the saved one.
    destination: Option<Destination>,
}

#[derive(Serialize)]
pub struct TestResult {
    ok: bool,
    message: String,
}

async fn test_dest(mut d: Destination) -> (TestResult, Destination) {
    if let Err(e) = d.validate() {
        return (TestResult { ok: false, message: e.to_string() }, d);
    }
    let r = match d.test().await {
        Ok(m) => TestResult { ok: true, message: m },
        Err(e) => TestResult { ok: false, message: format!("{e:#}") },
    };
    (r, d)
}

pub async fn test_new(_a: Admin, Json(inp): Json<TestInput>) -> ApiResult<Json<TestResult>> {
    let d = inp.destination.ok_or_else(|| ApiError::bad_request("destination is required"))?;
    Ok(Json(test_dest(d).await.0))
}

pub async fn test_plan(
    State(s): State<AppState>,
    _a: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<TestInput>,
) -> ApiResult<Json<TestResult>> {
    let mut p = load(&s, id)?;
    let d = match inp.destination {
        Some(mut d) => {
            d.merge_secrets(&p.destination);
            d
        }
        None => p.destination.clone(),
    };
    let (r, d) = test_dest(d).await;
    if r.ok && inp_is_saved(&d, &p.destination) {
        p.destination = d; // remember a newly pinned host key
        save(&s, &p)?;
    }
    Ok(Json(r))
}

fn inp_is_saved(a: &Destination, b: &Destination) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

pub async fn files(State(s): State<AppState>, _a: Admin, Path(id): Path<Uuid>) -> ApiResult<Json<Vec<RemoteFile>>> {
    let mut p = load(&s, id)?;
    Ok(Json(p.destination.list().await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?))
}

#[derive(Deserialize)]
pub struct RestoreInput {
    /// A file at the plan's destination…
    name: Option<String>,
    /// …or an uploaded file, base64.
    file: Option<String>,
    #[serde(default)]
    passphrase: Option<String>,
    /// Only check that it decrypts and is intact.
    #[serde(default)]
    verify_only: bool,
}

#[derive(Serialize)]
pub struct RestoreResult {
    verified: backup::Verified,
    staged: bool,
    message: String,
}

async fn restore_common(
    s: &AppState,
    admin: &Admin,
    bytes: Vec<u8>,
    inp: &RestoreInput,
    plan_pass: Option<&str>,
) -> ApiResult<Json<RestoreResult>> {
    let pass = inp.passphrase.as_deref().filter(|p| !p.is_empty()).or(plan_pass);
    let data_dir = std::path::PathBuf::from(&s.info.data_dir);
    if inp.verify_only {
        let v = backup::verify(&bytes, pass).map_err(|e| ApiError::bad_request(e.to_string()))?;
        return Ok(Json(RestoreResult { verified: v, staged: false, message: "The backup decrypts and its database is intact.".into() }));
    }
    let v = backup::stage_restore(&data_dir, &bytes, pass).map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.db.audit(
        admin.name(),
        "backup.restore_staged",
        v.header.domain.as_deref().unwrap_or(""),
        &format!("Backup from {}", v.header.created_at),
    );
    Ok(Json(RestoreResult {
        verified: v,
        staged: true,
        message: "The restore is ready. Restart the Management Server to apply it; the current data is kept in a pre-restore folder."
            .into(),
    }))
}

pub async fn restore_from_plan(
    State(s): State<AppState>,
    admin: Admin,
    Path(id): Path<Uuid>,
    Json(inp): Json<RestoreInput>,
) -> ApiResult<Json<RestoreResult>> {
    let mut p = load(&s, id)?;
    let name = inp.name.clone().ok_or_else(|| ApiError::bad_request("Choose a backup"))?;
    let bytes = p.destination.download(&name).await.map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let pass = p.passphrase.clone();
    restore_common(&s, &admin, bytes, &inp, if pass.is_empty() { None } else { Some(pass.as_str()) }).await
}

pub async fn restore_upload(State(s): State<AppState>, admin: Admin, Json(inp): Json<RestoreInput>) -> ApiResult<Json<RestoreResult>> {
    let b64 = inp.file.clone().ok_or_else(|| ApiError::bad_request("Choose a backup file"))?;
    let bytes =
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| ApiError::bad_request("The file couldn't be read"))?;
    restore_common(&s, &admin, bytes, &inp, None).await
}

#[derive(Deserialize)]
pub struct DownloadInput {
    #[serde(default)]
    passphrase: Option<String>,
}

#[derive(Serialize)]
pub struct DownloadResult {
    file_name: String,
    /// base64
    data: String,
    encrypted: bool,
}

/// Make a backup and return it to the Admin Center to save locally.
pub async fn download_now(State(s): State<AppState>, admin: Admin, Json(inp): Json<DownloadInput>) -> ApiResult<Json<DownloadResult>> {
    let pass = inp.passphrase.filter(|p| !p.is_empty());
    let bytes =
        backup::create(&s.db, std::path::Path::new(&s.info.data_dir), pass.as_deref()).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let file_name = backup::file_name(&s.db);
    s.db.audit(admin.name(), "backup.download", &file_name, if pass.is_some() { "encrypted" } else { "NOT encrypted" });
    Ok(Json(DownloadResult { file_name, data: base64::engine::general_purpose::STANDARD.encode(bytes), encrypted: pass.is_some() }))
}

/// Run plans whose time has come. Called every minute by the server.
pub async fn run_due(s: &AppState) {
    let now = Utc::now();
    let Ok(plans) = load_all(s) else { return };
    for p in plans {
        if p.enabled && p.next_run_at.map(|t| t <= now).unwrap_or(false) {
            let r = run_plan(s, p, "Scheduled backup").await;
            if !r.ok {
                tracing::warn!("scheduled backup failed: {}", r.message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedule_next() {
        let base = Utc.with_ymd_and_hms(2026, 11, 25, 12, 0, 0).unwrap();
        let s = Schedule { frequency: Frequency::Hourly, time: two_am(), weekday: 0 };
        assert_eq!(s.next_after(base), Some(base + Duration::hours(1)));
        let d = Schedule { frequency: Frequency::Daily, time: "02:00".into(), weekday: 0 };
        let n = d.next_after(base).unwrap();
        assert!(n > base && n - base <= Duration::hours(24));
        let w = Schedule { frequency: Frequency::Weekly, time: "02:00".into(), weekday: 6 };
        let n = w.next_after(base).unwrap();
        assert_eq!(n.with_timezone(&Local).weekday().num_days_from_monday(), 6);
        assert!(Schedule { frequency: Frequency::Manual, time: two_am(), weekday: 0 }.next_after(base).is_none());
    }

    #[test]
    fn retention_keeps_newest() {
        let now = Utc::now();
        let f = |n: &str, days: i64| RemoteFile { name: n.into(), size: 1, modified: Some(now - Duration::days(days)) };
        let files = vec![
            f("opendirectory-x-20261101-000000.odbackup", 24),
            f("opendirectory-x-20261120-000000.odbackup", 5),
            f("opendirectory-x-20261124-000000.odbackup", 1),
            f("opendirectory-x-20261125-000000.odbackup", 0),
            f("notes.txt", 100),
        ];
        let v = retention_victims(&files, &Retention { keep_last: 2, max_age_days: 0 }, "opendirectory-x-20261125-000000.odbackup", now);
        assert_eq!(v, vec!["opendirectory-x-20261120-000000.odbackup", "opendirectory-x-20261101-000000.odbackup"]);
        let v = retention_victims(&files, &Retention { keep_last: 0, max_age_days: 7 }, "x", now);
        assert_eq!(v, vec!["opendirectory-x-20261101-000000.odbackup"]);
    }
}
