//! `od explain`: plain-language definitions of directory terms.

pub const TERMS: &[(&str, &str)] = &[
    ("domain", "The boundary of your directory, named like a DNS domain (corp.example.com). Everything in OpenDirectory belongs to one domain."),
    ("ou", "Organizational Unit. A folder for users, groups and endpoints. Group Policy is linked to OUs, and child OUs inherit their parent's policies."),
    ("user", "A person's account. Users sign in to managed endpoints with their username and password. Domain Admins can also sign in to the Admin Center."),
    ("group", "A security group: a named set of users. Use groups to decide who can sign in where and who is an admin on endpoints."),
    ("endpoint", "Any managed computer (Windows, macOS or Linux) running the Endpoint Agent."),
    ("agent", "The Endpoint Agent: a small background service on each endpoint that reports inventory, applies policy, manages sign-in accounts and runs scripts."),
    ("gpo", "Group Policy Object. A named set of settings (screen lock, firewall, sign-in accounts...) linked to OUs or to every endpoint. OpenDirectory GPOs work on Windows, macOS and Linux."),
    ("rsop", "Resultant Set of Policy. The final settings on one endpoint after every GPO that applies is merged, showing which GPO won each setting."),
    ("priority", "When two GPOs set the same thing, the one with the lower priority number wins, like GPO link order in Active Directory."),
    ("enrollment", "Connecting a computer to OpenDirectory. Create an enrollment token in the Admin Center, then run the agent's enroll command on the computer."),
    ("token", "An enrollment token: a one-time or limited-use code that lets a computer enroll. Tokens can drop new endpoints into an OU automatically."),
    ("fingerprint", "The TLS certificate fingerprint of your Management Server. Agents and admin tools check it so they only ever talk to your real server, with no internet or public CA needed."),
    ("compliance", "Whether an endpoint matches its policy. Each setting reports compliant, applied, non-compliant, failed or unsupported."),
    ("rmm", "Remote Monitoring and Management: inventory, remote scripts and health of endpoints. OpenDirectory includes RMM alongside the directory."),
    ("mdm", "Mobile Device Management: managing phones and tablets through Apple's and Google's management protocols. Planned for a later release."),
    ("dc", "Domain Controller: a server that answers Active Directory protocols (LDAP, Kerberos). OpenDirectory's Domain Controller role is planned; today it manages endpoints through its agent."),
    ("management server", "The OpenDirectory backend that stores the directory and talks to agents and admin tools. It runs inside the Admin Center or on its own."),
    ("admin center", "OpenDirectory Admin Center: the desktop app administrators use to manage the directory and endpoints."),
];

pub fn lookup(term: &str) -> Option<(&'static str, &'static str)> {
    let t = term.trim().to_lowercase();
    let t = match t.as_str() {
        "ous" | "organizational unit" | "organisational unit" => "ou",
        "users" | "account" => "user",
        "groups" | "security group" => "group",
        "endpoints" | "device" | "devices" | "computer" => "endpoint",
        "endpoint agent" => "agent",
        "gpos" | "policy" | "policies" | "group policy" => "gpo",
        "server" => "management server",
        "domain controller" => "dc",
        other => other,
    }
    .to_string();
    TERMS.iter().find(|(k, _)| *k == t).copied()
}
