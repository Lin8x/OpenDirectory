//! End-user experience settings: the managed-device notice (with recorded
//! acknowledgement), consent before remote sessions, and which remote tools
//! this device accepts.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use od_core::policy::{SessionConsent, Setting};
use od_core::{ComplianceItem, ComplianceState};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use crate::desktop;

/// Remote session rules from policy. With no policy, sessions start without
/// a prompt, as in earlier versions.
#[derive(Debug, Clone, Copy)]
pub struct SessionRules {
    pub consent: SessionConsent,
    pub unattended: bool,
    pub shell: bool,
    pub desktop: bool,
}

impl Default for SessionRules {
    fn default() -> Self {
        SessionRules { consent: SessionConsent::Off, unattended: true, shell: true, desktop: true }
    }
}

impl SessionRules {
    pub fn from_settings(settings: &[Setting]) -> SessionRules {
        let mut r = SessionRules::default();
        for s in settings {
            match s {
                Setting::RemoteSessionConsent { mode, unattended_access } => {
                    r.consent = SessionConsent::parse(mode);
                    r.unattended = *unattended_access;
                }
                Setting::RemoteTools { remote_shell, remote_desktop } => {
                    r.shell = *remote_shell;
                    r.desktop = *remote_desktop;
                }
                _ => {}
            }
        }
        r
    }

    /// Decide whether a remote session may start. `Err` carries the reason
    /// shown to the admin.
    pub async fn check(&self, desktop_session: bool, admin_label: &str) -> Result<(), String> {
        let what = if desktop_session { "remote desktop" } else { "remote shell" };
        if desktop_session && !self.desktop || !desktop_session && !self.shell {
            return Err(format!("Remote {} is turned off on this device by policy.", if desktop_session { "desktop" } else { "shell" }));
        }
        if self.consent == SessionConsent::Off {
            return Ok(());
        }
        let user = desktop::console_user().await;
        if user.is_none() {
            return if self.unattended {
                Ok(())
            } else {
                Err("Nobody is signed in to approve the session, and unattended access is off.".into())
            };
        }
        match self.consent {
            SessionConsent::Notify => {
                desktop::notify("Remote session started", &format!("{admin_label} from IT started a {what} session on this device.")).await;
                Ok(())
            }
            SessionConsent::Ask => {
                let q = format!("{admin_label} from IT wants to start a {what} session on this device. Allow it?");
                match desktop::ask("Allow remote session?", &q, "Allow", "Deny", Duration::from_secs(60)).await {
                    Some(true) => Ok(()),
                    Some(false) => Err("The user declined the remote session.".into()),
                    None => Err("The user didn't answer the remote session prompt within 60 seconds.".into()),
                }
            }
            SessionConsent::Off => Ok(()),
        }
    }
}

/// Who acknowledged which revision of the notice, and when.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NoticeAck {
    pub revision: String,
    pub at: chrono::DateTime<chrono::Utc>,
    /// false when the notice was only shown (no acknowledgement asked).
    #[serde(default)]
    pub acknowledged: bool,
}

/// Shows the managed-device notice to each new console user.
pub struct NoticeWatcher {
    last_check: Option<Instant>,
    asking: Option<(String, String)>,
    /// Users who pressed "Not now": asked again at their next sign-in, or
    /// after 8 hours.
    snoozed: BTreeMap<String, (String, Instant)>,
    session_of: BTreeMap<String, String>,
    tx: mpsc::UnboundedSender<(String, String, bool)>,
    rx: mpsc::UnboundedReceiver<(String, String, bool)>,
}

impl Default for NoticeWatcher {
    fn default() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        NoticeWatcher { last_check: None, asking: None, snoozed: BTreeMap::new(), session_of: BTreeMap::new(), tx, rx }
    }
}

fn revision(title: &str, text: &str) -> String {
    hex::encode(&Sha256::digest(format!("{title}\n{text}").as_bytes())[..8])
}

impl NoticeWatcher {
    /// Show the notice where needed and report compliance. Returns the
    /// compliance item and whether `acks` changed.
    pub async fn maintain(&mut self, settings: &[Setting], acks: &mut BTreeMap<String, NoticeAck>) -> (Option<ComplianceItem>, bool) {
        let mut changed = false;
        while let Ok((user, rev, ok)) = self.rx.try_recv() {
            self.asking = None;
            if !ok {
                let session = self.session_of.get(&user).cloned().unwrap_or_default();
                self.snoozed.insert(user.clone(), (session, Instant::now()));
            }
            if ok || !acks.get(&user).is_some_and(|a| a.revision == rev) {
                acks.insert(user, NoticeAck { revision: rev, at: chrono::Utc::now(), acknowledged: ok });
                changed = true;
            }
        }
        let Some((title, text, require)) = settings.iter().find_map(|s| match s {
            Setting::ConsentNotice { title, text, require_acknowledgement } => {
                Some((title.clone(), text.clone(), *require_acknowledgement))
            }
            _ => None,
        }) else {
            return (None, changed);
        };
        let rev = revision(&title, &text);
        if self.last_check.map(|t| t.elapsed() < Duration::from_secs(60)).unwrap_or(false) {
            return (Some(Self::report(&rev, require, acks, self.asking.as_ref())), changed);
        }
        self.last_check = Some(Instant::now());
        if let Some(u) = desktop::console_user().await {
            let done = acks.get(&u.name).is_some_and(|a| a.revision == rev && (a.acknowledged || !require));
            let snoozed =
                self.snoozed.get(&u.name).is_some_and(|(sess, at)| *sess == u.session && at.elapsed() < Duration::from_secs(8 * 3600));
            self.session_of.insert(u.name.clone(), u.session.clone());
            if !done && !snoozed && self.asking.is_none() {
                self.asking = Some((u.name.clone(), rev.clone()));
                let tx = self.tx.clone();
                let (name, r) = (u.name.clone(), rev.clone());
                tokio::spawn(async move {
                    let ok = if require {
                        desktop::acknowledge(&title, &text, "I understand", Duration::from_secs(600)).await == Some(true)
                    } else {
                        desktop::notify(&title, &text).await;
                        false
                    };
                    let _ = tx.send((name, r, ok));
                });
            }
        }
        (Some(Self::report(&rev, require, acks, self.asking.as_ref())), changed)
    }

    fn report(rev: &str, require: bool, acks: &BTreeMap<String, NoticeAck>, asking: Option<&(String, String)>) -> ComplianceItem {
        let current: Vec<(&String, &NoticeAck)> = acks.iter().filter(|(_, a)| a.revision == rev).collect();
        let fmt = |v: &[(&String, &NoticeAck)]| {
            v.iter().map(|(u, a)| format!("{u} ({})", a.at.format("%Y-%m-%d %H:%M UTC"))).collect::<Vec<_>>().join(", ")
        };
        let item = |state, message: String| ComplianceItem { setting: "consent_notice".into(), state, message };
        if require {
            let ok: Vec<_> = current.iter().copied().filter(|(_, a)| a.acknowledged).collect();
            if let Some((u, _)) = asking {
                return item(ComplianceState::NonCompliant, format!("Waiting for {u} to acknowledge the notice"));
            }
            if ok.is_empty() {
                item(ComplianceState::NonCompliant, "No one has acknowledged the current notice yet".into())
            } else {
                item(ComplianceState::Compliant, format!("Acknowledged by {}", fmt(&ok)))
            }
        } else if current.is_empty() {
            item(ComplianceState::Applied, "Shown at the next sign-in".into())
        } else {
            item(ComplianceState::Compliant, format!("Shown to {}", fmt(&current)))
        }
    }
}
