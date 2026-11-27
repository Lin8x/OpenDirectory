//! Start the stream host where it can see the screen.

use anyhow::Result;
use std::path::Path;

pub enum HostProcess {
    Child(Option<tokio::process::Child>),
    #[cfg(windows)]
    Win(isize),
}

/// Launch `exe args` in the console user's desktop session.
#[cfg(not(windows))]
pub async fn in_console_session(exe: &Path, args: &[String]) -> Result<HostProcess> {
    let mut cmd = if cfg!(target_os = "macos") && is_root() {
        // A LaunchDaemon can't capture the screen; run in the GUI session.
        let uid = std::fs::metadata("/dev/console").map(|m| std::os::unix::fs::MetadataExt::uid(&m)).unwrap_or(0);
        let mut c = tokio::process::Command::new("/bin/launchctl");
        c.arg("asuser").arg(uid.to_string()).arg(exe);
        c
    } else {
        // Linux: the agent is root, so capture (x11grab with the session's
        // XAUTHORITY, or kmsgrab) and uinput work from here.
        tokio::process::Command::new(exe)
    };
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.kill_on_drop(true);
    Ok(HostProcess::Child(Some(cmd.spawn()?)))
}

#[cfg(not(windows))]
fn is_root() -> bool {
    std::env::var("USER").map(|u| u == "root").unwrap_or(false) || std::env::var("HOME").map(|h| h == "/var/root").unwrap_or(false)
}

/// On Windows the agent runs as SYSTEM in session 0, which has no desktop.
/// Like other remote desktop and game-streaming hosts, start the host as
/// SYSTEM in the active console session, on the interactive desktop.
#[cfg(windows)]
pub async fn in_console_session(exe: &Path, args: &[String]) -> Result<HostProcess> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        DuplicateTokenEx, SecurityImpersonation, SetTokenInformation, TokenPrimary, TokenSessionId, TOKEN_ALL_ACCESS,
    };
    use windows_sys::Win32::System::RemoteDesktop::WTSGetActiveConsoleSessionId;
    use windows_sys::Win32::System::Threading::{
        CreateProcessAsUserW, GetCurrentProcess, OpenProcessToken, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    let quote = |s: &str| if s.contains(' ') || s.is_empty() { format!("\"{s}\"") } else { s.to_string() };
    let mut line = quote(&exe.display().to_string());
    for a in args {
        line.push(' ');
        line.push_str(&quote(a));
    }
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let mut cmdline = wide(&line);
    let mut desktop = wide("winsta0\\default");

    unsafe {
        let session = WTSGetActiveConsoleSessionId();
        if session == 0xFFFF_FFFF {
            anyhow::bail!("Nobody is at the console of this endpoint (no active session to stream)");
        }
        let mut own: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ALL_ACCESS, &mut own) == 0 {
            anyhow::bail!("OpenProcessToken failed: {}", std::io::Error::last_os_error());
        }
        let mut dup: HANDLE = 0;
        let ok = DuplicateTokenEx(own, TOKEN_ALL_ACCESS, std::ptr::null(), SecurityImpersonation, TokenPrimary, &mut dup);
        CloseHandle(own);
        if ok == 0 {
            anyhow::bail!("DuplicateTokenEx failed: {}", std::io::Error::last_os_error());
        }
        // Needs SYSTEM (SeTcbPrivilege). Running interactively, start normally.
        let moved = SetTokenInformation(dup, TokenSessionId, &session as *const u32 as *const _, 4) != 0;
        if !moved {
            CloseHandle(dup);
            let mut cmd = tokio::process::Command::new(exe);
            cmd.args(args).kill_on_drop(true);
            return Ok(HostProcess::Child(Some(cmd.spawn()?)));
        }
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.lpDesktop = desktop.as_mut_ptr();
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();
        let ok = CreateProcessAsUserW(
            dup,
            std::ptr::null(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &si,
            &mut pi,
        );
        CloseHandle(dup);
        if ok == 0 {
            anyhow::bail!("Couldn't start the stream host in the user's session: {}", std::io::Error::last_os_error());
        }
        CloseHandle(pi.hThread);
        Ok(HostProcess::Win(pi.hProcess))
    }
}

/// The host connected; it exits by itself when the session ends.
pub fn detach(p: &mut HostProcess) {
    match p {
        HostProcess::Child(c) => {
            // Reap it when it exits, without killing it when this is dropped.
            if let Some(mut owned) = c.take() {
                tokio::spawn(async move {
                    let _ = owned.wait().await;
                });
            }
        }
        #[cfg(windows)]
        HostProcess::Win(h) => unsafe {
            windows_sys::Win32::Foundation::CloseHandle(*h);
            *h = 0;
        },
    }
}

pub fn kill(p: &mut HostProcess) {
    match p {
        HostProcess::Child(c) => {
            if let Some(c) = c.as_mut() {
                let _ = c.start_kill();
            }
        }
        #[cfg(windows)]
        HostProcess::Win(h) => unsafe {
            if *h != 0 {
                windows_sys::Win32::System::Threading::TerminateProcess(*h, 1);
                windows_sys::Win32::Foundation::CloseHandle(*h);
                *h = 0;
            }
        },
    }
}
