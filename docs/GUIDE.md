# OpenDirectory, explained

This guide is for anyone who wants to understand how OpenDirectory is built without already knowing Rust, Cargo or Tauri. It explains the tools, walks through the repository folder by folder, shows how the pieces talk to each other while running, and covers the everyday workflow, releases and what comes next.

If you only want to run commands, the short version is in the [README](../README.md#development). For deeper technical detail, see [ARCHITECTURE.md](ARCHITECTURE.md).

---

## 1. What OpenDirectory is, in one paragraph

OpenDirectory does the day-to-day job of Windows Server with Active Directory and Group Policy, plus some of what an RMM tool does (inventory, remote scripts), but it manages Windows, macOS and Linux from one console. There are four programs:

| Program | Think of it as | Runs where |
| --- | --- | --- |
| **Admin Center** | The management console (like Active Directory Users and Computers plus the Group Policy Management Console) | An admin's desktop |
| **Management Server** | The domain controller: it holds the directory database and computes policy | Inside the Admin Center, or on its own server |
| **Endpoint Agent** | The client-side service that applies policy and reports back | Every managed computer |
| **`od` CLI** | PowerShell's AD module: everything the console does, from a terminal | Anywhere |

---

## 2. The tools, and why they were picked

### Rust: the programming language

Rust is a programming language, like C, C# or Go. It compiles to a single native program (an `.exe` on Windows, a plain binary on macOS and Linux), so nothing extra needs to be installed on the machines that run it. No .NET runtime, no Java, no Python.

It was chosen because:

- **The agent runs as SYSTEM / root on every endpoint.** A crash or a memory bug there is a security problem. Rust's compiler refuses to build code with whole classes of memory bugs (use-after-free, buffer overflows, data races) that cause most security holes in C and C++ software.
- **One codebase, three operating systems.** The same Rust code builds for Windows, macOS and Linux. Where an OS differs (creating a local account, configuring the firewall), the code has a small branch per OS.
- **Small and fast.** The agent sits in the background on every computer, so it should use very little memory and CPU. Rust programs are about as lean as C ones.
- **Shared code.** The server, agent, CLI and desktop app are all Rust, so they share one definition of what a user, a GPO or a check-in message looks like. If one side changes the format, the others fail to compile instead of breaking in production.

You don't need to know Rust to use the scripts in `scripts/`. You need it installed, because the scripts compile the code.

### Cargo: Rust's build tool and package manager

Cargo comes with Rust. It does the jobs that `npm`, `pip` or `make` do in other languages:

- **Builds the code.** `cargo build` compiles a program; `cargo test` runs its tests.
- **Downloads libraries.** A Rust library is called a **crate**. Each project lists the crates it needs in a file called `Cargo.toml`, and Cargo fetches them from [crates.io](https://crates.io). `Cargo.lock` records the exact versions so every build is the same.
- **Workspaces.** One repository can hold several crates that build together. OpenDirectory's root `Cargo.toml` is a workspace listing all of them, and they share one `target/` folder for build output.

### xtask, and why there are scripts

Developer chores (set up, run in dev mode, lint, cut a release) are written as a small Rust program in `xtask/`, a common Rust convention. Because it's Rust, it works identically on Windows, macOS and Linux, which shell scripts alone can't promise.

Running it directly means typing `cargo x <task>`. The scripts in `scripts/` are shortcuts so you don't have to: `./scripts/dev.sh` (or `.\scripts\dev.ps1` on Windows) does exactly what `cargo x dev` does. Each script just finds the repository, checks that Rust is installed (and tells you how to install it if not), then hands off to xtask.

### Tauri: how the desktop app is made

Tauri is a framework for building desktop apps. The screens are built like a website (HTML, CSS and TypeScript, here with React), and Tauri shows them inside the operating system's built-in web view: WebView2 on Windows, WebKit on macOS, WebKitGTK on Linux. Everything behind the screens is Rust.

It was chosen over Electron (what Slack, VS Code and Teams use) because:

- **Size.** Electron ships a full copy of Chrome with every app (100+ MB). Tauri uses the web view that's already on the OS, so the installer is a few megabytes.
- **Rust behind the UI.** The Admin Center can run the Management Server *inside itself*, because the server is a Rust library and so is the app's backend. That's what makes "Host the directory on this computer" work with no separate install.
- **Security.** The UI can only call the specific Rust functions the app exposes. It has no network access of its own.
- **Built-in signed updates** through the Tauri updater plugin.

### The rest of the stack

| Piece | Used for |
| --- | --- |
| **React + TypeScript** | The Admin Center's screens (`apps/admin-center/src`). TypeScript is JavaScript with types, so mistakes are caught before running. |
| **Vite** | Serves the UI during development and reloads it the moment you save a file. |
| **Tailwind CSS** | Styling. |
| **Node.js / npm** | Needed only to build the UI. Not needed on endpoints or servers. |
| **axum** | The Rust web framework the Management Server's REST API is built on. |
| **SQLite** | The directory database: a single file, no database server to install. |
| **tokio** | Lets the Rust programs handle many connections at once. |

---

## 3. A tour of the repository

```
OpenDirectory/
├── Cargo.toml          Workspace: lists every Rust crate and the shared version number
├── Cargo.lock          Exact versions of every downloaded library
├── .cargo/config.toml  Defines the `cargo x` shortcut
├── crates/             The Rust programs and libraries
│   ├── od-core/        Shared definitions and the Group Policy engine
│   ├── od-client/      HTTPS client used by the CLI and Admin Center
│   ├── od-server/      Management Server
│   ├── od-agent/       Endpoint Agent
│   └── od-cli/         The `od` command
├── apps/admin-center/  The desktop app
│   ├── src/            React UI: pages, components, glossary
│   └── src-tauri/      Rust side of the app, plus tauri.conf.json
├── xtask/              Developer tasks (`cargo x …`)
├── scripts/            Shortcuts for every xtask task (.sh and .ps1)
├── docs/               This guide, architecture and roadmap
├── .github/workflows/  CI and release automation on GitHub Actions
├── .githooks/          Checks that run on commit and push
├── CHANGELOG.md        Generated at each release
└── CONTRIBUTING.md     Commit conventions and code style
```

### `crates/od-core`: the shared language

Every other part depends on this. It defines:

- **The directory model** (`model.rs`): organization, OUs, users, security groups, endpoints, GPOs, jobs.
- **The agent protocol** (`protocol.rs`): exactly what an agent sends at check-in and what the server sends back.
- **The Group Policy engine** (`policy.rs`): given an endpoint's OU and all GPO links, it works out the Resultant Set of Policy (RSoP), honoring inheritance down the OU tree and GPO priority. The settings catalog also records which OS supports each setting and its AD equivalent.

### `crates/od-server`: the Management Server

The domain controller equivalent.

- `db.rs` stores everything in SQLite.
- `api/` is the REST API at `/api/v1`: directory objects, endpoints, policies, and the agent check-in endpoint.
- `auth.rs` handles admin sign-in (Argon2id password hashes, session tokens, account lockout) and agent authentication.
- `tls.rs` creates a self-signed certificate on first start. Clients pin its SHA-256 fingerprint, the way SSH pins host keys, so there's no certificate authority to set up.
- `crypto.rs` seals directory passwords so they can be handed to agents for creating local accounts.
- `lib.rs` exposes `start()`, so the server can run inside the Admin Center. `main.rs` is the standalone `opendirectory-server` program, which calls the same function.

### `crates/od-agent`: the Endpoint Agent

Installed on each managed computer as a Windows Service, a launchd daemon on macOS, or a systemd unit on Linux (`service.rs`). Its parts:

- `agent.rs`: the check-in loop, every 15 seconds.
- `inventory.rs`: hardware, OS, network and storage details.
- `policy.rs`: applies GPO settings using each OS's native tools (`netsh`, `pwpolicy`, `ufw` and so on) and reports compliance.
- `accounts.rs`: creates and disables the local sign-in accounts that GPOs assign.
- `jobs.rs`: runs remote scripts and other queued jobs.
- `state.rs`: remembers its enrollment and which local accounts it created, so it never touches accounts it didn't make.

### `crates/od-client` and `crates/od-cli`

`od-client` is a small HTTPS client with certificate pinning, shared by the CLI and the Admin Center. `od-cli` is the `od` command. `od explain <term>` prints the same plain-language glossary that the Admin Center's Learn page shows.

### `apps/admin-center`: the desktop app

- `src/pages/` has one file per screen: Overview, Users, Groups, OrgUnits, Endpoints, Policies, Enrollment, Jobs, Audit, Learn, Settings, and the Onboarding wizard.
- `src/lib/api.ts` is how screens talk to the server (through Rust, see below). `glossary.ts` holds the Learn-mode explanations. `updates.tsx` handles the update prompt.
- `src-tauri/src/main.rs` is the Rust side: it starts or connects to a Management Server, forwards API calls, runs the tray icon, and wires up the updater.
- `src-tauri/tauri.conf.json` sets the app's name, version, window, installer formats and the updater's public key.

### `xtask/` and `scripts/`

`xtask/src/main.rs` contains every developer task. `scripts/` holds one `.sh` and one `.ps1` per task, plus `x.sh` / `x.ps1` that run any task by name.

### `.github/workflows/`

- `ci.yml` runs on every pull request and every push to `main`: format check, clippy, tests, and builds of the agent and Admin Center on Windows, macOS and Linux.
- `release.yml` runs on every push to `main` and publishes a release when the version is new (section 6).

### `.githooks/`

Git runs these on your machine once `setup` (or `hooks`) has enabled them:

- `commit-msg` rejects commit messages that aren't in the Conventional Commits format.
- `pre-commit` checks Rust formatting.
- `pre-push` runs `lint`.

---

## 4. How the pieces talk to each other

```
 ┌──────────── Admin Center ────────────┐
 │  React UI ──▶ Rust (Tauri) ──▶ embedded Management Server (Host mode)
 └───────────────────┬──────────────────┘
                     │ HTTPS :8443, pinned certificate
   od CLI ──────────▶│◀────────── other admins' Admin Centers
                     ▼
            Management Server
      REST API · SQLite · GPO engine
          ▲          ▲          ▲
          │ check-in every 15 s (agent always calls out)
     Agent (Win)  Agent (macOS)  Agent (Linux)
```

**Inside the Admin Center.** The React screens never touch the network. When a screen needs data, it calls a Rust function named `api_request`, and Rust makes the request. In **Host** mode that request goes to the Management Server running inside the same app over loopback only, so nothing leaves the machine and it works fully offline. Closing the window leaves the server running from the tray. In **Connect** mode Rust sends the request over HTTPS to a remote server, checking its pinned certificate fingerprint.

**Admins and the CLI.** Admins sign in with a username and password and get a session token, sent as `Authorization: Bearer …` on each request. Five wrong passwords lock that username out for five minutes. Every change lands in the audit log with the admin who made it.

**Enrolling an endpoint.** On the Enrollment page you create a token. Running `opendirectory-agent enroll --server … --token … --fingerprint … --install` on a computer exchanges the token for that endpoint's own permanent credential and installs the agent as a service that starts at boot.

**Check-in.** Every 15 seconds the agent calls the server with its compliance results and, when something changed, its inventory. The server replies with:

1. The endpoint's RSoP: every setting that applies to it, and which GPO it came from.
2. The local sign-in accounts it should have, with passwords for accounts that are new or changed.
3. Any queued jobs, such as a remote script.

The agent applies all of that, and the next check-in reports how it went. Because the agent always calls out, endpoints need no open inbound ports and work behind NAT and home routers. If the server can't be reached, the agent backs off and keeps trying.

**Why there's no domain join.** Windows Home can't join an AD domain and ignores Group Policy. Instead of relying on the OS's domain features, the agent creates real local accounts with the directory username and password, and applies settings through local APIs that exist on every edition of Windows (and on macOS and Linux). That's how one GPO can cover a Windows Home laptop, a Mac and an Ubuntu server.

---

## 5. The development workflow

### One-time setup

1. Install **Rust** from [rustup.rs](https://rustup.rs). On Linux and macOS:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
   On Windows: `winget install Rustlang.Rustup`. Open a new terminal afterwards.
2. Install **Node.js 20 or newer** from [nodejs.org](https://nodejs.org).
3. On Linux, install the desktop app libraries: `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`. On macOS run `xcode-select --install`. Windows 11 already has WebView2.
4. Run setup, which installs UI dependencies, Rust's formatter and linter, enables the git hooks and runs `doctor`:
   ```bash
   ./scripts/setup.sh
   ```
   On Windows: `.\scripts\setup.ps1`. If PowerShell refuses to run it, run `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` once.

`./scripts/doctor.sh` can be run at any time. It checks everything above and prints the exact fix for anything missing.

The first build takes several minutes because Cargo downloads and compiles every library. Later builds only recompile what changed.

### Day to day

| You want to… | Run |
| --- | --- |
| Work on the Admin Center with live reload | `./scripts/dev.sh` |
| Work on the UI in a browser, no desktop app needed | `./scripts/dev.sh --browser`, then open http://localhost:1420 |
| Run the tests | `./scripts/test.sh` |
| Check formatting and lint before pushing | `./scripts/lint.sh` |
| Run exactly what CI runs | `./scripts/check.sh` |
| Fix formatting | `./scripts/fmt.sh` |
| Build release binaries | `./scripts/build.sh` (add `--app` for the installer) |
| Commit with a well-formed message | `./scripts/commit.sh` |
| Free disk space | `./scripts/clean.sh` |
| See every task | `./scripts/x.sh help` |

On Windows, swap `./scripts/<name>.sh` for `.\scripts\<name>.ps1`.

Don't run the scripts with `sudo`. Nothing in them needs root, and building as root leaves files you can't change later. If you do use sudo, the script says so and runs the task as you instead.

Release binaries end up in `target/release/` and installers in `target/release/bundle/`.

### Commits and pull requests

Commit messages follow **Conventional Commits**: a type, an optional scope, and a short summary, for example `feat(agent): add BitLocker enforcement` or `fix(app): wrap long values in Settings`. The changelog is generated from these, so the format matters. `./scripts/commit.sh` asks you a few questions and writes the message for you.

Work happens on a branch, goes up as a pull request, and CI must pass before it's merged into `main`. See [CONTRIBUTING.md](../CONTRIBUTING.md).

---

## 6. Releases and in-app updates

### Cutting a release

The version number lives in three files that must agree: `Cargo.toml`, `apps/admin-center/package.json` and `apps/admin-center/src-tauri/tauri.conf.json`. `./scripts/version.sh` shows them.

Versions follow semantic versioning, `MAJOR.MINOR.PATCH`: patch for fixes, minor for new features, major for breaking changes.

```bash
./scripts/release.sh patch --dry-run   # preview: new version and changelog, changes nothing
./scripts/release.sh patch             # or minor, major, or an exact version like 1.4.0
```

A real release:

1. Checks you're on an up-to-date `main` with no uncommitted changes.
2. Runs lint and tests.
3. Bumps the version in all three files and updates `Cargo.lock`.
4. Adds a section to `CHANGELOG.md`, grouping commits since the last release into Features, Fixes and so on.
5. Commits `chore(release): vX.Y.Z`, tags it `vX.Y.Z`, and pushes.

If `main` is protected, add `--no-push`, push the release commit to a branch, and merge it as a pull request instead.

### What GitHub does next

`release.yml` runs on every push to `main`. If the version in `Cargo.toml` has no GitHub Release yet, it:

1. Builds the Admin Center installers: `.msi` and `.exe` for Windows, a universal `.dmg` for macOS, and `.deb`, `.rpm` and `.AppImage` for Linux.
2. Builds the agent, server and CLI binaries for each OS.
3. Signs the update bundles, if the signing key is in GitHub's secrets.
4. Publishes everything as a GitHub Release, along with `latest.json`, the file the Admin Center checks for updates.

If the version has already been released, it does nothing. So merging ordinary changes never publishes anything; only a version bump does.

### How the Admin Center updates itself

When the Admin Center opens, it downloads `latest.json` from the newest GitHub Release. If there's a newer version, it offers **Update now** or **Remind me later** (which waits a day). Admins can turn checks off in Settings → Updates.

Updates are signed. The private signing key stays with the maintainer and in GitHub's secrets; the public half is built into the app (`tauri.conf.json`). The app refuses any download whose signature doesn't match, so a tampered file can't be installed. Two one-time steps set this up:

```bash
./scripts/updater-keys.sh      # creates the key pair and writes the public key into tauri.conf.json
./scripts/updater-secrets.sh   # uploads the private key and its password to GitHub secrets (needs `gh`)
```

Keep the private key and its password safe. A new key would stop existing installs from accepting updates.

### Agents

The Enrollment page downloads the agent straight from the latest release, or you can copy the file over by hand for offline networks. Agents don't update themselves yet; that's on the roadmap.

---

## 7. What's planned next

From [ROADMAP.md](ROADMAP.md), in order:

1. **Real-hardware validation** on Windows 10/11 Home and Pro and macOS 14+, with signed installers. Linux is verified end to end today.
2. **Revert removed settings.** When a setting leaves a GPO, restore the endpoint's previous value instead of leaving the last applied one.
3. **Live remote sessions.** An interactive remote shell first, then remote desktop with a consent prompt for the user.
4. **Basic mobile device tracking.** iPhone and Android inventory by serial number, IMEI and owner, then real MDM enrollment through Apple MDM and Android Enterprise.
5. **Software deployment and patch reporting.** A package catalog and installed-software list per endpoint.
6. **Domain Controller role.** LDAP and Kerberos, so AD-aware apps and domain-joined Windows Pro machines can authenticate against OpenDirectory, plus sync with an existing AD or Samba domain.
7. **Delegated administration.** Roles narrower than Domain Admin (help desk, read-only), scoped to OUs.
8. **Agent self-update** from GitHub Releases, rolled out by OU.
9. **High availability.** Server replication, and backup and restore from the Admin Center.

Known limitations today: no LDAP/Kerberos yet, settings removed from a GPO aren't reverted, disk encryption is reported but not enforced, remote scripts aren't interactive, and mobile devices aren't managed.

---

## 8. Glossary

| Term | Meaning |
| --- | --- |
| **Crate** | A Rust package: a library or a program. |
| **Workspace** | Several crates in one repository that build together. |
| **`Cargo.toml`** | A crate's (or workspace's) manifest: name, version, dependencies. |
| **Clippy** | Rust's linter. Finds likely mistakes and style problems. |
| **rustfmt** | Rust's code formatter. |
| **rustup** | The installer and version manager for Rust. |
| **Web view** | The OS's built-in browser engine that Tauri draws the UI in. |
| **CI** | Continuous Integration: GitHub Actions running checks on every pull request. |
| **OU** | Organizational Unit: a folder in the directory tree that GPOs link to. |
| **GPO** | Group Policy Object: a named set of settings applied to endpoints. |
| **RSoP** | Resultant Set of Policy: the final settings an endpoint gets after inheritance and priority are applied. |
| **Security Group** | A set of users. GPOs use groups to decide who gets a sign-in account on which endpoints. |
| **Endpoint** | A managed computer running the agent. |
| **Enrollment token** | A short-lived code that lets a new endpoint join the directory. |
| **Certificate pinning** | Trusting one specific server certificate by its fingerprint instead of a certificate authority. |
| **Conventional Commits** | A commit message format (`type(scope): summary`) that the changelog is built from. |

The Admin Center's Learn page and `od explain <term>` cover the directory terms in more depth.
