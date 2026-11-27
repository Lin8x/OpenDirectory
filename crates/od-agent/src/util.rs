//! Running OS commands safely: no shell interpolation, optional stdin for
//! secrets (never on the command line), and a timeout.

use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Default)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
    /// stderr if present, else stdout, trimmed to one readable line.
    pub fn message(&self) -> String {
        let s = if self.stderr.trim().is_empty() { &self.stdout } else { &self.stderr };
        let s = s.trim();
        let line = s.lines().last().unwrap_or("").trim();
        if line.len() > 300 {
            line[..line.char_indices().nth(300).map(|c| c.0).unwrap_or(line.len())].to_string()
        } else {
            line.to_string()
        }
    }
}

pub async fn run_with(program: &str, args: &[&str], stdin: Option<&str>, timeout: Duration) -> Result<Output> {
    run_full(program, args, stdin, &Default::default(), timeout).await
}

/// Run with extra environment variables (script parameters).
pub async fn run_env(program: &str, args: &[&str], env: &std::collections::BTreeMap<String, String>, timeout: Duration) -> Result<Output> {
    run_full(program, args, None, env, timeout).await
}

async fn run_full(
    program: &str,
    args: &[&str],
    stdin: Option<&str>,
    env: &std::collections::BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Output> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .envs(env)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        // CREATE_NO_WINDOW: don't flash console windows on the user's desktop.
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().with_context(|| format!("start {program}"))?;
    if let Some(input) = stdin {
        if let Some(mut si) = child.stdin.take() {
            si.write_all(input.as_bytes()).await?;
            drop(si);
        }
    }
    let out = tokio::time::timeout(timeout, child.wait_with_output()).await.with_context(|| format!("{program} timed out"))??;
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Run with a 2-minute timeout.
pub async fn run(program: &str, args: &[&str]) -> Result<Output> {
    run_with(program, args, None, Duration::from_secs(120)).await
}

#[allow(dead_code)]
pub async fn run_stdin(program: &str, args: &[&str], stdin: &str) -> Result<Output> {
    run_with(program, args, Some(stdin), Duration::from_secs(120)).await
}

/// Is `program` on PATH?
#[allow(dead_code)]
pub fn which(program: &str) -> bool {
    let exts: Vec<String> = if cfg!(windows) { vec![".exe".into(), ".cmd".into(), String::new()] } else { vec![String::new()] };
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| exts.iter().any(|e| d.join(format!("{program}{e}")).is_file())))
        .unwrap_or(false)
}

/// Run a PowerShell script. `stdin` lines can be read with `[Console]::In.ReadLine()`.
#[allow(dead_code)]
pub async fn powershell(script: &str, stdin: Option<&str>) -> Result<Output> {
    run_with(
        "powershell.exe",
        &["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script],
        stdin,
        Duration::from_secs(300),
    )
    .await
}

/// Write a file atomically-ish, creating parent directories.
#[allow(dead_code)]
pub fn write_file(path: &std::path::Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("od-tmp");
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Write `contents` only if it differs. Returns true when the file changed.
#[allow(dead_code)]
pub fn ensure_file(path: &std::path::Path, contents: &str) -> Result<bool> {
    if std::fs::read_to_string(path).map(|c| c == contents).unwrap_or(false) {
        return Ok(false);
    }
    write_file(path, contents)?;
    Ok(true)
}

/// Constant-time string comparison for secrets.
pub fn ct_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}
