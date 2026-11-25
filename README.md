# OpenDirectory

OpenDirectory is a cross-platform directory and endpoint management suite. It gives you the parts of Windows Server + Active Directory that matter day to day — users, security groups, organizational units, Group Policy, remote scripts and device inventory — but it manages **Windows (including Home and unactivated editions), macOS and Linux** from one place.

It ships as a native desktop app (the **Admin Center**) that works fully offline, a command-line tool (`od`), a **Management Server** you can run inside the Admin Center or on a dedicated machine, and an **Endpoint Agent** that runs as a background service on every managed computer.

> Status: early but working. Linux has been verified end to end. Windows builds and cross-compiles; macOS is built in CI. Both still need real-hardware testing. See [docs/ROADMAP.md](docs/ROADMAP.md).

## Components

| Component | Binary | What it is |
| --- | --- | --- |
| Admin Center | `opendirectory-admin-center` | Native desktop app (Tauri). Hosts the Management Server in-process, or connects to a remote one. Tray icon keeps it running. |
| Management Server | `opendirectory-server` | Directory database (SQLite), Group Policy engine, REST API over TLS. Same code as the embedded server. |
| Endpoint Agent | `opendirectory-agent` | Background service: Windows Service, launchd daemon, or systemd unit. Checks in every 15 seconds. |
| CLI | `od` | Everything the Admin Center does, scriptable, with `od explain <term>` for newcomers. |

## Quick start

```bash
./scripts/setup.sh        # first time only (Windows: .\scripts\setup.ps1)
./scripts/build.sh        # server, agent and CLI
./scripts/build.sh --app  # also the Admin Center installer (needs Node 20+ and, on Linux, webkit2gtk 4.1)
```

1. Open the Admin Center and pick **Host the directory on this computer**. The setup wizard creates your domain and the first Domain Admin, then asks you to pick a **security baseline** (Custom, Minimal, Home/Homelab, Small Business, Enterprise, Education or High Security) and layer modifiers on top: security level, patch rings, device ownership, admin model, USB policy, remote access, compliance packs (HIPAA, PCI DSS, SOC 2, CMMC, GDPR, FERPA, CIPA) and rollout (everyone, a Pilot OU first, or review first). A review screen shows every OU, group and GPO before anything is created. Turn on **Launch at startup** so the directory comes back after a reboot.
2. Go to **Enrollment**, create a token, and run the command it shows on each computer (as Administrator / root):
   ```bash
   opendirectory-agent enroll --server https://<host>:8443 --token XXXX-XXXX-XXXX-XXXX --fingerprint <sha256> --install
   ```
3. Create users and add them to a group, then add a **Sign-in accounts** setting to a GPO. The agent creates matching local accounts on each endpoint, so those people can log in.

Or run a headless server with no GUI at all:

```bash
sudo opendirectory-server install-service     # systemd, launchd or a Windows Service; starts at boot
od init --server https://<host>:8443 --fingerprint <sha256>   # interactive wizard, or pass --baseline, --organization, ... flags
od status
```

`od init --help` lists every flag, so setup can be fully scripted (`OD_PASSWORD` supplies the password). `od init --dry-run` prints the plan without creating anything.

### Identity and remote support

- **Password policies:** a Default Domain Password Policy plus fine-grained policies (PSOs) for groups, with history, expiry and lockout. Users change their own password, or redeem a one-time code from the help desk, with "Change password" on any sign-in screen.
- **LAPS:** each endpoint keeps a local admin account with its own rotating password, stored encrypted on the server; viewing it is audited.
- **Recycle Bin:** deleted users, groups, OUs, GPOs and scripts come back with their memberships and passwords for 180 days.
- **Patch Management:** pending OS updates on every endpoint, with scan and install for endpoints, groups or OUs.
- **Remote shell and desktop:** an interactive terminal on any endpoint, and RDP, VNC or SSH through an encrypted tunnel (desktop app). Endpoints never need inbound ports.

### Network Services

The Management Server can host the network roles a Windows Server usually provides: a **DNS server** for the domain (with records for every endpoint), a **DHCP server**, an internal **certificate authority** (with a GPO setting that makes endpoints trust it) and a **RADIUS server** for VPNs and network devices (PAP and MS-CHAPv2). Each is off until you turn it on in **Network Services**.

### Docker and web mode

The Management Server can serve the Admin Center to any browser (`--web-ui <folder>`, or a `web` folder next to the binary). The official image does this out of the box:

```bash
docker compose up -d     # builds the image from this repository and starts it on port 8443
```

A [`docker-compose.yml`](docker-compose.yml) is included and works as a Portainer stack. See [docs/DOCKER.md](docs/DOCKER.md).

### Backup & Restore

One `.odbackup` file holds the whole configuration (directory database, master key, TLS certificate), encrypted with a passphrase (Argon2id + XChaCha20-Poly1305). Backup plans run on a schedule (hourly, daily or weekly) with retention (keep the newest N, delete after N days) to a local folder or mounted SMB/NFS share, SFTP (host key pinned), FTPS/FTP, WebDAV, S3-compatible storage (AWS, R2, B2, Wasabi, MinIO), any HTTPS endpoint on any port, Google Drive, OneDrive or Dropbox. Destination credentials are stored encrypted and never shown again. Restores are verified first and applied at the next server start, keeping the old data in a `pre-restore` folder.

### Script Library and GPO scripts

Save scripts with parameters (passed as `OD_PARAM_<NAME>` environment variables, never pasted into the script), start from 20 built-in templates for Windows, macOS and Linux, and run them against any mix of endpoints, security groups (computer members) and OUs. Attach a script to a GPO to run it **at startup, at sign-in, on a schedule or once**; it inherits through OU links like GPO Scripts in AD. From the CLI: `od script list`, `od script run "Disk usage report" --ou Servers -p PATH=/var`.

## Repository layout

```
crates/od-core     shared model, Group Policy engine (RSoP), agent protocol
crates/od-client   HTTPS client with certificate pinning
crates/od-server   Management Server (axum + SQLite)
crates/od-agent    Endpoint Agent
crates/od-cli      `od`
apps/admin-center  Admin Center (React + Tauri)
docs/              architecture and roadmap
```

## Development

New to Rust, Cargo or Tauri? Start with [docs/GUIDE.md](docs/GUIDE.md), a plain-language tour of the whole project.

Every developer task has a script in `scripts/`: `.sh` for Linux and macOS, `.ps1` for Windows. They all hand off to `xtask/`, a small Rust program, so each task behaves the same on every OS. If Rust isn't installed yet, the scripts tell you how to install it.

```bash
./scripts/setup.sh        # first time: UI deps, rustfmt/clippy, git hooks, then doctor
./scripts/doctor.sh       # check your toolchain and environment, with a fix for anything missing
./scripts/dev.sh          # Admin Center with hot reload
./scripts/dev.sh --browser   # dev Management Server + UI in your browser
./scripts/test.sh         # Rust tests + UI typecheck
./scripts/lint.sh         # rustfmt, clippy (warnings are errors), UI typecheck
./scripts/fmt.sh          # format Rust code
./scripts/build.sh --app  # release binaries + the Admin Center installer for this OS
./scripts/clean.sh        # remove build output
./scripts/commit.sh       # guided Conventional Commit
./scripts/x.sh help       # everything else
```

On Windows, run the same names with `.ps1` in PowerShell, for example `.\scripts\dev.ps1 --browser`. If PowerShell refuses to run scripts, allow local scripts once with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`.

| Script | Does |
| --- | --- |
| `setup` | First-time setup: UI dependencies, Rust components, git hooks, then `doctor` |
| `doctor` | Checks Rust, Node, OS packages, git and signing setup, and prints a fix for each problem |
| `dev` | Runs the Admin Center with hot reload (`--browser` for a dev server + browser UI) |
| `build` | Release build of the server, agent and CLI (`--app` adds the Admin Center installer) |
| `test` | Rust tests and the UI typecheck |
| `lint` | Formatting check, clippy and the UI typecheck |
| `check` | `lint` + `test`, exactly what CI runs |
| `fmt` | Formats all Rust code |
| `clean` | Deletes build output (`target/`, UI `dist/`) |
| `commit` | Walks you through a Conventional Commit message |
| `hooks` | Turns on the git hooks in `.githooks/` |
| `version` | Shows the version in every manifest and whether they agree |
| `release` | Cuts a release (see below) |
| `updater-keys` | Generates the in-app update signing key (one time) |
| `updater-secrets` | Uploads that key to GitHub Actions secrets with `gh` |
| `x` | Runs any task by name: `./scripts/x.sh <task>` |

Prefer typing Cargo? `cargo x <task>` does the same thing as `./scripts/<task>.sh`.

### Releasing

```bash
./scripts/release.sh patch --dry-run   # preview the version bump and changelog
./scripts/release.sh patch             # or minor / major / 1.4.0
```

`release` checks you're on an up-to-date `main` with a clean tree, runs lint and tests, bumps the version everywhere, prepends the changelog from Conventional Commits, commits `chore(release): vX.Y.Z`, tags it and pushes. Whenever `main` carries a version that has no GitHub Release yet, `.github/workflows/release.yml` builds installers for every OS and publishes the release. If `main` is protected, use `--no-push`, push the commit to a branch and merge it as a PR: the release is published when it lands.

**In-app updates.** The Admin Center checks the latest GitHub Release when it opens and offers *Update now* or *Remind me later*; admins can turn checks off in Settings. Updates are signed: run `./scripts/updater-secrets.sh` once, with your signing key, to give GitHub Actions the key it signs with.

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions and the git hooks.

Read [docs/GUIDE.md](docs/GUIDE.md) for a beginner-friendly tour, and [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the technical detail.

## Downloads

Prebuilt installers for the Admin Center and binaries for the agent, server and CLI are attached to each [GitHub Release](../../releases). Releases are built by `.github/workflows/release.yml` whenever `main` carries a version that has no release yet.
