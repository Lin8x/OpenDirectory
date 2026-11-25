# Architecture

OpenDirectory follows the same shape as an Active Directory domain with Group Policy and an endpoint management tool such as Intune or SCCM, but uses one agent per computer instead of relying on the operating system's own domain join. That choice is what lets it manage Windows Home, unactivated Windows, macOS and Linux the same way.

```
 ┌────────────────────────── Admin Center (desktop app) ─────────────────────────┐
 │  React UI  ──invoke("api_request")──▶  Rust shell  ──▶  embedded Management   │
 │                                         (Tauri)          Server (optional)    │
 └───────────────────────────────────────────────┬───────────────────────────────┘
                                                 │ HTTPS :8443 (pinned TLS)
           od CLI ──────────────────────────────▶│◀───────────── other admins
                                                 │
                    ┌────────────────────────────┴───────────────────────────┐
                    │                Management Server                       │
                    │  REST API /api/v1 · SQLite directory · GPO engine      │
                    └───────▲───────────────────▲────────────────────▲───────┘
                            │ check-in /15 s     │                    │
                   Endpoint Agent        Endpoint Agent        Endpoint Agent
                   Windows Service       launchd daemon        systemd unit
```

## Management Server (`od-server`)

- **Storage.** One SQLite database (bundled, no external DB) holding the organization, OUs, users, groups, endpoints, enrollment tokens, GPOs, jobs and the audit log.
- **API.** axum over TLS on `0.0.0.0:8443`. On first start it creates a self-signed certificate; clients pin its SHA-256 fingerprint instead of trusting a CA, the same way SSH pins host keys. A loopback-only HTTP listener exists for the embedded case so the desktop app never leaves the machine.
- **Authentication.**
  - Admins: username + password (Argon2id hashes), then `Authorization: Bearer <session>`.
  - Agents: `Authorization: Endpoint <id>.<secret>`, issued once at enrollment; only a hash is stored server-side.
- **User passwords for endpoints.** Directory passwords are stored twice: an Argon2 hash for directory sign-in, and a copy sealed with XChaCha20-Poly1305 under a server key in the data directory. The sealed copy is what lets the server hand an agent the password it needs to create a matching local account. It is sent only to authenticated agents, only for accounts that GPOs assign to that endpoint, and only when the account version changed.
- **Embedded or standalone.** `od_server::start()` is a library call. The Admin Center calls it in-process ("Host the directory on this computer"); `opendirectory-server` is the same code as a headless service.

## Group Policy engine (`od-core::policy`)

A GPO is a list of **intents** ("lock the screen after 10 minutes"), not registry keys. Each intent maps to a native mechanism per OS; the catalog records which OS supports what and the AD equivalent, and the UI shows that to the admin.

- **Targeting.** A GPO links to *all endpoints* or to OUs. Links inherit down the OU tree, like AD.
- **Precedence.** Lower priority number wins when two GPOs set the same setting. `Sign-in accounts` and `Required software` accumulate across GPOs instead of overriding.
- **RSoP.** The server computes the Resultant Set of Policy for each endpoint and sends it on every check-in; the Admin Center and `od endpoint rsop` show the same result, including which GPO each setting came from.

| Setting | Windows | macOS | Linux |
| --- | --- | --- | --- |
| Password minimum length | `net accounts` | `pwpolicy` | pam_pwquality |
| Screen lock | `InactivityTimeoutSecs` policy value | `com.apple.screensaver` idle time + ask for password | GNOME dconf (locked) |
| Firewall | `netsh advfirewall` | Application Firewall (`socketfilterfw`) | ufw / firewalld |
| Disk encryption | BitLocker status (report) | FileVault status via `fdesetup` (report) | LUKS/dm-crypt via `lsblk` (report) |
| Automatic updates | Windows Update `AU` policy keys | `com.apple.SoftwareUpdate` preferences | unattended-upgrades / dnf-automatic |
| Login banner | `legalnoticecaption` / `legalnoticetext` | `/Library/Security/PolicyBanner.txt` | `/etc/issue` |
| Required software | winget | Homebrew | apt / dnf |
| Sign-in accounts | `New-LocalUser` | `dscl` | `useradd` |

Settings an OS can't do are reported as *Unsupported* in compliance instead of failing silently.

## Endpoint Agent (`od-agent`)

### Running in the background at all times

`opendirectory-agent install` registers the agent with the OS service manager, which starts it at boot (before anyone signs in) and restarts it if it crashes:

| OS | Mechanism | Location |
| --- | --- | --- |
| Windows | Windows Service `OpenDirectoryAgent`, LocalSystem, automatic start, `sc failure` restart actions | `C:\ProgramData\OpenDirectory\agent` |
| macOS | LaunchDaemon `dev.opendirectory.agent` with `KeepAlive` | `/Library/Application Support/OpenDirectory/agent` |
| Linux | systemd unit `opendirectory-agent.service`, `Restart=always` | `/var/lib/opendirectory-agent` |

The agent runs as SYSTEM / root because creating accounts and changing security settings requires it.

### Check-in loop

Every 15 seconds the agent sends inventory (on change or every few minutes), compliance results and the versions of the accounts it manages. The server answers with the current RSoP, the accounts to create or update, and queued jobs (remote script, refresh inventory, restart agent). Network failures back off exponentially. Pull-only means endpoints need no inbound ports and work behind NAT.

### Sign-in accounts (user logins on each OS)

OpenDirectory does not domain-join machines. Instead, a GPO's *Sign-in accounts* setting names security groups; the agent creates a **local account** for every member, with the same username and password as the directory. Members can be made local administrators.

Safety rules:

- The agent only modifies accounts it created (tracked in its state file). Pre-existing local accounts with the same name are left alone and reported.
- Removing a user from the group, or disabling them in the directory, **disables** the local account; it never deletes home folders.
- Reserved names (`root`, `Administrator`, `Guest`, system accounts…) are refused by both server and agent.
- Passwords are passed to OS tools on stdin, never on the command line.

### Why this works on Windows Home and unactivated Windows

Windows Home can't join an AD domain and ignores Group Policy, and the Pro features of unactivated Windows are the same as activated. The agent avoids both: it is an ordinary Windows Service using local APIs (local users, registry, `netsh`, winget) that exist on every edition. Activation status does not affect services, local accounts or the registry.

## Admin Center (`apps/admin-center`)

- **Native app, not a web console.** Tauri 2 wraps a React UI in the OS webview. All API calls go through a Rust command (`api_request`) so the UI has no network access of its own and works with no internet connection.
- **Two modes.** *Host* runs the Management Server inside the app (data in the app data folder) and keeps it running from the tray when the window is closed. *Connect* talks to a remote server with a pinned fingerprint.
- **Onboarding.** Welcome screen → domain setup wizard → Overview with a getting-started checklist. *Learn mode* adds plain-language explanations with the matching AD term on every page; the Learn page is a full glossary (`od explain` uses the same text).

## Security model summary

- TLS everywhere off-box, with certificate pinning for agents, CLI and remote Admin Centers.
- Enrollment tokens are single- or limited-use and expire after 7 days by default.
- Every change is written to the audit log with the acting admin.
- The last Domain Admin can't be deleted, disabled or demoted.
- The server key that seals user passwords lives in the data directory; protect and back up that directory like an AD database (NTDS.dit).

## Known limitations

- No LDAP/Kerberos yet, so AD-aware applications can't authenticate against OpenDirectory directly (see roadmap).
- Settings removed from a GPO are not reverted on the endpoint.
- Disk encryption is reported, not enforced (enabling BitLocker/FileVault/LUKS needs user interaction or recovery key escrow).
- Remote scripts are fire-and-collect, not interactive. Live remote desktop is on the roadmap.
- Mobile devices are not managed yet.

## Updates and releases

- **Admin Center.** Uses the Tauri updater. On launch it fetches `latest.json` from the newest GitHub Release, and if a newer version exists it shows *Update now* / *Remind me later* (snoozes for a day). Checks can be turned off in Settings → Updates. Update bundles are signed with a minisign key; the public half is in `tauri.conf.json`, so a tampered download is rejected before install.
- **Release pipeline.** `cargo x release <patch|minor|major>` bumps every manifest, writes `CHANGELOG.md` from Conventional Commits, tags `vX.Y.Z` and pushes. `.github/workflows/release.yml` then builds the Admin Center installers (Windows `.msi`/`.exe`, macOS universal `.dmg`, Linux `.deb`/`.rpm`/`.AppImage`), the agent/server/CLI binaries for each OS, signs the update bundles when the `TAURI_SIGNING_PRIVATE_KEY` secrets are present, and publishes everything to the GitHub Release.
- **Endpoint Agent.** The Enrollment page downloads the agent straight from the latest release (`releases/latest/download/opendirectory-agent-<os>-<arch>`), or you can copy the file over for offline networks. Agent self-update is on the roadmap.

## Account lockout

Five wrong passwords for the same username lock Admin Center sign-in for that name for five minutes (AD's account lockout threshold and duration). Every failure is in the audit log under *Failed sign-ins*.
