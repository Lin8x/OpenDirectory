//! Background mode: a separate, hidden desktop session on the endpoint.
//!
//! Instead of capturing the console, the agent starts a *virtual display*
//! (an X server with no physical screen), starts a desktop session on it as
//! the chosen account, and streams that. The person at the console sees and
//! feels nothing: their screen, keyboard and mouse are never touched, and
//! their session keeps running. Apps installed on the endpoint are available
//! in the background session, and run with that account's settings and files.
//!
//! * **Linux**: Xvfb (or TigerVNC's Xvnc, or a headless KWin compositor with
//!   rootful Xwayland on KDE machines that have neither) on a free display
//!   number, an X authority cookie private to the session, and the desktop
//!   session in its own D-Bus session bus and runtime directory. Input goes
//!   to that display with XTEST, not to the console's input devices.
//! * **Windows / macOS**: not done here. Windows background sessions are
//!   separate Remote Desktop Services sessions, opened through browser RDP
//!   (see the server's remote module); macOS has no supported way to start a
//!   second GUI session for an agent.
//!
//! The session ends, and everything started in it is stopped, when the
//! viewer disconnects.

use anyhow::Result;
use od_core::protocol::BackgroundOptions;

/// The display and X authority file the stream host should use.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub struct Target {
    pub display: String,
    pub xauthority: String,
}

#[cfg(target_os = "linux")]
pub use linux::BackgroundSession;

#[cfg(not(target_os = "linux"))]
pub struct BackgroundSession;

#[cfg(not(target_os = "linux"))]
impl BackgroundSession {
    pub async fn start(_o: &BackgroundOptions) -> Result<BackgroundSession> {
        if cfg!(windows) {
            anyhow::bail!(
                "On Windows, background mode uses a separate Remote Desktop session: choose Background mode with RDP in the Admin Center."
            )
        }
        anyhow::bail!(
            "Background mode isn't available on macOS: macOS has no supported way for a management agent to start a second, hidden desktop session."
        )
    }
    pub fn target(&self) -> Target {
        unreachable!()
    }
    pub fn describe(&self) -> String {
        String::new()
    }
}

/// Desktop environments, in the order `auto` tries them: (name, program, args).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const DESKTOPS: &[(&str, &str, &[&str])] = &[
    ("plasma", "startplasma-x11", &[]),
    ("xfce", "xfce4-session", &[]),
    ("mate", "mate-session", &[]),
    ("cinnamon", "cinnamon-session", &[]),
    ("lxqt", "startlxqt", &[]),
    ("openbox", "openbox-session", &[]),
    ("xterm", "xterm", &["-geometry", "120x36+40+40"]),
];

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use anyhow::{bail, Context};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    /// A local account, from the passwd database.
    #[derive(Debug, Clone)]
    struct Account {
        name: String,
        uid: u32,
        gid: u32,
        home: String,
        shell: String,
    }

    fn parse_passwd(line: &str) -> Option<Account> {
        let f: Vec<&str> = line.trim().split(':').collect();
        if f.len() < 7 {
            return None;
        }
        Some(Account {
            name: f[0].into(),
            uid: f[2].parse().ok()?,
            gid: f[3].parse().ok()?,
            home: f[5].into(),
            shell: if f[6].is_empty() { "/bin/sh".into() } else { f[6].into() },
        })
    }

    async fn lookup(key: &str) -> Option<Account> {
        // getent covers LDAP/SSSD accounts too; fall back to /etc/passwd.
        if let Ok(o) = crate::util::run("getent", &["passwd", key]).await {
            if let Some(a) = o.stdout.lines().next().and_then(parse_passwd) {
                return Some(a);
            }
        }
        std::fs::read_to_string("/etc/passwd").ok()?.lines().filter_map(parse_passwd).find(|a| a.name == key || a.uid.to_string() == key)
    }

    fn euid() -> u32 {
        unsafe { libc::geteuid() }
    }

    fn find_bin(name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        for d in ["/usr/bin", "/usr/local/bin", "/bin", "/usr/sbin", "/usr/lib/xorg"] {
            dirs.push(d.into());
        }
        dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
    }

    /// Which X server runs the virtual display.
    enum Server {
        Xvfb(PathBuf),
        Xvnc(PathBuf),
        /// Headless KWin plus rootful Xwayland (KDE Plasma machines).
        KwinXwayland {
            kwin: PathBuf,
            xwayland: PathBuf,
        },
    }

    impl Server {
        fn detect() -> Option<Server> {
            if let Some(p) = find_bin("Xvfb") {
                return Some(Server::Xvfb(p));
            }
            if let Some(p) = find_bin("Xvnc") {
                return Some(Server::Xvnc(p));
            }
            if let (Some(kwin), Some(xwayland)) = (find_bin("kwin_wayland"), find_bin("Xwayland")) {
                return Some(Server::KwinXwayland { kwin, xwayland });
            }
            None
        }
        fn label(&self) -> &'static str {
            match self {
                Server::Xvfb(_) => "Xvfb",
                Server::Xvnc(_) => "Xvnc",
                Server::KwinXwayland { .. } => "KWin virtual + Xwayland",
            }
        }
    }

    const NO_SERVER: &str = "Background mode needs a virtual display server on this endpoint. Install Xvfb: \
Debian/Ubuntu: apt install xvfb; Fedora/RHEL: dnf install xorg-x11-server-Xvfb; openSUSE: zypper install xvfb-run; Arch: pacman -S xorg-server-xvfb.";

    pub struct BackgroundSession {
        display: u32,
        dir: PathBuf,
        account: Account,
        server: &'static str,
        desktop: String,
        /// Process groups we started (each child is a group leader).
        children: Vec<Child>,
    }

    impl BackgroundSession {
        pub async fn start(o: &BackgroundOptions) -> Result<BackgroundSession> {
            let account = resolve_account(&o.run_as).await?;
            let server = Server::detect().ok_or_else(|| anyhow::anyhow!(NO_SERVER))?;
            let xauth =
                find_bin("xauth").ok_or_else(|| anyhow::anyhow!("Background mode needs xauth (package xauth or xorg-x11-xauth)."))?;
            let (desktop_name, desktop_cmd) = pick_desktop(&o.desktop)?;
            let (w, h) = (o.width.clamp(640, 7680) & !1, o.height.clamp(480, 4320) & !1);

            // A private directory for the session: X authority, logs, and the
            // session's XDG_RUNTIME_DIR, so it shares nothing with the user's
            // console session.
            let base =
                if euid() == 0 { PathBuf::from("/run/opendirectory/background") } else { std::env::temp_dir().join("od-background") };
            std::fs::create_dir_all(&base)?;
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o711))?;
            let dir = base.join(hex::encode(rand::random::<[u8; 6]>()));
            std::fs::create_dir(&dir)?;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
            chown(&dir, &account)?;

            let display = free_display().context("no free X display number")?;
            let mut s = BackgroundSession {
                display,
                dir: dir.clone(),
                account: account.clone(),
                server: server.label(),
                desktop: desktop_name.to_string(),
                children: vec![],
            };

            // X authority: only holders of this cookie (the session, and the
            // stream host running as root) can connect to the display.
            let xa = dir.join("Xauthority");
            let cookie = hex::encode(rand::random::<[u8; 16]>());
            let st = Command::new(&xauth)
                .args(["-q", "-f"])
                .arg(&xa)
                .args(["add", &format!(":{display}"), "MIT-MAGIC-COOKIE-1", &cookie])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()?;
            if !st.success() {
                bail!("xauth couldn't write the session's X authority file");
            }
            chown(&xa, &account)?;
            let xa_s = xa.display().to_string();

            // 1. The virtual display.
            let disp = format!(":{display}");
            let geometry = format!("{w}x{h}");
            match &server {
                Server::Xvfb(p) => {
                    let screen = format!("{w}x{h}x24");
                    let c = s.spawn(
                        p,
                        &[&disp, "-screen", "0", &screen, "-auth", &xa_s, "-nolisten", "tcp", "-noreset", "+extension", "RANDR"],
                        &[],
                        "xserver",
                    )?;
                    s.children.push(c);
                }
                Server::Xvnc(p) => {
                    // No VNC listener: the agent captures it like any X display.
                    let c = s.spawn(
                        p,
                        &[
                            &disp,
                            "-geometry",
                            &geometry,
                            "-depth",
                            "24",
                            "-auth",
                            &xa_s,
                            "-nolisten",
                            "tcp",
                            "-rfbport",
                            "-1",
                            "-SecurityTypes",
                            "None",
                        ],
                        &[],
                        "xserver",
                    )?;
                    s.children.push(c);
                }
                Server::KwinXwayland { kwin, xwayland } => {
                    let socket = format!("od-bg-{display}");
                    let (ws, hs) = (w.to_string(), h.to_string());
                    let mut args: Vec<String> = vec![];
                    let prog = match find_bin("dbus-run-session") {
                        Some(d) => {
                            args.extend(["--".into(), kwin.display().to_string()]);
                            d
                        }
                        None => kwin.clone(),
                    };
                    args.extend(["--virtual", "--no-lockscreen", "--width", &ws, "--height", &hs, "--socket", &socket].map(String::from));
                    let a: Vec<&str> = args.iter().map(String::as_str).collect();
                    // The compositor is plumbing, not the user's desktop: keep
                    // its output settings out of the account's ~/.config.
                    let cfg = dir.join("compositor-config");
                    std::fs::create_dir_all(&cfg)?;
                    chown(&cfg, &account)?;
                    let cfg = cfg.display().to_string();
                    let c = s.spawn(&prog, &a, &[("XDG_CONFIG_HOME", &cfg)], "compositor")?;
                    s.children.push(c);
                    let sock = dir.join(&socket);
                    wait_for(&sock, Duration::from_secs(15), &mut s.children[0])
                        .with_context(|| format!("the headless compositor didn't start{}", s.log_tail("compositor")))?;
                    let c = s.spawn(
                        xwayland,
                        &[&disp, "-geometry", &geometry, "-fullscreen", "-auth", &xa_s, "-nolisten", "tcp", "-noreset"],
                        &[("WAYLAND_DISPLAY", &socket)],
                        "xserver",
                    )?;
                    s.children.push(c);
                }
            }
            let sock = PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
            let last = s.children.len() - 1;
            wait_for(&sock, Duration::from_secs(15), &mut s.children[last])
                .with_context(|| format!("the virtual display ({}) didn't start{}", s.server, s.log_tail("xserver")))?;

            // 2. The desktop session, on its own D-Bus session bus.
            let (prog, args): (PathBuf, Vec<String>) = match find_bin("dbus-run-session") {
                Some(d) => {
                    let mut a = vec!["--".to_string(), desktop_cmd.0.display().to_string()];
                    a.extend(desktop_cmd.1.iter().map(|x| x.to_string()));
                    (d, a)
                }
                None => (desktop_cmd.0.clone(), desktop_cmd.1.iter().map(|x| x.to_string()).collect()),
            };
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            // KDE's display configuration would otherwise switch a virtual
            // display to its largest mode; the read-only backend keeps the
            // size that was asked for.
            let c = s.spawn(
                &prog,
                &a,
                &[("DISPLAY", &disp), ("XAUTHORITY", &xa_s), ("XDG_SESSION_TYPE", "x11"), ("KSCREEN_BACKEND", "QScreen")],
                "desktop",
            )?;
            s.children.push(c);
            tracing::info!("background session {}: {} on {disp} as {} ({})", s.dir.display(), s.server, s.account.name, s.desktop);
            Ok(s)
        }

        pub fn target(&self) -> Target {
            Target { display: format!(":{}", self.display), xauthority: self.dir.join("Xauthority").display().to_string() }
        }

        pub fn describe(&self) -> String {
            format!("{} desktop as {} on a {} virtual display", self.desktop, self.account.name, self.server)
        }

        /// Start a program as the session's account, in its own process group.
        fn spawn(&self, prog: &Path, args: &[&str], extra: &[(&str, &str)], log: &str) -> Result<Child> {
            let a = &self.account;
            let out = std::fs::File::create(self.dir.join(format!("{log}.log")))?;
            let err = out.try_clone()?;
            let mut cmd = Command::new(prog);
            cmd.args(args)
                .env_clear()
                .env("HOME", &a.home)
                .env("USER", &a.name)
                .env("LOGNAME", &a.name)
                .env("SHELL", &a.shell)
                .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
                .env("XDG_RUNTIME_DIR", &self.dir)
                .env("LANG", std::env::var("LANG").unwrap_or_else(|_| system_lang()))
                .current_dir(if Path::new(&a.home).is_dir() { a.home.as_str() } else { "/" })
                .stdin(Stdio::null())
                .stdout(out)
                .stderr(err);
            for (k, v) in extra {
                cmd.env(k, v);
            }
            let switch = euid() == 0 && a.uid != 0;
            let (uid, gid, name) = (a.uid, a.gid, std::ffi::CString::new(a.name.clone())?);
            unsafe {
                cmd.pre_exec(move || {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if switch && (libc::initgroups(name.as_ptr(), gid as _) != 0 || libc::setgid(gid) != 0 || libc::setuid(uid) != 0) {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
            cmd.spawn().with_context(|| format!("start {}", prog.display()))
        }

        fn log_tail(&self, log: &str) -> String {
            let t = std::fs::read_to_string(self.dir.join(format!("{log}.log"))).unwrap_or_default();
            let lines: Vec<&str> = t.lines().filter(|l| !l.trim().is_empty()).collect();
            let tail = lines[lines.len().saturating_sub(6)..].join("\n");
            if tail.is_empty() {
                String::new()
            } else {
                format!(":\n{tail}")
            }
        }
    }

    impl Drop for BackgroundSession {
        fn drop(&mut self) {
            // Stop everything started for the session: our process groups,
            // then anything else still attached to its display (apps that
            // started their own session or daemonized).
            let groups: Vec<i32> = self.children.iter().map(|c| c.id() as i32).collect();
            let display = format!(":{}", self.display);
            let uid = self.account.uid;
            let dir = self.dir.clone();
            let mut children = std::mem::take(&mut self.children);
            let stop = move |sig: i32| {
                for g in &groups {
                    unsafe {
                        libc::kill(-g, sig);
                    }
                }
                for pid in processes_on_display(&display, uid) {
                    unsafe {
                        libc::kill(pid, sig);
                    }
                }
            };
            stop(libc::SIGTERM);
            std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline && children.iter_mut().any(|c| matches!(c.try_wait(), Ok(None))) {
                    std::thread::sleep(Duration::from_millis(200));
                }
                stop(libc::SIGKILL);
                for c in children.iter_mut() {
                    let _ = c.wait();
                }
                let _ = std::fs::remove_dir_all(&dir);
                tracing::info!("background session {} ended", dir.display());
            });
        }
    }

    /// Processes of `uid` whose environment has `DISPLAY=<display>` exactly.
    fn processes_on_display(display: &str, uid: u32) -> Vec<i32> {
        use std::os::unix::fs::MetadataExt;
        let want = format!("DISPLAY={display}");
        let me = std::process::id() as i32;
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir("/proc") else { return out };
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
            if pid == me || e.metadata().map(|m| m.uid()).ok() != Some(uid) {
                continue;
            }
            if let Ok(env) = std::fs::read(e.path().join("environ")) {
                if env.split(|b| *b == 0).any(|v| v == want.as_bytes()) {
                    out.push(pid);
                }
            }
        }
        out
    }

    async fn resolve_account(run_as: &str) -> Result<Account> {
        let run_as = run_as.trim();
        let a = match run_as {
            "" | "console_user" => {
                let u = crate::desktop::console_user().await.ok_or_else(|| {
                    anyhow::anyhow!("Nobody is signed in at the console. Choose an account to run the background session as.")
                })?;
                lookup(&u.name).await.ok_or_else(|| anyhow::anyhow!("Couldn't look up the console user {}", u.name))?
            }
            "system" | "root" => {
                lookup("0").await.unwrap_or(Account { name: "root".into(), uid: 0, gid: 0, home: "/root".into(), shell: "/bin/sh".into() })
            }
            name => {
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || "._-@".contains(c)) {
                    bail!("\"{name}\" isn't a valid account name");
                }
                lookup(name).await.ok_or_else(|| anyhow::anyhow!("There's no account named {name} on this endpoint"))?
            }
        };
        if a.uid != 0 && (a.shell.ends_with("nologin") || a.shell.ends_with("/false")) {
            bail!("{} can't sign in (its shell is {}), so it can't run a desktop session", a.name, a.shell);
        }
        // Tests can point the session at a scratch home so a live desktop
        // doesn't touch the real profile.
        #[cfg(test)]
        let a = match std::env::var("OD_BG_HOME") {
            Ok(h) => Account { home: h, ..a },
            Err(_) => a,
        };
        if euid() != 0 && a.uid != euid() {
            bail!("The agent isn't running as root, so it can only start a background session as its own account");
        }
        Ok(a)
    }

    fn pick_desktop(wanted: &str) -> Result<(&'static str, (PathBuf, &'static [&'static str]))> {
        let installed: Vec<(&'static str, PathBuf, &'static [&'static str])> =
            DESKTOPS.iter().filter_map(|(n, p, a)| find_bin(p).map(|path| (*n, path, *a))).collect();
        let found = match wanted {
            "" | "auto" => installed.first(),
            w => installed.iter().find(|(n, ..)| *n == w),
        };
        match found {
            Some((n, p, a)) => Ok((n, (p.clone(), a))),
            None if installed.is_empty() => {
                bail!("No desktop environment that can run on a virtual display is installed (KDE Plasma, Xfce, MATE, Cinnamon, LXQt, Openbox or xterm).")
            }
            None => bail!(
                "The {wanted} desktop isn't installed on this endpoint. Installed: {}",
                installed.iter().map(|(n, ..)| *n).collect::<Vec<_>>().join(", ")
            ),
        }
    }

    fn free_display() -> Option<u32> {
        (90..190).find(|n| !Path::new(&format!("/tmp/.X{n}-lock")).exists() && !Path::new(&format!("/tmp/.X11-unix/X{n}")).exists())
    }

    fn wait_for(path: &Path, timeout: Duration, child: &mut Child) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if path.exists() {
                return Ok(());
            }
            if let Ok(Some(st)) = child.try_wait() {
                bail!("it exited ({st})");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        bail!("timed out")
    }

    fn chown(p: &Path, a: &Account) -> Result<()> {
        if euid() != 0 {
            return Ok(());
        }
        std::os::unix::fs::chown(p, Some(a.uid), Some(a.gid)).with_context(|| format!("chown {}", p.display()))
    }

    fn system_lang() -> String {
        std::fs::read_to_string("/etc/default/locale")
            .or_else(|_| std::fs::read_to_string("/etc/locale.conf"))
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("LANG=").map(|v| v.trim_matches('"').to_string())))
            .unwrap_or_else(|| "C.UTF-8".into())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn passwd_lines_parse() {
            let a = parse_passwd("dan:x:1000:1000:Dan,,,:/home/dan:/bin/bash").unwrap();
            assert_eq!((a.name.as_str(), a.uid, a.gid, a.home.as_str()), ("dan", 1000, 1000, "/home/dan"));
            assert!(parse_passwd("broken:x").is_none());
            assert_eq!(parse_passwd("svc:x:5:5::/:").unwrap().shell, "/bin/sh");
        }

        /// Live check on a Linux desktop (needs an X server backend and the
        /// desktop named in OD_BG_DESKTOP, default xterm):
        /// `OD_BG_OUT=/tmp/bg cargo test -p od-agent background_session_live -- --ignored`
        /// Writes before.png and after.png (after typing into the session).
        #[tokio::test]
        #[ignore]
        async fn background_session_live() {
            let out = PathBuf::from(std::env::var("OD_BG_OUT").unwrap_or_else(|_| "/tmp/od-bg-test".into()));
            std::fs::create_dir_all(&out).unwrap();
            let me = lookup(&euid().to_string()).await.unwrap();
            let o = BackgroundOptions {
                run_as: me.name.clone(),
                desktop: std::env::var("OD_BG_DESKTOP").unwrap_or_else(|_| "xterm".into()),
                width: 1280,
                height: 720,
            };
            let s = BackgroundSession::start(&o).await.unwrap();
            println!("{}", s.describe());
            let t = s.target();
            std::env::set_var("XAUTHORITY", &t.xauthority);
            let wait: u64 = std::env::var("OD_BG_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
            tokio::time::sleep(Duration::from_secs(wait)).await;
            let shot = |name: &str| {
                let st = Command::new("ffmpeg")
                    .args(["-y", "-loglevel", "error", "-f", "x11grab", "-i", &t.display, "-frames:v", "1"])
                    .arg(out.join(name))
                    .env("XAUTHORITY", &t.xauthority)
                    .status()
                    .unwrap();
                assert!(st.success());
            };
            shot("before.png");
            let mut x = crate::stream::input::XTest::new(&t.display).unwrap();
            use crate::stream::InputEvent as E;
            x.apply(&E::Move { x: 0.2, y: 0.2 });
            x.apply(&E::Button { b: 0, d: true });
            x.apply(&E::Button { b: 0, d: false });
            for c in ["KeyE", "KeyC", "KeyH", "KeyO", "Space", "KeyH", "KeyI", "Enter"] {
                x.apply(&E::Key { code: c.into(), d: true });
                x.apply(&E::Key { code: c.into(), d: false });
            }
            drop(x);
            tokio::time::sleep(Duration::from_secs(1)).await;
            shot("after.png");
            let disp = t.display.clone();
            drop(s);
            tokio::time::sleep(Duration::from_secs(6)).await;
            assert!(processes_on_display(&disp, me.uid).is_empty(), "session processes left behind");
        }
    }
}
