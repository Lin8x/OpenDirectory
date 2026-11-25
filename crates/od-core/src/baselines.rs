//! Security baselines and modifiers for onboarding: pick a starting point
//! (like Microsoft Security Baselines or CIS Benchmarks), layer modifiers on
//! top, and get a plan of OUs, groups and GPOs to create. Pure data, so the
//! Admin Center can preview exactly what will be created before anything is.

use serde::{Deserialize, Serialize};

use crate::policy::Setting;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Baseline {
    /// Nothing preconfigured.
    Custom,
    /// Directory, inventory and check-in only.
    Minimal,
    Home,
    #[default]
    SmallBusiness,
    Enterprise,
    Education,
    HighSecurity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLevel {
    Relaxed,
    Balanced,
    Strict,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EducationVariant {
    K12,
    HigherEd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    Automatic,
    /// Pilot ring gets updates at once, everyone else after a delay.
    Rings,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ownership {
    Corporate,
    Byod,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdminModel {
    Single,
    Delegated,
    /// Tier 0 (identity), Tier 1 (servers), Tier 2 (workstations).
    Tiered,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAccess {
    Off,
    /// Remote Desktop and SSH stay as the OS ships them.
    Unmanaged,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rollout {
    /// Apply to every endpoint right away.
    Everyone,
    /// Link the GPOs to a Pilot OU only; widen the links when you're happy.
    Pilot,
    /// Create the GPOs turned off, to review first.
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompliancePack {
    Hipaa,
    PciDss,
    Soc2,
    Cmmc,
    Gdpr,
    Ferpa,
    Cipa,
}

impl CompliancePack {
    pub fn label(self) -> &'static str {
        match self {
            CompliancePack::Hipaa => "HIPAA",
            CompliancePack::PciDss => "PCI DSS",
            CompliancePack::Soc2 => "SOC 2",
            CompliancePack::Cmmc => "CMMC",
            CompliancePack::Gdpr => "GDPR",
            CompliancePack::Ferpa => "FERPA",
            CompliancePack::Cipa => "CIPA",
        }
    }
}

/// Everything layered on top of a baseline. `None` means "use the baseline's default".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Modifiers {
    #[serde(default)]
    pub security_level: Option<SecurityLevel>,
    #[serde(default)]
    pub education: Option<EducationVariant>,
    #[serde(default)]
    pub updates: Option<UpdatePolicy>,
    #[serde(default)]
    pub ownership: Option<Ownership>,
    #[serde(default)]
    pub admin_model: Option<AdminModel>,
    /// Make sign-in accounts local administrators.
    #[serde(default)]
    pub local_admin: Option<bool>,
    #[serde(default)]
    pub remote_access: Option<RemoteAccess>,
    /// "allow", "read_only" or "block".
    #[serde(default)]
    pub usb: Option<String>,
    #[serde(default)]
    pub audit_retention_days: Option<u32>,
    #[serde(default)]
    pub compliance: Vec<CompliancePack>,
    /// Computer naming convention, e.g. `LAPTOP-%USER%-###`. Stored for reference.
    #[serde(default)]
    pub naming_convention: Option<String>,
    #[serde(default)]
    pub time_server: Option<String>,
    #[serde(default)]
    pub rollout: Option<Rollout>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineInfo {
    pub id: Baseline,
    pub name: &'static str,
    pub tagline: &'static str,
    pub description: &'static str,
    pub best_for: &'static str,
    pub default_level: Option<SecurityLevel>,
    /// What the baseline wants but OpenDirectory can't enforce yet.
    pub not_yet: Vec<&'static str>,
}

pub fn catalog() -> Vec<BaselineInfo> {
    use Baseline::*;
    use SecurityLevel::*;
    vec![
        BaselineInfo { id: Custom, name: "Custom (blank)", tagline: "Start empty", description: "Creates only the domain, the built-in Domain Admins and Domain Users groups, and your admin account. You build every OU and GPO yourself.", best_for: "Experienced admins with a plan", default_level: None, not_yet: vec![] },
        BaselineInfo { id: Minimal, name: "Minimal", tagline: "Directory and inventory only", description: "Users, groups, a Computers OU and hardware inventory. Nothing is enforced on endpoints.", best_for: "Evaluating OpenDirectory or tracking assets", default_level: None, not_yet: vec![] },
        BaselineInfo { id: Home, name: "Home / Homelab", tagline: "A few people and machines", description: "Automatic updates, firewall, a simple password policy and remote access to your own machines. You stay an administrator everywhere.", best_for: "Families, homelabs and tinkerers", default_level: Some(Relaxed), not_yet: vec![] },
        BaselineInfo { id: SmallBusiness, name: "Small Business", tagline: "The common office setup", description: "Staff, Workstations, Servers and IT OUs, sensible passwords and lockout, screen lock, firewall, disk encryption checks and automatic patching. Staff aren't local administrators.", best_for: "Offices of 5 to 250 people", default_level: Some(Balanced), not_yet: vec![] },
        BaselineInfo { id: Enterprise, name: "Enterprise", tagline: "Many IT staff, many sites", description: "Department and device OUs, a tiered admin model (Tier 0/1/2), patch rings with a pilot group, full security auditing and hardened network defaults.", best_for: "Larger organizations with an IT team", default_level: Some(Strict), not_yet: vec!["Rotating local admin passwords (LAPS)", "Delegated admin roles scoped to OUs", "MFA for the Admin Center"] },
        BaselineInfo { id: Education, name: "Education", tagline: "Schools, labs and campuses", description: "Students, Faculty, Staff and Labs OUs, locked-down lab computers and stricter student settings. The K-12 or Higher Ed variant adjusts strictness and BYOD.", best_for: "K-12 schools, colleges and universities", default_level: Some(Balanced), not_yet: vec!["Shared-device reset at sign-out", "App allow-lists", "Web filtering"] },
        BaselineInfo { id: HighSecurity, name: "High Security (hardened)", tagline: "Modeled on CIS Level 2 and DISA STIG", description: "Long complex passwords, aggressive lockout, a 5-minute screen lock, USB storage blocked, remote access off, full auditing and every hardening setting on. The most secure and the least convenient.", best_for: "Sensitive or regulated work", default_level: Some(Maximum), not_yet: vec!["Application allow-listing", "MFA everywhere", "Smart card sign-in"] },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedOu {
    pub name: String,
    pub description: String,
    /// Name of the parent OU in this plan.
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannedGroup {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedPolicy {
    pub name: String,
    pub description: String,
    pub priority: i32,
    pub enabled: bool,
    /// Empty with `all_endpoints` false means unlinked.
    pub all_endpoints: bool,
    /// OU names from this plan.
    pub ou_links: Vec<String>,
    pub settings: Vec<Setting>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NoteKind {
    Info,
    Warning,
    /// Something the baseline asks for that isn't built yet.
    NotYet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanNote {
    pub kind: NoteKind,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub baseline: Baseline,
    pub level: Option<SecurityLevel>,
    pub ous: Vec<PlannedOu>,
    pub groups: Vec<PlannedGroup>,
    pub policies: Vec<PlannedPolicy>,
    /// OU the first admin goes into.
    pub admin_ou: Option<String>,
    pub audit_retention_days: Option<u32>,
    pub naming_convention: Option<String>,
    /// Group whose members get sign-in accounts on endpoints (a "Sign-in Accounts" GPO).
    pub signin_group: Option<String>,
    pub signin_admin: bool,
    pub rollout: Rollout,
    pub notes: Vec<PlanNote>,
}

fn ou(name: &str, desc: &str, parent: Option<&str>) -> PlannedOu {
    PlannedOu { name: name.into(), description: desc.into(), parent: parent.map(str::to_string) }
}
fn group(name: &str, desc: &str) -> PlannedGroup {
    PlannedGroup { name: name.into(), description: desc.into() }
}

struct Levels {
    min_len: u32,
    lockout: u32,
    lock_minutes: u32,
    screen: u32,
    complexity: bool,
    history: u32,
}

fn levels(l: SecurityLevel) -> Levels {
    match l {
        SecurityLevel::Relaxed => Levels { min_len: 8, lockout: 10, lock_minutes: 5, screen: 20, complexity: false, history: 0 },
        SecurityLevel::Balanced => Levels { min_len: 12, lockout: 5, lock_minutes: 15, screen: 10, complexity: false, history: 5 },
        SecurityLevel::Strict => Levels { min_len: 14, lockout: 5, lock_minutes: 30, screen: 10, complexity: true, history: 12 },
        SecurityLevel::Maximum => Levels { min_len: 15, lockout: 3, lock_minutes: 60, screen: 5, complexity: true, history: 24 },
    }
}

/// What each security level means, for the slider's explanation.
pub fn level_summary(l: SecurityLevel) -> String {
    let v = levels(l);
    format!(
        "{}-character passwords{}, lock after {} wrong tries for {} min, screen lock after {} min idle{}",
        v.min_len,
        if v.complexity { " with mixed character types" } else { "" },
        v.lockout,
        v.lock_minutes,
        v.screen,
        if v.history > 0 { format!(", {} old passwords remembered", v.history) } else { String::new() }
    )
}

/// Turn a baseline and modifiers into a concrete plan.
pub fn plan(b: Baseline, m: &Modifiers) -> Plan {
    use Baseline::*;
    let info = catalog().into_iter().find(|i| i.id == b).expect("every baseline is in the catalog");
    let mut notes: Vec<PlanNote> = info
        .not_yet
        .iter()
        .map(|t| PlanNote {
            kind: NoteKind::NotYet,
            text: format!("{t} isn't available yet; it will be added to this baseline when it ships."),
        })
        .collect();
    let mut note = |kind: NoteKind, text: String| notes.push(PlanNote { kind, text });

    let mut level = m.security_level.or(info.default_level);
    let compliance = !m.compliance.is_empty();
    if compliance
        && level.map(|l| l < SecurityLevel::Strict).unwrap_or(true)
        && matches!(b, SmallBusiness | Enterprise | Education | HighSecurity)
    {
        note(
            NoteKind::Warning,
            format!("{} usually expects Strict security or higher; the level was raised to Strict.", packs(&m.compliance)),
        );
        level = Some(SecurityLevel::Strict);
    }
    if let (Some(chosen), Some(def)) = (m.security_level, info.default_level) {
        if chosen < def {
            note(
                NoteKind::Warning,
                format!("You lowered the security level below this baseline's default ({def:?}). Settings follow your choice."),
            );
        }
    }

    let edu = m.education.unwrap_or(EducationVariant::K12);
    let ownership = m.ownership.unwrap_or(if b == Home { Ownership::Byod } else { Ownership::Corporate });
    let admin_model = m.admin_model.unwrap_or(match b {
        Enterprise | HighSecurity => AdminModel::Tiered,
        Education => AdminModel::Delegated,
        _ => AdminModel::Single,
    });

    // ---------------------------------------------------------------- structure
    let mut ous = vec![];
    let mut groups = vec![];
    let mut admin_ou = None;
    match b {
        Custom => {}
        Minimal => ous.push(ou("Computers", "Enrolled endpoints", None)),
        Home => {
            ous.push(ou("Family", "People in the household", None));
            ous.push(ou("Devices", "Home computers", None));
            ous.push(ou("Lab", "Homelab servers and VMs", None));
        }
        SmallBusiness | HighSecurity => {
            ous.push(ou("Staff", "People in the organization", None));
            ous.push(ou("Workstations", "Desktops and laptops", None));
            ous.push(ou("Servers", "Servers and infrastructure", None));
            ous.push(ou("IT", "IT administrators", None));
            admin_ou = Some("IT".to_string());
        }
        Enterprise => {
            ous.push(ou("Departments", "People, by department", None));
            for d in ["Finance", "HR", "Sales", "Engineering", "Operations"] {
                ous.push(ou(d, &format!("{d} department"), Some("Departments")));
            }
            ous.push(ou("Workstations", "Desktops and laptops", None));
            ous.push(ou("Laptops", "Portable computers", Some("Workstations")));
            ous.push(ou("Desktops", "Fixed computers", Some("Workstations")));
            ous.push(ou("Servers", "Servers and infrastructure", None));
            ous.push(ou("Admin", "Administrative accounts, by tier", None));
            admin_ou = Some("Admin".to_string());
        }
        Education => {
            ous.push(ou("Students", "Student accounts", None));
            ous.push(ou("Faculty", "Teachers and lecturers", None));
            ous.push(ou("Staff", "School staff", None));
            ous.push(ou("Labs", "Shared lab and classroom computers", None));
            ous.push(ou("Staff Devices", "Computers assigned to faculty and staff", None));
            if edu == EducationVariant::HigherEd {
                ous.push(ou("Research", "Research groups and their machines", None));
            }
            ous.push(ou("IT", "IT administrators", None));
            admin_ou = Some("IT".to_string());
        }
    }
    if b != Custom && b != Minimal && ownership != Ownership::Corporate {
        ous.push(ou("BYOD", "Personally owned devices with light-touch management", None));
    }
    match admin_model {
        AdminModel::Single => {}
        AdminModel::Delegated => {
            groups.push(group("Help Desk", "Resets passwords and runs support scripts"));
            groups.push(group("Workstation Admins", "Local administrators on workstations"));
        }
        AdminModel::Tiered => {
            if b != Custom && b != Minimal {
                for (n, p) in [("Tier 0", "Admin"), ("Tier 1", "Admin"), ("Tier 2", "Admin")] {
                    if ous.iter().any(|o| o.name == p) {
                        ous.push(ou(n, &format!("{n} administrative accounts"), Some(p)));
                    }
                }
            }
            groups.push(group("Tier 0 Admins", "Identity: the directory and Management Server"));
            groups.push(group("Tier 1 Admins", "Servers and applications"));
            groups.push(group("Tier 2 Admins", "Workstations and help desk"));
            note(NoteKind::Info, "Tier groups are created now. Scoping each tier to its OUs arrives with delegated administration.".into());
        }
    }
    match b {
        Education => {
            groups.push(group("Students", "Every student"));
            groups.push(group("Faculty", "Every teacher or lecturer"));
        }
        SmallBusiness | Enterprise | HighSecurity => groups.push(group("Staff", "Everyone who signs in to company computers")),
        Home => groups.push(group("Family", "Everyone in the household")),
        _ => {}
    }

    // ---------------------------------------------------------------- policies
    let mut policies = vec![];
    let updates = m.updates.unwrap_or(match b {
        Enterprise | HighSecurity => UpdatePolicy::Rings,
        Custom | Minimal => UpdatePolicy::Manual,
        _ => UpdatePolicy::Automatic,
    });
    let usb = m.usb.clone().unwrap_or_else(|| match b {
        HighSecurity => "block".into(),
        Education | Enterprise => "read_only".into(),
        _ => "allow".into(),
    });
    let remote = m.remote_access.unwrap_or(match b {
        HighSecurity => RemoteAccess::Off,
        _ => RemoteAccess::Unmanaged,
    });

    if let Some(lv) = level {
        let v = levels(lv);
        let mut s = vec![
            Setting::PasswordMinLength { length: v.min_len },
            Setting::AccountLockout { threshold: v.lockout, duration_minutes: v.lock_minutes },
            Setting::ScreenLock { minutes: v.screen },
            Setting::Firewall { enabled: true },
            Setting::GuestAccount { enabled: false },
            Setting::AutoLogin { allowed: false },
        ];
        if v.complexity {
            s.push(Setting::PasswordComplexity { required: true });
        }
        if v.history > 0 {
            s.push(Setting::PasswordHistory { count: v.history });
        }
        if m.compliance.contains(&CompliancePack::PciDss) {
            s.push(Setting::PasswordMaxAge { days: 90 });
            note(NoteKind::Info, "PCI DSS: passwords expire every 90 days, because MFA isn't available for endpoint sign-in yet.".into());
        }
        if b != Home {
            s.push(Setting::DiskEncryption { required: true });
            s.push(Setting::Antivirus { required: true });
        }
        if lv >= SecurityLevel::Balanced {
            s.push(Setting::AppReputation { enabled: true });
            s.push(Setting::DisableAutorun { enabled: true });
            s.push(Setting::Smb1 { enabled: false });
        }
        if lv >= SecurityLevel::Strict || compliance {
            s.push(Setting::SecurityAuditing { enabled: true });
            s.push(Setting::HideLastUsername { enabled: true });
            s.push(Setting::Llmnr { enabled: false });
        }
        if updates != UpdatePolicy::Manual {
            s.push(Setting::AutomaticUpdates { enabled: true });
        }
        if usb != "allow" {
            s.push(Setting::UsbStorage { mode: usb.clone() });
        }
        match remote {
            RemoteAccess::Off => {
                s.push(Setting::RemoteDesktop { enabled: false });
                s.push(Setting::SshServer { enabled: false });
            }
            RemoteAccess::On => s.push(Setting::RemoteDesktop { enabled: true }),
            RemoteAccess::Unmanaged => {}
        }
        if let Some(ts) = m.time_server.clone().filter(|t| !t.trim().is_empty()) {
            s.push(Setting::TimeServer { server: ts.trim().to_string() });
        }
        if lv == SecurityLevel::Maximum {
            s.push(Setting::LoginBanner {
                title: "Authorized use only".into(),
                text: "This computer is for authorized users. Activity may be monitored and recorded.".into(),
            });
        }
        policies.push(PlannedPolicy {
            name: "Default Endpoint Policy".into(),
            description: format!(
                "{} baseline at {:?} security. Applies to every endpoint, like the Default Domain Policy in AD.",
                info.name, lv
            ),
            priority: 1000,
            enabled: true,
            all_endpoints: true,
            ou_links: vec![],
            settings: s,
        });
    }

    // Sign-in accounts: who can sign in, and whether they're local admins.
    let local_admin = m.local_admin.unwrap_or(b == Home);
    let signin_group = match b {
        Home => Some("Family"),
        SmallBusiness | Enterprise | HighSecurity => Some("Staff"),
        _ => None,
    };
    if let Some(g) = signin_group {
        note(
            NoteKind::Info,
            format!(
                "A \"Sign-in Accounts\" GPO lets members of {g} sign in to managed computers{}. Add people to {g} to give them access.",
                if local_admin { " as local administrators" } else { "" }
            ),
        );
    }

    if updates == UpdatePolicy::Rings && b != Custom && b != Minimal {
        ous.push(ou("Pilot", "Endpoints that get updates and new GPOs first", None));
        policies.push(PlannedPolicy {
            name: "Patch Ring: Broad".into(),
            description: "Everyone except the Pilot OU waits 7 days for quality updates and 60 for feature updates.".into(),
            priority: 900,
            enabled: true,
            all_endpoints: true,
            ou_links: vec![],
            settings: vec![Setting::UpdateDeferral { quality_days: 7, feature_days: 60 }],
        });
        policies.push(PlannedPolicy {
            name: "Patch Ring: Pilot".into(),
            description: "The Pilot OU gets updates as soon as they're released.".into(),
            priority: 100,
            enabled: true,
            all_endpoints: false,
            ou_links: vec!["Pilot".into()],
            settings: vec![Setting::UpdateDeferral { quality_days: 0, feature_days: 0 }],
        });
    }

    if b == Education {
        let lab_level = if edu == EducationVariant::K12 { SecurityLevel::Strict } else { SecurityLevel::Balanced };
        let v = levels(lab_level);
        policies.push(PlannedPolicy {
            name: "Lab Computers".into(),
            description: "Shared classroom and lab computers: no USB storage, quick screen lock and no remote access.".into(),
            priority: 200,
            enabled: true,
            all_endpoints: false,
            ou_links: vec!["Labs".into()],
            settings: vec![
                Setting::UsbStorage { mode: if edu == EducationVariant::K12 { "block".into() } else { "read_only".into() } },
                Setting::ScreenLock { minutes: v.screen.min(10) },
                Setting::RemoteDesktop { enabled: false },
                Setting::HideLastUsername { enabled: true },
            ],
        });
        if m.compliance.iter().any(|c| matches!(c, CompliancePack::Ferpa | CompliancePack::Cipa)) || edu == EducationVariant::K12 {
            note(
                NoteKind::NotYet,
                "CIPA web filtering needs a DNS filter or proxy; OpenDirectory will integrate one in the Integrations area.".into(),
            );
        }
    }
    if b == HighSecurity {
        policies.push(PlannedPolicy {
            name: "Server Hardening".into(),
            description: "Servers: SSH on for administration, everything else locked down.".into(),
            priority: 200,
            enabled: true,
            all_endpoints: false,
            ou_links: vec!["Servers".into()],
            settings: vec![Setting::SshServer { enabled: true }, Setting::RemoteDesktop { enabled: false }],
        });
    }
    if ownership != Ownership::Corporate && ous.iter().any(|o| o.name == "BYOD") {
        policies.push(PlannedPolicy {
            name: "BYOD Light Touch".into(),
            description: "Personally owned devices: only screen lock and encryption checks, nothing that changes how the owner uses it."
                .into(),
            priority: 150,
            enabled: true,
            all_endpoints: false,
            ou_links: vec!["BYOD".into()],
            settings: vec![
                Setting::ScreenLock { minutes: 10 },
                Setting::DiskEncryption { required: true },
                Setting::UsbStorage { mode: "allow".into() },
            ],
        });
        note(NoteKind::Info, "BYOD devices go in the BYOD OU. Its GPO wins over stricter defaults for the settings it sets, but the Default Endpoint Policy still applies to everything else.".into());
    }

    // Rollout.
    let rollout = m.rollout.unwrap_or(Rollout::Everyone);
    match rollout {
        Rollout::Everyone => {}
        Rollout::Pilot => {
            if !ous.iter().any(|o| o.name == "Pilot") {
                ous.push(ou("Pilot", "Endpoints that get new GPOs first", None));
            }
            for p in policies.iter_mut().filter(|p| p.all_endpoints) {
                p.all_endpoints = false;
                p.ou_links = vec!["Pilot".into()];
            }
            note(NoteKind::Info, "Staged rollout: GPOs that would apply to every endpoint are linked to the Pilot OU only. Turn on \"All endpoints\" in each GPO when you're ready.".into());
        }
        Rollout::Review => {
            for p in &mut policies {
                p.enabled = false;
            }
            note(
                NoteKind::Info,
                "Review first: every GPO is created turned off. Nothing changes on endpoints until you enable them.".into(),
            );
        }
    }

    let audit_retention_days = m.audit_retention_days.or(if compliance { Some(365) } else { None });
    if compliance {
        note(
            NoteKind::Info,
            format!(
                "{}: audit log kept {} days. These settings help meet the technical controls; they don't make you compliant on their own.",
                packs(&m.compliance),
                audit_retention_days.unwrap_or(365)
            ),
        );
    }
    if m.compliance.contains(&CompliancePack::Hipaa) || m.compliance.contains(&CompliancePack::Cmmc) {
        note(NoteKind::NotYet, "MFA for the Admin Center (required by HIPAA and CMMC guidance) isn't available yet.".into());
    }

    Plan {
        baseline: b,
        level,
        ous,
        groups,
        policies,
        admin_ou,
        audit_retention_days,
        naming_convention: m.naming_convention.clone().filter(|n| !n.trim().is_empty()),
        signin_group: signin_group.map(str::to_string),
        signin_admin: local_admin,
        rollout,
        notes,
    }
}

fn packs(p: &[CompliancePack]) -> String {
    p.iter().map(|c| c.label()).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &PlannedPolicy) -> Vec<&'static str> {
        p.settings.iter().map(|s| s.kind()).collect()
    }

    #[test]
    fn every_baseline_plans_without_duplicate_settings() {
        for b in catalog() {
            let p = plan(b.id, &Modifiers::default());
            for pol in &p.policies {
                let mut k = kinds(pol);
                let n = k.len();
                k.sort();
                k.dedup();
                assert_eq!(n, k.len(), "{:?} {} has duplicate settings", b.id, pol.name);
                for l in &pol.ou_links {
                    assert!(p.ous.iter().any(|o| &o.name == l), "{:?}: {} links to missing OU {l}", b.id, pol.name);
                }
            }
            for o in &p.ous {
                if let Some(par) = &o.parent {
                    assert!(p.ous.iter().any(|x| &x.name == par), "{:?}: {} has missing parent", b.id, o.name);
                }
            }
        }
    }

    #[test]
    fn custom_and_minimal_enforce_nothing() {
        assert!(plan(Baseline::Custom, &Modifiers::default()).policies.is_empty());
        assert!(plan(Baseline::Minimal, &Modifiers::default()).policies.is_empty());
    }

    #[test]
    fn high_security_is_strict() {
        let p = plan(Baseline::HighSecurity, &Modifiers::default());
        let d = &p.policies[0];
        assert!(d.settings.contains(&Setting::UsbStorage { mode: "block".into() }));
        assert!(d.settings.contains(&Setting::PasswordMinLength { length: 15 }));
        assert!(d.settings.contains(&Setting::RemoteDesktop { enabled: false }));
    }

    #[test]
    fn compliance_raises_level_and_review_disables() {
        let m = Modifiers {
            security_level: Some(SecurityLevel::Relaxed),
            compliance: vec![CompliancePack::Hipaa],
            rollout: Some(Rollout::Review),
            ..Default::default()
        };
        let p = plan(Baseline::SmallBusiness, &m);
        assert_eq!(p.level, Some(SecurityLevel::Strict));
        assert!(p.policies.iter().all(|x| !x.enabled));
        assert_eq!(p.audit_retention_days, Some(365));
        assert!(p.notes.iter().any(|n| n.kind == NoteKind::Warning));
    }

    #[test]
    fn pilot_rollout_relinks() {
        let p = plan(Baseline::SmallBusiness, &Modifiers { rollout: Some(Rollout::Pilot), ..Default::default() });
        assert!(p.policies.iter().all(|x| !x.all_endpoints));
        assert!(p.ous.iter().any(|o| o.name == "Pilot"));
    }
}
