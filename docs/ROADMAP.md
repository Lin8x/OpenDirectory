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

## Next

1. **Real-hardware validation** on Windows 10/11 Home and Pro and on macOS 14+. Ship signed installers (MSI, pkg, deb/rpm).
2. **Revert removed settings.** When a setting leaves a GPO, return the endpoint to its previous value instead of leaving it as last applied.
3. **Live remote session.** Interactive shell first, then remote desktop (screen streaming through the server, with user consent prompts).
4. **Mobile device tracking (basic MDM).** Inventory for iPhone and Android by serial number, IMEI, owner and IP. Later, real enrollment through Apple MDM (needs an APNs certificate) and Android Enterprise.
5. **Software deployment and patch reporting.** Package catalog, per-endpoint installed software list.
6. **Domain Controller role.** LDAP and Kerberos endpoints so AD-aware apps and Windows Pro machines can authenticate against OpenDirectory, plus sync from and to an existing Active Directory or Samba domain.
7. **Delegated administration.** Roles narrower than Domain Admin (help desk, read-only), scoped to OUs.
8. **Agent self-update** from GitHub Releases, staged by OU.
9. **High availability.** Server replication and backup/restore from the Admin Center.
