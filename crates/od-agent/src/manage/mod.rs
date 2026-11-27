//! Endpoint management for the Admin Center: services, installed software,
//! processes and containers. Queries come in over the wake-up long poll and
//! are answered right away; actions arrive as jobs.

pub mod containers;
pub mod processes;
pub mod services;
pub mod software;

use std::sync::Mutex;

use od_core::manage::{Area, ManageAction, ManageQuery, QueryRequest, QueryResult};
use od_core::policy::{SessionConsent, Setting};
use od_core::protocol::JobResult;

/// The latest effective settings, from the check-in loop.
static SETTINGS: Mutex<Vec<Setting>> = Mutex::new(Vec::new());

pub fn set_settings(s: &[Setting]) {
    *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = s.to_vec();
}

fn settings() -> Vec<Setting> {
    SETTINGS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Is this management area turned on by the Management tools GPO setting?
/// With no such setting everything is allowed.
pub fn allowed(area: Area) -> bool {
    for s in settings() {
        if let Setting::ManagementTools { services, software, processes, containers } = s {
            return match area {
                Area::Services => services,
                Area::Software => software,
                Area::Processes => processes,
                Area::Containers => containers,
            };
        }
    }
    true
}

fn refused(area: Area) -> String {
    format!("{} is turned off on this device by the Management tools policy setting.", area.label())
}

pub fn spawn_query(req: QueryRequest, client: od_client::Client) {
    tokio::spawn(async move {
        let result = answer(&req.query).await;
        if let Err(e) = client.post::<serde_json::Value, _>(&format!("/agent/queries/{}", req.id), &result).await {
            tracing::warn!("answering query {}: {e}", req.id);
        }
    });
}

fn ok<T: serde::Serialize>(v: anyhow::Result<T>) -> QueryResult {
    match v {
        Ok(d) => QueryResult { ok: true, data: serde_json::to_value(d).unwrap_or_default(), error: String::new() },
        Err(e) => QueryResult { ok: false, data: serde_json::Value::Null, error: format!("{e:#}") },
    }
}

pub async fn answer(q: &ManageQuery) -> QueryResult {
    let Some(area) = q.area() else {
        return QueryResult { ok: false, error: "This query needs a newer Endpoint Agent".into(), ..Default::default() };
    };
    if !allowed(area) {
        return QueryResult { ok: false, error: refused(area), ..Default::default() };
    }
    match q {
        ManageQuery::Services => ok(services::list().await),
        ManageQuery::Software => ok(software::list().await),
        ManageQuery::Processes => ok(processes::list().await),
        ManageQuery::Containers => ok(containers::list().await),
        ManageQuery::ContainerLogs { id, tail } => ok(containers::logs(id, *tail).await),
        ManageQuery::Unknown => unreachable!(),
    }
}

/// Run a management action from a job.
pub async fn act(action: &ManageAction, requested_by: &str) -> JobResult {
    let Some(area) = action.area() else {
        return fail("This action needs a newer Endpoint Agent; update the agent");
    };
    if !allowed(area) {
        return fail(&refused(area));
    }
    if let Err(e) = action.validate() {
        return fail(&e);
    }
    let r = match action {
        ManageAction::Service { name, control } => services::control(name, *control).await,
        ManageAction::ServiceStartup { name, startup } => services::set_startup(name, *startup).await,
        ManageAction::Package { operation, name, manager } => software::package(*operation, name, manager).await,
        ManageAction::KillProcess { pid, tree, expect_name } => processes::kill(*pid, *tree, expect_name).await,
        ManageAction::Container { id, control } => containers::control(id, *control).await,
        ManageAction::PullImage { image } => containers::pull(image).await,
        ManageAction::RemoveImage { id } => containers::remove_image(id).await,
        ManageAction::Prune => containers::prune().await,
        ManageAction::Unknown => unreachable!(),
    };
    let result = match r {
        Ok(out) => JobResult { exit_code: Some(0), output: if out.trim().is_empty() { "Done".into() } else { out }, success: true },
        Err(e) => fail(&format!("{e:#}")),
    };
    if result.success {
        tell_user(action, requested_by).await;
    }
    result
}

fn fail(msg: &str) -> JobResult {
    JobResult { exit_code: None, output: msg.to_string(), success: false }
}

/// When remote session consent is "notify" or "ask", tell the signed-in user
/// about changes IT makes, like they're told about remote sessions.
async fn tell_user(action: &ManageAction, who: &str) {
    let mode = settings().into_iter().find_map(|s| match s {
        Setting::RemoteSessionConsent { mode, .. } => Some(SessionConsent::parse(&mode)),
        _ => None,
    });
    if matches!(mode, Some(SessionConsent::Notify | SessionConsent::Ask)) && crate::desktop::console_user().await.is_some() {
        let who = if who.is_empty() { "An administrator" } else { who };
        crate::desktop::notify("Change made by IT", &format!("{who} from IT: {}", action.summary())).await;
    }
}

/// Combine stdout and stderr of a finished command for the job output.
pub(crate) fn combined(o: &crate::util::Output) -> String {
    let mut s = o.stdout.trim_end().to_string();
    if !o.stderr.trim().is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(o.stderr.trim_end());
    }
    s
}

/// Run a command and fail with its message when it exits non-zero.
pub(crate) async fn must(program: &str, args: &[&str], timeout_secs: u64) -> anyhow::Result<String> {
    let o = crate::util::run_with(program, args, None, std::time::Duration::from_secs(timeout_secs)).await?;
    if o.ok() {
        Ok(combined(&o))
    } else {
        let msg = combined(&o);
        anyhow::bail!("{program} failed{}", if msg.is_empty() { String::new() } else { format!(":\n{msg}") })
    }
}
