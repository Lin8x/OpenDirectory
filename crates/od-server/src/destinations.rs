//! Where backups go: a local or mounted folder (including SMB/NFS shares),
//! SFTP, FTP/FTPS, WebDAV, S3-compatible storage, a custom HTTPS endpoint,
//! Google Drive, OneDrive and Dropbox. Each supports upload, list and delete,
//! which is all scheduling and retention need.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Shown instead of a stored secret; sending it back means "keep the old one".
pub const KEEP: &str = "••••••••";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Destination {
    /// A folder on this server. Use a mounted path or a UNC path (\\server\share)
    /// for an SMB or NFS share.
    Local { path: String },
    Sftp {
        host: String,
        #[serde(default = "port_22")]
        port: u16,
        username: String,
        #[serde(default)]
        password: String,
        /// OpenSSH private key (PEM), instead of or as well as a password.
        #[serde(default)]
        private_key: String,
        #[serde(default)]
        path: String,
        /// Pinned host key fingerprint (SHA256:...). Learned on first connect if empty.
        #[serde(default)]
        host_key: String,
    },
    Ftp {
        host: String,
        #[serde(default = "port_21")]
        port: u16,
        username: String,
        #[serde(default)]
        password: String,
        #[serde(default)]
        path: String,
        /// Explicit FTPS (AUTH TLS). Plain FTP sends the password unencrypted.
        #[serde(default = "yes")]
        tls: bool,
    },
    Webdav {
        /// Folder URL, e.g. https://cloud.example.com/remote.php/dav/files/me/Backups
        url: String,
        #[serde(default)]
        username: String,
        #[serde(default)]
        password: String,
    },
    S3 {
        /// e.g. https://s3.us-east-1.amazonaws.com, https://<account>.r2.cloudflarestorage.com, http://minio:9000
        endpoint: String,
        #[serde(default = "us_east_1")]
        region: String,
        bucket: String,
        #[serde(default)]
        prefix: String,
        access_key: String,
        #[serde(default)]
        secret_key: String,
        /// Path-style URLs (needed by MinIO and most self-hosted storage).
        #[serde(default)]
        path_style: bool,
    },
    /// Any server that accepts an HTTPS PUT, on any host and port.
    Http {
        /// Base URL; the file name is appended.
        url: String,
        #[serde(default)]
        auth_header: String,
        #[serde(default)]
        auth_value: String,
    },
    GoogleDrive {
        client_id: String,
        #[serde(default)]
        client_secret: String,
        #[serde(default)]
        refresh_token: String,
        /// ID of the Drive folder (from its URL).
        folder_id: String,
    },
    Onedrive {
        client_id: String,
        #[serde(default)]
        client_secret: String,
        #[serde(default)]
        refresh_token: String,
        #[serde(default = "backups_folder")]
        folder: String,
    },
    Dropbox {
        app_key: String,
        #[serde(default)]
        app_secret: String,
        #[serde(default)]
        refresh_token: String,
        #[serde(default = "backups_slash")]
        folder: String,
    },
}

fn port_22() -> u16 {
    22
}
fn port_21() -> u16 {
    21
}
fn yes() -> bool {
    true
}
fn us_east_1() -> String {
    "us-east-1".into()
}
fn backups_folder() -> String {
    "OpenDirectory Backups".into()
}
fn backups_slash() -> String {
    "/OpenDirectory Backups".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteFile {
    pub name: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

/// Is this one of our backup files?
pub fn is_backup(name: &str) -> bool {
    name.starts_with("opendirectory-") && name.ends_with(".odbackup")
}

impl Destination {
    pub fn kind(&self) -> &'static str {
        match self {
            Destination::Local { .. } => "Local or mounted folder",
            Destination::Sftp { .. } => "SFTP",
            Destination::Ftp { tls: true, .. } => "FTPS",
            Destination::Ftp { .. } => "FTP",
            Destination::Webdav { .. } => "WebDAV",
            Destination::S3 { .. } => "S3-compatible",
            Destination::Http { .. } => "HTTPS endpoint",
            Destination::GoogleDrive { .. } => "Google Drive",
            Destination::Onedrive { .. } => "OneDrive",
            Destination::Dropbox { .. } => "Dropbox",
        }
    }

    /// Mutable references to every secret field.
    fn secrets_mut(&mut self) -> Vec<&mut String> {
        match self {
            Destination::Local { .. } => vec![],
            Destination::Sftp { password, private_key, .. } => vec![password, private_key],
            Destination::Ftp { password, .. } => vec![password],
            Destination::Webdav { password, .. } => vec![password],
            Destination::S3 { secret_key, .. } => vec![secret_key],
            Destination::Http { auth_value, .. } => vec![auth_value],
            Destination::GoogleDrive { client_secret, refresh_token, .. } => vec![client_secret, refresh_token],
            Destination::Onedrive { client_secret, refresh_token, .. } => vec![client_secret, refresh_token],
            Destination::Dropbox { app_secret, refresh_token, .. } => vec![app_secret, refresh_token],
        }
    }

    /// Copy for display: secrets that are set become [`KEEP`].
    pub fn redacted(&self) -> Destination {
        let mut d = self.clone();
        for s in d.secrets_mut() {
            if !s.is_empty() {
                *s = KEEP.into();
            }
        }
        d
    }

    /// Replace [`KEEP`] placeholders with the stored values.
    pub fn merge_secrets(&mut self, old: &Destination) {
        let mut old = old.clone();
        if std::mem::discriminant(self) != std::mem::discriminant(&old) {
            for s in self.secrets_mut() {
                if s == KEEP {
                    s.clear();
                }
            }
            return;
        }
        for (new, prev) in self.secrets_mut().into_iter().zip(old.secrets_mut()) {
            if new == KEEP {
                *new = std::mem::take(prev);
            }
        }
    }

    pub fn validate(&self) -> Result<()> {
        let need = |v: &str, what: &str| if v.trim().is_empty() { Err(anyhow!("{what} is required")) } else { Ok(()) };
        match self {
            Destination::Local { path } => need(path, "Folder")?,
            Destination::Sftp { host, username, password, private_key, .. } => {
                need(host, "Host")?;
                need(username, "Username")?;
                if password.is_empty() && private_key.is_empty() {
                    bail!("Enter a password or a private key");
                }
            }
            Destination::Ftp { host, username, .. } => {
                need(host, "Host")?;
                need(username, "Username")?;
            }
            Destination::Webdav { url, .. } | Destination::Http { url, .. } => {
                if !url.starts_with("https://") && !url.starts_with("http://") {
                    bail!("The URL must start with https://");
                }
            }
            Destination::S3 { endpoint, bucket, access_key, secret_key, .. } => {
                need(endpoint, "Endpoint")?;
                need(bucket, "Bucket")?;
                need(access_key, "Access key")?;
                need(secret_key, "Secret key")?;
            }
            Destination::GoogleDrive { client_id, refresh_token, folder_id, .. } => {
                need(client_id, "Client ID")?;
                need(refresh_token, "Refresh token")?;
                need(folder_id, "Folder ID")?;
            }
            Destination::Onedrive { client_id, refresh_token, .. } => {
                need(client_id, "Client ID")?;
                need(refresh_token, "Refresh token")?;
            }
            Destination::Dropbox { app_key, refresh_token, .. } => {
                need(app_key, "App key")?;
                need(refresh_token, "Refresh token")?;
            }
        }
        Ok(())
    }

    /// Warnings shown next to a destination, e.g. unencrypted transport.
    pub fn warnings(&self) -> Vec<String> {
        let mut w = vec![];
        match self {
            Destination::Ftp { tls: false, .. } => {
                w.push("Plain FTP sends your password and backups unencrypted. Use FTPS or SFTP.".into())
            }
            Destination::Webdav { url, .. } | Destination::Http { url, .. } if url.starts_with("http://") => {
                w.push("This URL isn't HTTPS, so credentials travel unencrypted.".into())
            }
            Destination::S3 { endpoint, .. } if endpoint.starts_with("http://") => w.push("This endpoint isn't HTTPS.".into()),
            _ => {}
        }
        w
    }

    pub async fn upload(&mut self, name: &str, data: &[u8]) -> Result<()> {
        match self {
            Destination::Local { path } => {
                let dir = PathBuf::from(path.as_str());
                std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
                let tmp = dir.join(format!("{name}.partial"));
                std::fs::write(&tmp, data).with_context(|| format!("write {}", tmp.display()))?;
                std::fs::rename(&tmp, dir.join(name))?;
                Ok(())
            }
            Destination::Sftp { .. } => sftp::upload(self, name, data).await,
            Destination::Ftp { .. } => ftp::run(self, ftp::Op::Upload(name, data)).await.map(|_| ()),
            Destination::Webdav { url, username, password } => {
                let c = http()?;
                let base = url.trim_end_matches('/');
                let _ = with_basic(c.request(reqwest::Method::from_bytes(b"MKCOL")?, format!("{base}/")), username, password).send().await;
                let r = with_basic(c.put(format!("{base}/{}", enc(name))), username, password).body(data.to_vec()).send().await?;
                ok(r).await.map(|_| ())
            }
            Destination::S3 { .. } => s3::put(self, name, data).await,
            Destination::Http { url, auth_header, auth_value } => {
                let mut rq = http()?.put(format!("{}/{}", url.trim_end_matches('/'), enc(name))).body(data.to_vec());
                if !auth_header.is_empty() {
                    rq = rq.header(auth_header.as_str(), auth_value.as_str());
                }
                ok(rq.send().await?).await.map(|_| ())
            }
            Destination::GoogleDrive { folder_id, .. } => {
                let folder_id = folder_id.clone();
                let tok = oauth::token(self).await?;
                let boundary = format!("od{}", uuid::Uuid::new_v4().simple());
                let meta = json!({ "name": name, "parents": [folder_id] });
                let mut body = format!("--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n--{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n").into_bytes();
                body.extend_from_slice(data);
                body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
                let r = http()?
                    .post("https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart&supportsAllDrives=true")
                    .bearer_auth(tok)
                    .header("Content-Type", format!("multipart/related; boundary={boundary}"))
                    .body(body)
                    .send()
                    .await?;
                ok(r).await.map(|_| ())
            }
            Destination::Onedrive { folder, .. } => {
                let folder = folder.clone();
                let tok = oauth::token(self).await?;
                let c = http()?;
                let item = format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}/{}", enc_path(&folder), enc(name));
                if data.len() <= 4 * 1024 * 1024 {
                    let r = c.put(format!("{item}:/content")).bearer_auth(&tok).body(data.to_vec()).send().await?;
                    return ok(r).await.map(|_| ());
                }
                let sess: Value =
                    ok(c.post(format!("{item}:/createUploadSession")).bearer_auth(&tok).json(&json!({})).send().await?).await?;
                let up = sess["uploadUrl"].as_str().ok_or_else(|| anyhow!("OneDrive didn't return an upload URL"))?;
                let chunk = 5 * 320 * 1024;
                let mut start = 0;
                while start < data.len() {
                    let end = (start + chunk).min(data.len());
                    let r = c
                        .put(up)
                        .header("Content-Range", format!("bytes {}-{}/{}", start, end - 1, data.len()))
                        .body(data[start..end].to_vec())
                        .send()
                        .await?;
                    ok(r).await?;
                    start = end;
                }
                Ok(())
            }
            Destination::Dropbox { folder, .. } => {
                let path = format!("{}/{}", folder.trim_end_matches('/'), name);
                let tok = oauth::token(self).await?;
                let arg = json!({ "path": path, "mode": "overwrite", "mute": true }).to_string();
                let r = http()?
                    .post("https://content.dropboxapi.com/2/files/upload")
                    .bearer_auth(tok)
                    .header("Dropbox-API-Arg", ascii_json(&arg))
                    .header("Content-Type", "application/octet-stream")
                    .body(data.to_vec())
                    .send()
                    .await?;
                ok(r).await.map(|_| ())
            }
        }
    }

    pub async fn list(&mut self) -> Result<Vec<RemoteFile>> {
        let mut files = match self {
            Destination::Local { path } => {
                let mut v = vec![];
                if let Ok(rd) = std::fs::read_dir(path.as_str()) {
                    for e in rd.flatten() {
                        let md = e.metadata()?;
                        v.push(RemoteFile {
                            name: e.file_name().to_string_lossy().into_owned(),
                            size: md.len(),
                            modified: md.modified().ok().map(DateTime::<Utc>::from),
                        });
                    }
                }
                v
            }
            Destination::Sftp { .. } => sftp::list(self).await?,
            Destination::Ftp { .. } => ftp::run(self, ftp::Op::List).await?,
            Destination::Webdav { url, username, password } => {
                let r = with_basic(
                    http()?.request(reqwest::Method::from_bytes(b"PROPFIND")?, format!("{}/", url.trim_end_matches('/'))),
                    username,
                    password,
                )
                .header("Depth", "1")
                .send()
                .await?;
                let body = ok_text(r).await?;
                body.split("response>")
                    .filter_map(|blk| {
                        let href = xml_tag(blk, "href")?;
                        let name = urldecode(href.trim_end_matches('/').rsplit('/').next()?);
                        Some(RemoteFile {
                            name,
                            size: xml_tag(blk, "getcontentlength").and_then(|s| s.parse().ok()).unwrap_or(0),
                            modified: xml_tag(blk, "getlastmodified")
                                .and_then(|s| DateTime::parse_from_rfc2822(&s).ok())
                                .map(|d| d.with_timezone(&Utc)),
                        })
                    })
                    .collect()
            }
            Destination::S3 { .. } => s3::list(self).await?,
            Destination::Http { .. } => bail!("A plain HTTPS endpoint can't list files, so retention isn't available for it"),
            Destination::GoogleDrive { folder_id, .. } => {
                let q = format!("'{}' in parents and trashed = false", folder_id.replace('\'', ""));
                let tok = oauth::token(self).await?;
                let v: Value = ok(http()?
                    .get("https://www.googleapis.com/drive/v3/files")
                    .bearer_auth(tok)
                    .query(&[
                        ("q", q.as_str()),
                        ("fields", "files(id,name,size,modifiedTime)"),
                        ("pageSize", "1000"),
                        ("supportsAllDrives", "true"),
                        ("includeItemsFromAllDrives", "true"),
                    ])
                    .send()
                    .await?)
                .await?;
                v["files"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| RemoteFile {
                        name: f["name"].as_str().unwrap_or("").into(),
                        size: f["size"].as_str().and_then(|s| s.parse().ok()).unwrap_or(0),
                        modified: f["modifiedTime"].as_str().and_then(|s| s.parse().ok()),
                    })
                    .collect()
            }
            Destination::Onedrive { folder, .. } => {
                let folder = folder.clone();
                let tok = oauth::token(self).await?;
                let r = http()?
                    .get(format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}:/children", enc_path(&folder)))
                    .bearer_auth(tok)
                    .send()
                    .await?;
                if r.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(vec![]);
                }
                let v: Value = ok(r).await?;
                v["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| RemoteFile {
                        name: f["name"].as_str().unwrap_or("").into(),
                        size: f["size"].as_u64().unwrap_or(0),
                        modified: f["lastModifiedDateTime"].as_str().and_then(|s| s.parse().ok()),
                    })
                    .collect()
            }
            Destination::Dropbox { folder, .. } => {
                let folder = folder.trim_end_matches('/').to_string();
                let tok = oauth::token(self).await?;
                let r = http()?
                    .post("https://api.dropboxapi.com/2/files/list_folder")
                    .bearer_auth(tok)
                    .json(&json!({ "path": folder }))
                    .send()
                    .await?;
                if r.status() == reqwest::StatusCode::CONFLICT {
                    return Ok(vec![]); // folder doesn't exist yet
                }
                let v: Value = ok(r).await?;
                v["entries"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| RemoteFile {
                        name: f["name"].as_str().unwrap_or("").into(),
                        size: f["size"].as_u64().unwrap_or(0),
                        modified: f["server_modified"].as_str().and_then(|s| s.parse().ok()),
                    })
                    .collect()
            }
        };
        files.retain(|f| is_backup(&f.name));
        files.sort_by(|a, b| b.name.cmp(&a.name));
        Ok(files)
    }

    pub async fn delete(&mut self, name: &str) -> Result<()> {
        if !is_backup(name) {
            bail!("Only OpenDirectory backup files can be deleted");
        }
        match self {
            Destination::Local { path } => Ok(std::fs::remove_file(PathBuf::from(path.as_str()).join(name))?),
            Destination::Sftp { .. } => sftp::delete(self, name).await,
            Destination::Ftp { .. } => ftp::run(self, ftp::Op::Delete(name)).await.map(|_| ()),
            Destination::Webdav { url, username, password } => {
                let r =
                    with_basic(http()?.delete(format!("{}/{}", url.trim_end_matches('/'), enc(name))), username, password).send().await?;
                ok(r).await.map(|_| ())
            }
            Destination::S3 { .. } => s3::delete(self, name).await,
            Destination::Http { .. } => bail!("A plain HTTPS endpoint can't delete files"),
            Destination::GoogleDrive { folder_id, .. } => {
                let q = format!("'{}' in parents and name = '{}' and trashed = false", folder_id.replace('\'', ""), name.replace('\'', ""));
                let tok = oauth::token(self).await?;
                let c = http()?;
                let v: Value = ok(c
                    .get("https://www.googleapis.com/drive/v3/files")
                    .bearer_auth(&tok)
                    .query(&[
                        ("q", q.as_str()),
                        ("fields", "files(id)"),
                        ("supportsAllDrives", "true"),
                        ("includeItemsFromAllDrives", "true"),
                    ])
                    .send()
                    .await?)
                .await?;
                for f in v["files"].as_array().into_iter().flatten() {
                    if let Some(id) = f["id"].as_str() {
                        ok(c.delete(format!("https://www.googleapis.com/drive/v3/files/{id}?supportsAllDrives=true"))
                            .bearer_auth(&tok)
                            .send()
                            .await?)
                        .await?;
                    }
                }
                Ok(())
            }
            Destination::Onedrive { folder, .. } => {
                let folder = folder.clone();
                let tok = oauth::token(self).await?;
                let r = http()?
                    .delete(format!("https://graph.microsoft.com/v1.0/me/drive/root:/{}/{}", enc_path(&folder), enc(name)))
                    .bearer_auth(tok)
                    .send()
                    .await?;
                ok(r).await.map(|_| ())
            }
            Destination::Dropbox { folder, .. } => {
                let path = format!("{}/{}", folder.trim_end_matches('/'), name);
                let tok = oauth::token(self).await?;
                ok(http()?
                    .post("https://api.dropboxapi.com/2/files/delete_v2")
                    .bearer_auth(tok)
                    .json(&json!({ "path": path }))
                    .send()
                    .await?)
                .await
                .map(|_| ())
            }
        }
    }

    pub async fn download(&mut self, name: &str) -> Result<Vec<u8>> {
        if !is_backup(name) {
            bail!("Only OpenDirectory backup files can be downloaded");
        }
        match self {
            Destination::Local { path } => Ok(std::fs::read(PathBuf::from(path.as_str()).join(name))?),
            Destination::Sftp { .. } => sftp::download(self, name).await,
            Destination::Ftp { .. } => {
                let v = ftp::run(self, ftp::Op::Download(name)).await?;
                Ok(hex::decode(&v.first().ok_or_else(|| anyhow!("Download failed"))?.name)?)
            }
            Destination::Webdav { url, username, password } => {
                let r = with_basic(http()?.get(format!("{}/{}", url.trim_end_matches('/'), enc(name))), username, password).send().await?;
                Ok(ok_bytes(r).await?)
            }
            Destination::S3 { .. } => s3::get(self, name).await,
            _ => bail!("Download this backup from the provider's website, then restore it from a file"),
        }
    }

    /// Prove the destination works: upload, list and delete a small probe file.
    pub async fn test(&mut self) -> Result<String> {
        let probe = format!("opendirectory-connection-test-{}.odbackup", Utc::now().format("%Y%m%d%H%M%S"));
        self.upload(&probe, b"OpenDirectory connection test. Safe to delete.").await.context("Uploading a test file")?;
        let can_list = !matches!(self, Destination::Http { .. });
        if can_list {
            let files = self.list().await.context("Listing files")?;
            if !files.iter().any(|f| f.name == probe) {
                bail!("The test file was uploaded but doesn't show up when listing the folder");
            }
            self.delete(&probe).await.context("Deleting the test file")?;
        }
        Ok(if can_list {
            "Upload, list and delete all work".into()
        } else {
            "Upload works (this destination can't list or delete, so retention is off)".into()
        })
    }
}

fn http() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .user_agent(concat!("OpenDirectory/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

fn with_basic(rq: reqwest::RequestBuilder, user: &str, pass: &str) -> reqwest::RequestBuilder {
    if user.is_empty() {
        rq
    } else {
        rq.basic_auth(user, Some(pass))
    }
}

async fn ok_text(r: reqwest::Response) -> Result<String> {
    let status = r.status();
    let body = r.text().await.unwrap_or_default();
    if !status.is_success() {
        let short: String = body.chars().take(300).collect();
        bail!("The server answered {status}: {short}");
    }
    Ok(body)
}
async fn ok(r: reqwest::Response) -> Result<Value> {
    let t = ok_text(r).await?;
    Ok(serde_json::from_str(&t).unwrap_or(Value::Null))
}
async fn ok_bytes(r: reqwest::Response) -> Result<Vec<u8>> {
    let status = r.status();
    if !status.is_success() {
        bail!("The server answered {status}");
    }
    Ok(r.bytes().await?.to_vec())
}

/// Percent-encode one path segment.
fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}
fn enc_path(s: &str) -> String {
    s.trim_matches('/').split('/').map(enc).collect::<Vec<_>>().join("/")
}
fn urldecode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
/// Dropbox headers must be ASCII: escape anything else as \uXXXX.
fn ascii_json(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() { c.to_string() } else { format!("\\u{:04x}", c as u32) }).collect()
}
/// Text of the first `<…:tag>` or `<tag>` element (namespace-agnostic).
fn xml_tag(blk: &str, tag: &str) -> Option<String> {
    let start =
        blk.find(&format!(":{tag}>")).map(|i| i + tag.len() + 2).or_else(|| blk.find(&format!("<{tag}>")).map(|i| i + tag.len() + 2))?;
    let end = blk[start..].find("</")? + start;
    Some(blk[start..end].trim().to_string())
}

// ------------------------------------------------------------------ OAuth
mod oauth {
    use super::*;

    /// Exchange the stored refresh token for an access token.
    pub async fn token(d: &Destination) -> Result<String> {
        let (url, form): (&str, Vec<(&str, &str)>) = match d {
            Destination::GoogleDrive { client_id, client_secret, refresh_token, .. } => (
                "https://oauth2.googleapis.com/token",
                vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                    ("client_id", client_id),
                    ("client_secret", client_secret),
                ],
            ),
            Destination::Onedrive { client_id, client_secret, refresh_token, .. } => {
                let mut f = vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token.as_str()),
                    ("client_id", client_id.as_str()),
                    ("scope", "Files.ReadWrite offline_access"),
                ];
                if !client_secret.is_empty() {
                    f.push(("client_secret", client_secret));
                }
                ("https://login.microsoftonline.com/common/oauth2/v2.0/token", f)
            }
            Destination::Dropbox { app_key, app_secret, refresh_token, .. } => (
                "https://api.dropboxapi.com/oauth2/token",
                vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", refresh_token),
                    ("client_id", app_key),
                    ("client_secret", app_secret),
                ],
            ),
            _ => bail!("not an OAuth destination"),
        };
        let v = ok(http()?.post(url).form(&form).send().await?)
            .await
            .context("Signing in to the provider (check the client ID, secret and refresh token)")?;
        v["access_token"].as_str().map(str::to_string).ok_or_else(|| anyhow!("The provider didn't return an access token"))
    }
}

// ------------------------------------------------------------------ S3 (SigV4)
mod s3 {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};

    fn hmac(key: &[u8], data: &str) -> Vec<u8> {
        let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("any key length");
        m.update(data.as_bytes());
        m.finalize().into_bytes().to_vec()
    }

    struct Req {
        url: String,
        host: String,
        canonical_uri: String,
    }

    fn target(d: &Destination, key: Option<&str>) -> Result<Req> {
        let Destination::S3 { endpoint, bucket, path_style, .. } = d else { unreachable!() };
        let ep = reqwest::Url::parse(endpoint.trim_end_matches('/')).context("Invalid S3 endpoint URL")?;
        let host_only = ep.host_str().ok_or_else(|| anyhow!("Invalid S3 endpoint URL"))?;
        let port = ep.port().map(|p| format!(":{p}")).unwrap_or_default();
        let key_part = key.map(|k| format!("/{}", k.split('/').map(enc).collect::<Vec<_>>().join("/"))).unwrap_or_else(|| "/".into());
        let (host, uri) = if *path_style {
            (format!("{host_only}{port}"), format!("/{}{}", enc(bucket), if key.is_some() { key_part } else { String::new() }))
        } else {
            (format!("{}.{host_only}{port}", bucket), key_part)
        };
        Ok(Req { url: format!("{}://{host}{uri}", ep.scheme()), host, canonical_uri: if uri.is_empty() { "/".into() } else { uri } })
    }

    fn sign(d: &Destination, method: &str, r: &Req, query: &[(String, String)], payload: &[u8]) -> Result<reqwest::header::HeaderMap> {
        let Destination::S3 { region, access_key, secret_key, .. } = d else { unreachable!() };
        let now = Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date = now.format("%Y%m%d").to_string();
        let payload_hash = hex::encode(Sha256::digest(payload));
        let mut q: Vec<(String, String)> = query.iter().map(|(k, v)| (enc(k), enc(v))).collect();
        q.sort();
        let cq = q.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&");
        let canonical = format!(
            "{method}\n{}\n{cq}\nhost:{}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{amz_date}\n\nhost;x-amz-content-sha256;x-amz-date\n{payload_hash}",
            r.canonical_uri, r.host
        );
        let scope = format!("{date}/{region}/s3/aws4_request");
        let to_sign = format!("AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}", hex::encode(Sha256::digest(canonical.as_bytes())));
        let k = hmac(format!("AWS4{secret_key}").as_bytes(), &date);
        let k = hmac(&k, region);
        let k = hmac(&k, "s3");
        let k = hmac(&k, "aws4_request");
        let sig = hex::encode(hmac(&k, &to_sign));
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-amz-date", amz_date.parse()?);
        h.insert("x-amz-content-sha256", payload_hash.parse()?);
        h.insert(
            "authorization",
            format!(
                "AWS4-HMAC-SHA256 Credential={access_key}/{scope}, SignedHeaders=host;x-amz-content-sha256;x-amz-date, Signature={sig}"
            )
            .parse()?,
        );
        Ok(h)
    }

    fn key_for(d: &Destination, name: &str) -> String {
        let Destination::S3 { prefix, .. } = d else { unreachable!() };
        let p = prefix.trim_matches('/');
        if p.is_empty() {
            name.to_string()
        } else {
            format!("{p}/{name}")
        }
    }

    pub async fn put(d: &Destination, name: &str, data: &[u8]) -> Result<()> {
        let r = target(d, Some(&key_for(d, name)))?;
        let h = sign(d, "PUT", &r, &[], data)?;
        ok(http()?.put(&r.url).headers(h).body(data.to_vec()).send().await?).await.map(|_| ())
    }
    pub async fn get(d: &Destination, name: &str) -> Result<Vec<u8>> {
        let r = target(d, Some(&key_for(d, name)))?;
        let h = sign(d, "GET", &r, &[], b"")?;
        ok_bytes(http()?.get(&r.url).headers(h).send().await?).await
    }
    pub async fn delete(d: &Destination, name: &str) -> Result<()> {
        let r = target(d, Some(&key_for(d, name)))?;
        let h = sign(d, "DELETE", &r, &[], b"")?;
        ok(http()?.delete(&r.url).headers(h).send().await?).await.map(|_| ())
    }
    pub async fn list(d: &Destination) -> Result<Vec<RemoteFile>> {
        let Destination::S3 { prefix, .. } = d else { unreachable!() };
        let p = prefix.trim_matches('/');
        let pfx = if p.is_empty() { String::new() } else { format!("{p}/") };
        let r = target(d, None)?;
        let query = vec![("list-type".to_string(), "2".to_string()), ("prefix".to_string(), pfx.clone())];
        let h = sign(d, "GET", &r, &query, b"")?;
        let body = ok_text(http()?.get(&r.url).headers(h).query(&query).send().await?).await?;
        Ok(body
            .split("<Contents>")
            .skip(1)
            .filter_map(|blk| {
                let key = xml_tag(blk, "Key")?;
                Some(RemoteFile {
                    name: key.strip_prefix(&pfx).unwrap_or(&key).to_string(),
                    size: xml_tag(blk, "Size").and_then(|s| s.parse().ok()).unwrap_or(0),
                    modified: xml_tag(blk, "LastModified").and_then(|s| s.parse().ok()),
                })
            })
            .collect())
    }
}

// ------------------------------------------------------------------ SFTP
mod sftp {
    use super::*;
    use russh::keys::PublicKeyOrCertificate;
    use russh_sftp::client::SftpSession;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Pin {
        expected: String,
        seen: Arc<std::sync::Mutex<String>>,
    }

    impl russh::client::Handler for Pin {
        type Error = anyhow::Error;
        async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
            let pk = match key {
                PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
                PublicKeyOrCertificate::Certificate(c) => russh::keys::PublicKey::from(c.public_key().clone()),
            };
            let fp = pk.fingerprint(russh::keys::HashAlg::Sha256).to_string();
            *self.seen.lock().unwrap_or_else(|e| e.into_inner()) = fp.clone();
            if self.expected.is_empty() || self.expected == fp {
                Ok(true)
            } else {
                Err(anyhow!(
                    "The server's host key changed (expected {}, got {fp}). If you replaced the server, clear the pinned key.",
                    self.expected
                ))
            }
        }
    }

    async fn connect(d: &mut Destination) -> Result<(SftpSession, String)> {
        let Destination::Sftp { host, port, username, password, private_key, path, host_key } = d else { unreachable!() };
        let seen = Arc::new(std::sync::Mutex::new(String::new()));
        let cfg = Arc::new(russh::client::Config { inactivity_timeout: Some(std::time::Duration::from_secs(120)), ..Default::default() });
        let mut s = russh::client::connect(cfg, (host.as_str(), *port), Pin { expected: host_key.clone(), seen: seen.clone() })
            .await
            .with_context(|| format!("Connecting to {host}:{port}"))?;
        // Trust on first use: remember the host key for next time.
        if host_key.is_empty() {
            *host_key = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
        }
        let mut authed = false;
        if !private_key.trim().is_empty() {
            let k = russh::keys::decode_secret_key(private_key.trim(), if password.is_empty() { None } else { Some(password.as_str()) })
                .context("Reading the private key")?;
            let hash = s.best_supported_rsa_hash().await.ok().flatten().flatten();
            authed =
                s.authenticate_publickey(username.as_str(), russh::keys::PrivateKeyWithHashAlg::new(Arc::new(k), hash)).await?.success();
        }
        if !authed && !password.is_empty() {
            authed = s.authenticate_password(username.as_str(), password.as_str()).await?.success();
        }
        if !authed {
            bail!("The SFTP server rejected the username, password or key");
        }
        let ch = s.channel_open_session().await?;
        ch.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(ch.into_stream()).await?;
        let dir = if path.trim().is_empty() { ".".to_string() } else { path.trim_end_matches('/').to_string() };
        if dir != "." && sftp.try_exists(&dir).await.map(|e| !e).unwrap_or(true) {
            let _ = sftp.create_dir(&dir).await;
        }
        Ok((sftp, dir))
    }

    pub async fn upload(d: &mut Destination, name: &str, data: &[u8]) -> Result<()> {
        let (s, dir) = connect(d).await?;
        let tmp = format!("{dir}/{name}.partial");
        let mut f = s.create(&tmp).await.context("Creating the file on the SFTP server")?;
        f.write_all(data).await?;
        f.shutdown().await?;
        let fin = format!("{dir}/{name}");
        let _ = s.remove_file(&fin).await;
        s.rename(&tmp, &fin).await?;
        Ok(())
    }
    pub async fn list(d: &mut Destination) -> Result<Vec<RemoteFile>> {
        let (s, dir) = connect(d).await?;
        Ok(s.read_dir(&dir)
            .await?
            .map(|e| {
                let md = e.metadata();
                RemoteFile {
                    name: e.file_name(),
                    size: md.size.unwrap_or(0),
                    modified: md.mtime.and_then(|t| DateTime::<Utc>::from_timestamp(t as i64, 0)),
                }
            })
            .collect())
    }
    pub async fn delete(d: &mut Destination, name: &str) -> Result<()> {
        let (s, dir) = connect(d).await?;
        s.remove_file(format!("{dir}/{name}")).await?;
        Ok(())
    }
    pub async fn download(d: &mut Destination, name: &str) -> Result<Vec<u8>> {
        let (s, dir) = connect(d).await?;
        let mut f = s.open(format!("{dir}/{name}")).await?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).await?;
        Ok(v)
    }
}

// ------------------------------------------------------------------ FTP / FTPS
mod ftp {
    use super::*;
    use suppaftp::tokio::{AsyncFtpStream, AsyncRustlsConnector, AsyncRustlsFtpStream, ImplAsyncFtpStream, TokioTlsStream};

    pub enum Op<'a> {
        Upload(&'a str, &'a [u8]),
        List,
        Delete(&'a str),
        /// Returns one entry whose name is the file's bytes, hex-encoded.
        Download(&'a str),
    }

    fn tls() -> AsyncRustlsConnector {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("TLS versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
        AsyncRustlsConnector::from(tokio_rustls::TlsConnector::from(Arc::new(cfg)))
    }

    async fn ops<T: TokioTlsStream + Send>(
        mut ftp: ImplAsyncFtpStream<T>,
        user: &str,
        pass: &str,
        path: &str,
        op: Op<'_>,
    ) -> Result<Vec<RemoteFile>> {
        ftp.login(user, pass).await.context("The FTP server rejected the username or password")?;
        ftp.transfer_type(suppaftp::types::FileType::Binary).await?;
        if !path.trim().is_empty() && ftp.cwd(path).await.is_err() {
            {
                let _ = ftp.mkdir(path).await;
                ftp.cwd(path).await.context("Opening the folder")?;
            }
        }
        let out = match op {
            Op::Upload(name, data) => {
                let mut r = data;
                ftp.put_file(name, &mut r).await?;
                vec![]
            }
            Op::List => ftp
                .nlst(None)
                .await?
                .into_iter()
                .map(|n| RemoteFile { name: n.rsplit('/').next().unwrap_or(&n).to_string(), size: 0, modified: None })
                .collect(),
            Op::Delete(name) => {
                ftp.rm(name).await?;
                vec![]
            }
            Op::Download(name) => {
                use tokio::io::AsyncReadExt;
                let mut stream = ftp.retr_as_stream(name).await?;
                let mut buf = Vec::new();
                stream.read_to_end(&mut buf).await?;
                stream.finish().await?;
                // Carry bytes through as hex to keep the return type simple.
                vec![RemoteFile { name: hex::encode(buf), size: 0, modified: None }]
            }
        };
        let _ = ftp.quit().await;
        Ok(out)
    }

    pub async fn run(d: &Destination, op: Op<'_>) -> Result<Vec<RemoteFile>> {
        let Destination::Ftp { host, port, username, password, path, tls: secure } = d else { unreachable!() };
        let addr = format!("{host}:{port}");
        let r = if *secure {
            let s = AsyncRustlsFtpStream::connect(&addr).await.with_context(|| format!("Connecting to {addr}"))?;
            let s = s.into_secure(tls(), host).await.context("Starting TLS (the server may not support FTPS)")?;
            ops(s, username, password, path, op).await?
        } else {
            let s = AsyncFtpStream::connect(&addr).await.with_context(|| format!("Connecting to {addr}"))?;
            ops(s, username, password, path, op).await?
        };
        Ok(r)
    }
}
