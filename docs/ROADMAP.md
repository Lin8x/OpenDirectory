# Roadmap

## Done (v0.1)

- Directory: users, security groups, organizational units, Domain Admin role, audit log, search.
- Group Policy: cross-platform GPOs with priority, OU links with inheritance, Resultant Set of Policy per endpoint.
- Settings: password minimum length, screen lock, firewall, disk encryption (report), automatic updates, login banner, required software, sign-in accounts.
- Endpoint Agent as a native background service on Windows, macOS and Linux, with enrollment tokens, hardware/OS/network/storage inventory, compliance reporting, remote scripts, inventory refresh and agent restart.
- Admin Center desktop app with embedded server, onboarding wizard, Learn mode and glossary, command palette, tray icon, light/dark theme.
- `od` CLI covering the same features.

## Done (v0.3)

- Script Library with 20 templates, parameters, and runs against endpoints, security groups (computer members) and OUs.
- GPO scripts: startup, sign-in, scheduled and run-once scripts that inherit through OU links.
- 18 more GPO settings: account lockout, password age, history and complexity, USB storage, Guest account, Remote Desktop, SSH server, automatic sign-in, NTP, hide last user name, AutoRun, SMBv1, LLMNR, antivirus, SmartScreen/Gatekeeper, security auditing, update deferral (patch rings).
- Onboarding with 7 security baselines, modifiers, compliance packs, staged rollout and a full preview.
- Launch at startup for the Admin Center; `opendirectory-server install-service`; headless `od init`.
- Clear "You're up to date" status for in-app updates.

## Done (v0.4)

- Backup & Restore: one encrypted `.odbackup` file; scheduled plans with retention to a folder or SMB/NFS share, SFTP, FTPS/FTP, WebDAV, S3-compatible storage, any HTTPS endpoint, Google Drive, OneDrive and Dropbox; verified restores.
- Branding: logo, accent color, product name and support contact.
- Integrations: import users and groups from Active Directory (LDAPS) and Entra ID; webhooks, Slack and Teams alerts; guides and reachability tests for SMB, DNS, DHCP, WSUS, RDS, print and AD CS.
- Classic console interface modeled on ADUC and GPMC.
- Network Lab (design and simulate) and a live Topology Map.
- Web mode and a Docker image with docker-compose and Portainer instructions.

## Done (v0.5)

- Recycle Bin for users, groups, OUs, GPOs and scripts (180 days, exact restore).
- Fine-grained password policies (PSOs) with precedence; Default Domain Password Policy; history, expiry and lockout; resultant policy per user.
- Self-service password change and one-time help-desk reset codes.
- LAPS: rotating local admin passwords escrowed on the server, audited reads, rotate on demand.
- Patch management: pending OS updates from Windows Update, apt, dnf and softwareupdate; scan and install on endpoints, groups or OUs.
- Interactive remote shell, and remote desktop/SSH through secure tunnels (desktop app).
- Agents wake up instantly for jobs, shells and rotations (long poll).

## Done (v0.6)

- DNS server: authoritative zone with admin records, the server's own records and dynamic records for endpoints and DHCP clients; forwarding; UDP and TCP.
- DHCP server: scopes, reservations, relay support and leases (off by default).
- Certificate authority: root CA, issue or sign CSRs, revoke, CRL; Trusted root certificate GPO setting.
- RADIUS server: PAP and MS-CHAPv2 for VPNs and network devices, per-client group restriction. EAP (WPA2/3-Enterprise) is not yet supported.

## Done (v0.7)

- Mobile devices (basic MDM): inventory of phones and tablets by serial, IMEI, owner and OS, and downloadable Apple configuration profiles (Wi-Fi, passcode, root CA). No over-the-air Apple MDM or Android Enterprise enrollment yet.
- LDAP server: read-only LDAP and LDAPS with an AD-style schema (users, groups, OUs, computers) for apps, NAS boxes and printers.
- Domain Controller role (experimental, Linux): provisions a Samba AD DC and syncs users, groups and passwords into it, so Windows, macOS and Linux can do a real domain join with Kerberos. Shows FSMO roles, sites and subnets, replication and trusts, with transfer, seize, site and replicate actions. Multi-DC and trusts are run through Samba and are only lightly wrapped.
- Domain join, printer deployment templates; secret script parameters hidden from job history.
- Machine certificate auto-enrollment GPO setting. Windows gets the key and certificate as files; importing into the Windows certificate store is still to do.
- DFS: guidance only (use Windows DFS or Samba). Print: deployment scripts only, no print server.

## Done (v0.8)

- Domain Controller role tested against a real Samba AD database (user create, password sync, group membership, disable and delete), run automatically when Samba's AD packages are installed.
- Network Lab: reference designs (Homelab, Campus, Enterprise tiered multi-site, High security hardened) and a Stop button.
- Knowledge Base on the Learn page: a guide per service or concept with a video, Wikipedia and further links, plus a Microsoft / AD counterpart tab.

## Done (v0.9 and v0.10)

- Knowledge Base links open from the desktop app; each topic has The standard, In OpenDirectory and Microsoft / AD views.
- Replica servers: join codes, a full copy every minute, read-only replicas that serve agents, sign-in, DNS, RADIUS and LDAP, automatic agent failover, and promotion with an epoch so a returning old primary is detected. Single writable primary (no multi-master); up to a minute of changes can be lost if the primary fails; no DHCP failover yet.

## Done (v0.12)

- Version-aware check-ins: older agents get only the settings and jobs they understand; agents skip unknown ones.
- Agent self-update from the Management Server with SHA-256 check, automatic rollback, and rollout rings (all, pilot OUs, off).

## Next

1. **Real-hardware validation** on Windows 10/11 and macOS 14+, and of the Domain Controller role on a clean Ubuntu or Debian server. Ship signed installers.
2. **Revert removed settings.** When a setting leaves a GPO, return the endpoint to its previous value.
3. **Windows certificate store import** for auto-enrolled certificates, and EAP-TLS in the RADIUS server.
4. **Over-the-air MDM.** Apple MDM (needs an APNs certificate) and Android Enterprise.
5. **Delegated administration.** Roles narrower than Domain Admin (help desk, read-only), scoped to OUs.
7. **DHCP failover** between primary and replica, and automatic promotion with a witness.
