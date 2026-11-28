//! Delegated administration (role-based access control), like AD's
//! Delegation of Control Wizard.
//!
//! A Domain Admin creates **delegations**. Each one grants a *trustee* (a user
//! or a security group) a set of *permissions* over a *scope* (specific
//! endpoints, OUs with or without their child OUs, or security groups), and
//! can expire, which makes it a time-limited (temporary) admin account.
//!
//! A delegated operator can sign in to the Admin Center, but every API answer
//! is filtered to their scope: endpoints, users, OUs and groups outside it
//! don't exist as far as they can tell (404, never 403, so names don't leak).
//! Everything that isn't explicitly delegable stays Domain Admins only.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use od_core::{Endpoint, Group, OrgUnit, Role, User};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

const META_KEY: &str = "delegations";

/// What a delegation allows inside its scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// See the endpoints, their inventory, compliance and job history.
    ViewEndpoints,
    /// Open a remote desktop session (VNC, RDP or the low-latency stream).
    RemoteDesktop,
    /// Background mode: a separate, hidden desktop session that the
    /// signed-in user doesn't see. Doesn't include seeing their screen.
    BackgroundSession,
    /// Open an interactive remote shell.
    RemoteShell,
    /// Refresh inventory, restart the agent, scan or install updates, message the user.
    EndpointActions,
    /// Run ad-hoc scripts and Script Library scripts on endpoints in scope.
    RunScripts,
    /// Read and rotate the LAPS password.
    ReadLaps,
    /// Chat with the signed-in user through Help Desk.
    HelpDesk,
    /// See user accounts in scope.
    ViewUsers,
    /// Reset passwords of accounts in scope (never Domain Admins).
    ResetPasswords,
    /// Start, stop, restart and set the startup type of services.
    ManageServices,
    /// Install, update or remove apps (winget, apt, dnf, brew and so on).
    ManageSoftware,
    /// End processes.
    ManageProcesses,
    /// Container and image actions, and reading container logs.
    ManageContainers,
}

impl Permission {
    pub const ALL: [Permission; 14] = [
        Permission::ViewEndpoints,
        Permission::RemoteDesktop,
        Permission::BackgroundSession,
        Permission::RemoteShell,
        Permission::EndpointActions,
        Permission::RunScripts,
        Permission::ReadLaps,
        Permission::HelpDesk,
        Permission::ViewUsers,
        Permission::ResetPasswords,
        Permission::ManageServices,
        Permission::ManageSoftware,
        Permission::ManageProcesses,
        Permission::ManageContainers,
    ];
    fn is_endpoint(self) -> bool {
        !matches!(self, Permission::ViewUsers | Permission::ResetPasswords)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrusteeKind {
    User,
    Group,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    Endpoint,
    Ou,
    Group,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeItem {
    pub kind: ScopeKind,
    pub id: Uuid,
    /// For OUs: also everything in child OUs (inheritance). Defaults to on.
    #[serde(default = "yes")]
    pub include_children: bool,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub trustee_kind: TrusteeKind,
    pub trustee_id: Uuid,
    pub scope: Vec<ScopeItem>,
    pub permissions: Vec<Permission>,
    /// Access ends at this time (temporary admin). `None` never expires.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

impl Delegation {
    pub fn active(&self, now: DateTime<Utc>) -> bool {
        self.enabled && !matches!(self.expires_at, Some(t) if t <= now)
    }
}

pub fn load(s: &AppState) -> Vec<Delegation> {
    s.db.meta_get(META_KEY).ok().flatten().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default()
}

pub fn save(s: &AppState, all: &[Delegation]) -> anyhow::Result<()> {
    s.db.meta_set(META_KEY, &serde_json::to_string(all)?)
}

/// The delegations that apply to a user right now.
pub fn for_user(s: &AppState, user: &User) -> Vec<Delegation> {
    let now = Utc::now();
    load(s)
        .into_iter()
        .filter(|d| d.active(now))
        .filter(|d| match d.trustee_kind {
            TrusteeKind::User => d.trustee_id == user.id,
            TrusteeKind::Group => user.group_ids.contains(&d.trustee_id),
        })
        .collect()
}

/// What a signed-in console account may do. Domain Admins can do everything.
#[derive(Clone)]
pub struct Access {
    pub domain_admin: bool,
    grants: Vec<Grant>,
}

/// One delegation, resolved to concrete ids.
#[derive(Clone)]
struct Grant {
    perms: HashSet<Permission>,
    endpoints: HashSet<Uuid>,
    /// OUs covered (child OUs already expanded where inherited).
    ous: HashSet<Uuid>,
    groups: HashSet<Uuid>,
    /// User members of the scoped groups.
    group_users: HashSet<Uuid>,
    /// Endpoint members of the scoped groups.
    group_endpoints: HashSet<Uuid>,
}

impl Access {
    pub fn full() -> Access {
        Access { domain_admin: true, grants: vec![] }
    }

    pub fn resolve(s: &AppState, user: &User) -> anyhow::Result<Access> {
        if user.role == Role::Admin {
            return Ok(Access::full());
        }
        let ds = for_user(s, user);
        if ds.is_empty() {
            return Ok(Access { domain_admin: false, grants: vec![] });
        }
        let ous = s.db.list_ous()?;
        let groups = s.db.list_groups()?;
        let grants = ds
            .iter()
            .map(|d| {
                let mut g = Grant {
                    perms: d.permissions.iter().copied().collect(),
                    endpoints: HashSet::new(),
                    ous: HashSet::new(),
                    groups: HashSet::new(),
                    group_users: HashSet::new(),
                    group_endpoints: HashSet::new(),
                };
                for item in &d.scope {
                    match item.kind {
                        ScopeKind::Endpoint => {
                            g.endpoints.insert(item.id);
                        }
                        ScopeKind::Ou => {
                            g.ous.insert(item.id);
                            if item.include_children {
                                g.ous.extend(descendants(&ous, item.id));
                            }
                        }
                        ScopeKind::Group => {
                            if let Some(gr) = groups.iter().find(|x| x.id == item.id) {
                                g.groups.insert(gr.id);
                                g.group_users.extend(gr.member_ids.iter().copied());
                                g.group_endpoints.extend(gr.endpoint_ids.iter().copied());
                            }
                        }
                    }
                }
                g
            })
            .collect();
        Ok(Access { domain_admin: false, grants })
    }

    /// Signed in, but nothing delegated: can't use the console.
    pub fn is_empty(&self) -> bool {
        !self.domain_admin && self.grants.is_empty()
    }

    pub fn endpoint(&self, e: &Endpoint, p: Permission) -> bool {
        self.domain_admin
            || self.grants.iter().any(|g| {
                g.perms.contains(&p)
                    && (g.endpoints.contains(&e.id) || g.group_endpoints.contains(&e.id) || e.ou_id.is_some_and(|o| g.ous.contains(&o)))
            })
    }

    /// A user account in scope. Domain Admins are never in a delegated scope
    /// (like AdminSDHolder protecting privileged accounts in AD).
    pub fn user(&self, u: &User, p: Permission) -> bool {
        if self.domain_admin {
            return true;
        }
        if u.role == Role::Admin {
            return false;
        }
        self.grants.iter().any(|g| g.perms.contains(&p) && (g.group_users.contains(&u.id) || u.ou_id.is_some_and(|o| g.ous.contains(&o))))
    }

    /// OUs the operator may see (to show names and the tree they manage).
    pub fn ou(&self, ou: &OrgUnit) -> bool {
        self.domain_admin || self.grants.iter().any(|g| g.ous.contains(&ou.id))
    }

    pub fn group(&self, gr: &Group) -> bool {
        self.domain_admin || self.grants.iter().any(|g| g.groups.contains(&gr.id))
    }

    /// Every permission held anywhere, for the UI to decide what to show.
    pub fn permissions(&self) -> Vec<Permission> {
        if self.domain_admin {
            return Permission::ALL.to_vec();
        }
        let mut v: Vec<Permission> = self.grants.iter().flat_map(|g| g.perms.iter().copied()).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn has_any(&self, p: Permission) -> bool {
        self.domain_admin || self.grants.iter().any(|g| g.perms.contains(&p))
    }

    /// Any endpoint-related permission implies seeing that endpoint.
    pub fn sees_endpoint(&self, e: &Endpoint) -> bool {
        self.domain_admin || Permission::ALL.iter().filter(|p| p.is_endpoint()).any(|p| self.endpoint(e, *p))
    }

    pub fn sees_user(&self, u: &User) -> bool {
        self.user(u, Permission::ViewUsers) || self.user(u, Permission::ResetPasswords)
    }
}

fn descendants(ous: &[OrgUnit], root: Uuid) -> Vec<Uuid> {
    let mut out = vec![];
    let mut stack = vec![root];
    while let Some(p) = stack.pop() {
        for o in ous.iter().filter(|o| o.parent_id == Some(p)) {
            if !out.contains(&o.id) && o.id != root {
                out.push(o.id);
                stack.push(o.id);
            }
        }
    }
    out
}
