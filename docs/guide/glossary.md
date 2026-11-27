# Glossary

[Guide home](README.md)

Every term used in OpenDirectory, in plain language. The **AD equivalent** column gives the closest name in Microsoft's world (Active Directory, Windows Server, Intune), so you can follow Microsoft documentation or talk to a Windows admin. A dash means there's no direct equivalent.

The chapter column points to where the idea is explained in full.

## The directory

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **Directory** | The database describing your organization: people, groups, computers and rules. | Active Directory Domain Services (AD DS) | [2](02-directory.md) |
| **Domain** | The name and boundary of your whole directory, written like `corp.contoso.com`. | AD domain | [2](02-directory.md) |
| **Organizational unit (OU)** | A folder in the directory for users, groups and computers. GPOs link to OUs, and child OUs inherit from their parents. | OU | [2](02-directory.md) |
| **Inheritance** | Child OUs automatically receive the GPOs linked to their parent OUs. | GPO inheritance | [3](03-group-policy.md) |
| **User** (user account) | One person's identity: username, password and profile details. | User object | [2](02-directory.md) |
| **Security group** | A named list of users you can grant things to as a whole. | Security group | [2](02-directory.md) |
| **Domain Admins** | Built-in group with full control. Only its members can use the Admin Center and the `od` command. | Domain Admins | [2](02-directory.md) |
| **Domain Users** | Built-in group that every user is in automatically. | Domain Users | [2](02-directory.md) |
| **Default Domain Password Policy** | The password and lockout rules for everyone. | Default Domain Policy (password and lockout settings) | [2](02-directory.md) |
| **Fine-grained password policy** | Different password rules for members of chosen groups. | Password Settings Object (PSO) | [2](02-directory.md) |
| **Account lockout** | Blocking sign-in for a while after too many wrong passwords. | Account lockout policy | [2](02-directory.md) |
| **Recycle Bin** | Keeps deleted users, groups, OUs, GPOs and scripts for 180 days so you can restore them exactly. | AD Recycle Bin | [2](02-directory.md) |
| **Audit Log** | A permanent record of every change and sign-in: who, what and when. | Security event log / directory service auditing | [8](08-walkthrough-first-endpoint.md) |

## Group Policy

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **Group Policy Object (GPO)** | A named bundle of settings applied to computers. | GPO | [3](03-group-policy.md) |
| **Setting** | One rule inside a GPO, such as "lock the screen after 10 minutes". | Policy setting / preference item | [3](03-group-policy.md) |
| **Link** | Attaches a GPO to the whole domain, OUs or individual endpoints, deciding where it applies. | GPO link | [3](03-group-policy.md) |
| **Priority** | A number on each GPO. When two GPOs disagree, the lower number wins. | Link order / precedence | [3](03-group-policy.md) |
| **Platform filter** | Makes a GPO skip chosen operating systems. | WMI filter | [3](03-group-policy.md) |
| **Resultant Set of Policy (RSoP)** | The final list of settings one computer gets, after inheritance and priority, with where each came from. | RSoP / `gpresult` | [3](03-group-policy.md) |
| **Compliance** | Whether a computer matches what its GPOs ask for. Each setting is Compliant, Applied, Non-compliant, Failed, Unsupported or Pending. | Compliance (Intune) / RSoP logging | [3](03-group-policy.md) |
| **Enforced / Report only / Unsupported** | How well an operating system supports a setting: set and kept, checked but not changed, or skipped. | — | [3](03-group-policy.md) |
| **Sign-in accounts** | A setting that gives every member of a group a real account on the computers the GPO applies to, optionally as local administrators. | Restricted Groups + "Allow log on locally" | [3](03-group-policy.md) |
| **Security baseline** | A ready-made starting set of OUs, groups and GPOs for a type of organization, chosen in the setup wizard. | Microsoft Security Baselines / CIS Benchmarks | [7](07-walkthrough-setup.md) |
| **Script Library** | Saved scripts you can run on demand or from a GPO at startup, sign-in, on a schedule or once. | GPO startup/logon scripts; an RMM script library | [3](03-group-policy.md), [9](09-everyday-tasks.md) |

## Endpoints and agents

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **Endpoint** | A computer or phone that OpenDirectory manages. | Computer object | [4](04-endpoints-and-agents.md) |
| **Endpoint Agent** | The background program on each endpoint that applies policy and reports back. | Group Policy client and client-side extensions; an RMM agent | [4](04-endpoints-and-agents.md) |
| **Service / daemon** | A program that starts with the computer and runs without anyone signed in. Windows says service; macOS and Linux say daemon. | Windows Service | [4](04-endpoints-and-agents.md) |
| **Check-in** | The agent contacting the server, every 15 seconds (15 minutes on Android), to report and fetch instructions. | Group Policy background refresh | [4](04-endpoints-and-agents.md) |
| **Inventory** | The hardware and software details each agent reports. | Hardware inventory (Configuration Manager) | [4](04-endpoints-and-agents.md) |
| **Enrollment** | The one-time process of installing the agent and joining a computer to your directory. | Domain join / Intune enrollment | [4](04-endpoints-and-agents.md) |
| **Enrollment token** | A code that allows computers to join, optionally placing them in an OU, with a use limit and expiry. | Pre-staged computer account / bulk enrollment token | [4](04-endpoints-and-agents.md) |
| **Enrollment file** | A ready-to-run installer the server builds with its address, token and fingerprint inside. | Provisioning package | [4](04-endpoints-and-agents.md) |
| **Endpoint credential** | The secret each enrolled computer uses to prove who it is at every check-in. | Computer account password | [4](04-endpoints-and-agents.md) |
| **Retire** | Removing an endpoint from management and revoking its credentials. | Deleting the computer object | [4](04-endpoints-and-agents.md) |
| **Local account** | An account stored on one computer rather than in a directory. OpenDirectory creates them to match your users. | Local user (SAM) | [4](04-endpoints-and-agents.md) |
| **LAPS** | Each computer keeps a local administrator account with its own rotating password, stored encrypted on the server. | Windows LAPS | — |
| **Job** | A task queued for an agent, such as running a script or installing updates. | Configuration Manager deployment | [4](04-endpoints-and-agents.md) |
| **Patch management** | Finding and installing pending operating system updates across computers. | WSUS / Windows Update for Business | [9](09-everyday-tasks.md) |

## Security and connections

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **TLS** | The encryption that protects connections, the same as the padlock in a browser. | TLS / LDAPS | [4](04-endpoints-and-agents.md) |
| **Certificate** | A digital ID card that proves a server or device is who it says it is. | Certificate | [4](04-endpoints-and-agents.md) |
| **Fingerprint** | A short, unique code identifying one certificate. Starts with `sha256:`. | Certificate thumbprint | [4](04-endpoints-and-agents.md) |
| **Pinning** | Remembering a server's fingerprint and refusing any server that doesn't match. | — (AD trusts the domain's Kerberos keys instead) | [4](04-endpoints-and-agents.md) |
| **Certificate authority (CA)** | Something that issues certificates others trust. | AD Certificate Services (AD CS) | [5](05-gateway-and-updates.md) |
| **SHA-256 checksum** | A fingerprint of a file, used to check a download wasn't damaged or tampered with. | File hash | [5](05-gateway-and-updates.md) |
| **Port** | A numbered door on a computer for a particular kind of connection. The Management Server uses 8443; the Internet Gateway uses 8444. | Port | [5](05-gateway-and-updates.md) |
| **LAN** | Your local network: everything connected to the same router or office network. | — | [5](05-gateway-and-updates.md) |
| **Port forwarding** | A router rule that sends incoming internet traffic on one port to one computer. | NAT rule / publishing rule | [5](05-gateway-and-updates.md) |
| **VPN** | A virtual private network: makes a remote computer act as if it's in the office. | Always On VPN / DirectAccess | [5](05-gateway-and-updates.md) |

## Working from anywhere and updates

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **Internet Gateway** | A second, agent-only entrance to the Management Server that's safe to expose to the internet, so laptops check in from anywhere. | Cloud Management Gateway / DirectAccess | [5](05-gateway-and-updates.md) |
| **Mutual TLS (mTLS)** | Both sides of a connection prove their identity with certificates, not just the server. | Certificate-based client authentication | [5](05-gateway-and-updates.md) |
| **Device certificate** | The certificate each agent presents to the Internet Gateway. Issued automatically, revoked when you retire the endpoint. | Computer certificate (AD CS auto-enrollment) | [5](05-gateway-and-updates.md) |
| **Device Identity CA** | The small built-in certificate authority that issues device certificates. | Enterprise CA | [5](05-gateway-and-updates.md) |
| **Agent self-update** | Agents download the matching version from your server, verify it, swap it in, and roll back if it fails. | Configuration Manager client upgrade | [5](05-gateway-and-updates.md) |
| **Rollout ring** | Who gets updates first: all endpoints, pilot OUs only, or nobody. | Deployment ring / pilot collection | [5](05-gateway-and-updates.md) |
| **Pilot** | A small group of computers that gets changes first, to catch problems early. | Pilot collection / pilot ring | [5](05-gateway-and-updates.md) |
| **Rollback** | Automatically going back to the previous version when an update fails. | — | [5](05-gateway-and-updates.md) |

## The person at the computer

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **End user** | The person using a managed computer. | End user | [6](06-end-user-experience.md) |
| **Managed-device notice** | A message at sign-in explaining the device is managed, with optional "I understand" acknowledgement. | Interactive logon message / Intune privacy notice | [6](06-end-user-experience.md) |
| **Remote session consent** | Whether the user is asked, told, or not notified before IT opens a remote shell or desktop. | RDS remote control rules ("with user's permission") | [6](06-end-user-experience.md) |
| **Unattended access** | Connecting to a computer when nobody is signed in. | — | [6](06-end-user-experience.md) |
| **Help desk chat** | A Contact IT shortcut on each computer for messaging IT, answered from the Help Desk page. | Company Portal: Contact IT | [6](06-end-user-experience.md) |
| **Remote tools** | Turning remote shell and remote desktop on or off per computer. | Remote Desktop / Remote Assistance policies | [6](06-end-user-experience.md) |
| **Remote shell** | A command window on a remote computer, opened from the Admin Center. | PowerShell Remoting (`Enter-PSSession`) | [8](08-walkthrough-first-endpoint.md) |
| **Remote desktop** | Seeing and controlling a remote computer's screen. | Remote Desktop / Remote Assistance | [6](06-end-user-experience.md) |

## The programs

| Term | What it means | AD equivalent | Chapter |
| --- | --- | --- | --- |
| **Admin Center** | The desktop app you manage everything from. Can host the Management Server inside itself. | ADUC + GPMC + Server Manager | [1](01-big-picture.md) |
| **Management Server** | The program that stores the directory and hands out policy. | Domain controller | [1](01-big-picture.md) |
| **`od` (CLI)** | A typed command that does everything the Admin Center does. `od explain <term>` explains any term. | ActiveDirectory PowerShell module | [1](01-big-picture.md) |
| **Learn mode** | Shows a plain-language explanation next to every technical term in the Admin Center. | — | [7](07-walkthrough-setup.md) |
| **Replica server** | A read-only copy of the Management Server that agents fail over to if the main one goes down. | Additional domain controller / RODC | — |
| **Domain Controller role** | Optional, experimental: runs a Samba AD domain controller so computers can do a real domain join. | Domain controller | [4](04-endpoints-and-agents.md) |
| **RMM** | Remote monitoring and management: inventory, health, scripts and remote access for endpoints. | Configuration Manager / third-party RMM | [1](01-big-picture.md) |
| **MDM** | Mobile device management: managing phones and tablets. | Intune | — |
