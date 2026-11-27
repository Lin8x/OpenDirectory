//! Enrollment files: ready-to-run installers the Admin Center hands out
//! instead of (or as well as) the enrollment command. The Management Server
//! builds them from the agent builds it keeps for agent updates, with the
//! server address, enrollment token and TLS fingerprint baked in.
//!
//! - Windows: a .zip with the agent and "Install OpenDirectory Agent.cmd",
//!   which asks for administrator rights and enrolls.
//! - macOS: one self-contained .command file (both architectures inside).
//! - Linux: one self-contained .sh file.
//! - enroll.conf: the file the .deb/.rpm/.pkg fleet packages read to enroll
//!   unattended.

use std::io::Write;

use axum::extract::{Path, State};
use axum::Json;
use base64::Engine;
use serde::{Deserialize, Serialize};

use super::agent_updates;
use crate::auth::Admin;
use crate::error::{ApiError, ApiResult};
use crate::AppState;

#[derive(Deserialize)]
pub struct FileRequest {
    /// "windows", "macos", "linux" or "conf".
    platform: String,
    /// Address the endpoint should use (the office address or the Internet
    /// Gateway). Defaults to this server's endpoint address.
    #[serde(default)]
    server: Option<String>,
}

#[derive(Serialize)]
pub struct FileResult {
    file_name: String,
    /// Base64 file contents.
    data: String,
    /// What to tell the admin about the file.
    instructions: String,
}

async fn build(s: &AppState, platform: &str) -> ApiResult<Vec<u8>> {
    if agent_updates::cached(s, platform).is_none() {
        agent_updates::fetch(s, platform).await.map_err(|e| {
            tracing::warn!("couldn't fetch the Endpoint Agent build for {platform}: {e:#}");
            ApiError::new(
                axum::http::StatusCode::NOT_FOUND,
                format!("This Management Server doesn't have the {platform} Endpoint Agent build yet. Get it in Settings > Endpoint Agent updates"),
            )
        })?;
    }
    let (path, _, _) = agent_updates::cached(s, platform).ok_or_else(|| ApiError::not_found("Agent build"))?;
    Ok(tokio::fs::read(path).await.map_err(anyhow::Error::from)?)
}

/// Single-quote a value for sh.
fn sh_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', "'\\''"))
}

fn slug(v: &str) -> String {
    let s: String = v.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "enrollment".into()
    } else {
        s.chars().take(40).collect()
    }
}

pub async fn create(
    State(s): State<AppState>,
    admin: Admin,
    Path(token): Path<String>,
    Json(req): Json<FileRequest>,
) -> ApiResult<Json<FileResult>> {
    let tok =
        s.db.valid_token(token.trim())?.ok_or_else(|| ApiError::bad_request("That enrollment token is invalid, expired or used up"))?;
    let server = req.server.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| s.info.endpoint_url.clone());
    if !(server.starts_with("https://") || server.starts_with("http://")) || server.contains(['\'', '"', '\n', '\r', ' ', '`', '$', '%']) {
        return Err(ApiError::bad_request("The server address must be a plain http(s) URL"));
    }
    let fp = s.info.tls_fingerprint.clone().unwrap_or_default();
    let name = slug(if tok.description.is_empty() { "enrollment" } else { &tok.description });
    let b64 = base64::engine::general_purpose::STANDARD;
    let reuse = match tok.max_uses {
        Some(1) => "The token is single-use, so the file enrolls one computer.".to_string(),
        Some(n) => format!("The token allows {n} computers in total."),
        None => "The token has no use limit; revoke it when you're done.".to_string(),
    };

    let (file_name, bytes, instructions) = match req.platform.as_str() {
        "conf" => {
            let mut c = format!("# OpenDirectory enrollment, read by the Endpoint Agent .deb/.rpm/.pkg packages.\n# Linux: /etc/opendirectory/enroll.conf   macOS: /Library/Application Support/OpenDirectory/enroll.conf\nSERVER={server}\nTOKEN={}\n", tok.token);
            if !fp.is_empty() {
                c.push_str(&format!("FINGERPRINT={fp}\n"));
            }
            (
                "enroll.conf".to_string(),
                c.into_bytes(),
                format!("Copy it to /etc/opendirectory/enroll.conf (Linux) or /Library/Application Support/OpenDirectory/enroll.conf (macOS) before installing the agent package, and the package enrolls by itself. {reuse}"),
            )
        }
        "windows" => {
            let exe = build(&s, "windows-x64").await?;
            let fp_arg = if fp.is_empty() { String::new() } else { format!(" --fingerprint {fp}") };
            let cmd = format!(
                "@echo off\r\n\
                 rem OpenDirectory Endpoint Agent: double-click to install and enroll this computer.\r\n\
                 net session >nul 2>&1\r\n\
                 if %errorlevel% neq 0 (\r\n\
                 \x20 powershell -NoProfile -Command \"Start-Process -FilePath '%~f0' -Verb RunAs\"\r\n\
                 \x20 exit /b\r\n\
                 )\r\n\
                 cd /d \"%~dp0\"\r\n\
                 echo Installing the OpenDirectory Endpoint Agent and enrolling with {server} ...\r\n\
                 opendirectory-agent.exe enroll --server {server} --token {token}{fp_arg} --install\r\n\
                 if %errorlevel% neq 0 (echo. & echo Enrollment failed. & pause & exit /b 1)\r\n\
                 echo. & echo Done. This computer is now managed by OpenDirectory.\r\n\
                 pause\r\n",
                token = tok.token
            );
            let readme = format!(
                "OpenDirectory Endpoint Agent\r\n\r\n1. Extract this folder.\r\n2. Double-click \"Install OpenDirectory Agent.cmd\" and allow it to make changes.\r\n\r\nIt enrolls with {server}. Windows SmartScreen may ask you to confirm because the agent isn't code-signed yet (More info > Run anyway).\r\n"
            );
            let zip = zip_store(&[
                ("Install OpenDirectory Agent.cmd", cmd.as_bytes(), false),
                ("opendirectory-agent.exe", &exe, false),
                ("README.txt", readme.as_bytes(), false),
            ])?;
            (
                format!("opendirectory-enroll-{name}-windows.zip"),
                zip,
                format!("Extract the .zip on the Windows computer and double-click \"Install OpenDirectory Agent.cmd\". {reuse}"),
            )
        }
        "macos" => {
            let arm = build(&s, "macos-arm64").await?;
            let x64 = build(&s, "macos-x64").await?;
            let script = self_extracting(
                "macOS",
                &server,
                &tok.token,
                &fp,
                "case \"$(uname -m)\" in arm64) part=arm64 ;; *) part=x64 ;; esac",
                &[("arm64", &arm), ("x64", &x64)],
            );
            (
                format!("opendirectory-enroll-{name}-macos.command"),
                script.into_bytes(),
                format!("On the Mac, right-click the .command file, choose Open and confirm (it isn't notarized yet), then enter an administrator password. {reuse}"),
            )
        }
        "linux" => {
            let bin = build(&s, "linux-x64").await?;
            let script = self_extracting("Linux", &server, &tok.token, &fp, "part=x64", &[("x64", &bin)]);
            (
                format!("opendirectory-enroll-{name}-linux.sh"),
                script.into_bytes(),
                format!("On the Linux computer run: sudo sh {}. {reuse}", format_args!("opendirectory-enroll-{name}-linux.sh")),
            )
        }
        _ => return Err(ApiError::bad_request("Platform must be windows, macos, linux or conf")),
    };
    s.db.audit(admin.name(), "enrollment.file", &req.platform, &format!("{file_name} for token {}", &tok.token[..4.min(tok.token.len())]));
    Ok(Json(FileResult { file_name, data: b64.encode(bytes), instructions }))
}

/// A shell script that carries agent binaries as base64 after a marker line,
/// extracts the one for this machine and enrolls.
fn self_extracting(os: &str, server: &str, token: &str, fp: &str, pick: &str, parts: &[(&str, &[u8])]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let fp_arg = if fp.is_empty() { String::new() } else { format!(" --fingerprint {}", sh_quote(fp)) };
    let mut out = format!(
        r#"#!/bin/sh
# OpenDirectory Endpoint Agent for {os}: installs the agent and enrolls this
# computer with {server}. Run it as root (it asks for sudo if needed).
set -e
if [ "$(id -u)" != 0 ]; then exec sudo sh "$0" "$@"; fi
{pick}
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
awk -v want="__OD_PAYLOAD_${{part}}__" '$0 == want {{on=1; next}} /^__OD_PAYLOAD_/ {{on=0}} on' "$0" | base64 -d > "$tmp/opendirectory-agent" 2>/dev/null ||
  awk -v want="__OD_PAYLOAD_${{part}}__" '$0 == want {{on=1; next}} /^__OD_PAYLOAD_/ {{on=0}} on' "$0" | base64 -D > "$tmp/opendirectory-agent"
chmod 0755 "$tmp/opendirectory-agent"
echo "Installing the OpenDirectory Endpoint Agent and enrolling with {server} ..."
"$tmp/opendirectory-agent" enroll --server {q_server} --token {q_token}{fp_arg} --install
echo "Done. This computer is now managed by OpenDirectory."
exit 0
"#,
        q_server = sh_quote(server),
        q_token = sh_quote(token),
    );
    for (name, bytes) in parts {
        out.push_str(&format!("__OD_PAYLOAD_{name}__\n"));
        let enc = b64.encode(bytes);
        for chunk in enc.as_bytes().chunks(76) {
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
            out.push('\n');
        }
    }
    out.push_str("__OD_PAYLOAD_END__\n");
    out
}

/// A minimal .zip writer (deflate). `exec` marks Unix-executable entries.
fn zip_store(files: &[(&str, &[u8], bool)]) -> anyhow::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data, exec) in files {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data)?;
        let comp = enc.finish()?;
        let offset = out.len() as u32;
        let name_b = name.as_bytes();
        // Local file header.
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        out.extend_from_slice(&8u16.to_le_bytes()); // deflate
        out.extend_from_slice(&0u32.to_le_bytes()); // time/date
        out.extend_from_slice(&crc.sum().to_le_bytes());
        out.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name_b);
        out.extend_from_slice(&comp);
        // Central directory entry.
        let mode: u32 = if *exec { 0o100755 } else { 0o100644 };
        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made by Unix
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&8u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&crc.sum().to_le_bytes());
        central.extend_from_slice(&(comp.len() as u32).to_le_bytes());
        central.extend_from_slice(&(data.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0u8; 8]); // extra, comment, disk, internal attrs
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_b);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    Ok(out)
}
