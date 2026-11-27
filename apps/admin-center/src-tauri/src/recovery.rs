//! "Forgot password?" on the sign-in screen, for a directory hosted on this
//! computer.
//!
//! The app asks the operating system to run a copy of itself elevated
//! (pkexec/polkit on Linux, UAC on Windows, Authorization Services on macOS).
//! The OS shows its own administrator prompt, so the app never sees the OS
//! password. The elevated copy runs the Management Server's break-glass
//! reset (`od_server::recovery`), which writes the audit log entry, and exits.
//! The new directory password goes to it in a private file that it deletes
//! after reading.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// First argument that turns the Admin Center binary into the elevated helper.
pub const HELPER_ARG: &str = "--break-glass-reset";
/// Same, for a factory reset of the hosted directory.
pub const FACTORY_ARG: &str = "--factory-reset";
const FILE_PREFIX: &str = "od-break-glass-";

#[derive(Serialize, Deserialize)]
struct Request {
    password: String,
}

#[derive(Serialize)]
pub struct Account {
    username: String,
    display_name: String,
    enabled: bool,
}

pub fn accounts(data_dir: &Path) -> Result<Vec<Account>, String> {
    od_server::recovery::admin_accounts(data_dir)
        .map(|v| v.into_iter().map(|(username, display_name, enabled)| Account { username, display_name, enabled }).collect())
        .map_err(|e| format!("{e:#}"))
}

/// Called first thing in `main`. When started as the elevated helper, do the
/// reset and exit without opening a window.
pub fn run_helper_if_requested() {
    let args: Vec<String> = std::env::args().collect();
    let (outcome, request) = match (args.get(1).map(String::as_str), args.get(2), args.get(3), args.get(4)) {
        (Some(HELPER_ARG), Some(data_dir), Some(user), Some(request)) => {
            let request = PathBuf::from(request);
            (helper(Path::new(data_dir), user, &request), request)
        }
        (Some(FACTORY_ARG), Some(data_dir), Some(request), None) => {
            let request = PathBuf::from(request);
            (factory_helper(Path::new(data_dir), &request), request)
        }
        (Some(HELPER_ARG | FACTORY_ARG), ..) => std::process::exit(2),
        _ => return,
    };
    let result = result_path(&request);
    let text = match &outcome {
        Ok(()) => "ok".to_string(),
        Err(e) => e.clone(),
    };
    // create_new: never write through a file or link that's already there.
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&result) {
        use std::io::Write;
        let _ = f.write_all(text.as_bytes());
    }
    std::process::exit(if outcome.is_ok() { 0 } else { 1 });
}

fn result_path(request: &Path) -> PathBuf {
    request.with_extension("result")
}

fn helper(data_dir: &Path, user: &str, request: &Path) -> Result<(), String> {
    if !request.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(FILE_PREFIX)) {
        return Err("Unexpected request file".into());
    }
    let raw = std::fs::read(request).map_err(|e| format!("Couldn't read the request: {e}"));
    let _ = std::fs::remove_file(request);
    if !od_server::recovery::is_elevated() {
        return Err("The reset must run with administrator rights.".into());
    }
    let req: Request = serde_json::from_slice(&raw?).map_err(|e| e.to_string())?;
    use od_server::recovery::{reset_password, NewPassword, ResetOptions};
    reset_password(
        data_dir,
        &ResetOptions {
            username: user.to_string(),
            password: NewPassword::Given(req.password),
            enable: true,
            via: Some("the Admin Center's \"Forgot password?\"".into()),
        },
    )
    .map_err(|e| format!("{e:#}"))
}

fn factory_helper(data_dir: &Path, request: &Path) -> Result<(), String> {
    if !request.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(FILE_PREFIX)) {
        return Err("Unexpected request file".into());
    }
    let _ = std::fs::remove_file(request);
    od_server::recovery::factory_reset(data_dir, Some("the Admin Center".into())).map(|_| ()).map_err(|e| format!("{e:#}"))
}

/// Factory reset of the hosted directory after the OS administrator prompt.
/// The caller must have stopped the server first.
pub fn factory_reset(work_dir: &Path, data_dir: &Path) -> Result<(), String> {
    run_elevated(work_dir, b"{}", |request| vec![FACTORY_ARG.to_string(), data_dir.display().to_string(), request.display().to_string()])
}

/// Reset `user`'s password after the OS administrator prompt. Blocks until
/// the prompt is answered and the helper exits.
pub fn reset(work_dir: &Path, data_dir: &Path, user: &str, password: &str) -> Result<(), String> {
    let body = serde_json::to_vec(&Request { password: password.to_string() }).map_err(|e| e.to_string())?;
    run_elevated(work_dir, &body, |request| {
        vec![HELPER_ARG.to_string(), data_dir.display().to_string(), user.to_string(), request.display().to_string()]
    })
}

/// Write `body` to a private request file, run this app elevated with the
/// arguments `args(request)` and return what the helper reported.
fn run_elevated(work_dir: &Path, body: &[u8], args: impl FnOnce(&Path) -> Vec<String>) -> Result<(), String> {
    let dir = work_dir.join("recovery");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let request = dir.join(format!("{FILE_PREFIX}{}.json", od_server::crypto::random_token(8)));
    let result = result_path(&request);
    let _ = std::fs::remove_file(&result);
    od_server::crypto::write_private(&request, body).map_err(|e| format!("{e:#}"))?;

    let exe = helper_exe()?;
    let launched = elevate(&exe, &args(&request));

    let _ = std::fs::remove_file(&request);
    let outcome = std::fs::read_to_string(&result).ok();
    let _ = std::fs::remove_file(&result);
    launched?;
    match outcome.as_deref() {
        Some("ok") => Ok(()),
        Some(msg) => Err(msg.to_string()),
        None => Err("The administrator step didn't finish. Nothing was changed.".into()),
    }
}

fn helper_exe() -> Result<PathBuf, String> {
    // An AppImage runs from a FUSE mount that root can't see; elevate the
    // AppImage file itself instead.
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Ok(PathBuf::from(p));
    }
    std::env::current_exe().map_err(|e| e.to_string())
}

const CANCELLED: &str = "The administrator prompt was cancelled. Nothing was changed.";

#[cfg(target_os = "linux")]
fn elevate(exe: &Path, args: &[String]) -> Result<(), String> {
    if std::env::var_os("FLATPAK_ID").is_some() {
        return Err("The Flatpak can't ask for administrator rights. Run `sudo opendirectory-server admin reset-password <user>` in a terminal instead.".into());
    }
    let status = std::process::Command::new("pkexec").arg(exe).args(args).status().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "pkexec (polkit) isn't installed, so the app can't ask for administrator rights. Run `sudo opendirectory-server admin reset-password <user>` in a terminal instead.".to_string()
        } else {
            e.to_string()
        }
    })?;
    match status.code() {
        Some(126) => Err(CANCELLED.into()),
        Some(127) => Err("This computer's administrator didn't approve the request.".into()),
        _ => Ok(()),
    }
}

#[cfg(target_os = "macos")]
fn elevate(exe: &Path, args: &[String]) -> Result<(), String> {
    let sh = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let cmd = std::iter::once(exe.display().to_string()).chain(args.iter().cloned()).map(|a| sh(&a)).collect::<Vec<_>>().join(" ");
    let script = format!(
        "do shell script \"{}\" with prompt \"OpenDirectory needs an administrator of this computer to continue.\" with administrator privileges",
        cmd.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let out = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if !out.status.success() && String::from_utf8_lossy(&out.stderr).contains("-128") {
        return Err(CANCELLED.into());
    }
    Ok(())
}

#[cfg(windows)]
fn elevate(exe: &Path, args: &[String]) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let wide = |s: &std::ffi::OsStr| s.encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let params = args.iter().map(|a| format!("\"{}\"", a.replace('"', ""))).collect::<Vec<_>>().join(" ");
    let verb = wide("runas".as_ref());
    let file = wide(exe.as_os_str());
    let params = wide(params.as_ref());
    // SAFETY: the struct is plain data; every pointer outlives the call.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            return Err(if GetLastError() == ERROR_CANCELLED { CANCELLED.into() } else { "Windows couldn't start the reset.".into() });
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 0u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
    }
    Ok(())
}
