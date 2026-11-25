//! GPO scripts: startup, sign-in, scheduled and run-once scripts that come
//! from Group Policy rather than an on-demand run. Results are reported to
//! the Management Server and appear in the endpoint's script history.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use chrono::Utc;
use od_core::scripts::{PolicyScript, ScriptRunReport, ScriptTrigger};

use crate::state::ScriptRunState;
use crate::{inventory, jobs};

/// Notices new sign-ins between check-ins.
#[derive(Default)]
pub struct LogonWatcher {
    known: Option<BTreeSet<String>>,
    last_poll: Option<Instant>,
}

impl LogonWatcher {
    /// Users who signed in since the last poll. The first poll only records who
    /// is already signed in, so restarting the agent doesn't re-run scripts.
    async fn new_sign_ins(&mut self) -> Vec<String> {
        if self.last_poll.map(|t| t.elapsed() < Duration::from_secs(60)).unwrap_or(false) {
            return vec![];
        }
        self.last_poll = Some(Instant::now());
        let now: BTreeSet<String> = inventory::collect().await.logged_in_users.into_iter().collect();
        let fresh = match &self.known {
            Some(prev) => now.difference(prev).cloned().collect(),
            None => vec![],
        };
        self.known = Some(now);
        fresh
    }
}

fn key(s: &PolicyScript) -> String {
    let t = match &s.trigger {
        ScriptTrigger::Startup => "startup".to_string(),
        ScriptTrigger::Logon => "logon".to_string(),
        ScriptTrigger::Interval { minutes } => format!("every{minutes}"),
        ScriptTrigger::Once => "once".to_string(),
    };
    format!("{}:{t}", s.script_id)
}

fn boot_minute() -> i64 {
    (sysinfo::System::boot_time() / 60) as i64
}

/// Run every script that is due. Returns true if run state changed.
pub async fn run_due(
    scripts: &[PolicyScript],
    runs: &mut BTreeMap<String, ScriptRunState>,
    logons: &mut LogonWatcher,
    client: &od_client::Client,
) -> bool {
    let mut changed = false;
    let wanted: BTreeSet<String> = scripts.iter().map(key).collect();
    let before = runs.len();
    runs.retain(|k, _| wanted.contains(k));
    changed |= runs.len() != before;

    let sign_ins = if scripts.iter().any(|s| s.trigger == ScriptTrigger::Logon) { logons.new_sign_ins().await } else { vec![] };
    let boot = boot_minute();

    for s in scripts {
        let k = key(s);
        let st = runs.get(&k).cloned().unwrap_or_default();
        let mut users: Vec<Option<String>> = vec![];
        match &s.trigger {
            ScriptTrigger::Startup => {
                // Allow a minute of jitter in the reported boot time.
                if (st.boot - boot).abs() > 1 {
                    users.push(None);
                }
            }
            ScriptTrigger::Once => {
                if st.revision != s.revision {
                    users.push(None);
                }
            }
            ScriptTrigger::Interval { minutes } => {
                let due = st.last_run.map(|t| Utc::now() - t >= chrono::Duration::minutes((*minutes).max(5) as i64)).unwrap_or(true);
                if due || st.revision != s.revision {
                    users.push(None);
                }
            }
            ScriptTrigger::Logon => users.extend(sign_ins.iter().cloned().map(Some)),
        }
        for user in users {
            let mut env = s.env.clone();
            if let Some(u) = &user {
                env.insert("OD_LOGON_USER".into(), u.clone());
            }
            let started_at = Utc::now();
            let id = format!("gpo-{}", uuid::Uuid::new_v4());
            let r = match jobs::run_script(&id, &s.shell, &s.body, &env).await {
                Ok(r) => r,
                Err(e) => {
                    od_core::protocol::JobResult { exit_code: None, output: format!("Couldn't run the script: {e:#}"), success: false }
                }
            };
            let mut source = format!("{} ({})", s.source, s.trigger.describe());
            if let Some(u) = &user {
                source.push_str(&format!(", for {u}"));
            }
            let report = ScriptRunReport {
                script_id: s.script_id,
                trigger: s.trigger.clone(),
                source,
                shell: s.shell.clone(),
                body: s.body.clone(),
                exit_code: r.exit_code,
                output: r.output,
                success: r.success,
                started_at,
            };
            if let Err(e) = client.post::<serde_json::Value, _>("/agent/script-runs", &report).await {
                tracing::warn!("reporting GPO script {}: {e}", s.name);
            }
            runs.insert(k.clone(), ScriptRunState { revision: s.revision.clone(), last_run: Some(Utc::now()), boot });
            changed = true;
        }
    }
    changed
}
