//! Developer tasks for OpenDirectory. Run `cargo x help`.
//!
//! Everything here is plain Rust and std, so it works the same on Windows,
//! macOS and Linux with nothing to install beyond the toolchain.

use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const APP_DIR: &str = "apps/admin-center";
const CORE_BINS: &[&str] = &["od-server", "od-agent", "od-cli"];
const COMMIT_TYPES: &[(&str, &str)] = &[
    ("feat", "A new feature"),
    ("fix", "A bug fix"),
    ("docs", "Documentation only"),
    ("style", "Formatting, no code change"),
    ("refactor", "Code change that neither fixes a bug nor adds a feature"),
    ("perf", "Performance improvement"),
    ("test", "Adding or fixing tests"),
    ("build", "Build system or dependencies"),
    ("ci", "CI configuration"),
    ("chore", "Maintenance"),
    ("revert", "Revert a previous commit"),
];

const HELP: &str = "\
OpenDirectory developer tasks

USAGE: cargo x <task> [options]

Getting started
  doctor                 Check your toolchain and environment, with fixes for anything missing
  setup                  Install frontend deps, Rust components and git hooks, then run doctor

Everyday
  dev                    Run the Admin Center desktop app with hot reload
  dev --browser          Run a dev Management Server + the UI in your browser (no webview needed)
  build                  Release build of the server, agent and CLI
  build --app            Also build the Admin Center installer for this OS
  test                   Rust tests + UI typecheck
  lint                   rustfmt check, clippy (warnings are errors), UI typecheck
  fmt                    Format all Rust code
  check                  lint + test (what CI runs)
  clean                  Remove build output (target/, UI dist/)

Git
  commit                 Write a Conventional Commit interactively (feat:, fix:, ...)
  hooks                  Install the git hooks in .githooks (commit-msg, pre-commit, pre-push)

Releasing
  version                Show the version in every manifest and whether they agree
  release <v|patch|minor|major> [--dry-run] [--no-push] [--skip-checks]
                         Bump versions, update CHANGELOG.md, commit, tag vX.Y.Z and push.
                         GitHub Actions then builds installers and publishes the release
                         (with signed in-app updates when the updater secrets are set).
  updater-keys           Generate the in-app update signing key (one time)
  updater-secrets [key]  Upload the signing key to GitHub Actions secrets using `gh`
";

fn main() {
    if let Err(e) = run() {
        eprintln!("\n{} {e:#}", paint("31", "error:"));
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    env::set_current_dir(root()).context("finding the repository root")?;
    let args: Vec<String> = env::args().skip(1).collect();
    let task = args.first().map(String::as_str).unwrap_or("help");
    let rest = &args[args.len().min(1)..];
    let flag = |f: &str| rest.iter().any(|a| a == f);
    match task {
        "doctor" => doctor(),
        "setup" => setup(),
        "dev" => dev(flag("--browser")),
        "build" => build(flag("--app")),
        "test" => test(),
        "lint" => lint(),
        "fmt" | "format" => sh("cargo", &["fmt", "--all"]),
        "check" | "ci" => lint().and_then(|_| test()),
        "clean" => clean(),
        "commit" => commit(),
        "hooks" => hooks(),
        "version" | "versions" => versions(),
        "release" => release(rest),
        "updater-keys" => updater_keys(),
        "updater-secrets" => updater_secrets(rest.first().map(String::as_str)),
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        other => bail!("unknown task `{other}`. Run `cargo x help`."),
    }
}

// ------------------------------------------------------------------ helpers

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the repo root").to_path_buf()
}

fn paint(code: &str, s: &str) -> String {
    if env::var_os("NO_COLOR").is_some() {
        s.to_string()
    } else {
        format!("\x1b[{code}m{s}\x1b[0m")
    }
}

fn step(s: &str) {
    println!("\n{} {s}", paint("1;34", "==>"));
}

/// npm/npx are .cmd shims on Windows and must be launched by that name.
fn program(p: &str) -> String {
    if cfg!(windows) && matches!(p, "npm" | "npx") {
        format!("{p}.cmd")
    } else {
        p.to_string()
    }
}

fn cmd(p: &str, args: &[&str]) -> Command {
    let mut c = Command::new(program(p));
    c.args(args);
    c
}

fn sh(p: &str, args: &[&str]) -> Result<()> {
    sh_in(".", p, args)
}

fn sh_in(dir: &str, p: &str, args: &[&str]) -> Result<()> {
    println!("{}", paint("2", &format!("$ {p} {}", args.join(" "))));
    let st = cmd(p, args).current_dir(dir).status().with_context(|| format!("running `{p}` (is it installed? try `cargo x doctor`)"))?;
    if !st.success() {
        bail!("`{p} {}` failed ({st})", args.join(" "));
    }
    Ok(())
}

fn output(p: &str, args: &[&str]) -> Option<String> {
    let o = cmd(p, args).stderr(Stdio::null()).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn prompt(q: &str) -> Result<String> {
    print!("{q}");
    io::stdout().flush()?;
    let mut s = String::new();
    io::stdin().lock().read_line(&mut s)?;
    Ok(s.trim().to_string())
}

fn confirm(q: &str) -> Result<bool> {
    Ok(matches!(prompt(&format!("{q} [y/N] "))?.to_lowercase().as_str(), "y" | "yes"))
}

/// Parses "1.80.0", "v20.11.1", "rustc 1.80.0 (abc 2024-07-21)" into (major, minor, patch).
fn semver(s: &str) -> Option<(u64, u64, u64)> {
    let tok = s.split_whitespace().find(|t| t.trim_start_matches('v').chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let core = tok.trim_start_matches('v').split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next().unwrap_or(Ok(0)).ok()?, it.next().unwrap_or(Ok(0)).ok()?))
}

// ------------------------------------------------------------------ doctor

#[derive(Default)]
struct Report {
    errors: u32,
    warnings: u32,
}

impl Report {
    fn ok(&self, what: &str, detail: &str) {
        println!("  {} {what} {}", paint("32", "✓"), paint("2", detail));
    }
    fn warn(&mut self, what: &str, fix: &str) {
        self.warnings += 1;
        println!("  {} {what}\n      {}", paint("33", "!"), paint("2", fix));
    }
    fn fail(&mut self, what: &str, fix: &str) {
        self.errors += 1;
        println!("  {} {what}\n      {}", paint("31", "✗"), paint("2", fix));
    }
    fn tool(&mut self, name: &str, args: &[&str], min: Option<(u64, u64, u64)>, required: bool, fix: &str) -> Option<String> {
        match output(name, args) {
            Some(v) => {
                let line = v.lines().next().unwrap_or_default().to_string();
                match (min, semver(&line)) {
                    (Some(m), Some(have)) if have < m => {
                        let msg = format!("{name} {}.{}.{} is older than {}.{}.{}", have.0, have.1, have.2, m.0, m.1, m.2);
                        if required {
                            self.fail(&msg, fix)
                        } else {
                            self.warn(&msg, fix)
                        }
                    }
                    _ => self.ok(name, &line),
                }
                Some(line)
            }
            None if required => {
                self.fail(&format!("{name} not found"), fix);
                None
            }
            None => {
                self.warn(&format!("{name} not found (optional)"), fix);
                None
            }
        }
    }
}

fn doctor() -> Result<()> {
    let mut r = Report::default();
    println!("OpenDirectory doctor ({} / {})", env::consts::OS, env::consts::ARCH);

    step("Rust");
    r.tool("rustc", &["--version"], Some((1, 80, 0)), true, "Install or update Rust: https://rustup.rs then `rustup update`");
    r.tool("cargo", &["--version"], None, true, "Install Rust: https://rustup.rs");
    if output("cargo", &["fmt", "--version"]).is_some() {
        r.ok("rustfmt", "");
    } else {
        r.fail("rustfmt missing", "rustup component add rustfmt");
    }
    if output("cargo", &["clippy", "--version"]).is_some() {
        r.ok("clippy", "");
    } else {
        r.fail("clippy missing", "rustup component add clippy");
    }

    step("Frontend (Admin Center)");
    r.tool("node", &["--version"], Some((20, 0, 0)), true, "Install Node.js 20 or newer: https://nodejs.org");
    r.tool("npm", &["--version"], None, true, "npm ships with Node.js");
    if Path::new(APP_DIR).join("node_modules").is_dir() {
        r.ok("UI dependencies installed", "");
    } else {
        r.fail("UI dependencies not installed", "cargo x setup   (or: cd apps/admin-center && npm ci)");
    }

    step("Desktop app prerequisites");
    match env::consts::OS {
        "linux" => {
            for (pkg, apt) in [
                ("webkit2gtk-4.1", "libwebkit2gtk-4.1-dev"),
                ("ayatana-appindicator3-0.1", "libayatana-appindicator3-dev"),
                ("librsvg-2.0", "librsvg2-dev"),
            ] {
                if output("pkg-config", &["--exists", pkg]).is_some() {
                    r.ok(pkg, "");
                } else {
                    r.warn(
                        &format!("{pkg} not found (needed for the desktop app)"),
                        &format!("sudo apt install {apt}   (Fedora: see https://tauri.app/start/prerequisites/)"),
                    );
                }
            }
        }
        "macos" => {
            if output("xcode-select", &["-p"]).is_some() {
                r.ok("Xcode Command Line Tools", "");
            } else {
                r.fail("Xcode Command Line Tools missing", "xcode-select --install");
            }
        }
        "windows" => {
            let wv = output(
                "reg",
                &["query", r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}", "/v", "pv"],
            );
            if wv.is_some() {
                r.ok("WebView2 runtime", "");
            } else {
                r.warn(
                    "WebView2 runtime not detected",
                    "Installed by default on Windows 11; otherwise https://developer.microsoft.com/microsoft-edge/webview2/",
                );
            }
            r.ok("MSVC build tools", "(needed by Rust on Windows; rustup installs them if missing)");
        }
        _ => {}
    }

    step("Git and releasing");
    r.tool("git", &["--version"], None, true, "Install git: https://git-scm.com");
    match output("git", &["config", "core.hooksPath"]) {
        Some(p) if p == ".githooks" => r.ok("git hooks installed", ".githooks"),
        _ => r.warn("git hooks not installed", "cargo x hooks"),
    }
    if output("git", &["config", "user.email"]).is_none() {
        r.warn("git user.email not set", "git config --global user.email you@example.com");
    }
    if r.tool(
        "gh",
        &["--version"],
        None,
        false,
        "Optional: GitHub CLI for `cargo x updater-secrets` and watching releases: https://cli.github.com",
    )
    .is_some()
        && output("gh", &["auth", "status"]).is_none()
    {
        r.warn("gh is not signed in", "gh auth login");
    }

    step("Project");
    match collect_versions() {
        Ok(vs) => {
            let first = &vs[0].1;
            if vs.iter().all(|(_, v)| v == first) {
                r.ok("versions agree", first);
            } else {
                r.fail("manifest versions disagree", "cargo x version   then fix by hand or run a release");
            }
        }
        Err(e) => r.fail("couldn't read versions", &e.to_string()),
    }
    let conf = fs::read_to_string(format!("{APP_DIR}/src-tauri/tauri.conf.json")).unwrap_or_default();
    if conf.contains("\"pubkey\"") {
        r.ok("in-app updater public key configured", "");
    } else {
        r.warn("in-app updater public key missing", "cargo x updater-keys");
    }

    println!();
    if r.errors == 0 {
        println!("{} {} warning(s). You're ready to build.", paint("32", "All required checks passed."), r.warnings);
        Ok(())
    } else {
        bail!("{} problem(s) and {} warning(s) found. Fix the ✗ items above.", r.errors, r.warnings)
    }
}

// ------------------------------------------------------------------ everyday tasks

fn setup() -> Result<()> {
    step("Rust components");
    if output("rustup", &["--version"]).is_some() {
        sh("rustup", &["component", "add", "rustfmt", "clippy"])?;
    }
    step("UI dependencies");
    sh_in(APP_DIR, "npm", &["ci"])?;
    step("Git hooks");
    hooks()?;
    step("Doctor");
    doctor()
}

fn dev(browser: bool) -> Result<()> {
    if !browser {
        return sh_in(APP_DIR, "npx", &["tauri", "dev"]);
    }
    let data = root().join("data").join("dev-server");
    fs::create_dir_all(&data)?;
    println!("Starting a dev Management Server on http://127.0.0.1:8080 (data in {})", data.display());
    let mut server = Command::new("cargo")
        .args(["run", "-p", "od-server", "--", "--insecure-http", "--listen", "127.0.0.1:8080", "--data-dir"])
        .arg(&data)
        .env("OD_DEV_CORS", "1")
        .spawn()
        .context("starting the dev server")?;
    println!("Open http://localhost:1420 once Vite is ready. Ctrl+C stops both.");
    let ui = sh_in(APP_DIR, "npm", &["run", "dev"]);
    let _ = server.kill();
    ui
}

fn build(app: bool) -> Result<()> {
    step("Server, agent and CLI");
    let mut args = vec!["build", "--release"];
    for b in CORE_BINS {
        args.extend(["-p", b]);
    }
    sh("cargo", &args)?;
    if app {
        step("Admin Center");
        sh_in(APP_DIR, "npx", &["tauri", "build"])?;
        println!("Installers are in target/release/bundle/");
    }
    println!("Binaries are in target/release/");
    Ok(())
}

fn test() -> Result<()> {
    step("Rust tests");
    sh("cargo", &["test", "--workspace", "--exclude", "od-admin-center"])?;
    step("UI typecheck");
    sh_in(APP_DIR, "npx", &["tsc", "-b"])
}

fn lint() -> Result<()> {
    step("rustfmt");
    sh("cargo", &["fmt", "--all", "--check"]).context("code isn't formatted; run `cargo x fmt`")?;
    step("clippy");
    sh("cargo", &["clippy", "--workspace", "--exclude", "od-admin-center", "--all-targets", "--", "-D", "warnings"])?;
    step("UI typecheck");
    sh_in(APP_DIR, "npx", &["tsc", "-b"])
}

fn clean() -> Result<()> {
    sh("cargo", &["clean"])?;
    for d in [format!("{APP_DIR}/dist"), format!("{APP_DIR}/src-tauri/gen")] {
        if Path::new(&d).exists() {
            fs::remove_dir_all(&d)?;
            println!("removed {d}");
        }
    }
    Ok(())
}

// ------------------------------------------------------------------ git

fn hooks() -> Result<()> {
    sh("git", &["config", "core.hooksPath", ".githooks"])?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in fs::read_dir(".githooks")? {
            let p = e?.path();
            let mut perm = fs::metadata(&p)?.permissions();
            perm.set_mode(0o755);
            fs::set_permissions(&p, perm)?;
        }
    }
    println!("Git hooks enabled: commit messages are checked, Rust formatting is checked before each commit and lint runs before push.");
    println!("Skip once with `git commit --no-verify` / `git push --no-verify`.");
    Ok(())
}

fn commit() -> Result<()> {
    let staged = output("git", &["diff", "--cached", "--name-only"]).unwrap_or_default();
    if staged.is_empty() {
        if confirm("Nothing is staged. Stage all changes (git add -A)?")? {
            sh("git", &["add", "-A"])?;
        } else {
            bail!("nothing to commit");
        }
    }
    println!("Staged:\n{}", output("git", &["diff", "--cached", "--stat"]).unwrap_or_default());
    println!("\nType of change:");
    for (i, (t, d)) in COMMIT_TYPES.iter().enumerate() {
        println!("  {:>2}. {:<9} {}", i + 1, t, paint("2", d));
    }
    let ty = loop {
        let a = prompt("Choose 1-11 or type a name: ")?;
        if let Ok(n) = a.parse::<usize>() {
            if (1..=COMMIT_TYPES.len()).contains(&n) {
                break COMMIT_TYPES[n - 1].0;
            }
        }
        if let Some((t, _)) = COMMIT_TYPES.iter().find(|(t, _)| *t == a) {
            break t;
        }
    };
    let scope = prompt("Scope, optional (agent, server, cli, app, core, docs…): ")?;
    let summary = loop {
        let s = prompt("Summary (imperative, e.g. \"add BitLocker enforcement\"): ")?;
        if !s.is_empty() && s.len() <= 72 {
            break s;
        }
        println!("Keep it between 1 and 72 characters.");
    };
    let body = prompt("Longer description, optional (one line): ")?;
    let breaking = prompt("Breaking change? Describe it, or leave empty: ")?;
    let issue = prompt("Closes issue #, optional: ")?;

    let mut header = ty.to_string();
    if !scope.is_empty() {
        header.push_str(&format!("({scope})"));
    }
    if !breaking.is_empty() {
        header.push('!');
    }
    let mut msg = format!("{header}: {summary}");
    if !body.is_empty() {
        msg.push_str(&format!("\n\n{body}"));
    }
    if !breaking.is_empty() {
        msg.push_str(&format!("\n\nBREAKING CHANGE: {breaking}"));
    }
    if !issue.is_empty() {
        msg.push_str(&format!("\n\nCloses #{}", issue.trim_start_matches('#')));
    }
    println!("\n{}\n", paint("1", &msg));
    if !confirm("Commit with this message?")? {
        bail!("cancelled");
    }
    sh("git", &["commit", "-m", &msg])
}

// ------------------------------------------------------------------ versions & release

const CARGO_TOML: &str = "Cargo.toml";
const PACKAGE_JSON: &str = "apps/admin-center/package.json";
const PACKAGE_LOCK: &str = "apps/admin-center/package-lock.json";
const TAURI_CONF: &str = "apps/admin-center/src-tauri/tauri.conf.json";

fn json_version(text: &str) -> Option<String> {
    let i = text.find("\"version\"")?;
    let rest = &text[i + 9..];
    let start = rest.find('"')? + 1;
    let end = rest[start..].find('"')? + start;
    Some(rest[start..end].to_string())
}

fn cargo_version(text: &str) -> Option<String> {
    let sec = &text[text.find("[workspace.package]")?..];
    let line = sec.lines().find(|l| l.trim_start().starts_with("version"))?;
    Some(line.split('"').nth(1)?.to_string())
}

fn collect_versions() -> Result<Vec<(&'static str, String)>> {
    let read = |p: &str| fs::read_to_string(p).with_context(|| format!("reading {p}"));
    Ok(vec![
        (CARGO_TOML, cargo_version(&read(CARGO_TOML)?).context("no [workspace.package] version")?),
        (PACKAGE_JSON, json_version(&read(PACKAGE_JSON)?).context("no version in package.json")?),
        (TAURI_CONF, json_version(&read(TAURI_CONF)?).context("no version in tauri.conf.json")?),
    ])
}

fn versions() -> Result<()> {
    let vs = collect_versions()?;
    for (f, v) in &vs {
        println!("  {v:<10} {f}");
    }
    if vs.iter().all(|(_, v)| *v == vs[0].1) {
        println!("{}", paint("32", "All manifests agree."));
        Ok(())
    } else {
        bail!("manifest versions disagree")
    }
}

fn replace_first(text: &str, from: &str, to: &str) -> Result<String> {
    let i = text.find(from).with_context(|| format!("`{from}` not found"))?;
    Ok(format!("{}{to}{}", &text[..i], &text[i + from.len()..]))
}

fn bump_files(old: &str, new: &str) -> Result<()> {
    let s = fs::read_to_string(CARGO_TOML)?;
    let sec = s.find("[workspace.package]").context("no [workspace.package]")?;
    let (head, tail) = s.split_at(sec);
    fs::write(CARGO_TOML, format!("{head}{}", replace_first(tail, &format!("version = \"{old}\""), &format!("version = \"{new}\""))?))?;
    for f in [PACKAGE_JSON, TAURI_CONF] {
        let s = fs::read_to_string(f)?;
        fs::write(f, replace_first(&s, &format!("\"version\": \"{old}\""), &format!("\"version\": \"{new}\""))?)?;
    }
    // package-lock.json repeats the root version at the top and under packages[""].
    if let Ok(s) = fs::read_to_string(PACKAGE_LOCK) {
        let pat = format!("\"version\": \"{old}\"");
        let mut out = s.clone();
        for _ in 0..2 {
            if let Some(i) = out.find(&pat) {
                if i < 400 {
                    out = format!("{}\"version\": \"{new}\"{}", &out[..i], &out[i + pat.len()..]);
                }
            }
        }
        fs::write(PACKAGE_LOCK, out)?;
    }
    Ok(())
}

fn next_version(cur: (u64, u64, u64), arg: &str) -> Result<String> {
    Ok(match arg {
        "patch" => format!("{}.{}.{}", cur.0, cur.1, cur.2 + 1),
        "minor" => format!("{}.{}.0", cur.0, cur.1 + 1),
        "major" => format!("{}.0.0", cur.0 + 1),
        v => {
            let v = v.trim_start_matches('v');
            let parsed = semver(v).with_context(|| format!("`{v}` isn't a version; use X.Y.Z, patch, minor or major"))?;
            if parsed <= cur {
                bail!("{v} is not newer than the current {}.{}.{}", cur.0, cur.1, cur.2);
            }
            v.to_string()
        }
    })
}

/// Groups commits since the last tag by Conventional Commit type.
fn changelog_section(version: &str, since: Option<&str>) -> String {
    let range = since.map(|t| format!("{t}..HEAD")).unwrap_or_else(|| "HEAD".into());
    let log = output("git", &["log", &range, "--no-merges", "--pretty=format:%s"]).unwrap_or_default();
    let groups: &[(&str, &[&str])] = &[
        ("Breaking changes", &["!"]),
        ("Features", &["feat"]),
        ("Fixes", &["fix"]),
        ("Performance", &["perf"]),
        ("Documentation", &["docs"]),
        ("Other changes", &["refactor", "build", "ci", "test", "style", "chore", "revert"]),
    ];
    let mut buckets: Vec<Vec<String>> = vec![Vec::new(); groups.len() + 1];
    for subject in log.lines().filter(|l| !l.is_empty() && !l.starts_with("chore(release)")) {
        let (head, text) = subject.split_once(": ").unwrap_or(("", subject));
        let breaking = head.ends_with('!');
        let ty = head.trim_end_matches('!').split('(').next().unwrap_or("");
        let scope = head.split_once('(').and_then(|(_, s)| s.split(')').next()).map(|s| format!("**{s}:** ")).unwrap_or_default();
        let idx = if breaking { Some(0) } else { groups.iter().position(|(_, tys)| tys.contains(&ty)) };
        let line = if idx.is_some() { format!("- {scope}{text}") } else { format!("- {subject}") };
        buckets[idx.unwrap_or(groups.len())].push(line);
    }
    let date = output("git", &["log", "-1", "--format=%cs"]).unwrap_or_default();
    let mut s = format!("## v{version} ({date})\n");
    let names: Vec<&str> = groups.iter().map(|(n, _)| *n).chain(["Uncategorized"]).collect();
    for (name, items) in names.iter().zip(&buckets) {
        if !items.is_empty() {
            s.push_str(&format!("\n### {name}\n\n{}\n", items.join("\n")));
        }
    }
    s
}

fn release(args: &[String]) -> Result<()> {
    let dry = args.iter().any(|a| a == "--dry-run");
    let no_push = args.iter().any(|a| a == "--no-push");
    let skip_checks = args.iter().any(|a| a == "--skip-checks");
    let target =
        args.iter().find(|a| !a.starts_with("--")).context("usage: cargo x release <X.Y.Z|patch|minor|major> [--dry-run] [--no-push]")?;

    let vs = collect_versions()?;
    if !vs.iter().all(|(_, v)| *v == vs[0].1) {
        versions()?;
    }
    let old = vs[0].1.clone();
    let new = next_version(semver(&old).context("current version isn't X.Y.Z")?, target)?;
    let tag = format!("v{new}");

    step(&format!("Preparing release {old} → {new}{}", if dry { " (dry run)" } else { "" }));
    let branch = output("git", &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if branch != "main" {
        if dry {
            println!("  note: you're on `{branch}`; a real release must be cut from main");
        } else {
            bail!("releases are cut from main; you're on `{branch}`");
        }
    }
    if !output("git", &["status", "--porcelain"]).unwrap_or_default().is_empty() && !dry {
        bail!("working tree has uncommitted changes; commit or stash them first");
    }
    if output("git", &["rev-parse", "-q", "--verify", &format!("refs/tags/{tag}")]).is_some() {
        bail!("tag {tag} already exists");
    }
    if !dry {
        sh("git", &["fetch", "--tags", "origin", "main"])?;
        let behind = output("git", &["rev-list", "--count", "HEAD..origin/main"]).unwrap_or_else(|| "0".into());
        if behind != "0" {
            bail!("main is {behind} commit(s) behind origin/main; run `git pull` first");
        }
    }

    if !skip_checks && !dry {
        lint()?;
        test()?;
    }

    let last_tag = output("git", &["describe", "--tags", "--abbrev=0"]);
    let section = changelog_section(&new, last_tag.as_deref());
    println!("\n{section}");
    if dry {
        println!(
            "Dry run: would bump {} files, prepend this to CHANGELOG.md, commit `chore(release): {tag}`, tag {tag} and push.",
            vs.len() + 1
        );
        return Ok(());
    }

    bump_files(&old, &new)?;
    // Refresh Cargo.lock for the workspace crates' new version.
    sh("cargo", &["update", "--workspace", "--offline"]).or_else(|_| sh("cargo", &["update", "--workspace"]))?;
    let existing = fs::read_to_string("CHANGELOG.md").unwrap_or_else(|_| {
        "# Changelog\n\nAll notable changes to OpenDirectory. Generated by `cargo x release` from Conventional Commits.\n".into()
    });
    let (intro, past) = match existing.find("\n## ") {
        Some(i) => existing.split_at(i + 1),
        None => (existing.as_str(), ""),
    };
    fs::write("CHANGELOG.md", format!("{}\n{section}\n{past}", intro.trim_end()))?;

    sh("git", &["add", CARGO_TOML, "Cargo.lock", PACKAGE_JSON, PACKAGE_LOCK, TAURI_CONF, "CHANGELOG.md"])?;
    sh("git", &["commit", "--no-verify", "-m", &format!("chore(release): {tag}")])?;
    sh("git", &["tag", "-a", &tag, "-m", &format!("OpenDirectory {tag}\n\n{section}")])?;

    if no_push {
        println!("\nTagged {tag} locally. Push when ready:  git push origin main {tag}");
        return Ok(());
    }
    sh("git", &["push", "origin", "main", &tag])?;
    println!("\n{} GitHub Actions is now building installers and will publish the release:", paint("32", &format!("Released {tag}.")));
    if let Some(url) = output("git", &["remote", "get-url", "origin"]) {
        let web = url.trim_end_matches(".git").replace("git@github.com:", "https://github.com/");
        println!("  {web}/actions/workflows/release.yml\n  {web}/releases/tag/{tag}");
    }
    Ok(())
}

// ------------------------------------------------------------------ in-app updater keys

fn updater_keys() -> Result<()> {
    let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")).context("no home directory")?;
    let key = PathBuf::from(home).join(".tauri").join("opendirectory-updater.key");
    if key.exists() {
        bail!("{} already exists. Keep using it; a new key would stop existing installs from updating.", key.display());
    }
    fs::create_dir_all(key.parent().unwrap())?;
    println!("Generating the update signing key. Choose a password and store it in your password manager.");
    sh_in(APP_DIR, "npx", &["tauri", "signer", "generate", "-w", &key.to_string_lossy()])?;
    let pubkey = fs::read_to_string(key.with_extension("key.pub"))?;
    let conf = fs::read_to_string(TAURI_CONF)?;
    let start = conf.find("\"pubkey\": \"").context("no pubkey field in tauri.conf.json")? + 11;
    let end = conf[start..].find('"').context("malformed pubkey")? + start;
    fs::write(TAURI_CONF, format!("{}{}{}", &conf[..start], pubkey.trim(), &conf[end..]))?;
    println!("\nPublic key written to {TAURI_CONF}. Commit it, then run `cargo x updater-secrets` to give CI the private key.");
    Ok(())
}

fn updater_secrets(key: Option<&str>) -> Result<()> {
    let key = match key {
        Some(k) => PathBuf::from(k),
        None => {
            let home = env::var_os("HOME").or_else(|| env::var_os("USERPROFILE")).context("no home directory")?;
            PathBuf::from(home).join(".tauri").join("opendirectory-updater.key")
        }
    };
    let private = fs::read_to_string(&key).with_context(|| format!("reading {}", key.display()))?;
    if output("gh", &["auth", "status"]).is_none() {
        bail!("GitHub CLI isn't installed or signed in. Install https://cli.github.com and run `gh auth login`.");
    }
    let password = match fs::read_to_string(key.with_extension("key.password")) {
        Ok(p) => p.trim().to_string(),
        Err(_) => prompt("Key password: ")?,
    };
    for (name, value) in [("TAURI_SIGNING_PRIVATE_KEY", private.trim().to_string()), ("TAURI_SIGNING_PRIVATE_KEY_PASSWORD", password)] {
        let mut child = cmd("gh", &["secret", "set", name]).stdin(Stdio::piped()).spawn()?;
        child.stdin.take().unwrap().write_all(value.as_bytes())?;
        if !child.wait()?.success() {
            bail!("`gh secret set {name}` failed");
        }
        println!("  {} {name}", paint("32", "✓"));
    }
    println!("Release builds will now publish signed in-app updates.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(semver("rustc 1.80.1 (3f5fd8dd4 2024-08-06)"), Some((1, 80, 1)));
        assert_eq!(semver("v20.11.0"), Some((20, 11, 0)));
        assert_eq!(semver("10.2"), Some((10, 2, 0)));
        assert_eq!(semver("0.2.0-beta.1"), Some((0, 2, 0)));
    }

    #[test]
    fn bumps() {
        assert_eq!(next_version((0, 1, 0), "patch").unwrap(), "0.1.1");
        assert_eq!(next_version((0, 1, 3), "minor").unwrap(), "0.2.0");
        assert_eq!(next_version((0, 9, 3), "major").unwrap(), "1.0.0");
        assert_eq!(next_version((0, 1, 0), "v0.3.0").unwrap(), "0.3.0");
        assert!(next_version((0, 2, 0), "0.1.9").is_err());
    }

    #[test]
    fn reads_manifest_versions() {
        assert_eq!(json_version("{\n  \"name\": \"x\",\n  \"version\": \"1.2.3\"\n}").as_deref(), Some("1.2.3"));
        assert_eq!(cargo_version("[workspace]\n[workspace.package]\nversion = \"0.4.0\"\n").as_deref(), Some("0.4.0"));
    }
}
