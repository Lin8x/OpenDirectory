//! Talking to the person at the computer from a headless service.
//!
//! The agent runs as root/SYSTEM with no window of its own, so to show a
//! notice or ask "Allow IT to connect?" it borrows the signed-in user's
//! desktop for a moment: a WTS message box on Windows, an AppleScript dialog
//! on macOS, and kdialog, zenity or notify-send on Linux. Nothing stays
//! running afterwards.

use std::time::Duration;

#[allow(unused_imports)]
use crate::util;

/// The user at the physical console (the one who would see a dialog).
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleUser {
    pub name: String,
    #[allow(dead_code)]
    pub(crate) session: String,
}

/// Who is signed in at the console right now, if anyone. Cheap enough to
/// call every minute (no full inventory).
pub async fn console_user() -> Option<ConsoleUser> {
    imp::console_user().await
}

/// Show a message without waiting for it to be dismissed.
pub async fn notify(title: &str, message: &str) -> bool {
    match console_user().await {
        Some(u) => imp::notify(&u, title, message).await,
        None => false,
    }
}

/// Ask the console user a yes/no question. `None` means nobody answered:
/// nobody is signed in, the dialog couldn't be shown, or it timed out.
pub async fn ask(title: &str, message: &str, yes: &str, no: &str, timeout: Duration) -> Option<bool> {
    let u = console_user().await?;
    imp::ask(&u, title, message, yes, no, timeout).await
}

/// Show a notice the user must click through. `Some(true)` once they press
/// `button`.
pub async fn acknowledge(title: &str, message: &str, button: &str, timeout: Duration) -> Option<bool> {
    let u = console_user().await?;
    imp::ask(&u, title, message, button, "Not now", timeout).await
}

// ------------------------------------------------------------------ Linux
#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    struct Session {
        user: ConsoleUser,
        uid: String,
        display: String,
        wayland: bool,
    }

    async fn session() -> Option<Session> {
        let list = util::run("loginctl", &["list-sessions", "--no-legend"]).await.ok()?;
        for line in list.stdout.lines() {
            let Some(id) = line.split_whitespace().next() else { continue };
            let Ok(o) = util::run(
                "loginctl",
                &["show-session", id, "-p", "Active", "-p", "Type", "-p", "Name", "-p", "User", "-p", "Display", "-p", "Seat"],
            )
            .await
            else {
                continue;
            };
            let get = |k: &str| o.stdout.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).unwrap_or("").trim().to_string();
            let ty = get("Type");
            if get("Active") != "yes" || !(ty == "x11" || ty == "wayland") || get("Name").is_empty() {
                continue;
            }
            return Some(Session {
                user: ConsoleUser { name: get("Name"), session: id.to_string() },
                uid: get("User"),
                display: get("Display"),
                wayland: ty == "wayland",
            });
        }
        None
    }

    pub async fn console_user() -> Option<ConsoleUser> {
        session().await.map(|s| s.user)
    }

    fn have(program: &str) -> bool {
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file())).unwrap_or(false)
    }

    /// Run a desktop program as the signed-in user, in their session.
    async fn as_user(s: &Session, program: &str, args: &[&str], timeout: Duration) -> Option<util::Output> {
        let runtime = format!("/run/user/{}", s.uid);
        let mut full: Vec<String> = vec!["-u".into(), s.user.name.clone(), "--".into(), "env".into()];
        full.push(format!("XDG_RUNTIME_DIR={runtime}"));
        full.push(format!("DBUS_SESSION_BUS_ADDRESS=unix:path={runtime}/bus"));
        full.push(format!("DISPLAY={}", if s.display.is_empty() { ":0" } else { &s.display }));
        if s.wayland {
            full.push("WAYLAND_DISPLAY=wayland-0".into());
        }
        full.push(program.into());
        full.extend(args.iter().map(|a| a.to_string()));
        let refs: Vec<&str> = full.iter().map(String::as_str).collect();
        util::run_with("runuser", &refs, None, timeout).await.ok()
    }

    pub async fn notify(u: &ConsoleUser, title: &str, message: &str) -> bool {
        let Some(s) = session().await.filter(|s| &s.user == u) else { return false };
        let t = Duration::from_secs(10);
        if have("notify-send") {
            if let Some(o) = as_user(&s, "notify-send", &["-a", "OpenDirectory", "-i", "dialog-information", title, message], t).await {
                return o.ok();
            }
        }
        if have("kdialog") {
            return as_user(&s, "kdialog", &["--title", title, "--passivepopup", message, "15"], t).await.map(|o| o.ok()).unwrap_or(false);
        }
        false
    }

    pub async fn ask(u: &ConsoleUser, title: &str, message: &str, yes: &str, no: &str, timeout: Duration) -> Option<bool> {
        let s = session().await.filter(|s| &s.user == u)?;
        let o = if have("kdialog") {
            as_user(&s, "kdialog", &["--title", title, "--yes-label", yes, "--no-label", no, "--yesno", message], timeout).await?
        } else if have("zenity") {
            let secs = timeout.as_secs().to_string();
            as_user(
                &s,
                "zenity",
                &["--question", "--title", title, "--text", message, "--ok-label", yes, "--cancel-label", no, "--timeout", &secs],
                timeout,
            )
            .await?
        } else {
            return None;
        };
        match o.code {
            Some(0) => Some(true),
            Some(1) => Some(false),
            _ => None, // timed out, or no dialog program could open a window
        }
    }
}

// ------------------------------------------------------------------ macOS
#[cfg(target_os = "macos")]
mod imp {
    use super::*;

    fn quote(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }

    pub async fn console_user() -> Option<ConsoleUser> {
        let o = util::run("stat", &["-f", "%Su", "/dev/console"]).await.ok()?;
        let name = o.stdout.trim().to_string();
        if name.is_empty() || name == "root" || name == "_mbsetupuser" {
            return None;
        }
        let uid = util::run("id", &["-u", &name]).await.ok()?.stdout.trim().to_string();
        Some(ConsoleUser { name, session: uid })
    }

    async fn osascript(u: &ConsoleUser, script: &str, timeout: Duration) -> Option<util::Output> {
        util::run_with("launchctl", &["asuser", &u.session, "sudo", "-u", &u.name, "osascript", "-e", script], None, timeout).await.ok()
    }

    pub async fn notify(u: &ConsoleUser, title: &str, message: &str) -> bool {
        let script = format!("display notification {} with title {}", quote(message), quote(title));
        osascript(u, &script, Duration::from_secs(10)).await.map(|o| o.ok()).unwrap_or(false)
    }

    pub async fn ask(u: &ConsoleUser, title: &str, message: &str, yes: &str, no: &str, timeout: Duration) -> Option<bool> {
        let script = format!(
            "display dialog {} with title {} buttons {{{}, {}}} default button {} with icon caution giving up after {}",
            quote(message),
            quote(title),
            quote(no),
            quote(yes),
            quote(yes),
            timeout.as_secs()
        );
        let o = osascript(u, &script, timeout + Duration::from_secs(5)).await?;
        if o.stdout.contains("gave up:true") {
            None
        } else if o.stdout.contains(&format!("button returned:{yes}")) {
            Some(true)
        } else if o.stdout.contains("button returned:") || o.code == Some(1) {
            // Code 1 is "User canceled".
            Some(false)
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------ Windows
#[cfg(windows)]
mod imp {
    use super::*;
    use windows_sys::Win32::System::RemoteDesktop::{
        WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSQuerySessionInformationW, WTSSendMessageW, WTSUserName, WTS_CURRENT_SERVER_HANDLE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IDNO, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn session_user(id: u32) -> Option<String> {
        unsafe {
            let mut buf: *mut u16 = std::ptr::null_mut();
            let mut bytes = 0u32;
            if WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, id, WTSUserName, &mut buf, &mut bytes) == 0 || buf.is_null() {
                return None;
            }
            let len = (0..).take_while(|&i| *buf.add(i) != 0).count();
            let name = String::from_utf16_lossy(std::slice::from_raw_parts(buf, len));
            WTSFreeMemory(buf as *mut _);
            Some(name).filter(|n| !n.is_empty())
        }
    }

    pub async fn console_user() -> Option<ConsoleUser> {
        let id = unsafe { WTSGetActiveConsoleSessionId() };
        if id == u32::MAX {
            return None;
        }
        session_user(id).map(|name| ConsoleUser { name, session: id.to_string() })
    }

    /// WTSSendMessage shows a message box on the user's desktop from a
    /// service. With `wait` it blocks until answered or timed out.
    async fn send(u: &ConsoleUser, title: &str, message: &str, style: u32, timeout: Duration, wait: bool) -> Option<i32> {
        let id: u32 = u.session.parse().ok()?;
        let (t, m) = (wide(title), wide(message));
        let secs = timeout.as_secs() as u32;
        tokio::task::spawn_blocking(move || unsafe {
            let mut resp: i32 = 0;
            let ok = WTSSendMessageW(
                WTS_CURRENT_SERVER_HANDLE,
                id,
                t.as_ptr(),
                (t.len() * 2) as u32,
                m.as_ptr(),
                (m.len() * 2) as u32,
                style | MB_TOPMOST | MB_SETFOREGROUND,
                secs,
                &mut resp,
                wait as i32,
            );
            (ok != 0).then_some(resp)
        })
        .await
        .ok()
        .flatten()
    }

    pub async fn notify(u: &ConsoleUser, title: &str, message: &str) -> bool {
        send(u, title, message, MB_OK | MB_ICONINFORMATION, Duration::from_secs(60), false).await.is_some()
    }

    pub async fn ask(u: &ConsoleUser, title: &str, message: &str, yes: &str, no: &str, timeout: Duration) -> Option<bool> {
        // Message boxes can't relabel their buttons, so say what Yes and No mean.
        let text = format!("{message}\n\nYes: {yes}\nNo: {no}");
        match send(u, title, &text, MB_YESNO | MB_ICONQUESTION, timeout, true).await? {
            r if r == IDYES => Some(true),
            r if r == IDNO => Some(false),
            _ => None, // IDTIMEOUT
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
mod imp {
    use super::*;
    pub async fn console_user() -> Option<ConsoleUser> {
        None
    }
    pub async fn notify(_: &ConsoleUser, _: &str, _: &str) -> bool {
        false
    }
    pub async fn ask(_: &ConsoleUser, _: &str, _: &str, _: &str, _: &str, _: Duration) -> Option<bool> {
        None
    }
}
