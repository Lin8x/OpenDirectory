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
# Build everything except the desktop app
cargo build --release

# Desktop app (needs Node 20+ and, on Linux, webkit2gtk 4.1)
cd apps/admin-center && npm ci && npx tauri build
```

1. Open the Admin Center and pick **Host the directory on this computer**. The setup wizard creates your domain, the first Domain Admin, default OUs (Staff, Workstations, Servers, IT), the Domain Admins / Domain Users groups and a Default Endpoint Policy.
2. Go to **Enrollment**, create a token, and run the command it shows on each computer (as Administrator / root):
   ```bash
   opendirectory-agent enroll --server https://<host>:8443 --token XXXX-XXXX-XXXX-XXXX --fingerprint <sha256> --install
   ```
3. Create users and add them to a group, then add a **Sign-in accounts** setting to a GPO. The agent creates matching local accounts on each endpoint, so those people can log in.

Or run a standalone server:

```bash
opendirectory-server --data-dir /var/lib/opendirectory --listen 0.0.0.0:8443
od login --server https://<host>:8443 --fingerprint <sha256> -u admin
od status
```

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

All developer tasks run through `cargo x`, a small Rust program in `xtask/`, so they work the same on Windows, macOS and Linux with nothing extra to install.

```bash
cargo x setup        # first time: UI deps, rustfmt/clippy, git hooks, then doctor
cargo x doctor       # check your toolchain and environment, with a fix for anything missing
cargo x dev          # Admin Center with hot reload
cargo x dev --browser   # dev Management Server + UI in your browser
cargo x test         # Rust tests + UI typecheck
cargo x lint         # rustfmt, clippy (warnings are errors), UI typecheck
cargo x fmt          # format Rust code
cargo x build --app  # release binaries + the Admin Center installer for this OS
cargo x commit       # guided Conventional Commit
cargo x help         # everything else
```

### Releasing

```bash
cargo x release patch --dry-run   # preview the version bump and changelog
cargo x release patch             # or minor / major / 1.4.0
```

`release` checks you're on an up-to-date `main` with a clean tree, runs lint and tests, bumps the version everywhere, prepends the changelog from Conventional Commits, commits `chore(release): vX.Y.Z`, tags it and pushes. Whenever `main` carries a version that has no GitHub Release yet, `.github/workflows/release.yml` builds installers for every OS and publishes the release. If `main` is protected, use `--no-push`, push the commit to a branch and merge it as a PR: the release is published when it lands.

**In-app updates.** The Admin Center checks the latest GitHub Release when it opens and offers *Update now* or *Remind me later*; admins can turn checks off in Settings. Updates are signed: run `cargo x updater-secrets` once, with your signing key, to give GitHub Actions the key it signs with.

See [CONTRIBUTING.md](CONTRIBUTING.md) for commit conventions and the git hooks.

Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together.

## Downloads

Prebuilt installers for the Admin Center and binaries for the agent, server and CLI are attached to each [GitHub Release](../../releases). Releases are built by `.github/workflows/release.yml` whenever a `v*` tag is pushed.
